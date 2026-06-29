//! Pure closeout recovery policy.
//!
//! Orchestration owns file, git, and sidecar mutation effects. This module owns
//! action-independent turn recovery decisions that can be proven from document
//! content facts.

/// Which side of a metadata-only drift is authoritative for closeout recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataDriftAuthority {
    /// The local side (snapshot for queue metadata drift, visible file for
    /// sidecar-visible drift) is authoritative and can be committed forward.
    Local,
    /// HEAD is authoritative and the local side should be restored from it.
    Head,
    /// Neither side is provably authoritative; recovery must fail closed.
    Ambiguous,
}

/// Why the closeout recovery mutation primitive is changing durable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseoutRecoveryMutationReason {
    BenignReplayBaseline,
    QueueOnlyReplayBaseline,
    CommitQueueMetadataDrift,
    ResetFromVisible,
    RestoreHeadMetadata,
    RetireWedgedWriteAppliedCapture,
    RetireSupersededCapturedOnlyOrphan,
    RespectManualTailRemoval,
}

impl CloseoutRecoveryMutationReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BenignReplayBaseline => "benign_replay_baseline",
            Self::QueueOnlyReplayBaseline => "queue_only_replay_baseline",
            Self::CommitQueueMetadataDrift => "commit_queue_metadata_drift",
            Self::ResetFromVisible => "reset_from_visible",
            Self::RestoreHeadMetadata => "restore_head_metadata",
            Self::RetireWedgedWriteAppliedCapture => "retire_wedged_write_applied_capture",
            Self::RetireSupersededCapturedOnlyOrphan => "retire_superseded_captured_only_orphan",
            Self::RespectManualTailRemoval => "respect_manual_tail_removal",
        }
    }

    pub const fn capture_refresh_event(self) -> &'static str {
        match self {
            Self::QueueOnlyReplayBaseline => "capture_baseline_refreshed_for_queue_only_drift",
            _ => "capture_baseline_refreshed_for_benign_drift",
        }
    }

    pub const fn capture_refresh_message(self) -> &'static str {
        match self {
            Self::QueueOnlyReplayBaseline => "queue-only drift detected",
            _ => "benign drift detected",
        }
    }
}

/// Decide the authoritative side of a content-equal metadata-only drift between
/// a `local` document string (the candidate to commit) and the committed `head`.
///
/// The decision turns on the live auto-queue continuation signal
/// (`#recovery-drift-authoritative-side`). Because the caller has already proven
/// the content components are byte-identical, the only durable state the diff can
/// destroy is an active queue continuation. Legitimate consumption of a queue
/// head always shows up as response/content drift, so a continuation that exists
/// in HEAD but is gone or re-headed in a metadata-only local drift cannot have
/// been legitimately consumed.
pub fn metadata_drift_authority(local: &str, head: &str) -> MetadataDriftAuthority {
    let local_head = agent_doc_queue::queue_continuation::live_continuation_head(local);
    let head_head = agent_doc_queue::queue_continuation::live_continuation_head(head);
    match (local_head, head_head) {
        // HEAD carries a live continuation that the local side dropped entirely
        // (deactivated / drained / fenced) with no consuming response.
        (None, Some(_)) => MetadataDriftAuthority::Head,
        // Both sides carry a live continuation but with different ready heads,
        // and content equality proves no response consumed the old head.
        (Some(local_id), Some(head_id)) if local_id != head_id => MetadataDriftAuthority::Ambiguous,
        // Same live head, HEAD has no live continuation at risk, or neither side
        // does.
        _ => MetadataDriftAuthority::Local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_drift_authority_head_when_local_drops_live_continuation() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("queue_active: true", "queue_active: false");
        assert_eq!(
            metadata_drift_authority(&local, head),
            MetadataDriftAuthority::Head
        );
    }

    #[test]
    fn metadata_drift_authority_local_when_no_live_head_continuation() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        assert_eq!(
            metadata_drift_authority(&local, head),
            MetadataDriftAuthority::Local
        );
    }

    #[test]
    fn metadata_drift_authority_ambiguous_when_live_heads_diverge() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("- do [#a]", "- do [#z]");
        assert_eq!(
            metadata_drift_authority(&local, head),
            MetadataDriftAuthority::Ambiguous
        );
    }

    #[test]
    fn closeout_recovery_mutation_reason_labels_are_stable() {
        assert_eq!(
            CloseoutRecoveryMutationReason::BenignReplayBaseline.as_str(),
            "benign_replay_baseline"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::QueueOnlyReplayBaseline.as_str(),
            "queue_only_replay_baseline"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::CommitQueueMetadataDrift.as_str(),
            "commit_queue_metadata_drift"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::ResetFromVisible.as_str(),
            "reset_from_visible"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::RestoreHeadMetadata.as_str(),
            "restore_head_metadata"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::RetireWedgedWriteAppliedCapture.as_str(),
            "retire_wedged_write_applied_capture"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::RetireSupersededCapturedOnlyOrphan.as_str(),
            "retire_superseded_captured_only_orphan"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::RespectManualTailRemoval.as_str(),
            "respect_manual_tail_removal"
        );
    }

    #[test]
    fn closeout_recovery_mutation_reason_owns_capture_refresh_labels() {
        assert_eq!(
            CloseoutRecoveryMutationReason::QueueOnlyReplayBaseline.capture_refresh_event(),
            "capture_baseline_refreshed_for_queue_only_drift"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::QueueOnlyReplayBaseline.capture_refresh_message(),
            "queue-only drift detected"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::BenignReplayBaseline.capture_refresh_event(),
            "capture_baseline_refreshed_for_benign_drift"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::BenignReplayBaseline.capture_refresh_message(),
            "benign drift detected"
        );
    }
}
