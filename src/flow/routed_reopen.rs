use super::types::RouteDecision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorDispatchState {
    Ready,
    Starting,
    Busy,
    WaitingInput,
    Blocked,
    Closed,
    Missing,
}

impl ActorDispatchState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Starting => "starting",
            Self::Busy => "busy",
            Self::WaitingInput => "waiting_input",
            Self::Blocked => "blocked",
            Self::Closed => "closed",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReopenMode {
    Managed,
    DispatchOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoutedReopenFacts {
    pub(crate) actor_state: ActorDispatchState,
    pub(crate) prompt_ready: bool,
    pub(crate) has_prompt_bearing_work: bool,
    pub(crate) mode: ReopenMode,
    pub(crate) degraded_authority: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoutedReopenOutcome {
    pub(crate) decision: RouteDecision,
    pub(crate) reason: &'static str,
}

pub(crate) fn decide_authoritative_reopen(facts: RoutedReopenFacts) -> RoutedReopenOutcome {
    if facts.degraded_authority {
        return RoutedReopenOutcome {
            decision: RouteDecision::FailClosed,
            reason: "degraded_authority",
        };
    }

    match facts.actor_state {
        ActorDispatchState::Ready if facts.prompt_ready => RoutedReopenOutcome {
            decision: RouteDecision::ReuseReady,
            reason: "ready_prompt",
        },
        ActorDispatchState::Ready => RoutedReopenOutcome {
            decision: RouteDecision::WaitForReady,
            reason: "ready_without_prompt_proof",
        },
        ActorDispatchState::Starting => RoutedReopenOutcome {
            decision: RouteDecision::WaitForReady,
            reason: "starting_requires_prompt_ready_barrier",
        },
        ActorDispatchState::Busy if facts.mode == ReopenMode::Managed => RoutedReopenOutcome {
            decision: RouteDecision::ReuseReady,
            reason: "managed_busy_actor_can_queue_once",
        },
        ActorDispatchState::Busy => RoutedReopenOutcome {
            decision: RouteDecision::FailClosed,
            reason: "dispatch_only_busy_actor_not_ready",
        },
        ActorDispatchState::WaitingInput
        | ActorDispatchState::Blocked
        | ActorDispatchState::Closed => RoutedReopenOutcome {
            decision: RouteDecision::FailClosed,
            reason: "actor_terminal_or_protected_state",
        },
        ActorDispatchState::Missing if facts.has_prompt_bearing_work => RoutedReopenOutcome {
            decision: RouteDecision::StartNew,
            reason: "missing_actor_with_prompt_work",
        },
        ActorDispatchState::Missing => RoutedReopenOutcome {
            decision: RouteDecision::StartNew,
            reason: "missing_actor",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_actor_waits_for_prompt_ready_barrier() {
        let outcome = decide_authoritative_reopen(RoutedReopenFacts {
            actor_state: ActorDispatchState::Starting,
            prompt_ready: false,
            has_prompt_bearing_work: true,
            mode: ReopenMode::DispatchOnly,
            degraded_authority: false,
        });

        assert_eq!(outcome.decision, RouteDecision::WaitForReady);
        assert_eq!(outcome.reason, "starting_requires_prompt_ready_barrier");
    }

    #[test]
    fn dispatch_only_busy_actor_fails_closed() {
        let outcome = decide_authoritative_reopen(RoutedReopenFacts {
            actor_state: ActorDispatchState::Busy,
            prompt_ready: false,
            has_prompt_bearing_work: true,
            mode: ReopenMode::DispatchOnly,
            degraded_authority: false,
        });

        assert_eq!(outcome.decision, RouteDecision::FailClosed);
    }

    #[test]
    fn managed_busy_actor_can_queue_once() {
        let outcome = decide_authoritative_reopen(RoutedReopenFacts {
            actor_state: ActorDispatchState::Busy,
            prompt_ready: false,
            has_prompt_bearing_work: true,
            mode: ReopenMode::Managed,
            degraded_authority: false,
        });

        assert_eq!(outcome.decision, RouteDecision::ReuseReady);
    }
}
