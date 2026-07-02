//! Repair outcome vocabulary for turn closeout recovery.

pub const AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR: &str =
    "ambiguous preflight_started patchback";
pub const RESPONSE_PATCHBACK_UNCOMMITTED_ERROR: &str = "response_patchback_uncommitted";
pub const EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR: &str =
    "empty preflight_started cycle has no response capture";
pub const STALE_EMPTY_PREFLIGHT_TTL_SECS: u64 = 60;

/// Outcome of an explicit run-cancel reclaim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelOutcome {
    /// An empty `preflight_started` cycle with no response capture was
    /// abandoned so the next dispatch can start a fresh cycle immediately.
    Abandoned,
    /// Nothing to reclaim: no open cycle for this document.
    NoOpenCycle,
    /// The open cycle is protected: it advanced past `preflight_started` or it
    /// already owns a response capture, so an explicit cancel must not discard
    /// it. Reclaim waits for the normal closeout or staleness path instead.
    Protected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairOutcome {
    Noop,
    ReplayedResponse,
    AlreadyApplied,
    ManualTailRemovalRespected,
    StaleCaptureRetired,
    StalePreflightLockRepaired,
    StalePreflightCycleAbandoned,
    CommitBoundaryRecovered,
    TemplateNormalized,
    CompletedBacklogReaped,
}

impl RepairOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Noop => "noop",
            Self::ReplayedResponse => "replayed_response",
            Self::AlreadyApplied => "already_applied",
            Self::ManualTailRemovalRespected => "manual_tail_removal_respected",
            Self::StaleCaptureRetired => "stale_capture_retired",
            Self::StalePreflightLockRepaired => "stale_preflight_lock_repaired",
            Self::StalePreflightCycleAbandoned => "stale_preflight_cycle_abandoned",
            Self::CommitBoundaryRecovered => "commit_boundary_recovered",
            Self::TemplateNormalized => "template_normalized",
            Self::CompletedBacklogReaped => "completed_backlog_reaped",
        }
    }

    pub const fn repaired(self) -> bool {
        !matches!(self, Self::Noop)
    }

    pub const fn replayed_response(self) -> bool {
        matches!(self, Self::ReplayedResponse)
    }

    pub const fn doctor_message(self) -> &'static str {
        match self {
            Self::Noop => "no repair applied",
            Self::ReplayedResponse => {
                "replayed a captured response through the normal closeout path"
            }
            Self::AlreadyApplied => {
                "completed a pending commit boundary for an already-applied response"
            }
            Self::ManualTailRemovalRespected => {
                "respected a manual assistant-tail removal while closing the cycle"
            }
            Self::StaleCaptureRetired => {
                "retired a wedged write-applied capture and rebuilt sidecars from the current document"
            }
            Self::StalePreflightLockRepaired => "closed a stale preflight-started cycle",
            Self::StalePreflightCycleAbandoned => "abandoned a stale empty preflight-started cycle",
            Self::CommitBoundaryRecovered => "recovered a missing commit boundary",
            Self::TemplateNormalized => "normalized template drift before closeout",
            Self::CompletedBacklogReaped => "reaped a stale completed backlog item during recovery",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_outcome_labels_are_stable() {
        assert_eq!(RepairOutcome::Noop.as_str(), "noop");
        assert_eq!(
            RepairOutcome::ReplayedResponse.as_str(),
            "replayed_response"
        );
        assert_eq!(RepairOutcome::AlreadyApplied.as_str(), "already_applied");
        assert_eq!(
            RepairOutcome::ManualTailRemovalRespected.as_str(),
            "manual_tail_removal_respected"
        );
        assert_eq!(
            RepairOutcome::StaleCaptureRetired.as_str(),
            "stale_capture_retired"
        );
        assert_eq!(
            RepairOutcome::StalePreflightLockRepaired.as_str(),
            "stale_preflight_lock_repaired"
        );
        assert_eq!(
            RepairOutcome::StalePreflightCycleAbandoned.as_str(),
            "stale_preflight_cycle_abandoned"
        );
        assert_eq!(
            RepairOutcome::CommitBoundaryRecovered.as_str(),
            "commit_boundary_recovered"
        );
        assert_eq!(
            RepairOutcome::TemplateNormalized.as_str(),
            "template_normalized"
        );
        assert_eq!(
            RepairOutcome::CompletedBacklogReaped.as_str(),
            "completed_backlog_reaped"
        );
    }

    #[test]
    fn repair_outcome_repair_flags_distinguish_noop() {
        assert!(!RepairOutcome::Noop.repaired());
        assert!(RepairOutcome::ReplayedResponse.repaired());
        assert!(RepairOutcome::ReplayedResponse.replayed_response());
        assert!(!RepairOutcome::AlreadyApplied.replayed_response());
    }

    #[test]
    fn cancel_outcome_vocabulary_is_stable() {
        assert_eq!(STALE_EMPTY_PREFLIGHT_TTL_SECS, 60);
        assert_eq!(
            AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR,
            "ambiguous preflight_started patchback"
        );
        assert_eq!(
            RESPONSE_PATCHBACK_UNCOMMITTED_ERROR,
            "response_patchback_uncommitted"
        );
        assert_eq!(
            EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR,
            "empty preflight_started cycle has no response capture"
        );
        assert_eq!(CancelOutcome::Abandoned, CancelOutcome::Abandoned);
        assert_ne!(CancelOutcome::NoOpenCycle, CancelOutcome::Protected);
    }
}
