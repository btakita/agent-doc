//! Pure post-child-exit policy for the supervisor run loop.

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
