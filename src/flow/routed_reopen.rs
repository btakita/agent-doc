use super::types::RouteDecision;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectPaneSubmitStatus {
    Accepted,
    TimedOut,
}

pub(crate) fn direct_pane_submit_acceptance_timeout() -> Duration {
    Duration::from_secs(5)
}

pub(crate) fn direct_pane_submit_acceptance_budget() -> Duration {
    // tmux/control-mode delivery can spend the whole acceptance window plus a
    // final capture poll before pane input disappears. Keep the budget above
    // that window so "over_budget" means slower than the path can observe.
    Duration::from_secs(6)
}

pub(crate) fn routed_dispatch_start_timeout(test_mode: bool) -> Duration {
    if test_mode {
        Duration::from_secs(1)
    } else {
        Duration::from_secs(10)
    }
}

pub(crate) fn fresh_route_start_ack_timeout(test_mode: bool) -> Duration {
    if test_mode {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(30)
    }
}

pub(crate) fn routed_cycle_ack_timeout(live_child_for_file: bool, test_mode: bool) -> Duration {
    if test_mode {
        if live_child_for_file {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(1)
        }
    } else if live_child_for_file {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(15)
    }
}

pub(crate) fn existing_pane_ready_timeout(test_mode: bool) -> Duration {
    if test_mode {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(15)
    }
}

pub(crate) fn dispatch_only_starting_pane_ready_timeout_for_binary(
    binary: Option<&str>,
    test_mode: bool,
) -> Duration {
    if test_mode {
        Duration::from_millis(250)
    } else if matches!(binary, Some("opencode")) {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(2)
    }
}

pub(crate) fn dispatch_only_starting_pane_recovery_timeout_for_binary(
    binary: Option<&str>,
    test_mode: bool,
) -> Duration {
    if test_mode {
        return Duration::from_millis(400);
    }
    match binary {
        Some("opencode") => Duration::from_secs(15),
        Some("claude") => Duration::from_secs(10),
        Some("codex") => Duration::from_secs(8),
        _ => Duration::from_secs(5),
    }
}

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
pub(crate) enum ActorRuntimeHealth {
    Healthy,
    Restartable,
    Halted { restart_count: u32 },
    Unreachable,
    NoSocket,
}

impl ActorRuntimeHealth {
    pub(crate) fn label(self) -> String {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReopenMode {
    Managed,
    DispatchOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchOnlyReopenDelivery {
    SupervisorIpcOnce,
    DirectPaneSubmit,
}

impl DispatchOnlyReopenDelivery {
    pub(crate) const fn submit_mode(self) -> &'static str {
        match self {
            DispatchOnlyReopenDelivery::SupervisorIpcOnce => "supervisor_normalized_submit",
            DispatchOnlyReopenDelivery::DirectPaneSubmit => "tmux_literal_enter_delayed",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            DispatchOnlyReopenDelivery::SupervisorIpcOnce => "supervisor_ipc_once",
            DispatchOnlyReopenDelivery::DirectPaneSubmit => "direct_pane_submit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoutedDispatchStartProof {
    CommandAcceptedOnly,
    HookPromptMatched,
    HookStateAdvanced,
}

impl RoutedDispatchStartProof {
    pub(crate) const fn dispatch_stage_label(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly => "accepted",
            Self::HookPromptMatched => "consumed",
            Self::HookStateAdvanced => "submitted",
        }
    }

    pub(crate) const fn proof_scope_label(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly => "accepted_only",
            Self::HookPromptMatched | Self::HookStateAdvanced => "dispatch_start",
        }
    }

    pub(crate) const fn proof_scope_description(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly => {
                "accepted-only; no harness dispatch-start proof was available"
            }
            Self::HookPromptMatched => "dispatch-start proof matched the routed prompt",
            Self::HookStateAdvanced => "dispatch-start proof observed newer harness prompt state",
        }
    }

    pub(crate) const fn startup_miss_label(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly => "acceptance",
            Self::HookPromptMatched => "consumption",
            Self::HookStateAdvanced => "submission",
        }
    }
}

pub(crate) fn direct_pane_submit_outcome(
    status: DirectPaneSubmitStatus,
    dispatch_start_proof: Option<RoutedDispatchStartProof>,
) -> &'static str {
    match (status, dispatch_start_proof) {
        (DirectPaneSubmitStatus::Accepted, _) => "accepted",
        (DirectPaneSubmitStatus::TimedOut, Some(_)) => "acceptance_unobserved_dispatch_proven",
        (DirectPaneSubmitStatus::TimedOut, None) => "acceptance_unobserved",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchStartProofDecision {
    Accepted,
    FailClosedAcceptedOnly,
}

pub(crate) fn decide_dispatch_start_proof(
    proof: RoutedDispatchStartProof,
    dispatch_start_proof_required: bool,
) -> DispatchStartProofDecision {
    if proof == RoutedDispatchStartProof::CommandAcceptedOnly && dispatch_start_proof_required {
        DispatchStartProofDecision::FailClosedAcceptedOnly
    } else {
        DispatchStartProofDecision::Accepted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DispatchOnlyProofPolicyFacts<'a> {
    pub(crate) harness_binary: &'a str,
    pub(crate) codex_dispatch_start_tracking_enabled: bool,
}

pub(crate) fn dispatch_only_dispatch_start_proof_required(
    facts: DispatchOnlyProofPolicyFacts<'_>,
) -> bool {
    match facts.harness_binary {
        "codex" => facts.codex_dispatch_start_tracking_enabled,
        "opencode" => true,
        _ => false,
    }
}

pub(crate) fn should_print_dispatch_only_unproven_progress(
    facts: DispatchOnlyProofPolicyFacts<'_>,
) -> bool {
    facts.harness_binary != "codex" || !facts.codex_dispatch_start_tracking_enabled
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DispatchOnlyProofOutcomeFacts<'a> {
    pub(crate) file_display: &'a str,
    pub(crate) pane: &'a str,
    pub(crate) harness_binary: &'a str,
    pub(crate) delivery: DispatchOnlyReopenDelivery,
    pub(crate) dispatch_start: RoutedDispatchStartProof,
    pub(crate) timeout_secs: u64,
}

pub(crate) fn dispatch_only_sent_log_message(facts: DispatchOnlyProofOutcomeFacts<'_>) -> String {
    format!(
        "route_dispatch_only_sent file={} pane={} harness={} delivery={} submit_mode={} proof={} proof_scope={}",
        facts.file_display,
        facts.pane,
        facts.harness_binary,
        facts.delivery.label(),
        facts.delivery.submit_mode(),
        facts.dispatch_start.dispatch_stage_label(),
        facts.dispatch_start.proof_scope_label()
    )
}

pub(crate) fn dispatch_only_sent_console_message(
    facts: DispatchOnlyProofOutcomeFacts<'_>,
) -> String {
    format!(
        "[route] dispatch-only {} reopen for {} was sent to pane {} via {} ({}) with {} proof ({})",
        facts.harness_binary,
        facts.file_display,
        facts.pane,
        facts.delivery.label(),
        facts.delivery.submit_mode(),
        facts.dispatch_start.dispatch_stage_label(),
        facts.dispatch_start.proof_scope_description()
    )
}

pub(crate) fn accepted_only_dispatch_start_log_message(
    facts: DispatchOnlyProofOutcomeFacts<'_>,
) -> String {
    format!(
        "route_dispatch_only_submit_unproven file={} pane={} harness={} delivery={} submit_mode={} proof=accepted proof_scope=accepted_only timeout_secs={}",
        facts.file_display,
        facts.pane,
        facts.harness_binary,
        facts.delivery.label(),
        facts.delivery.submit_mode(),
        facts.timeout_secs
    )
}

pub(crate) fn accepted_only_dispatch_start_refusal_message(
    facts: DispatchOnlyProofOutcomeFacts<'_>,
) -> String {
    format!(
        "dispatch-only {} reopen for {} was accepted in pane {} via {} ({}), but only pane-input acceptance proof was available after waiting {}s; treating this as not dispatched because no dispatch-start proof was recorded. Restore an idle {} prompt or restart the session and reroute again",
        facts.harness_binary,
        facts.file_display,
        facts.pane,
        facts.delivery.label(),
        facts.delivery.submit_mode(),
        facts.timeout_secs,
        facts.harness_binary
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoutedReopenFacts {
    pub(crate) actor_state: ActorDispatchState,
    pub(crate) prompt_ready: bool,
    pub(crate) has_prompt_bearing_work: bool,
    pub(crate) mode: ReopenMode,
    pub(crate) degraded_authority: bool,
    pub(crate) dispatch_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoutedReopenOutcome {
    pub(crate) decision: RouteDecision,
    pub(crate) reason: &'static str,
}

pub(crate) fn decide_authoritative_reopen(facts: RoutedReopenFacts) -> RoutedReopenOutcome {
    if facts.degraded_authority || !facts.dispatch_eligible {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PromptReadyBarrierFacts {
    pub(crate) actor_state: ActorDispatchState,
    pub(crate) prompt_ready: bool,
    pub(crate) dispatch_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptReadyBarrierDecision {
    Ready,
    Terminal,
    Continue,
}

pub(crate) fn classify_prompt_ready_barrier(
    facts: PromptReadyBarrierFacts,
) -> PromptReadyBarrierDecision {
    if facts.actor_state == ActorDispatchState::Ready
        && facts.prompt_ready
        && facts.dispatch_eligible
    {
        return PromptReadyBarrierDecision::Ready;
    }
    if actor_start_wait_terminal_state(facts.actor_state) {
        return PromptReadyBarrierDecision::Terminal;
    }
    PromptReadyBarrierDecision::Continue
}

pub(crate) const fn actor_start_wait_terminal_state(state: ActorDispatchState) -> bool {
    matches!(
        state,
        ActorDispatchState::Closed | ActorDispatchState::Blocked
    )
}

pub(crate) const fn actor_dispatch_blocker_reason(
    state: ActorDispatchState,
) -> Option<&'static str> {
    match state {
        ActorDispatchState::Ready => None,
        ActorDispatchState::Starting => Some("the authoritative actor is still starting"),
        ActorDispatchState::Busy => Some("the authoritative actor is busy"),
        ActorDispatchState::WaitingInput => {
            Some("the authoritative actor is waiting for user input")
        }
        ActorDispatchState::Closed => Some("the authoritative actor is closed"),
        ActorDispatchState::Blocked => Some("the authoritative actor is blocked"),
        ActorDispatchState::Missing => Some("the authoritative actor is missing"),
    }
}

pub(crate) const fn actor_can_queue_optimistically(state: ActorDispatchState) -> bool {
    matches!(state, ActorDispatchState::Busy)
}

pub(crate) const fn actor_waiting_input_recoverable(state: ActorDispatchState) -> bool {
    matches!(state, ActorDispatchState::WaitingInput)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BusyPaneAutoFixOutcome {
    RetryRoute,
    RetryRouteAfterSupervisorRestart,
    RetryRouteAfterFreshRestart,
    FailClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BusyPaneAutoFixFacts {
    pub(crate) test_hook_changed: bool,
    pub(crate) fix_made_changes: bool,
    pub(crate) supervisor_healthy: bool,
    pub(crate) restarted_supervisor: bool,
}

pub(crate) fn busy_existing_pane_auto_fix_outcome(
    facts: BusyPaneAutoFixFacts,
) -> BusyPaneAutoFixOutcome {
    if facts.restarted_supervisor {
        return BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart;
    }
    if facts.test_hook_changed || facts.fix_made_changes {
        return BusyPaneAutoFixOutcome::RetryRoute;
    }
    if facts.supervisor_healthy {
        BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart
    } else {
        BusyPaneAutoFixOutcome::FailClosed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AuthoritativeRuntimeFacts {
    pub(crate) health: ActorRuntimeHealth,
    pub(crate) actor_state_present: bool,
}

pub(crate) fn authoritative_actor_dispatch_guard_reason(
    facts: AuthoritativeRuntimeFacts,
) -> Option<String> {
    if facts.health != ActorRuntimeHealth::Healthy {
        return Some(format!("supervisor health is {}", facts.health.label()));
    }
    if !facts.actor_state_present {
        return Some("supervisor actor_state is missing".to_string());
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DegradedAuthoritativeActorFacts<'a> {
    pub(crate) actor_pane: &'a str,
    pub(crate) transition_caller: &'a str,
    pub(crate) transition_reason: &'a str,
    pub(crate) registered_pane: Option<&'a str>,
    pub(crate) live_owner_pane: Option<&'a str>,
}

pub(crate) fn can_use_degraded_authoritative_actor(
    facts: DegradedAuthoritativeActorFacts<'_>,
) -> bool {
    if facts.transition_caller == "register" && facts.transition_reason == "register" {
        return false;
    }
    facts.registered_pane == Some(facts.actor_pane)
        || facts.live_owner_pane == Some(facts.actor_pane)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DegradedAuthoritativeActorRefusal<'a> {
    pub(crate) harness_binary: &'a str,
    pub(crate) file_display: &'a str,
    pub(crate) generation: u64,
    pub(crate) pane_id: &'a str,
    pub(crate) reason: &'a str,
    pub(crate) runtime_actor_state: &'a str,
}

pub(crate) fn degraded_authoritative_actor_refusal_message(
    facts: DegradedAuthoritativeActorRefusal<'_>,
) -> String {
    format!(
        "dispatch-only {} reroute for {} refused before input because authoritative actor generation {} on pane {} has degraded supervisor state ({}, runtime_actor_state={}). Restart or rebind the owner with `agent-doc start {}` and rerun the route after dispatch-start proof is available.",
        facts.harness_binary,
        facts.file_display,
        facts.generation,
        facts.pane_id,
        facts.reason,
        facts.runtime_actor_state,
        facts.file_display
    )
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
            dispatch_eligible: true,
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
            dispatch_eligible: true,
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
            dispatch_eligible: true,
        });

        assert_eq!(outcome.decision, RouteDecision::ReuseReady);
    }

    #[test]
    fn prompt_ready_barrier_requires_state_prompt_and_eligibility() {
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Starting,
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Continue
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Ready,
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Continue
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Ready,
                prompt_ready: true,
                dispatch_eligible: false,
            }),
            PromptReadyBarrierDecision::Continue
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Ready,
                prompt_ready: true,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Ready
        );
    }

    #[test]
    fn prompt_ready_barrier_surfaces_terminal_states() {
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Closed,
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Terminal
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Blocked,
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Terminal
        );
    }

    #[test]
    fn degraded_authority_requires_current_pane_binding() {
        let facts = DegradedAuthoritativeActorFacts {
            actor_pane: "%42",
            transition_caller: "route",
            transition_reason: "dispatch_bind",
            registered_pane: Some("%42"),
            live_owner_pane: None,
        };
        assert!(can_use_degraded_authoritative_actor(facts));

        assert!(can_use_degraded_authoritative_actor(
            DegradedAuthoritativeActorFacts {
                registered_pane: None,
                live_owner_pane: Some("%42"),
                ..facts
            }
        ));
        assert!(!can_use_degraded_authoritative_actor(
            DegradedAuthoritativeActorFacts {
                registered_pane: Some("%99"),
                live_owner_pane: Some("%99"),
                ..facts
            }
        ));
        assert!(!can_use_degraded_authoritative_actor(
            DegradedAuthoritativeActorFacts {
                transition_caller: "register",
                transition_reason: "register",
                registered_pane: Some("%42"),
                live_owner_pane: Some("%42"),
                ..facts
            }
        ));
    }

    #[test]
    fn degraded_refusal_names_rebind_recovery_before_input() {
        let message =
            degraded_authoritative_actor_refusal_message(DegradedAuthoritativeActorRefusal {
                harness_binary: "codex",
                file_display: "/tmp/doc.md",
                generation: 2,
                pane_id: "%42",
                reason: "supervisor health is no_socket",
                runtime_actor_state: "missing",
            });

        assert!(message.contains("refused before input"));
        assert!(message.contains("runtime_actor_state=missing"));
        assert!(message.contains("agent-doc start /tmp/doc.md"));
        assert!(message.contains("dispatch-start proof"));
    }

    #[test]
    fn runtime_guard_requires_healthy_supervisor_with_actor_state() {
        assert!(
            authoritative_actor_dispatch_guard_reason(AuthoritativeRuntimeFacts {
                health: ActorRuntimeHealth::Healthy,
                actor_state_present: true,
            })
            .is_none()
        );
        assert!(
            authoritative_actor_dispatch_guard_reason(AuthoritativeRuntimeFacts {
                health: ActorRuntimeHealth::NoSocket,
                actor_state_present: true,
            })
            .unwrap()
            .contains("no_socket")
        );
        assert!(
            authoritative_actor_dispatch_guard_reason(AuthoritativeRuntimeFacts {
                health: ActorRuntimeHealth::Healthy,
                actor_state_present: false,
            })
            .unwrap()
            .contains("missing")
        );
    }

    #[test]
    fn accepted_only_dispatch_start_proof_fails_only_when_required() {
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::CommandAcceptedOnly, true),
            DispatchStartProofDecision::FailClosedAcceptedOnly
        );
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::CommandAcceptedOnly, false),
            DispatchStartProofDecision::Accepted
        );
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::HookPromptMatched, true),
            DispatchStartProofDecision::Accepted
        );
    }

    #[test]
    fn direct_submit_outcome_separates_acceptance_from_dispatch_proof() {
        assert_eq!(
            direct_pane_submit_outcome(DirectPaneSubmitStatus::Accepted, None),
            "accepted"
        );
        assert_eq!(
            direct_pane_submit_outcome(DirectPaneSubmitStatus::TimedOut, None),
            "acceptance_unobserved"
        );
        assert_eq!(
            direct_pane_submit_outcome(
                DirectPaneSubmitStatus::TimedOut,
                Some(RoutedDispatchStartProof::HookStateAdvanced),
            ),
            "acceptance_unobserved_dispatch_proven"
        );
    }

    #[test]
    fn dispatch_only_proof_policy_requires_hook_visible_codex_and_opencode() {
        assert!(dispatch_only_dispatch_start_proof_required(
            DispatchOnlyProofPolicyFacts {
                harness_binary: "codex",
                codex_dispatch_start_tracking_enabled: true,
            }
        ));
        assert!(!dispatch_only_dispatch_start_proof_required(
            DispatchOnlyProofPolicyFacts {
                harness_binary: "codex",
                codex_dispatch_start_tracking_enabled: false,
            }
        ));
        assert!(dispatch_only_dispatch_start_proof_required(
            DispatchOnlyProofPolicyFacts {
                harness_binary: "opencode",
                codex_dispatch_start_tracking_enabled: false,
            }
        ));
        assert!(!dispatch_only_dispatch_start_proof_required(
            DispatchOnlyProofPolicyFacts {
                harness_binary: "claude",
                codex_dispatch_start_tracking_enabled: false,
            }
        ));
    }

    #[test]
    fn dispatch_only_sent_messages_preserve_proof_scope() {
        let facts = DispatchOnlyProofOutcomeFacts {
            file_display: "/tmp/doc.md",
            pane: "%7",
            harness_binary: "codex",
            delivery: DispatchOnlyReopenDelivery::DirectPaneSubmit,
            dispatch_start: RoutedDispatchStartProof::CommandAcceptedOnly,
            timeout_secs: 10,
        };

        let log = dispatch_only_sent_log_message(facts);
        assert!(log.contains("proof=accepted"));
        assert!(log.contains("proof_scope=accepted_only"));

        let refusal = accepted_only_dispatch_start_refusal_message(facts);
        assert!(refusal.contains("only pane-input acceptance proof was available"));
        assert!(refusal.contains("treating this as not dispatched"));
    }

    #[test]
    fn busy_pane_auto_fix_decision_prefers_explicit_retry_evidence() {
        assert_eq!(
            busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
                test_hook_changed: false,
                fix_made_changes: false,
                supervisor_healthy: true,
                restarted_supervisor: false,
            }),
            BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart
        );
        assert_eq!(
            busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
                test_hook_changed: true,
                fix_made_changes: false,
                supervisor_healthy: false,
                restarted_supervisor: false,
            }),
            BusyPaneAutoFixOutcome::RetryRoute
        );
        assert_eq!(
            busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
                test_hook_changed: false,
                fix_made_changes: false,
                supervisor_healthy: false,
                restarted_supervisor: true,
            }),
            BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart
        );
    }
}
