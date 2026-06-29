//! Pure supervisor policy for agent harness changes.
//!
//! The controller/runtime detects that the document frontmatter now resolves to a
//! different harness. This module decides when that should restart the supervised
//! agent without touching panes, documents, or IPC.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentChangeRestartAction {
    /// No harness change, the knob is off, or nothing to do.
    None,
    /// Harness changed + knob on + the pane is at a quiet dispatch-ready prompt.
    Restart,
    /// Harness changed + knob on, but the pane is not at a quiet boundary yet.
    WaitForBoundary,
}

pub fn agent_change_restart_decision(
    harness_changed: bool,
    knob_on: bool,
    prompt_visible: bool,
    turn_active: bool,
) -> AgentChangeRestartAction {
    if !harness_changed || !knob_on {
        return AgentChangeRestartAction::None;
    }
    if !prompt_visible || turn_active {
        return AgentChangeRestartAction::WaitForBoundary;
    }
    AgentChangeRestartAction::Restart
}

pub fn harness_change_forces_fresh_spawn(running_binary: &str, resolved_binary: &str) -> bool {
    running_binary != resolved_binary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_change_restart_decision_policy() {
        use AgentChangeRestartAction as A;

        assert_eq!(
            agent_change_restart_decision(false, true, true, false),
            A::None
        );
        assert_eq!(
            agent_change_restart_decision(true, false, true, false),
            A::None
        );
        assert_eq!(
            agent_change_restart_decision(true, true, true, false),
            A::Restart
        );
        assert_eq!(
            agent_change_restart_decision(true, true, true, true),
            A::WaitForBoundary
        );
        assert_eq!(
            agent_change_restart_decision(true, true, false, false),
            A::WaitForBoundary
        );
    }

    #[test]
    fn harness_change_forces_fresh_spawn_predicate() {
        assert!(!harness_change_forces_fresh_spawn("claude", "claude"));
        assert!(!harness_change_forces_fresh_spawn("codex", "codex"));
        assert!(!harness_change_forces_fresh_spawn("opencode", "opencode"));

        assert!(harness_change_forces_fresh_spawn("claude", "opencode"));
        assert!(harness_change_forces_fresh_spawn("claude", "codex"));
        assert!(harness_change_forces_fresh_spawn("codex", "claude"));
    }

    #[test]
    fn restart_action_is_the_phase1b_trigger_value() {
        use AgentChangeRestartAction as A;

        let should_request_restart = |d: A| matches!(d, A::Restart);
        assert!(should_request_restart(agent_change_restart_decision(
            true, true, true, false
        )));
        assert!(!should_request_restart(agent_change_restart_decision(
            true, true, true, true
        )));
        assert!(!should_request_restart(agent_change_restart_decision(
            true, true, false, false
        )));
        assert!(!should_request_restart(agent_change_restart_decision(
            false, true, true, false
        )));
        assert!(!should_request_restart(agent_change_restart_decision(
            true, false, true, false
        )));
    }
}
