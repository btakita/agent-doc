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

pub fn child_launch_plan(first_run: bool, auto_trigger_next_launch: bool) -> ChildLaunchPlan {
    ChildLaunchPlan {
        restart_requested: !first_run,
        auto_trigger: auto_trigger_next_launch || !first_run,
        // A pre-first-prompt Ctrl+D is stale on EVERY launch, so this is unconditional.
        arm_stale_ctrl_d_suppression: true,
    }
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
            child_launch_plan(true, false),
            ChildLaunchPlan {
                restart_requested: false,
                auto_trigger: false,
                arm_stale_ctrl_d_suppression: true,
            },
            "initial supervisor launch should open the harness without typing agent-doc"
        );
        assert_eq!(
            child_launch_plan(false, false),
            ChildLaunchPlan {
                restart_requested: true,
                auto_trigger: true,
                arm_stale_ctrl_d_suppression: true,
            },
            "replacement launch must re-submit the owning document"
        );
        assert_eq!(
            child_launch_plan(true, true),
            ChildLaunchPlan {
                restart_requested: false,
                auto_trigger: true,
                arm_stale_ctrl_d_suppression: true,
            },
            "fresh restart still needs to re-submit agent-doc after the new prompt"
        );
    }

    #[test]
    fn every_child_launch_arms_stale_ctrl_d_suppression() {
        // `#stale-ctrl-d-arm` regression: the suppression flag was previously never
        // armed anywhere (dead code), so a Ctrl+D buffered across an `execve` recycle
        // EOF-killed the freshly-adopted child. Every launch shape must arm it.
        for first_run in [false, true] {
            for auto_trigger in [false, true] {
                assert!(
                    child_launch_plan(first_run, auto_trigger).arm_stale_ctrl_d_suppression,
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
