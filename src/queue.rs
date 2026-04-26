//! # Module: queue
//!
//! Pure functions for parsing and mutating the `agent:queue` component body.
//!
//! Hybrid syntax:
//! - `- text` → single-line prompt
//! - `~~~prompt` / `---` → multi-line prompt fence
//! - `--- start [at <datetime>]` / `~~~start` → start fence (activation signal)
//! - `--- stop` / `~~~stop` → stop fence (breakpoint)
//!
//! Activation resolution (Phase 2):
//! - `resolve_activation()` determines whether the queue should be active
//!   based on (in priority order): `auto` attribute, inline start fence,
//!   exchange trigger (`do queue`/`run queue`), persisted `queue_active` state.
//!
//! Halt detection (Phase 3):
//! - `detect_head_prompt_modified()` compares the head prompt between snapshot
//!   and file to detect user edits between cycles.
//! - Stop fence at head → halt signal for preflight.
//!
//! This module is I/O-free. Callers handle reading/writing files.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueEntry {
    Prompt(QueuePrompt),
    StartFence(Option<String>),
    StopFence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuePrompt {
    pub text: String,
    pub multiline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueComponent {
    pub auto: bool,
    pub entries: Vec<QueueEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueTrigger {
    Auto,
    StartFence,
    ExchangeRequest,
    Persisted,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueueActivation {
    pub active: bool,
    pub trigger: Option<QueueTrigger>,
    pub deferred: bool,
    pub start_at: Option<String>,
    pub consumed_start_fence: bool,
    pub entries_after: Vec<QueueEntry>,
}

/// Resolve whether the queue should be activated this cycle.
///
/// Priority order:
/// 1. `auto` attribute on `<!-- agent:queue auto -->` → immediate if prompts exist
/// 2. Start fence at head of entries → consume bare `--- start`; defer `--- start at <time>`
/// 3. Exchange trigger (`do queue` / `run queue` detected in diff)
/// 4. Persisted `queue_active: true` from frontmatter
///
/// Returns the resolved activation state including whether a start fence was consumed.
pub fn resolve_activation(
    entries: &[QueueEntry],
    has_auto: bool,
    exchange_triggered: bool,
    persisted_active: bool,
) -> QueueActivation {
    let has_prompts = !prompts(entries).is_empty();

    // Priority 1: auto attribute
    if has_auto && has_prompts {
        return QueueActivation {
            active: true,
            trigger: Some(QueueTrigger::Auto),
            entries_after: entries.to_vec(),
            ..Default::default()
        };
    }

    // Priority 2: start fence at head
    if let Some(QueueEntry::StartFence(datetime)) = entries.first() {
        if let Some(dt) = datetime {
            return QueueActivation {
                deferred: true,
                start_at: Some(dt.clone()),
                entries_after: entries.to_vec(),
                ..Default::default()
            };
        } else {
            let remaining: Vec<QueueEntry> = entries[1..].to_vec();
            let has_remaining_prompts = !prompts(&remaining).is_empty();
            return QueueActivation {
                active: has_remaining_prompts,
                trigger: if has_remaining_prompts {
                    Some(QueueTrigger::StartFence)
                } else {
                    None
                },
                consumed_start_fence: true,
                entries_after: remaining,
                ..Default::default()
            };
        }
    }

    // Priority 3: exchange trigger
    if exchange_triggered && has_prompts {
        return QueueActivation {
            active: true,
            trigger: Some(QueueTrigger::ExchangeRequest),
            entries_after: entries.to_vec(),
            ..Default::default()
        };
    }

    // Priority 4: persisted active state
    if persisted_active && has_prompts {
        return QueueActivation {
            active: true,
            trigger: Some(QueueTrigger::Persisted),
            entries_after: entries.to_vec(),
            ..Default::default()
        };
    }

    QueueActivation {
        entries_after: entries.to_vec(),
        ..Default::default()
    }
}

/// Reconstruct an `<!-- agent:queue -->` opening tag without the `auto` attribute.
pub fn strip_auto_from_tag(tag: &str) -> String {
    tag.replace(" auto", "")
}

pub fn parse(body: &str) -> Result<Vec<QueueEntry>> {
    let mut entries = Vec::new();
    let mut lines = body.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if line.starts_with("- ") {
            entries.push(QueueEntry::Prompt(QueuePrompt {
                text: line[2..].to_string(),
                multiline: false,
            }));
            continue;
        }

        if is_start_fence(trimmed) {
            let datetime = parse_start_datetime(trimmed);
            entries.push(QueueEntry::StartFence(datetime));
            continue;
        }

        if is_stop_fence(trimmed) {
            entries.push(QueueEntry::StopFence);
            continue;
        }

        if is_prompt_fence_open(trimmed) {
            let closer = fence_closer(trimmed);
            let mut prompt_lines = Vec::new();
            let mut found_close = false;
            while let Some(inner) = lines.next() {
                if inner.trim() == closer {
                    found_close = true;
                    break;
                }
                prompt_lines.push(inner);
            }
            if !found_close {
                bail!("unclosed prompt fence: {}", trimmed);
            }
            let text = prompt_lines.join("\n");
            if !text.trim().is_empty() {
                entries.push(QueueEntry::Prompt(QueuePrompt {
                    text,
                    multiline: true,
                }));
            }
            continue;
        }

        if is_bare_fence_open(trimmed) {
            let mut prompt_lines = Vec::new();
            let mut found_close = false;
            while let Some(inner) = lines.next() {
                if inner.trim() == "---" {
                    found_close = true;
                    break;
                }
                prompt_lines.push(inner);
            }
            if !found_close {
                bail!("unclosed --- fence");
            }
            let text = prompt_lines.join("\n");
            if !text.trim().is_empty() {
                entries.push(QueueEntry::Prompt(QueuePrompt {
                    text,
                    multiline: true,
                }));
            }
            continue;
        }

        bail!("unexpected content in agent:queue: {:?}", trimmed);
    }

    Ok(entries)
}

pub fn render(entries: &[QueueEntry]) -> String {
    let mut out = String::new();
    for entry in entries {
        match entry {
            QueueEntry::Prompt(p) => {
                if p.multiline {
                    out.push_str("---\n");
                    out.push_str(&p.text);
                    if !p.text.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("---\n");
                } else {
                    out.push_str("- ");
                    out.push_str(&p.text);
                    out.push('\n');
                }
            }
            QueueEntry::StartFence(dt) => {
                if let Some(datetime) = dt {
                    out.push_str(&format!("--- start at {}\n", datetime));
                } else {
                    out.push_str("--- start\n");
                }
            }
            QueueEntry::StopFence => {
                out.push_str("--- stop\n");
            }
        }
    }
    out
}

fn is_start_fence(line: &str) -> bool {
    line == "--- start"
        || line.starts_with("--- start ")
        || line == "~~~start"
}

fn parse_start_datetime(line: &str) -> Option<String> {
    if line == "--- start" || line == "~~~start" {
        return None;
    }
    let rest = line.strip_prefix("--- start ")?;
    let rest = rest.strip_prefix("at ").unwrap_or(rest);
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_stop_fence(line: &str) -> bool {
    line == "--- stop" || line == "~~~stop"
}

fn is_prompt_fence_open(line: &str) -> bool {
    line == "~~~prompt"
}

fn fence_closer(_open: &str) -> &'static str {
    "~~~"
}

fn is_bare_fence_open(line: &str) -> bool {
    line == "---" && !line.starts_with("--- ")
}

pub fn has_auto_attr(attrs: &std::collections::HashMap<String, String>) -> bool {
    attrs.contains_key("auto")
}

pub fn prompts(entries: &[QueueEntry]) -> Vec<&QueuePrompt> {
    entries
        .iter()
        .filter_map(|e| match e {
            QueueEntry::Prompt(p) => Some(p),
            _ => None,
        })
        .collect()
}

pub fn first_prompt(entries: &[QueueEntry]) -> Option<&QueuePrompt> {
    entries.iter().find_map(|e| match e {
        QueueEntry::Prompt(p) => Some(p),
        _ => None,
    })
}

pub fn remove_first_prompt(entries: &[QueueEntry]) -> Vec<QueueEntry> {
    let mut result = Vec::with_capacity(entries.len());
    let mut removed = false;
    for entry in entries {
        if !removed {
            if let QueueEntry::Prompt(_) = entry {
                removed = true;
                continue;
            }
        }
        result.push(entry.clone());
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueHaltReason {
    StopFence,
    ItemModified,
}

/// Check if the head prompt has been modified between snapshot and file entries.
///
/// Compares the text of the first `Prompt` entry in each list. Returns `true`
/// if either list has no prompt or the texts differ.
///
/// This is the "stop-on-change" mechanism: if the user edits the next-to-consume
/// item between cycles, the queue halts immediately.
pub fn detect_head_prompt_modified(
    snapshot_entries: &[QueueEntry],
    file_entries: &[QueueEntry],
) -> bool {
    let snap_head = first_prompt(snapshot_entries);
    let file_head = first_prompt(file_entries);
    match (snap_head, file_head) {
        (Some(s), Some(f)) => s.text != f.text,
        (None, None) => false,
        _ => true,
    }
}

/// Check if a stop fence is at the head of the entries (before any prompt).
pub fn has_stop_fence_at_head(entries: &[QueueEntry]) -> bool {
    matches!(entries.first(), Some(QueueEntry::StopFence))
}

/// Check if a time-gated start fence is at the head of the entries.
/// Returns `Some(datetime)` if a time gate is found.
pub fn time_gate_at_head(entries: &[QueueEntry]) -> Option<&str> {
    match entries.first() {
        Some(QueueEntry::StartFence(Some(dt))) => Some(dt.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_line_items() {
        let body = "- do #fix1\n- do #fix2\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            })
        );
        assert_eq!(
            entries[1],
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix2".to_string(),
                multiline: false,
            })
        );
        assert_eq!(
            entries[2],
            QueueEntry::Prompt(QueuePrompt {
                text: "run tests".to_string(),
                multiline: false,
            })
        );
    }

    #[test]
    fn parse_multiline_tilde_prompt() {
        let body = "~~~prompt\nReview the changes in src/.\nCheck for edge cases.\n~~~\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "Review the changes in src/.\nCheck for edge cases.".to_string(),
                multiline: true,
            })
        );
    }

    #[test]
    fn parse_multiline_dash_prompt() {
        let body = "---\nReview the changes.\nThen run cargo test.\n---\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "Review the changes.\nThen run cargo test.".to_string(),
                multiline: true,
            })
        );
    }

    #[test]
    fn parse_mixed_syntax() {
        let body = "- do #fix1\n---\nReview changes.\n---\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], QueueEntry::Prompt(p) if !p.multiline));
        assert!(matches!(&entries[1], QueueEntry::Prompt(p) if p.multiline));
        assert!(matches!(&entries[2], QueueEntry::Prompt(p) if !p.multiline));
    }

    #[test]
    fn parse_empty_queue() {
        let body = "";
        let entries = parse(body).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_blank_lines_only() {
        let body = "\n\n\n";
        let entries = parse(body).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_start_fence_bare() {
        let body = "--- start\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], QueueEntry::StartFence(None));
    }

    #[test]
    fn parse_start_fence_with_time() {
        let body = "--- start at 17:00 ET\n- run nightly tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::StartFence(Some("17:00 ET".to_string()))
        );
    }

    #[test]
    fn parse_start_fence_without_at() {
        let body = "--- start 17:00 ET\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::StartFence(Some("17:00 ET".to_string()))
        );
    }

    #[test]
    fn parse_tilde_start_fence() {
        let body = "~~~start\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], QueueEntry::StartFence(None));
    }

    #[test]
    fn parse_stop_fence() {
        let body = "- do #fix1\n--- stop\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1], QueueEntry::StopFence);
    }

    #[test]
    fn parse_tilde_stop_fence() {
        let body = "- do #fix1\n~~~stop\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1], QueueEntry::StopFence);
    }

    #[test]
    fn parse_multiple_time_gates() {
        let body = "- do #fix1\n--- start 17:00 ET\n- run nightly\n--- start 18:00 ET\n- coverage\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(
            entries[1],
            QueueEntry::StartFence(Some("17:00 ET".to_string()))
        );
        assert_eq!(
            entries[3],
            QueueEntry::StartFence(Some("18:00 ET".to_string()))
        );
    }

    #[test]
    fn parse_auto_attribute() {
        let mut attrs = std::collections::HashMap::new();
        attrs.insert("auto".to_string(), String::new());
        assert!(has_auto_attr(&attrs));

        let empty = std::collections::HashMap::new();
        assert!(!has_auto_attr(&empty));
    }

    #[test]
    fn parse_error_on_unexpected_content() {
        let body = "random text that is not a list item or fence\n";
        assert!(parse(body).is_err());
    }

    #[test]
    fn parse_error_on_unclosed_fence() {
        let body = "~~~prompt\nSome content without closing fence\n";
        assert!(parse(body).is_err());
    }

    #[test]
    fn parse_error_on_unclosed_dash_fence() {
        let body = "---\nContent without closing dashes\n";
        assert!(parse(body).is_err());
    }

    #[test]
    fn render_single_line() {
        let entries = vec![QueueEntry::Prompt(QueuePrompt {
            text: "do #fix1".to_string(),
            multiline: false,
        })];
        assert_eq!(render(&entries), "- do #fix1\n");
    }

    #[test]
    fn render_multiline() {
        let entries = vec![QueueEntry::Prompt(QueuePrompt {
            text: "Review changes.\nRun tests.".to_string(),
            multiline: true,
        })];
        assert_eq!(render(&entries), "---\nReview changes.\nRun tests.\n---\n");
    }

    #[test]
    fn render_start_fence() {
        let entries = vec![QueueEntry::StartFence(None)];
        assert_eq!(render(&entries), "--- start\n");
    }

    #[test]
    fn render_start_fence_with_time() {
        let entries = vec![QueueEntry::StartFence(Some("17:00 ET".to_string()))];
        assert_eq!(render(&entries), "--- start at 17:00 ET\n");
    }

    #[test]
    fn render_stop_fence() {
        let entries = vec![QueueEntry::StopFence];
        assert_eq!(render(&entries), "--- stop\n");
    }

    #[test]
    fn render_mixed() {
        let entries = vec![
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            }),
            QueueEntry::StopFence,
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix2".to_string(),
                multiline: false,
            }),
        ];
        assert_eq!(render(&entries), "- do #fix1\n--- stop\n- do #fix2\n");
    }

    #[test]
    fn roundtrip_preserves_structure() {
        let body = "- do #fix1\n--- start at 17:00 ET\n- run nightly\n--- stop\n- do #fix2\n";
        let entries = parse(body).unwrap();
        let rendered = render(&entries);
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(entries, reparsed);
    }

    #[test]
    fn prompts_filters_to_prompt_entries() {
        let entries = vec![
            QueueEntry::StartFence(None),
            QueueEntry::Prompt(QueuePrompt {
                text: "task1".to_string(),
                multiline: false,
            }),
            QueueEntry::StopFence,
            QueueEntry::Prompt(QueuePrompt {
                text: "task2".to_string(),
                multiline: false,
            }),
        ];
        let p = prompts(&entries);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].text, "task1");
        assert_eq!(p[1].text, "task2");
    }

    #[test]
    fn first_prompt_skips_control_fences() {
        let entries = vec![
            QueueEntry::StartFence(None),
            QueueEntry::Prompt(QueuePrompt {
                text: "task1".to_string(),
                multiline: false,
            }),
        ];
        assert_eq!(first_prompt(&entries).unwrap().text, "task1");
    }

    #[test]
    fn first_prompt_none_when_empty() {
        let entries: Vec<QueueEntry> = vec![];
        assert!(first_prompt(&entries).is_none());
    }

    #[test]
    fn remove_first_prompt_preserves_control_fences() {
        let entries = vec![
            QueueEntry::StartFence(None),
            QueueEntry::Prompt(QueuePrompt {
                text: "task1".to_string(),
                multiline: false,
            }),
            QueueEntry::Prompt(QueuePrompt {
                text: "task2".to_string(),
                multiline: false,
            }),
        ];
        let result = remove_first_prompt(&entries);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], QueueEntry::StartFence(None));
        assert!(matches!(&result[1], QueueEntry::Prompt(p) if p.text == "task2"));
    }

    #[test]
    fn empty_multiline_fence_is_skipped() {
        let body = "---\n\n---\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            })
        );
    }

    #[test]
    fn blank_lines_between_items_ignored() {
        let body = "- do #fix1\n\n\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
    }

    // --- Activation resolution tests ---

    fn make_prompt(text: &str) -> QueueEntry {
        QueueEntry::Prompt(QueuePrompt {
            text: text.to_string(),
            multiline: false,
        })
    }

    #[test]
    fn activation_auto_with_prompts() {
        let entries = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let act = resolve_activation(&entries, true, false, false);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::Auto));
        assert!(!act.consumed_start_fence);
        assert_eq!(act.entries_after.len(), 2);
    }

    #[test]
    fn activation_auto_empty_queue() {
        let entries: Vec<QueueEntry> = vec![];
        let act = resolve_activation(&entries, true, false, false);
        assert!(!act.active);
        assert!(act.trigger.is_none());
    }

    #[test]
    fn activation_start_fence_bare() {
        let entries = vec![
            QueueEntry::StartFence(None),
            make_prompt("do #fix1"),
        ];
        let act = resolve_activation(&entries, false, false, false);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::StartFence));
        assert!(act.consumed_start_fence);
        assert_eq!(act.entries_after.len(), 1);
        assert!(matches!(&act.entries_after[0], QueueEntry::Prompt(p) if p.text == "do #fix1"));
    }

    #[test]
    fn activation_start_fence_bare_no_prompts_after() {
        let entries = vec![QueueEntry::StartFence(None)];
        let act = resolve_activation(&entries, false, false, false);
        assert!(!act.active);
        assert!(act.trigger.is_none());
        assert!(act.consumed_start_fence);
        assert!(act.entries_after.is_empty());
    }

    #[test]
    fn activation_start_fence_with_time_defers() {
        let entries = vec![
            QueueEntry::StartFence(Some("17:00 ET".to_string())),
            make_prompt("run nightly"),
        ];
        let act = resolve_activation(&entries, false, false, false);
        assert!(!act.active);
        assert!(act.deferred);
        assert_eq!(act.start_at, Some("17:00 ET".to_string()));
        assert!(!act.consumed_start_fence);
        assert_eq!(act.entries_after.len(), 2);
    }

    #[test]
    fn activation_exchange_trigger() {
        let entries = vec![make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, true, false);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::ExchangeRequest));
    }

    #[test]
    fn activation_exchange_trigger_empty_queue() {
        let entries: Vec<QueueEntry> = vec![];
        let act = resolve_activation(&entries, false, true, false);
        assert!(!act.active);
    }

    #[test]
    fn activation_persisted_active() {
        let entries = vec![make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, false, true);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::Persisted));
    }

    #[test]
    fn activation_persisted_empty_queue() {
        let entries: Vec<QueueEntry> = vec![];
        let act = resolve_activation(&entries, false, false, true);
        assert!(!act.active);
    }

    #[test]
    fn activation_auto_takes_precedence_over_exchange() {
        let entries = vec![make_prompt("task")];
        let act = resolve_activation(&entries, true, true, false);
        assert_eq!(act.trigger, Some(QueueTrigger::Auto));
    }

    #[test]
    fn activation_start_fence_takes_precedence_over_exchange() {
        let entries = vec![QueueEntry::StartFence(None), make_prompt("task")];
        let act = resolve_activation(&entries, false, true, false);
        assert_eq!(act.trigger, Some(QueueTrigger::StartFence));
        assert!(act.consumed_start_fence);
    }

    #[test]
    fn activation_none_when_no_triggers() {
        let entries = vec![make_prompt("task")];
        let act = resolve_activation(&entries, false, false, false);
        assert!(!act.active);
        assert!(act.trigger.is_none());
    }

    #[test]
    fn strip_auto_from_tag_removes_auto() {
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue auto -->"),
            "<!-- agent:queue -->"
        );
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue auto patch=append -->"),
            "<!-- agent:queue patch=append -->"
        );
    }

    #[test]
    fn strip_auto_from_tag_noop_without_auto() {
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue -->"),
            "<!-- agent:queue -->"
        );
    }

    // --- Phase 3: halt detection tests ---

    #[test]
    fn detect_head_modified_same_prompt() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_different_prompt() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix1 EDITED"), make_prompt("do #fix2")];
        assert!(detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_prompt_removed() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix2")];
        assert!(detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_both_empty() {
        let snap: Vec<QueueEntry> = vec![];
        let file: Vec<QueueEntry> = vec![];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_snap_empty_file_has() {
        let snap: Vec<QueueEntry> = vec![];
        let file = vec![make_prompt("new item")];
        assert!(detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_ignores_later_changes() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix1"), make_prompt("do #fix2 EDITED")];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_skips_control_fences() {
        let snap = vec![QueueEntry::StopFence, make_prompt("task")];
        let file = vec![QueueEntry::StopFence, make_prompt("task")];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn stop_fence_at_head_detected() {
        let entries = vec![QueueEntry::StopFence, make_prompt("task")];
        assert!(has_stop_fence_at_head(&entries));
    }

    #[test]
    fn stop_fence_not_at_head() {
        let entries = vec![make_prompt("task"), QueueEntry::StopFence];
        assert!(!has_stop_fence_at_head(&entries));
    }

    #[test]
    fn stop_fence_empty_entries() {
        let entries: Vec<QueueEntry> = vec![];
        assert!(!has_stop_fence_at_head(&entries));
    }

    #[test]
    fn time_gate_at_head_detected() {
        let entries = vec![
            QueueEntry::StartFence(Some("17:00 ET".to_string())),
            make_prompt("task"),
        ];
        assert_eq!(time_gate_at_head(&entries), Some("17:00 ET"));
    }

    #[test]
    fn time_gate_at_head_bare_start_not_time_gate() {
        let entries = vec![QueueEntry::StartFence(None), make_prompt("task")];
        assert_eq!(time_gate_at_head(&entries), None);
    }

    #[test]
    fn time_gate_at_head_prompt_first() {
        let entries = vec![
            make_prompt("task"),
            QueueEntry::StartFence(Some("17:00 ET".to_string())),
        ];
        assert_eq!(time_gate_at_head(&entries), None);
    }
}
