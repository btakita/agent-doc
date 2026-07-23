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
    /// Current harness identity reported by the live supervisor. This is
    /// independent of the persisted actor record and lets route distinguish a
    /// pending switch from a completed switch whose writeback lagged.
    pub current_harness: Option<String>,
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

pub fn mismatched_authoritative_actor_can_be_replaced(
    runtime: &SupervisorRuntime,
    actor_state: RouteActorState,
) -> bool {
    runtime.health != SupervisorHealth::Healthy || actor_state == RouteActorState::Closed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentGenerationReadyTransitionFacts<'a> {
    pub current_generation: u64,
    pub transition_generation: u64,
    pub transition_reason: &'a str,
    pub actor_state: RouteActorState,
}

/// Does the actor's last transition already prove current-generation dispatch
/// readiness without needing a fresh pane capture?
pub fn transition_proves_current_generation_ready(
    facts: CurrentGenerationReadyTransitionFacts<'_>,
) -> bool {
    facts.transition_generation == facts.current_generation
        && matches!(
            facts.transition_reason,
            "prompt_ready" | "dispatch_ready_prompt" | "idle_pane_reconcile"
        )
        && facts.actor_state == RouteActorState::Ready
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeferToBoundaryRestartRecoveryFacts<'a> {
    pub supervisor_health: SupervisorHealth,
    pub actor_state: RouteActorState,
    pub queue_paused: bool,
    /// True when the actor's own in-flight restart is what made the pane busy
    /// (`#actorswitchdeferbusyself`). A harness switch necessarily transitions the
    /// actor to `busy` while it tears down the old child and spawns the new one, so
    /// a bare `busy` reading during that window says nothing about operator action.
    /// Telling the operator to "Interrupt and restart to force the harness switch"
    /// right after they successfully restarted is both wrong and destructive advice —
    /// it asks them to interrupt the very restart that is completing the switch.
    pub restart_in_flight: bool,
    pub recovery_command: &'a str,
}

/// Lifecycle transition reasons that mean "this actor is busy because it is
/// restarting itself", not "the operator left a turn running".
pub fn actor_busy_is_self_induced_restart(last_transition_reason: &str) -> bool {
    const SELF_RESTART_REASONS: [&str; 4] = [
        "restart_continue_spawn",
        "ipc_restart_requested",
        "restart_fresh_spawn",
        "agent_harness_switch_writeback",
    ];
    SELF_RESTART_REASONS.contains(&last_transition_reason.trim())
}

/// Build the operator-actionable recovery suffix for a route defer to the
/// supervisor's boundary restart.
pub fn defer_to_boundary_restart_recovery_hint(
    facts: DeferToBoundaryRestartRecoveryFacts<'_>,
) -> String {
    if facts.queue_paused
        || facts.supervisor_health == SupervisorHealth::Unreachable
        || facts.supervisor_health == SupervisorHealth::NoSocket
    {
        let blocker = if facts.queue_paused {
            "queue is paused"
        } else {
            "supervisor is unreachable"
        };
        format!(
            ". {} — the boundary restart will not fire until it is healthy and resumed. Run: {}",
            blocker, facts.recovery_command
        )
    } else if facts.restart_in_flight
        && (facts.actor_state == RouteActorState::Busy
            || facts.actor_state == RouteActorState::Starting)
    {
        // `#actorswitchdeferbusyself`: the busy window belongs to the restart that is
        // already switching the harness. Never point the operator at `--force` here.
        ". the harness restart is already in flight — the switch completes at that boundary, so wait for it and run Agent Doc again. No interrupt or forced restart is needed.".to_string()
    } else if facts.actor_state == RouteActorState::Busy
        || facts.actor_state == RouteActorState::Starting
    {
        format!(
            ". pane is {} (not dispatch-ready) — run: {} --force",
            facts.actor_state.as_str(),
            facts.recovery_command
        )
    } else {
        format!(
            ". The supervisor idle-watch will restart the harness at the next idle boundary. To force it now: {}",
            facts.recovery_command
        )
    }
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
            current_harness: None,
        };
        assert!(authoritative_actor_dispatch_target_eligible(&healthy));

        let degraded = SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
            current_harness: None,
        };
        assert_eq!(
            authoritative_actor_dispatch_guard_reason(degraded.facts()).as_deref(),
            Some("supervisor health is no_socket")
        );

        let missing_state = SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: None,
            current_harness: None,
        };
        assert_eq!(
            authoritative_actor_dispatch_guard_reason(missing_state.facts()).as_deref(),
            Some("supervisor actor_state is missing")
        );
    }

    #[test]
    fn mismatched_authoritative_actor_can_be_replaced_only_when_not_live_authority() {
        let healthy_ready = SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(RouteActorState::Ready),
            current_harness: None,
        };
        assert!(
            !mismatched_authoritative_actor_can_be_replaced(&healthy_ready, RouteActorState::Ready),
            "a healthy ready actor from another harness is still authoritative and must block"
        );

        let healthy_closed = SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(RouteActorState::Closed),
            current_harness: None,
        };
        assert!(
            mismatched_authoritative_actor_can_be_replaced(
                &healthy_closed,
                RouteActorState::Closed
            ),
            "a closed actor from another harness should not strand a fresh harness start"
        );

        let unreachable = SupervisorRuntime {
            health: SupervisorHealth::Unreachable,
            actor_state: None,
            current_harness: None,
        };
        assert!(
            mismatched_authoritative_actor_can_be_replaced(&unreachable, RouteActorState::Ready),
            "an unreachable supervisor cannot prove live cross-harness ownership"
        );
    }

    fn ready_transition_facts(reason: &str) -> CurrentGenerationReadyTransitionFacts<'_> {
        CurrentGenerationReadyTransitionFacts {
            current_generation: 5,
            transition_generation: 5,
            transition_reason: reason,
            actor_state: RouteActorState::Ready,
        }
    }

    #[test]
    fn transition_proves_ready_accepts_idle_pane_reconcile() {
        let facts = ready_transition_facts("idle_pane_reconcile");
        assert!(
            transition_proves_current_generation_ready(facts),
            "idle_pane_reconcile is supervisor-proven direct pane evidence and must satisfy the route ready barrier"
        );
    }

    #[test]
    fn transition_proves_ready_accepts_prompt_ready_and_dispatch_ready_prompt() {
        for reason in ["prompt_ready", "dispatch_ready_prompt"] {
            let facts = ready_transition_facts(reason);
            assert!(
                transition_proves_current_generation_ready(facts),
                "{reason} must remain a valid ready-proof reason"
            );
        }
    }

    #[test]
    fn transition_proves_ready_rejects_unmatched_reason() {
        let facts = ready_transition_facts("starting_actor_timeout");
        assert!(
            !transition_proves_current_generation_ready(facts),
            "an unmatched transition reason must not satisfy the ready barrier"
        );
    }

    #[test]
    fn transition_proves_ready_rejects_stale_generation() {
        let mut facts = ready_transition_facts("idle_pane_reconcile");
        facts.transition_generation = 3;
        assert!(
            !transition_proves_current_generation_ready(facts),
            "a prior-generation transition must not satisfy the current-generation ready barrier"
        );
    }

    #[test]
    fn transition_proves_ready_rejects_non_ready_actor() {
        let mut facts = ready_transition_facts("idle_pane_reconcile");
        facts.actor_state = RouteActorState::Busy;
        assert!(
            !transition_proves_current_generation_ready(facts),
            "a non-Ready actor must not satisfy the ready barrier even with a matching reason"
        );
    }

    #[test]
    fn defer_to_boundary_restart_recovery_names_unhealthy_or_paused_blocker() {
        let command = "agent-doc session restart-supervisor doc.md";

        let paused = defer_to_boundary_restart_recovery_hint(DeferToBoundaryRestartRecoveryFacts {
            supervisor_health: SupervisorHealth::Healthy,
            actor_state: RouteActorState::Ready,
            queue_paused: true,
            restart_in_flight: false,
            recovery_command: command,
        });
        assert!(paused.contains("queue is paused"));
        assert!(paused.contains(command));

        let unreachable =
            defer_to_boundary_restart_recovery_hint(DeferToBoundaryRestartRecoveryFacts {
                supervisor_health: SupervisorHealth::NoSocket,
                actor_state: RouteActorState::Ready,
                queue_paused: false,
                restart_in_flight: false,
                recovery_command: command,
            });
        assert!(unreachable.contains("supervisor is unreachable"));
        assert!(unreachable.contains(command));
    }

    #[test]
    fn defer_to_boundary_restart_recovery_can_force_busy_or_wait_for_idle() {
        let command = "agent-doc session restart-supervisor doc.md";

        let busy = defer_to_boundary_restart_recovery_hint(DeferToBoundaryRestartRecoveryFacts {
            supervisor_health: SupervisorHealth::Healthy,
            actor_state: RouteActorState::Busy,
            queue_paused: false,
            restart_in_flight: false,
            recovery_command: command,
        });
        assert!(busy.contains("pane is busy"));
        assert!(busy.contains("--force"));

        let ready = defer_to_boundary_restart_recovery_hint(DeferToBoundaryRestartRecoveryFacts {
            supervisor_health: SupervisorHealth::Healthy,
            actor_state: RouteActorState::Ready,
            queue_paused: false,
            restart_in_flight: false,
            recovery_command: command,
        });
        assert!(ready.contains("idle-watch will restart"));
        assert!(ready.contains(command));
    }

    #[test]
    fn defer_to_boundary_restart_never_tells_operator_to_force_its_own_restart() {
        // `#actorswitchdeferbusyself`: the live repro. The operator ran JB
        // `Restart Agent`, the supervisor spawned the new harness, and the actor was
        // busy doing exactly that when route checked. The old hint said "pane is not
        // at a dispatch-ready boundary. Use Interrupt and restart to force the harness
        // switch" — telling the operator to interrupt the restart that was completing
        // their switch.
        let command = "agent-doc session restart-supervisor doc.md";

        for state in [RouteActorState::Busy, RouteActorState::Starting] {
            let hint =
                defer_to_boundary_restart_recovery_hint(DeferToBoundaryRestartRecoveryFacts {
                    supervisor_health: SupervisorHealth::Healthy,
                    actor_state: state,
                    queue_paused: false,
                    restart_in_flight: true,
                    recovery_command: command,
                });
            assert!(
                hint.contains("already in flight"),
                "expected wait-for-boundary guidance, got: {hint}"
            );
            assert!(
                !hint.contains("--force"),
                "must never suggest forcing an in-flight restart, got: {hint}"
            );
            assert!(
                !hint.contains("Interrupt"),
                "must never suggest interrupting an in-flight restart, got: {hint}"
            );
        }

        // A genuinely operator-blocked busy pane keeps the forceful guidance.
        let operator_busy =
            defer_to_boundary_restart_recovery_hint(DeferToBoundaryRestartRecoveryFacts {
                supervisor_health: SupervisorHealth::Healthy,
                actor_state: RouteActorState::Busy,
                queue_paused: false,
                restart_in_flight: false,
                recovery_command: command,
            });
        assert!(operator_busy.contains("--force"));

        // A paused queue / unreachable supervisor still outranks the restart window:
        // the boundary restart genuinely will not fire, so waiting would hang.
        let paused = defer_to_boundary_restart_recovery_hint(DeferToBoundaryRestartRecoveryFacts {
            supervisor_health: SupervisorHealth::Healthy,
            actor_state: RouteActorState::Busy,
            queue_paused: true,
            restart_in_flight: true,
            recovery_command: command,
        });
        assert!(paused.contains("queue is paused"));
    }

    #[test]
    fn self_induced_restart_reasons_are_recognized() {
        // Reasons the supervisor itself writes while swapping harnesses.
        for reason in [
            "restart_continue_spawn",
            "ipc_restart_requested",
            "restart_fresh_spawn",
            "agent_harness_switch_writeback",
        ] {
            assert!(
                actor_busy_is_self_induced_restart(reason),
                "{reason} should count as a self-induced restart"
            );
        }
        // Reasons that mean a real turn is running — the operator IS blocked.
        for reason in ["auto_trigger_inject", "dispatch_submit", "prompt_ready", ""] {
            assert!(
                !actor_busy_is_self_induced_restart(reason),
                "{reason} must not be mistaken for a self-induced restart"
            );
        }
    }
}
