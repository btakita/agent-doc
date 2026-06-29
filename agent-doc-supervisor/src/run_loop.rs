//! Pure post-child-exit policy for the supervisor run loop.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildLaunchPlan {
    pub use_continue_args: bool,
    pub auto_trigger: bool,
}

pub fn child_launch_plan(first_run: bool, auto_trigger_next_launch: bool) -> ChildLaunchPlan {
    ChildLaunchPlan {
        use_continue_args: !first_run,
        auto_trigger: auto_trigger_next_launch || !first_run,
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
                use_continue_args: false,
                auto_trigger: false,
            },
            "initial supervisor launch should open the harness without typing agent-doc"
        );
        assert_eq!(
            child_launch_plan(false, false),
            ChildLaunchPlan {
                use_continue_args: true,
                auto_trigger: true,
            },
            "continue-mode restart should resume and re-submit agent-doc"
        );
        assert_eq!(
            child_launch_plan(true, true),
            ChildLaunchPlan {
                use_continue_args: false,
                auto_trigger: true,
            },
            "fresh restart still needs to re-submit agent-doc after the new prompt"
        );
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
