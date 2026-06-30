//! Pure queue drain and context-clear policy.
//!
//! These decisions answer whether the realtime queue can dispatch its next
//! trigger or context clear. They do not read documents, inspect panes, or
//! submit commands; callers provide the observed executor/document facts.

pub const CLEAR_COOLDOWN_RESUME_IDLE_TICKS: u32 = 4;
pub const CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleQueueDrainDecision {
    Dispatch,
    SkipClearCooldown,
    SkipNotIdle,
    SkipTurnActive,
    SkipNoActiveHead,
    SkipEditorTyping,
    SkipAlreadyDispatched,
    SkipRouteSubmitInFlight,
    SkipSelfDrivingLoopOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleQueueContextResetDecision {
    Reset,
    SkipNoActiveHead,
    SkipNotIdle,
    SkipTurnActive,
    SkipEditorTyping,
    SkipRouteSubmitInFlight,
    SkipAlreadyResetHead,
    SkipNoResetNeeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleQueueContextClearInFlightDecision {
    Ignore,
    WaitForIdle,
    ResubmitPendingClear,
    WaitForPendingClear,
    AwaitSettle,
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleQueueContextClearInFlightFacts {
    pub marker_active: bool,
    pub prompt_visible: bool,
    pub turn_active: bool,
    pub route_submit_in_flight: bool,
    pub clear_already_pending: Option<bool>,
    pub already_resubmitted: bool,
    pub settled_idle_ticks: u32,
    pub settle_threshold: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleQueueContextClearInFlightSettleFacts {
    pub awaiting_clear_settle: bool,
    pub prompt_visible: bool,
    pub turn_active: bool,
    pub route_submit_in_flight: bool,
    pub clear_already_pending: Option<bool>,
    pub settled_idle_ticks: u32,
    pub settle_threshold: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleQueueContextClearInFlightSettle {
    pub settled_idle_ticks: u32,
    pub settled_now: bool,
}

pub fn clean_session_head_forces_context_reset(
    active_head_is_clean_session: bool,
    clear_cooldown_active: bool,
) -> bool {
    active_head_is_clean_session && !clear_cooldown_active
}

pub fn idle_queue_context_reset_decision(
    prompt_visible: bool,
    turn_active: bool,
    route_submit_in_flight: bool,
    active_head: Option<&str>,
    last_context_reset_head: Option<&str>,
    reset_required: bool,
) -> IdleQueueContextResetDecision {
    let Some(head) = active_head else {
        return IdleQueueContextResetDecision::SkipNoActiveHead;
    };
    if !prompt_visible {
        return IdleQueueContextResetDecision::SkipNotIdle;
    }
    if turn_active {
        return IdleQueueContextResetDecision::SkipTurnActive;
    }
    if route_submit_in_flight {
        return IdleQueueContextResetDecision::SkipRouteSubmitInFlight;
    }
    if last_context_reset_head == Some(head) {
        return IdleQueueContextResetDecision::SkipAlreadyResetHead;
    }
    if reset_required {
        IdleQueueContextResetDecision::Reset
    } else {
        IdleQueueContextResetDecision::SkipNoResetNeeded
    }
}

pub fn idle_queue_context_reset_decision_with_editor_typing(
    prompt_visible: bool,
    turn_active: bool,
    route_submit_in_flight: bool,
    editor_typing_active: bool,
    active_head: Option<&str>,
    last_context_reset_head: Option<&str>,
    reset_required: bool,
) -> IdleQueueContextResetDecision {
    if active_head.is_some() && editor_typing_active {
        return IdleQueueContextResetDecision::SkipEditorTyping;
    }
    idle_queue_context_reset_decision(
        prompt_visible,
        turn_active,
        route_submit_in_flight,
        active_head,
        last_context_reset_head,
        reset_required,
    )
}

pub fn idle_queue_drain_decision(
    clear_cooldown_active: bool,
    prompt_visible: bool,
    turn_active: bool,
    self_driving_loop_active: bool,
    route_submit_in_flight: bool,
    active_head: Option<&str>,
    last_dispatched: Option<&str>,
) -> IdleQueueDrainDecision {
    if clear_cooldown_active {
        return IdleQueueDrainDecision::SkipClearCooldown;
    }
    match active_head {
        None => IdleQueueDrainDecision::SkipNoActiveHead,
        Some(_) if !prompt_visible => IdleQueueDrainDecision::SkipNotIdle,
        Some(_) if turn_active => IdleQueueDrainDecision::SkipTurnActive,
        Some(_) if route_submit_in_flight => IdleQueueDrainDecision::SkipRouteSubmitInFlight,
        Some(_) if self_driving_loop_active => IdleQueueDrainDecision::SkipSelfDrivingLoopOwner,
        Some(head) if last_dispatched == Some(head) => {
            IdleQueueDrainDecision::SkipAlreadyDispatched
        }
        Some(_) => IdleQueueDrainDecision::Dispatch,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleQueueDrainDecisionFacts<'a> {
    pub clear_cooldown_active: bool,
    pub prompt_visible: bool,
    pub turn_active: bool,
    pub self_driving_loop_active: bool,
    pub route_submit_in_flight: bool,
    pub editor_typing_active: bool,
    pub active_head: Option<&'a str>,
    pub last_dispatched: Option<&'a str>,
}

pub fn idle_queue_drain_decision_with_editor_typing(
    facts: IdleQueueDrainDecisionFacts<'_>,
) -> IdleQueueDrainDecision {
    if !facts.clear_cooldown_active && facts.active_head.is_some() && facts.editor_typing_active {
        return IdleQueueDrainDecision::SkipEditorTyping;
    }
    idle_queue_drain_decision(
        facts.clear_cooldown_active,
        facts.prompt_visible,
        facts.turn_active,
        facts.self_driving_loop_active,
        facts.route_submit_in_flight,
        facts.active_head,
        facts.last_dispatched,
    )
}

pub fn clear_cooldown_resume_ready(
    clear_cooldown_active: bool,
    has_active_head: bool,
    prompt_visible: bool,
    turn_active: bool,
    deferred_operator_clear_pending: bool,
    settled_idle_ticks: u32,
    resume_threshold: u32,
) -> bool {
    clear_cooldown_active
        && has_active_head
        && prompt_visible
        && !turn_active
        && !deferred_operator_clear_pending
        && settled_idle_ticks >= resume_threshold
}

pub fn drain_blocked_awaiting_clear_settle(
    awaiting_clear_settle: bool,
    prompt_visible: bool,
    turn_active: bool,
    settled_idle_ticks: u32,
    settle_threshold: u32,
) -> bool {
    if !awaiting_clear_settle {
        return false;
    }
    if !prompt_visible || turn_active {
        return true;
    }
    settled_idle_ticks < settle_threshold
}

pub fn idle_queue_context_clear_in_flight_decision(
    facts: IdleQueueContextClearInFlightFacts,
) -> IdleQueueContextClearInFlightDecision {
    if !facts.marker_active {
        return IdleQueueContextClearInFlightDecision::Ignore;
    }
    if !facts.prompt_visible || facts.turn_active || facts.route_submit_in_flight {
        return IdleQueueContextClearInFlightDecision::WaitForIdle;
    }
    if drain_dispatch_dedup_skip(facts.clear_already_pending) {
        return if facts.already_resubmitted {
            IdleQueueContextClearInFlightDecision::WaitForPendingClear
        } else {
            IdleQueueContextClearInFlightDecision::ResubmitPendingClear
        };
    }
    if facts.settled_idle_ticks >= facts.settle_threshold {
        IdleQueueContextClearInFlightDecision::Settled
    } else {
        IdleQueueContextClearInFlightDecision::AwaitSettle
    }
}

pub fn context_clear_in_flight_marker_active(
    written_at: u64,
    now_secs: u64,
    ttl_secs: u64,
) -> bool {
    now_secs.saturating_sub(written_at) <= ttl_secs
}

pub fn idle_queue_context_clear_in_flight_settle_ticks(
    facts: IdleQueueContextClearInFlightSettleFacts,
) -> IdleQueueContextClearInFlightSettle {
    let settled_idle_ticks = if facts.awaiting_clear_settle
        && facts.prompt_visible
        && !facts.turn_active
        && !facts.route_submit_in_flight
        && !drain_dispatch_dedup_skip(facts.clear_already_pending)
    {
        facts.settled_idle_ticks.saturating_add(1)
    } else {
        0
    };
    IdleQueueContextClearInFlightSettle {
        settled_idle_ticks,
        settled_now: facts.awaiting_clear_settle && settled_idle_ticks >= facts.settle_threshold,
    }
}

pub fn drain_dispatch_dedup_skip(payload_already_pending: Option<bool>) -> bool {
    matches!(payload_already_pending, Some(true))
}

pub fn stale_drain_recycle_yield_requested(
    would_recycle_at_boundary: bool,
    drain_owner_active: bool,
    turn_boundary: bool,
) -> bool {
    would_recycle_at_boundary && drain_owner_active && !turn_boundary
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BetweenTurnCommandKind {
    Clear,
    AgentDoc,
}

impl BetweenTurnCommandKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "/clear",
            Self::AgentDoc => "/agent-doc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BetweenTurnEnqueuePlan {
    pub kept: Vec<BetweenTurnCommandKind>,
    pub deduped: usize,
}

impl BetweenTurnEnqueuePlan {
    pub fn kept_labels(&self) -> String {
        self.kept
            .iter()
            .map(|kind| kind.label())
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn command_occurrences(text: &str, command: &str) -> usize {
    let command = command.trim();
    if command.is_empty() {
        return 0;
    }
    text.match_indices(command).count()
}

pub fn between_turn_enqueue_plan<'a>(
    requested: impl IntoIterator<Item = &'a str>,
    clear_command: &str,
    trigger_command: &str,
) -> BetweenTurnEnqueuePlan {
    let mut clear_count = 0usize;
    let mut trigger_count = 0usize;
    for item in requested {
        clear_count += command_occurrences(item, clear_command);
        trigger_count += command_occurrences(item, trigger_command);
    }
    let mut kept = Vec::with_capacity(2);
    if clear_count > 0 {
        kept.push(BetweenTurnCommandKind::Clear);
    }
    if trigger_count > 0 {
        kept.push(BetweenTurnCommandKind::AgentDoc);
    }
    BetweenTurnEnqueuePlan {
        kept,
        deduped: clear_count
            .saturating_add(trigger_count)
            .saturating_sub(clear_count.min(1).saturating_add(trigger_count.min(1))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_queue_drain_dispatches_only_at_ready_boundary() {
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::Dispatch
        );
        assert_eq!(
            idle_queue_drain_decision(false, false, false, false, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipNotIdle
        );
        assert_eq!(
            idle_queue_drain_decision(false, true, true, false, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipTurnActive
        );
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, false, None, None),
            IdleQueueDrainDecision::SkipNoActiveHead
        );
    }

    #[test]
    fn idle_queue_drain_respects_dedup_routes_loops_and_typing() {
        assert_eq!(
            idle_queue_drain_decision(
                false,
                true,
                false,
                false,
                false,
                Some("do [#a]"),
                Some("do [#a]")
            ),
            IdleQueueDrainDecision::SkipAlreadyDispatched
        );
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, true, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipRouteSubmitInFlight
        );
        assert_eq!(
            idle_queue_drain_decision(false, true, false, true, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipSelfDrivingLoopOwner
        );
        assert_eq!(
            idle_queue_drain_decision_with_editor_typing(IdleQueueDrainDecisionFacts {
                clear_cooldown_active: false,
                prompt_visible: true,
                turn_active: false,
                self_driving_loop_active: false,
                route_submit_in_flight: false,
                editor_typing_active: true,
                active_head: Some("do [#a]"),
                last_dispatched: None,
            }),
            IdleQueueDrainDecision::SkipEditorTyping
        );
    }

    #[test]
    fn context_reset_waits_for_executor_and_editor_safety() {
        assert_eq!(
            idle_queue_context_reset_decision(true, false, false, Some("do [#a]"), None, true),
            IdleQueueContextResetDecision::Reset
        );
        assert_eq!(
            idle_queue_context_reset_decision(
                true,
                false,
                false,
                Some("do [#a]"),
                Some("do [#a]"),
                true
            ),
            IdleQueueContextResetDecision::SkipAlreadyResetHead
        );
        assert_eq!(
            idle_queue_context_reset_decision_with_editor_typing(
                true,
                false,
                false,
                true,
                Some("do [#a]"),
                None,
                true
            ),
            IdleQueueContextResetDecision::SkipEditorTyping
        );
    }

    #[test]
    fn clear_cooldown_and_settle_rules_hold_until_idle_threshold() {
        assert!(clear_cooldown_resume_ready(
            true,
            true,
            true,
            false,
            false,
            CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
            CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
        ));
        assert!(!clear_cooldown_resume_ready(
            true,
            true,
            true,
            true,
            false,
            99,
            CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
        ));
        assert!(drain_blocked_awaiting_clear_settle(
            true,
            true,
            false,
            CLEAR_COOLDOWN_RESUME_IDLE_TICKS - 1,
            CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
        ));
        assert!(!drain_blocked_awaiting_clear_settle(
            true,
            true,
            false,
            CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
            CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
        ));
    }

    #[test]
    fn in_flight_clear_recovery_resubmits_then_settles() {
        use IdleQueueContextClearInFlightDecision::*;

        let base = IdleQueueContextClearInFlightFacts {
            marker_active: true,
            prompt_visible: true,
            turn_active: false,
            route_submit_in_flight: false,
            clear_already_pending: Some(true),
            already_resubmitted: false,
            settled_idle_ticks: 0,
            settle_threshold: 4,
        };
        assert_eq!(
            idle_queue_context_clear_in_flight_decision(base),
            ResubmitPendingClear
        );
        assert_eq!(
            idle_queue_context_clear_in_flight_decision(IdleQueueContextClearInFlightFacts {
                already_resubmitted: true,
                ..base
            }),
            WaitForPendingClear
        );
        assert_eq!(
            idle_queue_context_clear_in_flight_decision(IdleQueueContextClearInFlightFacts {
                clear_already_pending: Some(false),
                settled_idle_ticks: 4,
                ..base
            }),
            Settled
        );
    }

    #[test]
    fn context_clear_in_flight_marker_freshness_uses_saturating_age() {
        assert!(context_clear_in_flight_marker_active(
            100,
            100 + CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS,
            CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS,
        ));
        assert!(!context_clear_in_flight_marker_active(
            100,
            100 + CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS + 1,
            CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS,
        ));
        assert!(
            context_clear_in_flight_marker_active(200, 100, CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS,),
            "clock skew must not underflow into a stale marker"
        );
    }

    #[test]
    fn context_clear_settle_ticks_require_consecutive_idle_without_pending_clear() {
        let base = IdleQueueContextClearInFlightSettleFacts {
            awaiting_clear_settle: true,
            prompt_visible: true,
            turn_active: false,
            route_submit_in_flight: false,
            clear_already_pending: Some(false),
            settled_idle_ticks: CLEAR_COOLDOWN_RESUME_IDLE_TICKS - 1,
            settle_threshold: CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
        };
        assert_eq!(
            idle_queue_context_clear_in_flight_settle_ticks(base),
            IdleQueueContextClearInFlightSettle {
                settled_idle_ticks: CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
                settled_now: true,
            }
        );
        for reset_case in [
            IdleQueueContextClearInFlightSettleFacts {
                prompt_visible: false,
                ..base
            },
            IdleQueueContextClearInFlightSettleFacts {
                turn_active: true,
                ..base
            },
            IdleQueueContextClearInFlightSettleFacts {
                route_submit_in_flight: true,
                ..base
            },
            IdleQueueContextClearInFlightSettleFacts {
                clear_already_pending: Some(true),
                ..base
            },
            IdleQueueContextClearInFlightSettleFacts {
                awaiting_clear_settle: false,
                ..base
            },
        ] {
            assert_eq!(
                idle_queue_context_clear_in_flight_settle_ticks(reset_case),
                IdleQueueContextClearInFlightSettle {
                    settled_idle_ticks: 0,
                    settled_now: false,
                }
            );
        }
    }

    #[test]
    fn stale_drain_recycle_yield_policy() {
        assert!(!stale_drain_recycle_yield_requested(false, true, false));
        assert!(!stale_drain_recycle_yield_requested(true, false, false));
        assert!(!stale_drain_recycle_yield_requested(true, true, true));
        assert!(stale_drain_recycle_yield_requested(true, true, false));
    }

    #[test]
    fn between_turn_enqueue_plan_dedups_clear_and_trigger() {
        let plan = between_turn_enqueue_plan(
            [
                "/clear",
                "/agent-doc file",
                "/clear /agent-doc file/agent-doc file",
            ],
            "/clear",
            "/agent-doc",
        );

        assert_eq!(
            plan.kept,
            vec![
                BetweenTurnCommandKind::Clear,
                BetweenTurnCommandKind::AgentDoc
            ]
        );
        assert_eq!(plan.kept_labels(), "/clear,/agent-doc");
        assert_eq!(plan.deduped, 3);
    }

    #[test]
    fn clean_session_head_forces_reset_except_during_cooldown() {
        assert!(clean_session_head_forces_context_reset(true, false));
        assert!(!clean_session_head_forces_context_reset(true, true));
        assert!(!clean_session_head_forces_context_reset(false, false));
    }
}
