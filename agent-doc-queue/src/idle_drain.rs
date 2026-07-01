//! Pure idle-queue payload selection.
//!
//! Orchestration decides how to render a harness trigger command. This module
//! only decides whether an active queue head should submit that trigger or a
//! literal slash command already present at the head.

pub fn idle_queue_head_slash_command(active_head: &str) -> Option<String> {
    crate::queue_command::slash_command_text(active_head)
}

pub fn idle_queue_drain_payload(active_head: &str, trigger_command: impl Into<String>) -> String {
    idle_queue_head_slash_command(active_head).unwrap_or_else(|| trigger_command.into())
}

pub fn idle_queue_drain_payload_kind(active_head: &str) -> &'static str {
    if idle_queue_head_slash_command(active_head).is_some() {
        "slash_command"
    } else {
        "trigger"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_queue_drain_payload_uses_trigger_for_ordinary_head() {
        let payload = idle_queue_drain_payload(
            "JB Run Agent Doc on sampleorders.md stalled.",
            "agent-doc tasks/sampleorders.md",
        );

        assert_eq!(payload, "agent-doc tasks/sampleorders.md");
        assert!(!payload.contains("JB Run Agent Doc on sampleorders.md stalled."));
        assert_eq!(
            idle_queue_drain_payload_kind("JB Run Agent Doc on sampleorders.md stalled."),
            "trigger"
        );
    }

    #[test]
    fn idle_queue_drain_payload_submits_literal_clear_command() {
        assert_eq!(
            idle_queue_drain_payload("  /clear  ", "agent-doc tasks/sampleorders.md"),
            "/clear"
        );
        assert_eq!(idle_queue_drain_payload_kind("/clear"), "slash_command");
    }

    #[test]
    fn idle_queue_drain_payload_submits_any_literal_slash_command() {
        assert_eq!(
            idle_queue_drain_payload("/model sonnet", "agent-doc tasks/sampleorders.md"),
            "/model sonnet"
        );
        assert_eq!(
            idle_queue_head_slash_command(" /model sonnet "),
            Some("/model sonnet".to_string())
        );
    }
}
