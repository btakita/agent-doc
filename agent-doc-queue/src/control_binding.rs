//! Pure queue marker/frontmatter control binding policy.
//!
//! This module owns the queue activation spellings shared between the
//! `<!-- agent:queue ... -->` marker and frontmatter `queue:` control. Callers
//! provide any snapshot content they want considered; this module does not read
//! or write files.

use anyhow::Result;
use std::collections::HashMap;

use agent_doc_frontmatter::frontmatter;

pub fn explicit_queue_go_mode(
    attrs: &HashMap<String, String>,
    frontmatter_queue: Option<&str>,
) -> bool {
    resolved_queue_binding(attrs, frontmatter_queue) == Some(QueueBindingMode::Go)
}

pub fn explicit_queue_start_mode(
    attrs: &HashMap<String, String>,
    frontmatter_queue: Option<&str>,
) -> bool {
    resolved_queue_binding(attrs, frontmatter_queue) == Some(QueueBindingMode::Start)
}

/// `stop` and the operator-only `pause` hold both keep the queue inactive.
pub fn explicit_queue_stop_mode(
    attrs: &HashMap<String, String>,
    frontmatter_queue: Option<&str>,
) -> bool {
    matches!(
        resolved_queue_binding(attrs, frontmatter_queue),
        Some(QueueBindingMode::Stop | QueueBindingMode::Pause)
    )
}

/// `#queueeditgo`: `queue: pause` is the operator's standing hold. The binary
/// never writes it (drain/halt writes `stop`), so it is the one control that
/// survives a queue edit instead of being re-armed to `go`.
pub fn explicit_queue_pause_mode(
    attrs: &HashMap<String, String>,
    frontmatter_queue: Option<&str>,
) -> bool {
    resolved_queue_binding(attrs, frontmatter_queue) == Some(QueueBindingMode::Pause)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueBindingMode {
    Start,
    Go,
    Stop,
    Pause,
}

impl QueueBindingMode {
    fn from_frontmatter(raw: Option<&str>) -> Option<Self> {
        match raw?.trim().to_ascii_lowercase().as_str() {
            "start" => Some(Self::Start),
            "go" => Some(Self::Go),
            "stop" => Some(Self::Stop),
            "pause" => Some(Self::Pause),
            _ => None,
        }
    }

    fn from_marker(attrs: &HashMap<String, String>) -> Option<Self> {
        if attrs.contains_key("stop") {
            Some(Self::Stop)
        } else if attrs.contains_key("go") {
            Some(Self::Go)
        } else if attrs.contains_key("start") {
            Some(Self::Start)
        } else {
            None
        }
    }

    fn frontmatter_value(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Go => "go",
            Self::Stop => "stop",
            Self::Pause => "pause",
        }
    }

    fn marker_token(self) -> Option<&'static str> {
        match self {
            Self::Start => Some("start"),
            Self::Go => Some("go"),
            Self::Stop | Self::Pause => None,
        }
    }
}

fn resolved_queue_binding(
    attrs: &HashMap<String, String>,
    frontmatter_queue: Option<&str>,
) -> Option<QueueBindingMode> {
    // The marker is the operator's ephemeral gesture surface. Let an explicit
    // marker token override a stale frontmatter projection; convergence will
    // then copy that gesture into the canonical `queue:` field. Without this
    // precedence, `<!-- agent:queue go -->` beside `queue: stop` is read as
    // stopped before convergence can observe and persist the marker edit,
    // producing the go-marker churn that #qactsync removes.
    QueueBindingMode::from_marker(attrs)
        .or_else(|| QueueBindingMode::from_frontmatter(frontmatter_queue))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QueueBindingState {
    marker_mode: Option<QueueBindingMode>,
    frontmatter_mode: Option<QueueBindingMode>,
    legacy_queue_active: Option<bool>,
    has_auto: bool,
}

pub fn converge_queue_control_binding_content(
    content: &str,
    snapshot_content: Option<&str>,
) -> Result<(String, bool)> {
    let Some(current) = queue_binding_state(content) else {
        return Ok((content.to_string(), false));
    };
    let previous = snapshot_content.and_then(queue_binding_state);
    let Some(target) = queue_binding_target(current, previous) else {
        return Ok((content.to_string(), false));
    };

    let mut updated = set_queue_marker_binding(content, target.marker_token())?;
    updated = frontmatter::merge_queue_control(&updated, target.frontmatter_value())?;
    let changed = updated != content;
    Ok((updated, changed))
}

fn queue_binding_state(content: &str) -> Option<QueueBindingState> {
    let (fm, _) = frontmatter::parse(content).ok()?;
    let components = agent_doc_element::element::parse(content).ok()?;
    let queue_component = components
        .iter()
        .find(|component| component.name == "queue")?;
    Some(QueueBindingState {
        marker_mode: QueueBindingMode::from_marker(&queue_component.attrs),
        frontmatter_mode: QueueBindingMode::from_frontmatter(fm.queue.as_deref()),
        legacy_queue_active: if fm.queue.is_none() {
            fm.queue_active
        } else {
            None
        },
        has_auto: crate::document_queue::has_auto_attr(&queue_component.attrs),
    })
}

fn queue_binding_target(
    current: QueueBindingState,
    previous: Option<QueueBindingState>,
) -> Option<QueueBindingMode> {
    let has_current_control = current.marker_mode.is_some()
        || current.frontmatter_mode.is_some()
        || current.legacy_queue_active.is_some()
        || current.has_auto;
    if !has_current_control && previous.is_none() {
        return None;
    }

    let marker_changed = previous.is_some_and(|prev| current.marker_mode != prev.marker_mode);
    let frontmatter_changed = previous.is_some_and(|prev| {
        current.frontmatter_mode != prev.frontmatter_mode
            || current.legacy_queue_active != prev.legacy_queue_active
    });

    if marker_changed && !frontmatter_changed {
        // A marker token disappearing (drain strip) is a stop, except under an
        // operator `pause`, which only the operator may lift.
        let fallback = if current.frontmatter_mode == Some(QueueBindingMode::Pause) {
            QueueBindingMode::Pause
        } else {
            QueueBindingMode::Stop
        };
        return Some(current.marker_mode.unwrap_or(fallback));
    }
    if frontmatter_changed && !marker_changed {
        // Realtime editor replicas publish intermediate frontmatter states while
        // the operator is typing. An absent or unrecognized value is not a
        // control gesture, so wait for start/go/stop instead of projecting the
        // unchanged marker back into the field and fighting the editor.
        if current.frontmatter_mode.is_none() && current.legacy_queue_active.is_none() {
            return None;
        }
        return current
            .frontmatter_mode
            .or_else(|| {
                current.legacy_queue_active.map(|active| {
                    if active {
                        QueueBindingMode::Start
                    } else {
                        QueueBindingMode::Stop
                    }
                })
            })
            .or(current.marker_mode)
            .or(Some(QueueBindingMode::Stop));
    }
    if marker_changed && frontmatter_changed {
        let marker_target = current.marker_mode.unwrap_or(QueueBindingMode::Stop);
        let frontmatter_target = current
            .frontmatter_mode
            .or_else(|| {
                current.legacy_queue_active.map(|active| {
                    if active {
                        QueueBindingMode::Start
                    } else {
                        QueueBindingMode::Stop
                    }
                })
            })
            .unwrap_or(QueueBindingMode::Stop);
        if marker_target != frontmatter_target {
            // A genuine two-sided edit has no lossless winner. Frontmatter is
            // the canonical durable representation, so it wins deterministically
            // and the conflict is visible instead of becoming silent churn.
            eprintln!(
                "[queue] warning: conflicting queue activation edits \
                 marker={marker_target:?} frontmatter={frontmatter_target:?}; \
                 choosing canonical frontmatter control (#qactsync)"
            );
        }
        return Some(frontmatter_target);
    }
    if let Some(marker_mode) = current.marker_mode {
        return Some(marker_mode);
    }
    if current.has_auto {
        return Some(QueueBindingMode::Start);
    }
    if previous.is_none() {
        return current.frontmatter_mode.or_else(|| {
            current.legacy_queue_active.map(|active| {
                if active {
                    QueueBindingMode::Start
                } else {
                    QueueBindingMode::Stop
                }
            })
        });
    }
    // `#qstartinert`: nothing changed on either side this cycle, and the marker
    // carries no control token. An explicitly authored frontmatter control is
    // still the operator's standing instruction, so honor it rather than
    // converging to `stop`.
    //
    // Collapsing to `stop` here was aimed at the post-drain shape (the token was
    // consumed off the marker), but that transition is a marker CHANGE and is
    // already handled above by `marker_changed`. Reaching this branch means the
    // marker never carried the token at all — the ordinary shape for a document
    // whose operator wrote `queue: start` in frontmatter and left the marker bare,
    // or one whose marker projection failed to persist. Forcing `stop` there
    // silently disarmed the queue on every pass: entries mirrored in, but
    // activation resolved inactive forever and the auto-loop never got a head.
    if let Some(frontmatter_mode) = current.frontmatter_mode {
        return Some(frontmatter_mode);
    }
    // A bare legacy `queue_active` flag with no explicit control is not a standing
    // instruction; keep the pre-existing conservative stop.
    if current.legacy_queue_active.is_some() {
        return Some(QueueBindingMode::Stop);
    }
    None
}

fn set_queue_marker_binding(content: &str, marker_token: Option<&str>) -> Result<String> {
    let components = agent_doc_element::element::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(content.to_string());
    };
    let raw_tag = &content[queue_component.open_start..queue_component.open_end];
    let new_tag = crate::document_queue::set_control_in_tag(raw_tag, marker_token);
    if new_tag == raw_tag {
        return Ok(content.to_string());
    }
    let mut rebuilt = String::with_capacity(content.len());
    rebuilt.push_str(&content[..queue_component.open_start]);
    rebuilt.push_str(&new_tag);
    rebuilt.push_str(&content[queue_component.open_end..]);
    Ok(rebuilt)
}

/// `#queueeditgo`: an operator edit to the queue's prompts is consent to run it.
///
/// When the pre-turn baseline (`snapshot_content`) and the current document
/// differ by at least one live prompt or preset the baseline did not carry
/// (added or reworded — removals and strikes alone are not a request to run),
/// and the operator did not touch the queue control itself this window, the
/// queue is armed to `go` in both the marker and the canonical `queue:` field.
/// `go` rather than `start` because only `go` keeps the drain continuing past
/// the first head.
///
/// `queue: pause` (or `pause` on the marker) is the operator's hold and blocks
/// the inference; `stop`, which the binary itself writes on drain, does not —
/// that was the reported defect: after a drain wrote `stop`, adding a queue item
/// and invoking Run Agent Doc left the item inert.
///
/// Returns `(content, changed)`. Pure; callers own I/O.
pub fn infer_queue_go_from_prompt_edit(
    content: &str,
    snapshot_content: Option<&str>,
) -> Result<(String, bool)> {
    let unchanged = || Ok((content.to_string(), false));
    let Some(snapshot) = snapshot_content else {
        return unchanged();
    };
    let Some(current) = queue_binding_state(content) else {
        return unchanged();
    };
    let Some(previous) = queue_binding_state(snapshot) else {
        return unchanged();
    };
    if current.marker_mode != previous.marker_mode
        || current.frontmatter_mode != previous.frontmatter_mode
        || current.legacy_queue_active != previous.legacy_queue_active
        || current.has_auto != previous.has_auto
    {
        // The operator is steering the control directly; that gesture wins and
        // `converge_queue_control_binding_content` owns projecting it.
        return unchanged();
    }
    let effective = current.marker_mode.or(current.frontmatter_mode);
    if matches!(
        effective,
        Some(QueueBindingMode::Go | QueueBindingMode::Pause)
    ) {
        return unchanged();
    }
    if !queue_prompts_edited(content, snapshot) {
        return unchanged();
    }
    let mut updated = set_queue_marker_binding(content, QueueBindingMode::Go.marker_token())?;
    updated = frontmatter::merge_queue_control(&updated, QueueBindingMode::Go.frontmatter_value())?;
    let changed = updated != content;
    Ok((updated, changed))
}

/// Normalized live queue prompt/preset lines, cosmetic progress and pin
/// markers removed so re-marking an existing item never reads as an edit.
fn live_queue_prompt_keys(content: &str) -> Option<Vec<String>> {
    let components = agent_doc_element::element::parse(content).ok()?;
    let queue_component = components
        .iter()
        .find(|component| component.name == "queue")?;
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = crate::document_queue::parse(body).ok()?;
    Some(
        entries
            .iter()
            .filter_map(|entry| match entry {
                crate::document_queue::QueueEntry::Prompt(prompt) => Some(prompt.text.as_str()),
                crate::document_queue::QueueEntry::Preset(preset) => Some(preset.as_str()),
                _ => None,
            })
            .map(|text| {
                agent_doc_document::queue_projection::strip_priority_markers(
                    &agent_doc_document::queue_projection::strip_in_progress_marker(text),
                )
                .trim()
                .to_string()
            })
            .filter(|text| !text.is_empty())
            .collect(),
    )
}

fn queue_prompts_edited(content: &str, snapshot: &str) -> bool {
    let (Some(current), Some(previous)) = (
        live_queue_prompt_keys(content),
        live_queue_prompt_keys(snapshot),
    ) else {
        return false;
    };
    let mut remaining = previous;
    current
        .iter()
        .any(|key| match remaining.iter().position(|prev| prev == key) {
            Some(index) => {
                remaining.swap_remove(index);
                false
            }
            None => true,
        })
}

pub fn strip_queue_activation_tokens_in_content(content: &str) -> Result<String> {
    let components = agent_doc_element::element::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(content.to_string());
    };
    let raw_tag = &content[queue_component.open_start..queue_component.open_end];
    let new_tag = crate::document_queue::strip_control_from_tag(
        &crate::document_queue::strip_auto_from_tag(raw_tag),
    );
    if new_tag == raw_tag {
        return Ok(content.to_string());
    }
    let mut rebuilt = String::with_capacity(content.len());
    rebuilt.push_str(&content[..queue_component.open_start]);
    rebuilt.push_str(&new_tag);
    rebuilt.push_str(&content[queue_component.open_end..]);
    Ok(rebuilt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_control_projects_to_frontmatter() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );

        let (updated, changed) = converge_queue_control_binding_content(content, None).unwrap();

        assert!(changed);
        assert!(updated.contains("queue: go\n"));
        assert!(updated.contains("<!-- agent:queue go -->"));
    }

    #[test]
    fn snapshot_frontmatter_change_can_stop_marker_control() {
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );
        let content = snapshot.replacen("queue: go", "queue: stop", 1);

        let (updated, changed) =
            converge_queue_control_binding_content(&content, Some(snapshot)).unwrap();

        assert!(changed);
        assert!(updated.contains("queue: stop\n"));
        assert!(updated.contains("<!-- agent:queue -->"));
    }

    #[test]
    fn partial_frontmatter_edit_does_not_restore_marker_control() {
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );
        let content = snapshot.replacen("queue: go", "queue: st", 1);

        let (updated, changed) =
            converge_queue_control_binding_content(&content, Some(snapshot)).unwrap();

        assert!(!changed);
        assert_eq!(updated, content);
    }

    #[test]
    fn rolling_fixed_point_baseline_allows_stop_then_marker_resume() {
        let started = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );
        let stop_gesture = started.replacen("queue: go", "queue: stop", 1);
        let (stopped, stop_changed) =
            converge_queue_control_binding_content(&stop_gesture, Some(started)).unwrap();
        assert!(stop_changed);
        assert!(stopped.contains("queue: stop\n"));
        assert!(stopped.contains("<!-- agent:queue -->"));

        let resume_gesture = stopped.replacen("<!-- agent:queue -->", "<!-- agent:queue go -->", 1);
        let (resumed, resume_changed) =
            converge_queue_control_binding_content(&resume_gesture, Some(&stopped)).unwrap();
        assert!(resume_changed);
        assert!(resumed.contains("queue: go\n"));
        assert!(resumed.contains("<!-- agent:queue go -->"));
    }

    #[test]
    fn marker_gesture_overrides_stale_frontmatter_without_snapshot() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );

        let (updated, changed) = converge_queue_control_binding_content(content, None).unwrap();

        assert!(changed);
        assert!(updated.contains("queue: go\n"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        let components = agent_doc_element::element::parse(&updated).unwrap();
        let queue = components.iter().find(|c| c.name == "queue").unwrap();
        assert!(explicit_queue_go_mode(&queue.attrs, Some("go")));
    }

    #[test]
    fn marker_change_away_from_stop_wins_over_stale_frontmatter() {
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );
        let content = snapshot.replacen("<!-- agent:queue -->", "<!-- agent:queue start -->", 1);

        let (updated, changed) =
            converge_queue_control_binding_content(&content, Some(snapshot)).unwrap();

        assert!(changed);
        assert!(updated.contains("queue: start\n"));
        assert!(updated.contains("<!-- agent:queue start -->"));
    }

    #[test]
    fn simultaneous_conflict_uses_canonical_frontmatter_and_reaches_fixed_point() {
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:queue start -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );
        let content = snapshot
            .replacen("queue: start", "queue: stop", 1)
            .replacen("<!-- agent:queue start -->", "<!-- agent:queue go -->", 1);

        let (updated, changed) =
            converge_queue_control_binding_content(&content, Some(snapshot)).unwrap();

        assert!(changed);
        assert!(updated.contains("queue: stop\n"));
        assert!(updated.contains("<!-- agent:queue -->"));

        let (fixed_point, changed_again) =
            converge_queue_control_binding_content(&updated, Some(snapshot)).unwrap();
        assert!(!changed_again);
        assert_eq!(fixed_point, updated);
    }

    /// `#qstartinert`: a steady-state frontmatter `queue: start` with a bare
    /// marker must stay `start` — convergence must not silently disarm it.
    ///
    /// Live repro (`tasks/brookebrodack-dev.md`): 11 queued heads, `queue: start`,
    /// bare `<!-- agent:queue -->`, snapshot identical. Every pass converged the
    /// operator's `start` to `stop`, so the queue populated but never armed.
    #[test]
    fn unchanged_frontmatter_start_with_bare_marker_stays_started() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );

        // Snapshot identical to content: nothing changed on either side.
        let (updated, _) = converge_queue_control_binding_content(content, Some(content)).unwrap();

        assert!(
            updated.contains("queue: start\n"),
            "operator's standing `queue: start` must survive convergence:\n{updated}"
        );
        assert!(
            !updated.contains("queue: stop"),
            "convergence must not disarm an unchanged operator control:\n{updated}"
        );
        assert!(
            updated.contains("<!-- agent:queue start -->"),
            "the frontmatter control must project onto the marker:\n{updated}"
        );
    }

    /// `#qstartinert` guard: the post-drain shape (marker token consumed, so the
    /// marker CHANGED) must still converge to `stop`.
    #[test]
    fn consumed_marker_token_still_converges_to_stop() {
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:queue start -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );
        // Drain stripped the control token off the marker.
        let content = snapshot.replacen("<!-- agent:queue start -->", "<!-- agent:queue -->", 1);

        let (updated, changed) =
            converge_queue_control_binding_content(&content, Some(snapshot)).unwrap();

        assert!(changed);
        assert!(
            updated.contains("queue: stop\n"),
            "a consumed marker token is a real stop transition:\n{updated}"
        );
    }

    #[test]
    fn strip_activation_tokens_removes_legacy_marker_controls() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "---\n\n",
            "<!-- agent:queue priority auto go -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );

        let stripped = strip_queue_activation_tokens_in_content(content).unwrap();

        assert!(stripped.contains("<!-- agent:queue priority -->"));
    }

    fn queue_doc(control: Option<&str>, marker: &str, items: &[&str]) -> String {
        let mut doc = String::from("---\nagent_doc_session: test\n");
        if let Some(control) = control {
            doc.push_str(&format!("queue: {control}\n"));
        }
        doc.push_str("---\n\n");
        doc.push_str(&format!("<!-- agent:queue{marker} -->\n"));
        for item in items {
            doc.push_str(&format!("- {item}\n"));
        }
        doc.push_str("<!-- /agent:queue -->\n");
        doc
    }

    #[test]
    fn queue_edit_after_drain_stop_arms_go() {
        // #queueeditgo: the drain wrote `queue: stop`; the operator then adds an
        // item and runs Run Agent Doc. The edit itself is consent to run.
        let snapshot = queue_doc(Some("stop"), "", &[]);
        let content = queue_doc(Some("stop"), "", &["fix the thing"]);

        let (updated, changed) =
            infer_queue_go_from_prompt_edit(&content, Some(&snapshot)).unwrap();

        assert!(changed);
        assert!(updated.contains("queue: go\n"), "{updated}");
        assert!(updated.contains("<!-- agent:queue go -->"), "{updated}");
        // Settled: a second pass over the armed document is a no-op, and the
        // binding convergence agrees with it.
        let (again, changed) = infer_queue_go_from_prompt_edit(&updated, Some(&snapshot)).unwrap();
        assert!(!changed);
        assert_eq!(again, updated);
    }

    #[test]
    fn queue_edit_with_no_control_arms_go() {
        let snapshot = queue_doc(None, "", &["old"]);
        let content = queue_doc(None, "", &["old reworded"]);

        let (updated, changed) =
            infer_queue_go_from_prompt_edit(&content, Some(&snapshot)).unwrap();

        assert!(changed);
        assert!(updated.contains("queue: go\n"), "{updated}");
    }

    #[test]
    fn queue_edit_under_start_upgrades_to_go() {
        let snapshot = queue_doc(Some("start"), "", &["a"]);
        let content = queue_doc(Some("start"), "", &["a", "b"]);

        let (updated, changed) =
            infer_queue_go_from_prompt_edit(&content, Some(&snapshot)).unwrap();

        assert!(changed);
        assert!(updated.contains("queue: go\n"), "{updated}");
    }

    #[test]
    fn queue_edit_under_pause_stays_paused() {
        let snapshot = queue_doc(Some("pause"), "", &[]);
        let content = queue_doc(Some("pause"), "", &["held item"]);

        let (updated, changed) =
            infer_queue_go_from_prompt_edit(&content, Some(&snapshot)).unwrap();

        assert!(!changed);
        assert_eq!(updated, content);
        let (fm, _) = frontmatter::parse(&content).unwrap();
        assert!(explicit_queue_stop_mode(
            &HashMap::new(),
            fm.queue.as_deref()
        ));
        assert!(explicit_queue_pause_mode(
            &HashMap::new(),
            fm.queue.as_deref()
        ));
    }

    #[test]
    fn queue_removal_or_marker_only_edit_does_not_arm() {
        // Removing a head (what the operator did to `#advance-review`) or only
        // re-marking an item is not a request to run.
        let snapshot = queue_doc(Some("stop"), "", &["\u{1f6a7} a", "b"]);
        for content in [
            queue_doc(Some("stop"), "", &["b"]),
            queue_doc(Some("stop"), "", &["a", "\u{1f4cc} b"]),
        ] {
            let (_, changed) = infer_queue_go_from_prompt_edit(&content, Some(&snapshot)).unwrap();
            assert!(!changed, "{content}");
        }
    }

    #[test]
    fn queue_edit_beside_control_gesture_defers_to_the_gesture() {
        // The operator stopped the queue AND added an item in one window: the
        // explicit control wins.
        let snapshot = queue_doc(Some("go"), " go", &[]);
        let content = queue_doc(Some("stop"), "", &["later"]);

        let (_, changed) = infer_queue_go_from_prompt_edit(&content, Some(&snapshot)).unwrap();

        assert!(!changed);
    }

    #[test]
    fn drained_marker_does_not_lift_operator_pause() {
        let snapshot = queue_doc(Some("pause"), " go", &["a"]);
        let content = queue_doc(Some("pause"), "", &["a"]);

        let (updated, _) =
            converge_queue_control_binding_content(&content, Some(&snapshot)).unwrap();

        assert!(updated.contains("queue: pause\n"), "{updated}");
    }

    #[test]
    fn queue_edit_without_baseline_is_inert() {
        let content = queue_doc(Some("stop"), "", &["x"]);
        let (_, changed) = infer_queue_go_from_prompt_edit(&content, None).unwrap();
        assert!(!changed);
    }
}
