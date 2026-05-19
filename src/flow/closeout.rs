use super::types::{CloseoutState, FlowEvent, FlowName, FlowOutcome, FlowStage};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CloseoutGuardReason {
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
    AlreadyCommitted,
    SnapshotDiffersFromHead,
    ParentPointerStale,
    SessionCheckInterrupted,
    CommitBoundaryRecovered,
    StalePreflightLockRepaired,
    StalePreflightCycleAbandoned,
}

impl CloseoutGuardReason {
    pub(crate) const fn as_str(self) -> &'static str {
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
            Self::AlreadyCommitted => "already_committed",
            Self::SnapshotDiffersFromHead => "snapshot_differs_from_head",
            Self::ParentPointerStale => "parent_pointer_stale",
            Self::SessionCheckInterrupted => "session_check_interrupted",
            Self::CommitBoundaryRecovered => "commit_boundary_recovered",
            Self::StalePreflightLockRepaired => "stale_preflight_lock_repaired",
            Self::StalePreflightCycleAbandoned => "stale_preflight_cycle_abandoned",
        }
    }
}

pub(crate) fn closeout_guard_event(
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: CloseoutGuardReason,
) -> FlowEvent {
    FlowEvent::new(FlowName::Closeout, stage, outcome).with_reason(reason.as_str())
}

pub(crate) fn log_closeout_guard_event(
    file: &Path,
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: CloseoutGuardReason,
) {
    super::proof::log_flow_event(file, closeout_guard_event(stage, outcome, reason));
}

pub(crate) fn closeout_state_from_cycle_phase(phase: &str) -> Option<CloseoutState> {
    match phase {
        "preflight_started" => Some(CloseoutState::PreflightStarted),
        "response_captured" => Some(CloseoutState::ResponseCaptured),
        "write_applied" => Some(CloseoutState::WriteApplied),
        "committed" => Some(CloseoutState::Committed),
        "abandoned" => Some(CloseoutState::Abandoned),
        _ => None,
    }
}

pub(crate) fn terminal_guard_outcome(state: CloseoutState) -> FlowOutcome {
    match state {
        CloseoutState::Committed => FlowOutcome::Completed,
        CloseoutState::Abandoned => FlowOutcome::FailedClosed,
        CloseoutState::PreflightStarted
        | CloseoutState::ResponseCaptured
        | CloseoutState::WriteApplied => FlowOutcome::Blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_is_terminal_completed() {
        assert_eq!(
            terminal_guard_outcome(CloseoutState::Committed),
            FlowOutcome::Completed
        );
    }

    #[test]
    fn closeout_guard_event_is_typed() {
        let event = closeout_guard_event(
            FlowStage::PreWriteGuard,
            FlowOutcome::Blocked,
            CloseoutGuardReason::PendingCaptureRecommendations,
        );

        assert_eq!(event.flow, FlowName::Closeout);
        assert_eq!(event.stage, FlowStage::PreWriteGuard);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(
            event.reason.as_deref(),
            Some("pending_capture_recommendations")
        );
    }
}
