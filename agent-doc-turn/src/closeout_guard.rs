//! Pure closeout guard vocabulary and terminal outcome policy.

use crate::CyclePhase;
use agent_doc_flow::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommittedWithoutResponseBodyEvidence<'a> {
    pub phase: CyclePhase,
    pub exchange_has_response_body: bool,
    pub capture_recorded: bool,
    pub response_hash_recorded: bool,
    pub queue_turn: bool,
    pub had_pending_mutations: bool,
    pub last_event: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommittedWithoutResponseBodyDecision {
    Pass,
    SkipNoopCommit,
    Interrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CloseoutGuardReason {
    MissingCycleState,
    OpenCycle,
    PendingCaptureTargetMissing,
    PendingCaptureInventoryShortfall,
    PendingCapturePlanShortfall,
    PendingCapturePromisedIdsMissing,
    PendingCaptureRequired,
    PendingCaptureRecommendations,
    PendingDoneMalformedTrackedItem,
    PendingDoneMissing,
    ReviewDoneSourceNotReviewed,
    AlreadyCommitted,
    SnapshotDiffersFromHead,
    ParentPointerStale,
    SessionCheckInterrupted,
    ResponsePatchbackUncommitted,
    CommitBoundaryRecovered,
    StalePreflightLockRepaired,
    StalePreflightCycleAbandoned,
    ReplicaDeliveryPending,
}

impl CloseoutGuardReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingCycleState => "missing_cycle_state",
            Self::OpenCycle => "open_cycle",
            Self::PendingCaptureTargetMissing => "pending_capture_target_missing",
            Self::PendingCaptureInventoryShortfall => "pending_capture_inventory_shortfall",
            Self::PendingCapturePlanShortfall => "pending_capture_plan_shortfall",
            Self::PendingCapturePromisedIdsMissing => "pending_capture_promised_ids_missing",
            Self::PendingCaptureRequired => "pending_capture_required",
            Self::PendingCaptureRecommendations => "pending_capture_recommendations",
            Self::PendingDoneMalformedTrackedItem => "pending_done_malformed_tracked_item",
            Self::PendingDoneMissing => "pending_done_missing",
            Self::ReviewDoneSourceNotReviewed => "review_done_source_not_reviewed",
            Self::AlreadyCommitted => "already_committed",
            Self::SnapshotDiffersFromHead => "snapshot_differs_from_head",
            Self::ParentPointerStale => "parent_pointer_stale",
            Self::SessionCheckInterrupted => "session_check_interrupted",
            Self::ResponsePatchbackUncommitted => "response_patchback_uncommitted",
            Self::CommitBoundaryRecovered => "commit_boundary_recovered",
            Self::StalePreflightLockRepaired => "stale_preflight_lock_repaired",
            Self::StalePreflightCycleAbandoned => "stale_preflight_cycle_abandoned",
            Self::ReplicaDeliveryPending => "replica_delivery_pending",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseoutGuardOutcome {
    Completed,
    Blocked,
    FailedClosed,
}

impl CloseoutGuardOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::FailedClosed => "failed_closed",
        }
    }
}

pub fn closeout_cycle_phase_from_str(phase: &str) -> Option<CyclePhase> {
    match phase {
        "preflight_started" => Some(CyclePhase::PreflightStarted),
        "response_captured" => Some(CyclePhase::ResponseCaptured),
        "write_applied" => Some(CyclePhase::WriteApplied),
        "committed" => Some(CyclePhase::Committed),
        "abandoned" => Some(CyclePhase::Abandoned),
        _ => None,
    }
}

pub fn closeout_guard_event(
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: CloseoutGuardReason,
) -> FlowEvent {
    FlowEvent::new(FlowName::Closeout, stage, outcome).with_reason(reason.as_str())
}

pub const fn closeout_terminal_guard_outcome(phase: CyclePhase) -> CloseoutGuardOutcome {
    match phase {
        CyclePhase::Committed => CloseoutGuardOutcome::Completed,
        CyclePhase::Abandoned => CloseoutGuardOutcome::FailedClosed,
        CyclePhase::PreflightStarted | CyclePhase::ResponseCaptured | CyclePhase::WriteApplied => {
            CloseoutGuardOutcome::Blocked
        }
    }
}

/// True when committed document content contains at least one assistant
/// `### Re:` response heading in `agent:exchange`.
pub fn exchange_has_assistant_response_body(content: &str) -> bool {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return false;
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return false;
    };
    exchange
        .content(content)
        .lines()
        .any(|line| line.trim_start().starts_with("### Re:"))
}

pub fn committed_without_response_body_decision(
    evidence: CommittedWithoutResponseBodyEvidence<'_>,
) -> CommittedWithoutResponseBodyDecision {
    if !matches!(evidence.phase, CyclePhase::Committed) {
        return CommittedWithoutResponseBodyDecision::Pass;
    }
    if evidence.exchange_has_response_body {
        return CommittedWithoutResponseBodyDecision::Pass;
    }
    if (evidence.capture_recorded || evidence.response_hash_recorded) && !evidence.queue_turn {
        return CommittedWithoutResponseBodyDecision::Pass;
    }
    if !evidence.had_pending_mutations {
        return CommittedWithoutResponseBodyDecision::Pass;
    }
    if evidence.last_event == "commit_already_current" {
        return CommittedWithoutResponseBodyDecision::SkipNoopCommit;
    }
    CommittedWithoutResponseBodyDecision::Interrupt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closeout_guard_reason_labels_are_stable() {
        assert_eq!(
            CloseoutGuardReason::MissingCycleState.as_str(),
            "missing_cycle_state"
        );
        assert_eq!(CloseoutGuardReason::OpenCycle.as_str(), "open_cycle");
        assert_eq!(
            CloseoutGuardReason::PendingCaptureTargetMissing.as_str(),
            "pending_capture_target_missing"
        );
        assert_eq!(
            CloseoutGuardReason::PendingCaptureInventoryShortfall.as_str(),
            "pending_capture_inventory_shortfall"
        );
        assert_eq!(
            CloseoutGuardReason::PendingCapturePlanShortfall.as_str(),
            "pending_capture_plan_shortfall"
        );
        assert_eq!(
            CloseoutGuardReason::PendingCapturePromisedIdsMissing.as_str(),
            "pending_capture_promised_ids_missing"
        );
        assert_eq!(
            CloseoutGuardReason::PendingCaptureRequired.as_str(),
            "pending_capture_required"
        );
        assert_eq!(
            CloseoutGuardReason::PendingCaptureRecommendations.as_str(),
            "pending_capture_recommendations"
        );
        assert_eq!(
            CloseoutGuardReason::PendingDoneMalformedTrackedItem.as_str(),
            "pending_done_malformed_tracked_item"
        );
        assert_eq!(
            CloseoutGuardReason::PendingDoneMissing.as_str(),
            "pending_done_missing"
        );
        assert_eq!(
            CloseoutGuardReason::ReviewDoneSourceNotReviewed.as_str(),
            "review_done_source_not_reviewed"
        );
        assert_eq!(
            CloseoutGuardReason::AlreadyCommitted.as_str(),
            "already_committed"
        );
        assert_eq!(
            CloseoutGuardReason::SnapshotDiffersFromHead.as_str(),
            "snapshot_differs_from_head"
        );
        assert_eq!(
            CloseoutGuardReason::ParentPointerStale.as_str(),
            "parent_pointer_stale"
        );
        assert_eq!(
            CloseoutGuardReason::SessionCheckInterrupted.as_str(),
            "session_check_interrupted"
        );
        assert_eq!(
            CloseoutGuardReason::ResponsePatchbackUncommitted.as_str(),
            "response_patchback_uncommitted"
        );
        assert_eq!(
            CloseoutGuardReason::CommitBoundaryRecovered.as_str(),
            "commit_boundary_recovered"
        );
        assert_eq!(
            CloseoutGuardReason::StalePreflightLockRepaired.as_str(),
            "stale_preflight_lock_repaired"
        );
        assert_eq!(
            CloseoutGuardReason::StalePreflightCycleAbandoned.as_str(),
            "stale_preflight_cycle_abandoned"
        );
        assert_eq!(
            CloseoutGuardReason::ReplicaDeliveryPending.as_str(),
            "replica_delivery_pending"
        );
    }

    #[test]
    fn closeout_terminal_guard_outcome_matches_cycle_phase() {
        assert_eq!(
            closeout_terminal_guard_outcome(CyclePhase::PreflightStarted),
            CloseoutGuardOutcome::Blocked
        );
        assert_eq!(
            closeout_terminal_guard_outcome(CyclePhase::ResponseCaptured),
            CloseoutGuardOutcome::Blocked
        );
        assert_eq!(
            closeout_terminal_guard_outcome(CyclePhase::WriteApplied),
            CloseoutGuardOutcome::Blocked
        );
        assert_eq!(
            closeout_terminal_guard_outcome(CyclePhase::Committed),
            CloseoutGuardOutcome::Completed
        );
        assert_eq!(
            closeout_terminal_guard_outcome(CyclePhase::Abandoned),
            CloseoutGuardOutcome::FailedClosed
        );
    }

    #[test]
    fn closeout_cycle_phase_labels_parse() {
        assert_eq!(
            closeout_cycle_phase_from_str("preflight_started"),
            Some(CyclePhase::PreflightStarted)
        );
        assert_eq!(
            closeout_cycle_phase_from_str("response_captured"),
            Some(CyclePhase::ResponseCaptured)
        );
        assert_eq!(
            closeout_cycle_phase_from_str("write_applied"),
            Some(CyclePhase::WriteApplied)
        );
        assert_eq!(
            closeout_cycle_phase_from_str("committed"),
            Some(CyclePhase::Committed)
        );
        assert_eq!(
            closeout_cycle_phase_from_str("abandoned"),
            Some(CyclePhase::Abandoned)
        );
        assert_eq!(closeout_cycle_phase_from_str("unknown"), None);
    }

    #[test]
    fn closeout_guard_event_is_typed() {
        let event = closeout_guard_event(
            FlowStage::PreCommitGuard,
            FlowOutcome::Blocked,
            CloseoutGuardReason::PendingCaptureRecommendations,
        );

        assert_eq!(event.flow, FlowName::Closeout);
        assert_eq!(event.stage, FlowStage::PreCommitGuard);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(
            event.reason.as_deref(),
            Some("pending_capture_recommendations")
        );
    }

    #[test]
    fn closeout_guard_event_carries_review_done_reason() {
        let event = closeout_guard_event(
            FlowStage::PreCommitGuard,
            FlowOutcome::FailedClosed,
            CloseoutGuardReason::ReviewDoneSourceNotReviewed,
        );

        assert_eq!(event.flow, FlowName::Closeout);
        assert_eq!(event.stage, FlowStage::PreCommitGuard);
        assert_eq!(event.outcome, FlowOutcome::FailedClosed);
        assert_eq!(
            event.reason.as_deref(),
            Some("review_done_source_not_reviewed")
        );
    }

    #[test]
    fn exchange_has_assistant_response_body_requires_re_heading_in_exchange() {
        let with_response = concat!(
            "<!-- agent:exchange -->\n",
            "### Session Summary\n\nCompacted.\n\n",
            "### Re: do [#task]\n\nDone.\n",
            "<!-- /agent:exchange -->\n",
        );
        let summary_only = concat!(
            "<!-- agent:exchange -->\n",
            "### Session Summary\n\nCompacted.\n",
            "<!-- /agent:exchange -->\n",
        );
        let outside_exchange = concat!(
            "### Re: do [#task]\n\nDone.\n\n",
            "<!-- agent:exchange -->\n",
            "### Session Summary\n\nCompacted.\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(exchange_has_assistant_response_body(with_response));
        assert!(!exchange_has_assistant_response_body(summary_only));
        assert!(!exchange_has_assistant_response_body(outside_exchange));
        assert!(!exchange_has_assistant_response_body("not <!-- closed"));
    }

    #[test]
    fn committed_without_response_body_decision_interrupts_only_missing_response_writes() {
        let base = CommittedWithoutResponseBodyEvidence {
            phase: CyclePhase::Committed,
            exchange_has_response_body: false,
            capture_recorded: false,
            response_hash_recorded: false,
            queue_turn: false,
            had_pending_mutations: true,
            last_event: "commit_success",
        };

        assert_eq!(
            committed_without_response_body_decision(base),
            CommittedWithoutResponseBodyDecision::Interrupt
        );
        assert_eq!(
            committed_without_response_body_decision(CommittedWithoutResponseBodyEvidence {
                phase: CyclePhase::WriteApplied,
                ..base
            }),
            CommittedWithoutResponseBodyDecision::Pass
        );
        assert_eq!(
            committed_without_response_body_decision(CommittedWithoutResponseBodyEvidence {
                exchange_has_response_body: true,
                ..base
            }),
            CommittedWithoutResponseBodyDecision::Pass
        );
        assert_eq!(
            committed_without_response_body_decision(CommittedWithoutResponseBodyEvidence {
                capture_recorded: true,
                ..base
            }),
            CommittedWithoutResponseBodyDecision::Pass
        );
        assert_eq!(
            committed_without_response_body_decision(CommittedWithoutResponseBodyEvidence {
                capture_recorded: true,
                queue_turn: true,
                ..base
            }),
            CommittedWithoutResponseBodyDecision::Interrupt
        );
        assert_eq!(
            committed_without_response_body_decision(CommittedWithoutResponseBodyEvidence {
                had_pending_mutations: false,
                ..base
            }),
            CommittedWithoutResponseBodyDecision::Pass
        );
        assert_eq!(
            committed_without_response_body_decision(CommittedWithoutResponseBodyEvidence {
                last_event: "commit_already_current",
                ..base
            }),
            CommittedWithoutResponseBodyDecision::SkipNoopCommit
        );
    }
}
