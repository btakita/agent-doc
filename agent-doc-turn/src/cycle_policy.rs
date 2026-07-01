//! Pure cycle-state policy helpers.
//!
//! Orchestration owns sidecar IO and timestamps. This module owns stable
//! vocabulary and normalization rules used while recording a turn checkpoint.

use agent_doc_element_backlog::backlog::normalize_pending_id;

/// Commit events that represent a stable terminal commit observation.
pub fn is_stable_commit_event(event: &str) -> bool {
    matches!(
        event,
        "commit" | "commit_success" | "commit_already_current"
    )
}

/// A no-op commit event: closeout found the snapshot already equal to `HEAD`,
/// so this cycle committed no new binary-owned work this turn.
pub fn is_noop_commit_event(event: &str) -> bool {
    event == "commit_already_current"
}

/// Normalize a checkpoint task identifier to the durable `#id` form.
pub fn normalize_checkpoint_task_id(id: &str) -> String {
    let normalized = normalize_pending_id(id);
    if normalized.is_empty() {
        String::new()
    } else {
        format!("#{normalized}")
    }
}

/// Trim, drop empty values, and preserve the first occurrence of each text item.
pub fn normalize_checkpoint_text_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .fold(Vec::new(), |mut acc, value| {
            if !acc.iter().any(|existing| existing == &value) {
                acc.push(value);
            }
            acc
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_commit_events_cover_success_and_noop_commit_vocabulary() {
        for event in ["commit", "commit_success", "commit_already_current"] {
            assert!(
                is_stable_commit_event(event),
                "{event} should be treated as a stable commit event"
            );
        }
        for event in ["", "write_template", "repair_applied", "synthetic_state"] {
            assert!(
                !is_stable_commit_event(event),
                "{event} should not be treated as a stable commit event"
            );
        }
    }

    #[test]
    fn noop_commit_event_is_only_already_current() {
        assert!(is_noop_commit_event("commit_already_current"));
        assert!(!is_noop_commit_event("commit"));
        assert!(!is_noop_commit_event("commit_success"));
    }

    #[test]
    fn checkpoint_task_id_normalizes_to_hash_prefixed_pending_id() {
        assert_eq!(normalize_checkpoint_task_id("abc"), "#abc");
        assert_eq!(normalize_checkpoint_task_id("#abc"), "#abc");
        assert_eq!(normalize_checkpoint_task_id("  #abc  "), "#abc");
        assert_eq!(normalize_checkpoint_task_id("  "), "");
    }

    #[test]
    fn checkpoint_text_list_trims_drops_empty_and_deduplicates_in_order() {
        let values = vec![
            "  #a  ".to_string(),
            "".to_string(),
            "#b".to_string(),
            "#a".to_string(),
            "  #c".to_string(),
        ];

        assert_eq!(
            normalize_checkpoint_text_list(&values),
            vec!["#a".to_string(), "#b".to_string(), "#c".to_string()]
        );
    }
}
