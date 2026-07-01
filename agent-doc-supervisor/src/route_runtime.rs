//! Pure route-facing supervisor runtime policy.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteActorState {
    Starting,
    Ready,
    Busy,
    WaitingInput,
    Closed,
    Blocked,
}

impl RouteActorState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Busy => "busy",
            Self::WaitingInput => "waiting_input",
            Self::Closed => "closed",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "starting" => Some(Self::Starting),
            "ready" => Some(Self::Ready),
            "busy" => Some(Self::Busy),
            "waiting_input" => Some(Self::WaitingInput),
            "closed" => Some(Self::Closed),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorHealth {
    Healthy,
    Restartable,
    Halted { restart_count: u32 },
    Unreachable,
    NoSocket,
}

impl SupervisorHealth {
    pub fn label(self) -> String {
        match self {
            Self::Healthy => "healthy".to_string(),
            Self::Restartable => "restartable".to_string(),
            Self::Halted { restart_count } => {
                format!("halted(restart_count={restart_count})")
            }
            Self::Unreachable => "unreachable".to_string(),
            Self::NoSocket => "no_socket".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorRuntime {
    pub health: SupervisorHealth,
    pub actor_state: Option<RouteActorState>,
}

impl SupervisorRuntime {
    pub fn actor_state_label(&self) -> &'static str {
        self.actor_state
            .map(RouteActorState::as_str)
            .unwrap_or("missing")
    }

    pub const fn facts(&self) -> AuthoritativeRuntimeFacts {
        AuthoritativeRuntimeFacts {
            health: self.health,
            actor_state_present: self.actor_state.is_some(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoritativeRuntimeFacts {
    pub health: SupervisorHealth,
    pub actor_state_present: bool,
}

pub fn effective_authoritative_actor_state(
    record_state: RouteActorState,
    runtime_state: Option<RouteActorState>,
) -> RouteActorState {
    if matches!(
        record_state,
        RouteActorState::Blocked | RouteActorState::Closed
    ) {
        return record_state;
    }
    runtime_state.unwrap_or(record_state)
}

pub fn authoritative_actor_dispatch_guard_reason(
    facts: AuthoritativeRuntimeFacts,
) -> Option<String> {
    if facts.health != SupervisorHealth::Healthy {
        return Some(format!("supervisor health is {}", facts.health.label()));
    }
    if !facts.actor_state_present {
        return Some("supervisor actor_state is missing".to_string());
    }
    None
}

pub fn authoritative_actor_dispatch_target_eligible(runtime: &SupervisorRuntime) -> bool {
    authoritative_actor_dispatch_guard_reason(runtime.facts()).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_actor_state_labels() {
        assert_eq!(
            RouteActorState::parse("ready"),
            Some(RouteActorState::Ready)
        );
        assert_eq!(
            RouteActorState::parse(" waiting_input "),
            Some(RouteActorState::WaitingInput)
        );
        assert_eq!(RouteActorState::parse("unknown"), None);
    }

    #[test]
    fn effective_authoritative_state_preserves_terminal_record_state() {
        assert_eq!(
            effective_authoritative_actor_state(
                RouteActorState::Blocked,
                Some(RouteActorState::Ready)
            ),
            RouteActorState::Blocked
        );
        assert_eq!(
            effective_authoritative_actor_state(
                RouteActorState::Starting,
                Some(RouteActorState::Ready)
            ),
            RouteActorState::Ready
        );
        assert_eq!(
            effective_authoritative_actor_state(RouteActorState::Busy, None),
            RouteActorState::Busy
        );
    }

    #[test]
    fn dispatch_guard_requires_healthy_runtime_with_actor_state() {
        let healthy = SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(RouteActorState::Ready),
        };
        assert!(authoritative_actor_dispatch_target_eligible(&healthy));

        let degraded = SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
        };
        assert_eq!(
            authoritative_actor_dispatch_guard_reason(degraded.facts()).as_deref(),
            Some("supervisor health is no_socket")
        );

        let missing_state = SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: None,
        };
        assert_eq!(
            authoritative_actor_dispatch_guard_reason(missing_state.facts()).as_deref(),
            Some("supervisor actor_state is missing")
        );
    }
}
