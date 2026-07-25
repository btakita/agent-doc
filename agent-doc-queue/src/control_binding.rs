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

pub fn explicit_queue_stop_mode(
    attrs: &HashMap<String, String>,
    frontmatter_queue: Option<&str>,
) -> bool {
    resolved_queue_binding(attrs, frontmatter_queue) == Some(QueueBindingMode::Stop)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueBindingMode {
    Start,
    Go,
    Stop,
}

impl QueueBindingMode {
    fn from_frontmatter(raw: Option<&str>) -> Option<Self> {
        match raw?.trim().to_ascii_lowercase().as_str() {
            "start" => Some(Self::Start),
            "go" => Some(Self::Go),
            "stop" => Some(Self::Stop),
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
        }
    }

    fn marker_token(self) -> Option<&'static str> {
        match self {
            Self::Start => Some("start"),
            Self::Go => Some("go"),
            Self::Stop => None,
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
        return Some(current.marker_mode.unwrap_or(QueueBindingMode::Stop));
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
}
