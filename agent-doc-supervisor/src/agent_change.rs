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
    /// Harness changed + knob on + a quiet boundary, but the only evidence for
    /// the change is a non-authoritative view of the document
    /// (`#harnessswitchstorm`). Surface the detection; do not respawn.
    AwaitAuthoritativeView,
}

/// Route-time policy for an explicit frontmatter harness change while the old
/// harness still owns a healthy live actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentChangeRouteAction {
    /// Accept the route as a pending handoff. The existing idle-watch boundary
    /// restart owns the eventual fresh spawn and document auto-trigger.
    AcceptBoundaryHandoff,
    /// Preserve the handoff, but require the operator-owned queue pause to be
    /// released before the boundary can run.
    HoldForQueueResume,
    /// The automatic handoff is disabled, so accepting it would silently lose
    /// the requested harness change.
    RejectRestartDisabled,
}

impl AgentChangeRouteAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::AcceptBoundaryHandoff => "accept_boundary_handoff",
            Self::HoldForQueueResume => "hold_for_queue_resume",
            Self::RejectRestartDisabled => "reject_restart_disabled",
        }
    }
}

/// Decide how route should surface a live-authority harness mismatch.
///
/// Stale/unhealthy/closed actors are filtered by the route runtime replacement
/// policy before this function is called. This boundary owns only the healthy
/// live actor case, where route must never inject the old harness.
pub const fn agent_change_route_decision(
    restart_enabled: bool,
    queue_paused: bool,
) -> AgentChangeRouteAction {
    if !restart_enabled {
        return AgentChangeRouteAction::RejectRestartDisabled;
    }
    if queue_paused {
        return AgentChangeRouteAction::HoldForQueueResume;
    }
    AgentChangeRouteAction::AcceptBoundaryHandoff
}

pub fn agent_change_restart_decision(
    harness_changed: bool,
    knob_on: bool,
    prompt_visible: bool,
    turn_active: bool,
) -> AgentChangeRestartAction {
    agent_change_restart_decision_from_view(harness_changed, knob_on, prompt_visible, turn_active, true)
}

/// `#harnessswitchstorm`: the same policy, plus the question of whether the
/// detected change is trustworthy enough to kill and respawn the operator's
/// agent on.
///
/// The detector and the restart executor must resolve `agent:` from the same
/// authority. When the detector falls back to a disk read while a live editor
/// holds an unsaved `agent:` edit, the two disagree — and the disagreement is
/// self-sustaining, because the executor re-resolves the live value, finds no
/// change (`agent_restart_respec_inert`), relaunches anyway, and the relaunch
/// resets the detector's per-thread dedupe. Observed 2026-08-23 on
/// `haiven-dev/tasks/backend.md`: an operator switched codex -> claude and hit
/// Restart Agent, and the supervisor spawned Claude Code six times in 30s
/// (`harness_change_detected old=claude new=codex` every ~5s) until the editor
/// buffer happened to reach disk. Each spawn lands in the operator's live pane,
/// so the loop destroyed a session at 70% context.
///
/// A restart is destructive and unrecoverable; waiting is neither. So a
/// non-authoritative view yields the detection and nothing else.
pub fn agent_change_restart_decision_from_view(
    harness_changed: bool,
    knob_on: bool,
    prompt_visible: bool,
    turn_active: bool,
    authoritative_view: bool,
) -> AgentChangeRestartAction {
    if !harness_changed || !knob_on {
        return AgentChangeRestartAction::None;
    }
    if !prompt_visible || turn_active {
        return AgentChangeRestartAction::WaitForBoundary;
    }
    if !authoritative_view {
        return AgentChangeRestartAction::AwaitAuthoritativeView;
    }
    AgentChangeRestartAction::Restart
}

pub fn harness_change_forces_fresh_spawn(running_binary: &str, resolved_binary: &str) -> bool {
    running_binary != resolved_binary
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#harnessswitchstorm` — the 2026-08-23 `backend.md` respawn loop.
    ///
    /// A quiet boundary plus a detected change is exactly the state that spawned
    /// Claude Code six times in 30s, because the "detected change" came from a
    /// disk read the executor immediately contradicted from the live buffer.
    /// A restart is destructive; the detection alone must not authorize one.
    #[test]
    fn a_non_authoritative_view_never_authorizes_a_respawn() {
        use AgentChangeRestartAction as A;

        assert_eq!(
            agent_change_restart_decision_from_view(true, true, true, false, false),
            A::AwaitAuthoritativeView,
            "a quiet boundary must not turn a stale read into a kill+respawn"
        );
        assert_eq!(
            agent_change_restart_decision_from_view(true, true, true, false, true),
            A::Restart,
            "the same change from the authority still restarts exactly once"
        );
    }

    /// The guard must not swallow the cheaper verdicts: a mid-turn or
    /// prompt-less pane still reports `WaitForBoundary`, and no change is still
    /// `None`, whatever the view was.
    #[test]
    fn the_view_gate_sits_below_the_existing_verdicts() {
        use AgentChangeRestartAction as A;

        for authoritative in [true, false] {
            assert_eq!(
                agent_change_restart_decision_from_view(false, true, true, false, authoritative),
                A::None
            );
            assert_eq!(
                agent_change_restart_decision_from_view(true, false, true, false, authoritative),
                A::None
            );
            assert_eq!(
                agent_change_restart_decision_from_view(true, true, true, true, authoritative),
                A::WaitForBoundary
            );
            assert_eq!(
                agent_change_restart_decision_from_view(true, true, false, false, authoritative),
                A::WaitForBoundary
            );
        }
    }

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
    fn live_route_harness_change_is_accepted_or_held_without_manual_restart() {
        use AgentChangeRouteAction as A;

        assert_eq!(
            agent_change_route_decision(true, false),
            A::AcceptBoundaryHandoff
        );
        assert_eq!(
            agent_change_route_decision(true, true),
            A::HoldForQueueResume
        );
        assert_eq!(
            agent_change_route_decision(false, false),
            A::RejectRestartDisabled
        );
        assert_eq!(
            agent_change_route_decision(false, true),
            A::RejectRestartDisabled
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
