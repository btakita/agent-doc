//! Pure post-child-exit policy for the supervisor run loop.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildLaunchPlan {
    /// The supervisor is launching a replacement child. Managed document
    /// owners must treat this as a fresh launch plus document re-trigger: CLI
    /// conveniences such as Claude `--continue` and Codex `resume --last` are
    /// process-global and can attach this pane to another document's session.
    pub restart_requested: bool,
    pub auto_trigger: bool,
    /// `#stale-ctrl-d-arm` — every fresh child launch (first run, restart, and
    /// recycle-`execve` adopt) must arm stale-Ctrl+D suppression until the child prints
    /// its first prompt. A Ctrl+D (EOF) that arrives BEFORE that first prompt is
    /// definitionally stale — most importantly one buffered in the inherited stdin fd
    /// across an `execve` recycle — and would otherwise EOF-kill the freshly-launched
    /// agent, dropping the interrupted turn to the restart-or-quit prompt (the observed
    /// "crashed and did not restart the turn"). The guard self-disarms once
    /// `prompt_visible_once` flips true, so an intentional operator Ctrl+D at a live
    /// prompt still reaches the child (the "forwarded quit keys own the keepalive
    /// prompt" contract is preserved).
    pub arm_stale_ctrl_d_suppression: bool,
}

pub fn child_launch_plan(
    first_run: bool,
    auto_trigger_next_launch: bool,
    fresh_harness_switch: bool,
) -> ChildLaunchPlan {
    ChildLaunchPlan {
        // A harness switch has no compatible conversation lineage to resume.
        // It is an explicitly-authorized fresh launch even though it occurs on
        // a replacement-loop iteration.
        restart_requested: !first_run && !fresh_harness_switch,
        auto_trigger: auto_trigger_next_launch || !first_run || fresh_harness_switch,
        // A pre-first-prompt Ctrl+D is stale on EVERY launch, so this is unconditional.
        arm_stale_ctrl_d_suppression: true,
    }
}

/// Pending-work facts observed after an operator Clear restarted the child at
/// an idle prompt (`#idlecleartriggerinject`).
///
/// Go-mode `agent:queue` heads are deliberately absent: the per-child idle-queue
/// watch is the single owner of queue continuation and drains a pending head on
/// the new child's first idle boundary by itself. Re-triggering the document on
/// top of it would add a second dispatcher for the same work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OperatorClearPendingWork {
    /// An agent-doc cycle is still open (a real pending turn the relaunch must
    /// resume, e.g. a captured response awaiting closeout).
    pub cycle_open: bool,
    /// The visible document carries unanswered operator steering / an explicit
    /// route relative to the closed cycle's baseline (`TurnAdmission`).
    pub operator_intent_pending: bool,
    /// The pending-work observation itself failed. Fail OPEN to the historical
    /// re-trigger: a wasted no-op turn is recoverable, a stranded prompt is not.
    pub observation_failed: bool,
}

/// Why a relaunch serving an explicit restart request does or does not
/// re-submit the document trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestedRestartTrigger {
    /// Not an operator Clear: Restart Agent / IPC restart keep re-triggering.
    RetriggerRestart,
    /// Operator Clear, but a real pending turn exists.
    RetriggerPendingWork(&'static str),
    /// Operator Clear at an idle prompt with nothing pending: the Clear's whole
    /// effect is the fresh context, so no `/agent-doc <file>` turn is injected.
    SuppressOperatorClearIdle,
}

impl RequestedRestartTrigger {
    pub const fn auto_trigger(self) -> bool {
        !matches!(self, Self::SuppressOperatorClearIdle)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RetriggerRestart => "restart_request",
            Self::RetriggerPendingWork(reason) => reason,
            Self::SuppressOperatorClearIdle => "operator_clear_no_pending_work",
        }
    }
}

/// Derive the relaunch trigger from restart provenance plus pending-work state
/// (`#idlecleartriggerinject`). Pure: the run loop observes, this decides.
pub fn requested_restart_trigger(
    operator_clear: bool,
    pending: OperatorClearPendingWork,
) -> RequestedRestartTrigger {
    if !operator_clear {
        return RequestedRestartTrigger::RetriggerRestart;
    }
    if pending.cycle_open {
        return RequestedRestartTrigger::RetriggerPendingWork("open_cycle");
    }
    if pending.operator_intent_pending {
        return RequestedRestartTrigger::RetriggerPendingWork("operator_intent_pending");
    }
    if pending.observation_failed {
        return RequestedRestartTrigger::RetriggerPendingWork("pending_work_unobservable");
    }
    RequestedRestartTrigger::SuppressOperatorClearIdle
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostChildExitAction {
    /// Route-owned single-cycle completion: exit the supervisor.
    RouteOwnedComplete,
    /// `stop_requested`: exit the whole supervisor.
    ExitSupervisor,
    /// `stop_agent_requested`: keep the supervisor alive and wait at keepalive.
    StopAgentKeepalive,
    /// `restart_requested`: continue the supervisor loop and restart the child.
    AutoRestart,
    /// Fall through to normal crash-policy / clean-exit classification.
    NormalExitClassification,
}

pub fn post_child_exit_action(
    route_owned_completion: bool,
    stop_requested: bool,
    stop_agent_requested: bool,
    restart_requested: bool,
) -> PostChildExitAction {
    if route_owned_completion {
        return PostChildExitAction::RouteOwnedComplete;
    }
    if stop_requested {
        return PostChildExitAction::ExitSupervisor;
    }
    if stop_agent_requested {
        return PostChildExitAction::StopAgentKeepalive;
    }
    if restart_requested {
        return PostChildExitAction::AutoRestart;
    }
    PostChildExitAction::NormalExitClassification
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_launch_plan_separates_fresh_args_from_auto_trigger() {
        assert_eq!(
            child_launch_plan(true, false, false),
            ChildLaunchPlan {
                restart_requested: false,
                auto_trigger: false,
                arm_stale_ctrl_d_suppression: true,
            },
            "initial supervisor launch should open the harness without typing agent-doc"
        );
        assert_eq!(
            child_launch_plan(false, false, false),
            ChildLaunchPlan {
                restart_requested: true,
                auto_trigger: true,
                arm_stale_ctrl_d_suppression: true,
            },
            "replacement launch must re-submit the owning document"
        );
        assert_eq!(
            child_launch_plan(true, true, false),
            ChildLaunchPlan {
                restart_requested: false,
                auto_trigger: true,
                arm_stale_ctrl_d_suppression: true,
            },
            "fresh restart still needs to re-submit agent-doc after the new prompt"
        );
        assert_eq!(
            child_launch_plan(false, false, true),
            ChildLaunchPlan {
                restart_requested: false,
                auto_trigger: true,
                arm_stale_ctrl_d_suppression: true,
            },
            "a harness switch must fresh-spawn and re-submit without entering exact-resume admission",
        );
    }

    /// `#idlecleartriggerinject`: an operator Clear at an idle prompt with no
    /// pending work must relaunch WITHOUT the `/agent-doc <file>` trigger, while
    /// every other restart shape keeps re-triggering.
    #[test]
    fn operator_clear_idle_restart_suppresses_document_trigger() {
        let idle = OperatorClearPendingWork::default();
        let decision = requested_restart_trigger(true, idle);
        assert_eq!(decision, RequestedRestartTrigger::SuppressOperatorClearIdle);
        assert!(!decision.auto_trigger());
        // The clear restart is `fresh` (first_run=true), so the launch plan's
        // trigger is exactly the derived decision.
        assert!(!child_launch_plan(true, decision.auto_trigger(), false).auto_trigger);
    }

    #[test]
    fn operator_clear_restart_with_pending_work_still_retriggers() {
        for (pending, reason) in [
            (
                OperatorClearPendingWork {
                    cycle_open: true,
                    ..Default::default()
                },
                "open_cycle",
            ),
            (
                OperatorClearPendingWork {
                    operator_intent_pending: true,
                    ..Default::default()
                },
                "operator_intent_pending",
            ),
            (
                OperatorClearPendingWork {
                    observation_failed: true,
                    ..Default::default()
                },
                "pending_work_unobservable",
            ),
        ] {
            let decision = requested_restart_trigger(true, pending);
            assert_eq!(
                decision,
                RequestedRestartTrigger::RetriggerPendingWork(reason)
            );
            assert!(decision.auto_trigger(), "{pending:?}");
            assert!(child_launch_plan(true, decision.auto_trigger(), false).auto_trigger);
        }
    }

    #[test]
    fn non_clear_restart_requests_keep_retriggering_regardless_of_pending_work() {
        for pending in [
            OperatorClearPendingWork::default(),
            OperatorClearPendingWork {
                cycle_open: true,
                operator_intent_pending: true,
                observation_failed: true,
            },
        ] {
            assert_eq!(
                requested_restart_trigger(false, pending),
                RequestedRestartTrigger::RetriggerRestart
            );
        }
    }

    #[test]
    fn every_child_launch_arms_stale_ctrl_d_suppression() {
        // `#stale-ctrl-d-arm` regression: the suppression flag was previously never
        // armed anywhere (dead code), so a Ctrl+D buffered across an `execve` recycle
        // EOF-killed the freshly-adopted child. Every launch shape must arm it.
        for first_run in [false, true] {
            for auto_trigger in [false, true] {
                assert!(
                    child_launch_plan(first_run, auto_trigger, false).arm_stale_ctrl_d_suppression,
                    "launch (first_run={first_run}, auto_trigger={auto_trigger}) must arm stale-Ctrl+D suppression"
                );
            }
        }
    }

    #[test]
    fn post_child_exit_action_priority_and_stop_agent_keepalive() {
        use PostChildExitAction::*;

        assert_eq!(
            post_child_exit_action(false, false, false, false),
            NormalExitClassification
        );
        assert_eq!(
            post_child_exit_action(true, true, true, true),
            RouteOwnedComplete
        );
        assert_eq!(
            post_child_exit_action(false, true, true, true),
            ExitSupervisor
        );
        assert_eq!(
            post_child_exit_action(false, false, true, true),
            StopAgentKeepalive
        );
        assert_eq!(
            post_child_exit_action(false, false, true, false),
            StopAgentKeepalive
        );
        assert_eq!(
            post_child_exit_action(false, false, false, true),
            AutoRestart
        );

        for restart in [false, true] {
            let action = post_child_exit_action(false, false, true, restart);
            assert_eq!(action, StopAgentKeepalive);
            assert_ne!(action, ExitSupervisor);
            assert_ne!(action, AutoRestart);
        }
    }
}
