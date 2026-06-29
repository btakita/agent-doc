//! Pure closeout guard vocabulary and terminal outcome policy.

use crate::CyclePhase;

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

pub const fn closeout_terminal_guard_outcome(phase: CyclePhase) -> CloseoutGuardOutcome {
    match phase {
        CyclePhase::Committed => CloseoutGuardOutcome::Completed,
        CyclePhase::Abandoned => CloseoutGuardOutcome::FailedClosed,
        CyclePhase::PreflightStarted | CyclePhase::ResponseCaptured | CyclePhase::WriteApplied => {
            CloseoutGuardOutcome::Blocked
        }
    }
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
}
