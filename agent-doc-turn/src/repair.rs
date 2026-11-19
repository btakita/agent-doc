//! Repair outcome vocabulary for turn closeout recovery.

pub const AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR: &str =
    "ambiguous preflight_started patchback";
pub const RESPONSE_PATCHBACK_UNCOMMITTED_ERROR: &str = "response_patchback_uncommitted";
pub const EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR: &str =
    "empty preflight_started cycle has no response capture";
pub const STALE_EMPTY_PREFLIGHT_TTL_SECS: u64 = 60;
/// `#qresiduewedge`: retirement reason for a captured response that repair
/// parked because the document already holds an answer under its `### Re:`
/// heading. A parked capture was never written, so it is no longer the
/// cycle's answer and never proves a free-text queue head answered.
pub const COMMITTED_HEADING_REPLAY_PARKED_REASON: &str = "committed_heading_replay_parked";

/// Evidence required to reclaim an unanswered preflight. A closeout-owner
/// release says that no closeout writer owns the cycle. The harness can still
/// be working before it has a response to capture, so release cannot settle
/// the cycle without a separate terminal turn receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmptyPreflightCancelAuthority {
    /// No proof the owning run stopped. A live first response may still be
    /// generating into this cycle, so the reclaim must refuse.
    Unproven,
    /// The caller cancelled the owning harness run before asking.
    RunCancelled,
    /// The closeout owner released this cycle. Reclaim also requires a newer
    /// harness turn-end receipt for the authoritative pane.
    OwnerReleased,
    /// `#runctrlclaude`: the owning pane's harness reported a session boundary
    /// (a `SessionStart` for `/clear`, a fresh start, or a resume; or a settled
    /// interrupt record in its transcript) strictly AFTER the cycle was last
    /// (re)entered by preflight, and no newer turn-active lease exists for that
    /// pane. A conversation that was cleared or interrupted cannot finish a
    /// response into this cycle, and any NEW turn either holds a fresh lease or
    /// re-entered the cycle (bumping its `updated_at` past the receipt), so
    /// both halves together prove the cycle is orphaned without waiting out the
    /// stall deadline. See [`harness_turn_end_proves_orphaned_preflight`].
    HarnessTurnEnded,
    /// The authoritative route observed the exact bound pane at a structurally
    /// empty dispatch-ready composer across two captures, while the turn fence
    /// reports no live lease. This covers harnesses such as Grok whose manual
    /// interrupt + `/clear` transition is visible in the pane but cannot emit a
    /// hook/transcript turn-end receipt. The controller still applies the
    /// terminal cycle + actor + inactive-turn fence atomically.
    StableDispatchReadyPane,
}

/// `#runctrlclaude`: does a harness turn-end receipt prove an empty
/// `preflight_started` cycle is orphaned?
///
/// - `turn_live`: a fresh turn-active lease exists for the owning pane NOW.
///   A live lease always protects, whatever the receipt says.
/// - `turn_ended_at`: the newest harness-authored turn-end receipt for the
///   owning pane (Unix seconds), if any.
/// - `cycle_updated_at`: the cycle's `updated_at`. A re-entrant preflight keeps
///   the cycle id but bumps this, so a later turn that re-entered the cycle is
///   never mistaken for the cleared one.
///
/// The comparison is STRICT. Receipts and cycle times are whole seconds, so a
/// receipt in the same second as the preflight is ambiguous about order and
/// must not authorize; the stall-deadline path still covers that case.
pub const fn harness_turn_end_proves_orphaned_preflight(
    turn_live: bool,
    turn_ended_at: Option<u64>,
    cycle_updated_at: u64,
) -> bool {
    if turn_live {
        return false;
    }
    match turn_ended_at {
        Some(ended_at) => ended_at > cycle_updated_at,
        None => false,
    }
}

impl EmptyPreflightCancelAuthority {
    /// Whether this proof authorizes abandoning an empty preflight cycle.
    pub const fn authorizes_cancel(self) -> bool {
        !matches!(self, Self::Unproven)
    }

    /// Whether this proof must ALSO observe a harness turn-end receipt newer
    /// than the cycle (and no live turn lease) before it may reclaim.
    pub const fn requires_harness_turn_end(self) -> bool {
        matches!(self, Self::OwnerReleased | Self::HarnessTurnEnded)
    }

    /// Whether the reclaim must read the authoritative actor's turn fence and
    /// abandon only while that fence is unchanged.
    pub const fn requires_turn_fence(self) -> bool {
        matches!(
            self,
            Self::OwnerReleased | Self::HarnessTurnEnded | Self::StableDispatchReadyPane
        )
    }

    /// Ops-log token naming which proof was (or was not) carried.
    pub const fn proof(self) -> &'static str {
        match self {
            Self::Unproven => "run_cancel_not_proven",
            Self::RunCancelled => "run_cancelled",
            Self::OwnerReleased => "owner_released",
            Self::HarnessTurnEnded => "harness_turn_ended",
            Self::StableDispatchReadyPane => "stable_dispatch_ready_pane",
        }
    }
}

/// Outcome of a preflight-cycle reclaim request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelOutcome {
    /// An empty `preflight_started` cycle with no response capture was
    /// abandoned after the caller proved the owning run was canceled, so the
    /// next dispatch can start a fresh cycle immediately.
    Abandoned,
    /// Nothing to reclaim: no open cycle for this document.
    NoOpenCycle,
    /// The open cycle is protected: run cancellation was not proven, it
    /// advanced past `preflight_started`, or it already owns a response capture.
    /// Reclaim waits for the normal closeout or staleness path instead.
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
    BlockedCaptureQuarantined,
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
            Self::BlockedCaptureQuarantined => "blocked_capture_quarantined",
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
                "retired a wedged write-applied capture and rebuilt recovery projections from the current document"
            }
            Self::StalePreflightLockRepaired => "closed a stale preflight-started cycle",
            Self::StalePreflightCycleAbandoned => "abandoned a stale empty preflight-started cycle",
            Self::CommitBoundaryRecovered => "recovered a missing commit boundary",
            Self::TemplateNormalized => "normalized template drift before closeout",
            Self::CompletedBacklogReaped => "reaped a stale completed backlog item during recovery",
            Self::BlockedCaptureQuarantined => {
                "quarantined an invalid transcript-shaped capture and reopened the document for a fresh response"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#duplicatepreflightunblock`: the reclaim must accept more than one
    /// proof, or it is reachable from exactly one operator-typed command.
    #[test]
    fn owner_release_authorizes_the_reclaim_and_only_unproven_refuses() {
        assert!(
            !EmptyPreflightCancelAuthority::Unproven.authorizes_cancel(),
            "with no proof a live first response may still be generating"
        );
        assert!(
            EmptyPreflightCancelAuthority::RunCancelled.authorizes_cancel(),
            "the operator path must keep working"
        );
        assert!(
            EmptyPreflightCancelAuthority::OwnerReleased.authorizes_cancel(),
            "a released owner is the second reclaim proof"
        );
    }

    /// `#duplicatepreflightunblock`: release alone is NOT the same fact run
    /// cancellation proves. The projection also reports a release when there
    /// was no owner to release, which reads identically to a fresh cycle whose
    /// model has not answered yet — reclaiming on it abandoned a live turn.
    #[test]
    fn owner_release_must_confirm_a_harness_turn_end() {
        assert!(
            EmptyPreflightCancelAuthority::OwnerReleased.requires_harness_turn_end(),
            "release alone cannot tell whether a cycle can still deliver a response"
        );
        assert!(
            !EmptyPreflightCancelAuthority::RunCancelled.requires_harness_turn_end(),
            "the caller stopped the run itself; there is nothing left to wait for"
        );
    }

    #[test]
    fn harness_turn_end_authority_needs_a_fence_but_never_a_stall() {
        let authority = EmptyPreflightCancelAuthority::HarnessTurnEnded;
        assert!(authority.authorizes_cancel());
        assert!(authority.requires_harness_turn_end());
        assert!(authority.requires_turn_fence());
        assert!(EmptyPreflightCancelAuthority::OwnerReleased.requires_turn_fence());
        assert!(EmptyPreflightCancelAuthority::OwnerReleased.requires_harness_turn_end());
        assert!(!EmptyPreflightCancelAuthority::RunCancelled.requires_turn_fence());
        assert!(!EmptyPreflightCancelAuthority::Unproven.requires_turn_fence());
    }

    #[test]
    fn stable_dispatch_ready_pane_authority_needs_a_fence_but_not_a_receipt_or_stall() {
        let authority = EmptyPreflightCancelAuthority::StableDispatchReadyPane;
        assert!(authority.authorizes_cancel());
        assert!(authority.requires_turn_fence());
        assert!(!authority.requires_harness_turn_end());
    }

    /// `#runctrlclaude` truth table. The only authorizing row is: no live
    /// lease AND a receipt strictly newer than the cycle's last (re)entry.
    #[test]
    fn harness_turn_end_receipt_truth_table() {
        let cycle_updated_at = 1_000;
        // Live lease: a new turn holds the pane; never authorize.
        for ended in [None, Some(999), Some(1_000), Some(1_001), Some(5_000)] {
            assert!(!harness_turn_end_proves_orphaned_preflight(
                true,
                ended,
                cycle_updated_at
            ));
        }
        // No receipt: nothing proves the harness reached a boundary.
        assert!(!harness_turn_end_proves_orphaned_preflight(
            false,
            None,
            cycle_updated_at
        ));
        // A receipt from before the cycle, or the same ambiguous second.
        assert!(!harness_turn_end_proves_orphaned_preflight(
            false,
            Some(999),
            cycle_updated_at
        ));
        assert!(!harness_turn_end_proves_orphaned_preflight(
            false,
            Some(1_000),
            cycle_updated_at
        ));
        // `/clear` after the preflight, no newer turn: orphaned.
        assert!(harness_turn_end_proves_orphaned_preflight(
            false,
            Some(1_001),
            cycle_updated_at
        ));
    }

    /// Each proof is distinguishable in the ops log, so a wedge can be told
    /// apart from a reclaim that was refused for want of proof.
    #[test]
    fn each_cancel_authority_names_a_distinct_proof() {
        let proofs = [
            EmptyPreflightCancelAuthority::Unproven.proof(),
            EmptyPreflightCancelAuthority::RunCancelled.proof(),
            EmptyPreflightCancelAuthority::OwnerReleased.proof(),
            EmptyPreflightCancelAuthority::HarnessTurnEnded.proof(),
            EmptyPreflightCancelAuthority::StableDispatchReadyPane.proof(),
        ];

        assert_eq!(
            EmptyPreflightCancelAuthority::Unproven.proof(),
            "run_cancel_not_proven",
            "the refusal token is grepped in ops logs; keep it stable"
        );
        let mut unique = proofs.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), proofs.len(), "proof tokens must not collide");
    }

    /// `#duplicatepreflightunblock`: the authority is only worth having if the
    /// drain that was wedged actually consults it.
    ///
    /// The whole defect was a wiring one — `cancel_empty_preflight` was called
    /// on a path that could never prove its authority, so the branch READ like
    /// automatic recovery while being structurally dead, and no test noticed.
    /// The live state (a duplicate dispatch against an owner-released empty
    /// preflight) is not reachable from a unit test, so guard the wiring
    /// structurally, the same way `#retainedmutdrop` guards its provenance
    /// sites.
    #[test]
    fn the_route_closeout_drain_reclaims_after_a_released_owner() {
        let drain = include_str!("../../agent-doc-route-io/src/closeout_drain.rs");

        // A declared-but-uncalled effect is exactly the shape that made the
        // original branch dead, so assert the CALL, not the field.
        assert!(
            drain.contains("(effects.cancel_empty_preflight_after_owner_release)(file)"),
            "the route closeout drain must CALL the owner-release reclaim; a \
             declared-but-unused effect is what let the wedge look handled"
        );
        let (_, after_owner_release) = drain
            .split_once("CloseoutDrainProjection::RecoverAfterOwnerRelease")
            .expect("the drain must still branch on a released owner");
        assert!(
            after_owner_release
                .contains("(effects.cancel_empty_preflight_after_owner_release)(file)"),
            "the reclaim belongs on the owner-release branch — that is the only \
             place the proof exists"
        );

        let runtime = include_str!("../../agent-doc-route-io/src/runtime_effects.rs");
        assert!(
            runtime.contains("cancel_preflight_cycle_after_owner_release"),
            "the drain effect must bind to the owner-release authority; binding \
             it to the unproven entry point is what made the original branch \
             structurally dead"
        );
    }

    /// `#runctrlclaude`: the drain's FIRST reclaim step must carry the
    /// harness turn-end proof. Bound to the unproven entry point it refused by
    /// construction (`run_cancel_not_proven`), which is the exact wedge seen
    /// after Ctrl-C + `/clear`.
    #[test]
    fn the_route_closeout_drain_first_reclaim_carries_the_harness_turn_end_proof() {
        let runtime = include_str!("../../agent-doc-route-io/src/runtime_effects.rs");
        let (_, cancel_fn) = runtime
            .split_once("fn route_cancel_empty_preflight(file: &Path)")
            .expect("the route still binds an empty-preflight cancel effect");
        let body = cancel_fn.split("\nfn ").next().unwrap();
        assert!(
            body.contains("cancel_preflight_cycle_after_harness_turn_end("),
            "route_cancel_empty_preflight must use the harness-turn-end authority: {body}"
        );
        assert!(
            !body.contains("cancel_preflight_cycle(\n"),
            "the unproven entry point can never reclaim"
        );

        let idle_watch = include_str!("../../agent-doc-start-runtime-io/src/idle_watch.rs");
        assert!(
            idle_watch.contains("cancel_preflight_cycle_after_harness_turn_end("),
            "the idle supervisor must also try the harness-turn-end reclaim"
        );
    }

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
