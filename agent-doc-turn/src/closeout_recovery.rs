//! Pure closeout recovery policy.
//!
//! Orchestration owns file, git, and sidecar mutation effects. This module owns
//! action-independent turn recovery decisions that can be proven from document
//! content facts.

use crate::CyclePhase;

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

/// Typed closeout recovery state (`#closeout-repair-churn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseoutRecoveryState {
    /// No recovery needed.
    Clean,
    /// Cycle still open (preflight_started / response_captured / write_applied).
    OpenCycle,
    /// Committed binary-owned work but the assistant response body is missing
    /// from HEAD (no capture, or a captured body not materialized in HEAD).
    MissingResponseBody,
    /// A visible `### Re:` response was patched directly into the document
    /// outside the binary write path.
    DirectResponsePatchback,
    /// Raw `<!-- agent:NAME -->` component markers were escaped into the
    /// committed exchange instead of applied as `<!-- patch:* -->` blocks.
    EscapedTemplatePatch,
    /// Snapshot differs from HEAD only by agent-doc-generated exchange artifacts
    /// (boundary / `(HEAD)` markers, answered-prompt-prefix canonicalization).
    BoundaryOnlyDrift,
    /// A reaped/closed item left a nested parent submodule pointer uncommitted
    /// while the document itself is clean.
    NestedParentPointerStale,
    /// An empty `preflight_started` cycle with no capture, response, or pending
    /// mutation.
    OpenEmptyPreflight,
    /// Snapshot differs from HEAD only by agent-doc-generated queue/frontmatter
    /// metadata; user/response and tracked-item content is byte-identical.
    QueueMetadataDrift,
    /// The visible/working file is stale relative to its sidecars (or vice versa)
    /// by metadata only, after an accepted metadata change.
    SidecarVisibleDrift,
    /// User-authored prompt/response content drifted vs HEAD.
    UnsafeUserContentDrift,
}

/// File/effect adapters classify concrete diffs into this smaller vocabulary
/// before asking the turn policy to pick the recovery state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseoutRecoveryDrift {
    /// Snapshot differs from HEAD only by agent-doc-generated exchange artifacts.
    BoundaryOnly,
    /// User/response and tracked-work content are byte-identical; only metadata
    /// changed.
    MetadataOnly,
    /// User-authored prompt/response or tracked-work content differs.
    Content,
}

/// Cycle-state facts relevant to closeout recovery classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseoutRecoveryCycleInput {
    pub phase: CyclePhase,
    pub has_capture: bool,
    pub has_response_hash: bool,
    pub had_pending_mutations: bool,
}

impl CloseoutRecoveryCycleInput {
    pub const fn is_empty_preflight(self) -> bool {
        matches!(self.phase, CyclePhase::PreflightStarted)
            && !self.has_capture
            && !self.has_response_hash
            && !self.had_pending_mutations
    }

    pub const fn is_committed_pending_mutation_without_response(self) -> bool {
        matches!(self.phase, CyclePhase::Committed)
            && !self.has_capture
            && !self.has_response_hash
            && self.had_pending_mutations
    }

    pub const fn needs_file_recovery_evidence(self) -> bool {
        matches!(self.phase, CyclePhase::Committed)
    }
}

/// Pure closeout recovery classification facts. Orchestration supplies these
/// facts after reading cycle state, snapshots, HEAD, and visible document state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CloseoutRecoveryStateInput {
    pub cycle: Option<CloseoutRecoveryCycleInput>,
    pub head_has_escaped_template_patch: bool,
    pub missing_captured_response_body: bool,
    pub direct_response_patchback: bool,
    pub snapshot_head_drift: Option<CloseoutRecoveryDrift>,
    pub snapshot_visible_drift: Option<CloseoutRecoveryDrift>,
    pub nested_parent_pointer_stale: bool,
}

/// Open-cycle sidecar facts needed to render a durable recovery command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCycleRecoveryCommandInput {
    pub cycle_id: String,
    pub phase: CyclePhase,
    pub baseline_file: Option<String>,
    pub target: Option<String>,
    pub has_pending_mutations: bool,
    pub capture_id: Option<String>,
}

/// File-qualified facts needed to render a single closeout recovery command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseoutRecoveryCommandInput {
    pub document: String,
    pub state: CloseoutRecoveryState,
    pub open_cycle: Option<OpenCycleRecoveryCommandInput>,
}

impl CloseoutRecoveryState {
    pub const ALL: [Self; 11] = [
        Self::Clean,
        Self::OpenCycle,
        Self::MissingResponseBody,
        Self::DirectResponsePatchback,
        Self::EscapedTemplatePatch,
        Self::BoundaryOnlyDrift,
        Self::NestedParentPointerStale,
        Self::OpenEmptyPreflight,
        Self::QueueMetadataDrift,
        Self::SidecarVisibleDrift,
        Self::UnsafeUserContentDrift,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::OpenCycle => "open_cycle",
            Self::MissingResponseBody => "missing_response_body",
            Self::DirectResponsePatchback => "direct_response_patchback",
            Self::EscapedTemplatePatch => "escaped_template_patch",
            Self::BoundaryOnlyDrift => "boundary_only_drift",
            Self::NestedParentPointerStale => "nested_parent_pointer_stale",
            Self::OpenEmptyPreflight => "open_empty_preflight",
            Self::QueueMetadataDrift => "queue_metadata_drift",
            Self::SidecarVisibleDrift => "sidecar_visible_drift",
            Self::UnsafeUserContentDrift => "unsafe_user_content_drift",
        }
    }
}

pub fn closeout_recovery_command(input: CloseoutRecoveryCommandInput) -> Option<String> {
    let f = input.document.as_str();
    Some(match input.state {
        CloseoutRecoveryState::Clean => return None,
        CloseoutRecoveryState::OpenCycle => {
            open_cycle_recovery_command(f, input.open_cycle.as_ref())
        }
        CloseoutRecoveryState::MissingResponseBody => format!(
            "pipe the final response (with `<!-- patch:exchange -->` blocks) through `agent-doc write --commit {f}`, then re-run `agent-doc session-check {f}`"
        ),
        CloseoutRecoveryState::DirectResponsePatchback => format!(
            "`agent-doc write --commit {f}` to absorb the visible `### Re:` response through the snapshot/commit boundary"
        ),
        CloseoutRecoveryState::EscapedTemplatePatch => format!(
            "rewrite the response with real `<!-- patch:exchange -->` blocks and rerun `agent-doc finalize {f}` — escaped component markers must not reach `agent:exchange`"
        ),
        CloseoutRecoveryState::BoundaryOnlyDrift => format!(
            "`agent-doc commit {f}` (boundary / `(HEAD)` marker or answered-prompt-prefix drift only — no response body to write)"
        ),
        CloseoutRecoveryState::NestedParentPointerStale => {
            format!("`agent-doc commit {f}` to update the nested parent submodule pointer")
        }
        CloseoutRecoveryState::OpenEmptyPreflight => format!(
            "`agent-doc cancel {f}` — an empty diagnostic preflight cycle with no captured response; abandoning it leaves no document drift"
        ),
        CloseoutRecoveryState::QueueMetadataDrift => format!(
            "`agent-doc commit {f}` (queue / `queue_active` / status metadata only — user/response content is unchanged, no response body to write)"
        ),
        CloseoutRecoveryState::SidecarVisibleDrift => format!(
            "`agent-doc reset --from-current --preserve-session {f}` then `agent-doc commit {f}` to rebuild stale sidecars from the visible file (metadata-only visible drift)"
        ),
        CloseoutRecoveryState::UnsafeUserContentDrift => format!(
            "preserve the user-authored content and finish through `agent-doc finalize {f}` (or `agent-doc write --commit {f}`) — do NOT `agent-doc commit`, which would commit unreviewed content drift as metadata"
        ),
    })
}

pub fn open_cycle_recovery_command(
    document: &str,
    state: Option<&OpenCycleRecoveryCommandInput>,
) -> String {
    let Some(state) = state else {
        return format!(
            "finish the response, then `agent-doc finalize {document}` (or `agent-doc write --commit {document}` to absorb an already-visible response)"
        );
    };
    let phase = state.phase.as_str();
    let baseline_arg = state
        .baseline_file
        .as_deref()
        .map(|path| format!(" --baseline-file {path}"))
        .unwrap_or_default();
    let target = state
        .target
        .as_deref()
        .map(|target| format!(" target={target:?}"))
        .unwrap_or_default();
    let pending = if state.has_pending_mutations {
        " pending_mutations=true"
    } else {
        ""
    };
    let capture = state
        .capture_id
        .as_deref()
        .map(|capture_id| format!(" capture_id={capture_id}"))
        .unwrap_or_default();
    format!(
        "resume durable checkpoint cycle={} phase={phase}{target}{pending}{capture}; finish the response, then `agent-doc finalize {document}{baseline_arg}` (or `agent-doc write --commit {document}` to absorb an already-visible response)",
        state.cycle_id
    )
}

pub fn classify_closeout_recovery_state_from_input(
    input: CloseoutRecoveryStateInput,
) -> CloseoutRecoveryState {
    let Some(cycle) = input.cycle else {
        return CloseoutRecoveryState::Clean;
    };

    match cycle.phase {
        CyclePhase::PreflightStarted if cycle.is_empty_preflight() => {
            return CloseoutRecoveryState::OpenEmptyPreflight;
        }
        CyclePhase::PreflightStarted | CyclePhase::ResponseCaptured | CyclePhase::WriteApplied => {
            return CloseoutRecoveryState::OpenCycle;
        }
        CyclePhase::Abandoned => return CloseoutRecoveryState::Clean,
        CyclePhase::Committed => {}
    }

    if input.head_has_escaped_template_patch {
        return CloseoutRecoveryState::EscapedTemplatePatch;
    }
    if input.missing_captured_response_body
        || cycle.is_committed_pending_mutation_without_response()
    {
        return CloseoutRecoveryState::MissingResponseBody;
    }
    if input.direct_response_patchback {
        return CloseoutRecoveryState::DirectResponsePatchback;
    }

    match input.snapshot_head_drift {
        Some(CloseoutRecoveryDrift::BoundaryOnly) => {
            return CloseoutRecoveryState::BoundaryOnlyDrift;
        }
        Some(CloseoutRecoveryDrift::MetadataOnly) => {
            return CloseoutRecoveryState::QueueMetadataDrift;
        }
        Some(CloseoutRecoveryDrift::Content) => {
            return CloseoutRecoveryState::UnsafeUserContentDrift;
        }
        None => {}
    }

    match input.snapshot_visible_drift {
        Some(CloseoutRecoveryDrift::BoundaryOnly | CloseoutRecoveryDrift::MetadataOnly) => {
            return CloseoutRecoveryState::SidecarVisibleDrift;
        }
        Some(CloseoutRecoveryDrift::Content) => {
            return CloseoutRecoveryState::UnsafeUserContentDrift;
        }
        None => {}
    }

    if input.nested_parent_pointer_stale {
        return CloseoutRecoveryState::NestedParentPointerStale;
    }

    CloseoutRecoveryState::Clean
}

/// Normalized signature of the user/response + tracked-item *content*
/// components (`exchange`, backlog, review, icebox, done), excluding pure
/// agent-doc metadata. Callers should pass text after any exchange-artifact
/// normalization that is relevant to their evidence source.
pub fn closeout_content_component_signature(normalized_doc: &str) -> String {
    let Ok(components) = agent_doc_element::element::parse(normalized_doc) else {
        return normalized_doc.to_string();
    };
    let mut sig = String::new();
    for component in &components {
        let is_content = component.name == "exchange"
            || agent_doc_element::element::is_backlog_component(&component.name)
            || agent_doc_element::element::is_review_component(&component.name)
            || agent_doc_element::element::is_icebox_component(&component.name)
            || agent_doc_element::element::is_backlog_done_component(&component.name);
        if is_content {
            sig.push_str(&component.name);
            sig.push('\u{0}');
            sig.push_str(component.content(normalized_doc).trim());
            sig.push('\n');
        }
    }
    sig
}

/// Input facts that are already known at a closeout recovery call site.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CloseoutRecoveryDecisionInput<'a> {
    /// A routed/JB prompt is waiting and should not be typed over an unresolved
    /// closeout.
    pub prompt_context_available: bool,
    /// Low-level blocker text from the caller, retained only as evidence on the
    /// typed decision boundary.
    pub blocker_reason: Option<&'a str>,
    /// Positive proof that the active capture is stale and superseded by visible
    /// exchange content, so retiring it will not drop the user's intended answer.
    pub stale_capture_supersession_proof: Option<&'a str>,
}

/// Typed closeout recovery policy boundary (`#smcloseoutdecision`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutRecoveryDecision {
    /// No closeout recovery remains.
    AlreadyCommitted,
    /// The existing response/cycle can be safely replayed or completed by the
    /// binary without choosing between competing user-authored contents.
    ReplaySafe {
        state: CloseoutRecoveryState,
        command: String,
    },
    /// A stale capture can be retired because superseding visible content proves
    /// the captured body should not be replayed.
    RetireStaleCapture {
        state: CloseoutRecoveryState,
        proof: String,
    },
    /// Sidecars are stale relative to the visible markdown and can be rebuilt
    /// from the visible file.
    ResetSidecarsFromVisible {
        state: CloseoutRecoveryState,
        command: String,
    },
    /// A new routed prompt must wait behind the unresolved closeout instead of
    /// being submitted to the pane.
    QueuePromptForAfterCloseout {
        state: CloseoutRecoveryState,
        reason: String,
    },
    /// Recovery is not safe because a required proof is missing.
    Blocked {
        state: CloseoutRecoveryState,
        missing_proof: String,
        recommended: String,
    },
}

impl CloseoutRecoveryDecision {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AlreadyCommitted => "already_committed",
            Self::ReplaySafe { .. } => "replay_safe",
            Self::RetireStaleCapture { .. } => "retire_stale_capture",
            Self::ResetSidecarsFromVisible { .. } => "reset_sidecars_from_visible",
            Self::QueuePromptForAfterCloseout { .. } => "queue_prompt_for_after_closeout",
            Self::Blocked { .. } => "blocked",
        }
    }

    pub const fn state(&self) -> Option<CloseoutRecoveryState> {
        match self {
            Self::AlreadyCommitted => None,
            Self::ReplaySafe { state, .. }
            | Self::RetireStaleCapture { state, .. }
            | Self::ResetSidecarsFromVisible { state, .. }
            | Self::QueuePromptForAfterCloseout { state, .. }
            | Self::Blocked { state, .. } => Some(*state),
        }
    }

    pub fn route_terminal_reason(&self) -> String {
        match self {
            Self::AlreadyCommitted => "closeout recovery already_committed".to_string(),
            Self::ReplaySafe { state, command } => format!(
                "closeout recovery replay_safe [{}]: {}",
                state.as_str(),
                command
            ),
            Self::RetireStaleCapture { state, proof } => format!(
                "closeout recovery retire_stale_capture [{}]: proof: {}",
                state.as_str(),
                proof
            ),
            Self::ResetSidecarsFromVisible { state, command } => format!(
                "closeout recovery reset_sidecars_from_visible [{}]: {}",
                state.as_str(),
                command
            ),
            Self::QueuePromptForAfterCloseout { state, .. } => format!(
                "closeout recovery queue_prompt_for_after_closeout [{}]: routed prompt queued behind unresolved closeout",
                state.as_str()
            ),
            Self::Blocked {
                state,
                missing_proof,
                recommended,
            } => format!(
                "closeout recovery blocked [{}]: missing proof: {}; recommended: {}",
                state.as_str(),
                missing_proof,
                recommended
            ),
        }
    }
}

pub fn closeout_recovery_decision_from_state(
    state: CloseoutRecoveryState,
    input: CloseoutRecoveryDecisionInput<'_>,
    recovery_command: Option<&str>,
) -> CloseoutRecoveryDecision {
    if input.prompt_context_available {
        return CloseoutRecoveryDecision::QueuePromptForAfterCloseout {
            state,
            reason: input
                .blocker_reason
                .unwrap_or_else(|| state.as_str())
                .to_string(),
        };
    }

    if state == CloseoutRecoveryState::Clean {
        return CloseoutRecoveryDecision::AlreadyCommitted;
    }

    if let Some(proof) = input.stale_capture_supersession_proof
        && matches!(
            state,
            CloseoutRecoveryState::MissingResponseBody
                | CloseoutRecoveryState::UnsafeUserContentDrift
        )
    {
        return CloseoutRecoveryDecision::RetireStaleCapture {
            state,
            proof: proof.to_string(),
        };
    }

    let command = || recovery_command.unwrap_or_default().to_string();
    match state {
        CloseoutRecoveryState::Clean => CloseoutRecoveryDecision::AlreadyCommitted,
        CloseoutRecoveryState::DirectResponsePatchback
        | CloseoutRecoveryState::BoundaryOnlyDrift
        | CloseoutRecoveryState::NestedParentPointerStale
        | CloseoutRecoveryState::OpenEmptyPreflight
        | CloseoutRecoveryState::QueueMetadataDrift => CloseoutRecoveryDecision::ReplaySafe {
            state,
            command: command(),
        },
        CloseoutRecoveryState::SidecarVisibleDrift => {
            CloseoutRecoveryDecision::ResetSidecarsFromVisible {
                state,
                command: command(),
            }
        }
        CloseoutRecoveryState::OpenCycle => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "open cycle must finish, be replayed, or be explicitly queued behind"
                .to_string(),
            recommended: command(),
        },
        CloseoutRecoveryState::MissingResponseBody => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "captured response body presence or supersession proof".to_string(),
            recommended: command(),
        },
        CloseoutRecoveryState::EscapedTemplatePatch => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "unescaped patchback blocks that can be applied safely".to_string(),
            recommended: command(),
        },
        CloseoutRecoveryState::UnsafeUserContentDrift => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "proof that visible user-authored content is metadata-only drift"
                .to_string(),
            recommended: command(),
        },
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

    #[test]
    fn closeout_recovery_state_labels_are_stable() {
        use CloseoutRecoveryState::*;
        let cases = [
            (Clean, "clean"),
            (OpenCycle, "open_cycle"),
            (MissingResponseBody, "missing_response_body"),
            (DirectResponsePatchback, "direct_response_patchback"),
            (EscapedTemplatePatch, "escaped_template_patch"),
            (BoundaryOnlyDrift, "boundary_only_drift"),
            (NestedParentPointerStale, "nested_parent_pointer_stale"),
            (OpenEmptyPreflight, "open_empty_preflight"),
            (QueueMetadataDrift, "queue_metadata_drift"),
            (SidecarVisibleDrift, "sidecar_visible_drift"),
            (UnsafeUserContentDrift, "unsafe_user_content_drift"),
        ];
        assert_eq!(cases.len(), CloseoutRecoveryState::ALL.len());
        for (state, label) in cases {
            assert_eq!(state.as_str(), label);
        }
    }

    #[test]
    fn recovery_command_maps_each_state_to_one_instruction() {
        use CloseoutRecoveryState::*;
        let document = "tasks/doc.md".to_string();
        assert_eq!(
            closeout_recovery_command(CloseoutRecoveryCommandInput {
                document: document.clone(),
                state: Clean,
                open_cycle: None,
            }),
            None
        );
        for (state, name, needle) in [
            (OpenCycle, "open_cycle", "agent-doc finalize"),
            (
                MissingResponseBody,
                "missing_response_body",
                "agent-doc write --commit",
            ),
            (
                DirectResponsePatchback,
                "direct_response_patchback",
                "absorb the visible",
            ),
            (
                EscapedTemplatePatch,
                "escaped_template_patch",
                "patch:exchange",
            ),
            (BoundaryOnlyDrift, "boundary_only_drift", "boundary"),
            (
                NestedParentPointerStale,
                "nested_parent_pointer_stale",
                "parent submodule pointer",
            ),
            (
                OpenEmptyPreflight,
                "open_empty_preflight",
                "agent-doc cancel",
            ),
            (
                QueueMetadataDrift,
                "queue_metadata_drift",
                "agent-doc commit",
            ),
            (
                SidecarVisibleDrift,
                "sidecar_visible_drift",
                "reset --from-current",
            ),
            (
                UnsafeUserContentDrift,
                "unsafe_user_content_drift",
                "do NOT `agent-doc commit`",
            ),
        ] {
            assert_eq!(state.as_str(), name);
            let cmd = closeout_recovery_command(CloseoutRecoveryCommandInput {
                document: document.clone(),
                state,
                open_cycle: None,
            })
            .expect("non-clean states have a command");
            assert!(
                cmd.contains(needle),
                "state {name} command {cmd:?} missing {needle:?}"
            );
            assert!(
                cmd.contains("tasks/doc.md"),
                "command should name the file: {cmd:?}"
            );
        }
    }

    #[test]
    fn open_cycle_recovery_command_names_durable_checkpoint() {
        let cmd = closeout_recovery_command(CloseoutRecoveryCommandInput {
            document: "tasks/doc.md".to_string(),
            state: CloseoutRecoveryState::OpenCycle,
            open_cycle: Some(OpenCycleRecoveryCommandInput {
                cycle_id: "cycle-123".to_string(),
                phase: CyclePhase::ResponseCaptured,
                baseline_file: Some("/tmp/baseline.md".to_string()),
                target: Some("#durablerecycle".to_string()),
                has_pending_mutations: true,
                capture_id: Some("capture-123".to_string()),
            }),
        })
        .unwrap();

        assert!(cmd.contains("resume durable checkpoint"), "{cmd}");
        assert!(cmd.contains("phase=response_captured"), "{cmd}");
        assert!(cmd.contains("target=\"#durablerecycle\""), "{cmd}");
        assert!(cmd.contains("pending_mutations=true"), "{cmd}");
        assert!(cmd.contains("capture_id=capture-123"), "{cmd}");
        assert!(cmd.contains("--baseline-file /tmp/baseline.md"), "{cmd}");
    }

    fn committed_cycle() -> CloseoutRecoveryCycleInput {
        CloseoutRecoveryCycleInput {
            phase: CyclePhase::Committed,
            has_capture: false,
            has_response_hash: false,
            had_pending_mutations: false,
        }
    }

    #[test]
    fn recovery_state_classifier_maps_cycle_facts_before_drift() {
        assert_eq!(
            classify_closeout_recovery_state_from_input(CloseoutRecoveryStateInput::default()),
            CloseoutRecoveryState::Clean
        );
        assert_eq!(
            classify_closeout_recovery_state_from_input(CloseoutRecoveryStateInput {
                cycle: Some(CloseoutRecoveryCycleInput {
                    phase: CyclePhase::PreflightStarted,
                    has_capture: false,
                    has_response_hash: false,
                    had_pending_mutations: false,
                }),
                snapshot_head_drift: Some(CloseoutRecoveryDrift::Content),
                ..CloseoutRecoveryStateInput::default()
            }),
            CloseoutRecoveryState::OpenEmptyPreflight
        );
        assert_eq!(
            classify_closeout_recovery_state_from_input(CloseoutRecoveryStateInput {
                cycle: Some(CloseoutRecoveryCycleInput {
                    phase: CyclePhase::ResponseCaptured,
                    has_capture: true,
                    has_response_hash: true,
                    had_pending_mutations: false,
                }),
                direct_response_patchback: true,
                ..CloseoutRecoveryStateInput::default()
            }),
            CloseoutRecoveryState::OpenCycle
        );
    }

    #[test]
    fn recovery_state_classifier_preserves_closeout_detection_order() {
        let cycle = committed_cycle();
        assert_eq!(
            classify_closeout_recovery_state_from_input(CloseoutRecoveryStateInput {
                cycle: Some(cycle),
                head_has_escaped_template_patch: true,
                direct_response_patchback: true,
                snapshot_head_drift: Some(CloseoutRecoveryDrift::Content),
                ..CloseoutRecoveryStateInput::default()
            }),
            CloseoutRecoveryState::EscapedTemplatePatch
        );
        assert_eq!(
            classify_closeout_recovery_state_from_input(CloseoutRecoveryStateInput {
                cycle: Some(CloseoutRecoveryCycleInput {
                    had_pending_mutations: true,
                    ..cycle
                }),
                direct_response_patchback: true,
                ..CloseoutRecoveryStateInput::default()
            }),
            CloseoutRecoveryState::MissingResponseBody
        );
        assert_eq!(
            classify_closeout_recovery_state_from_input(CloseoutRecoveryStateInput {
                cycle: Some(cycle),
                direct_response_patchback: true,
                snapshot_head_drift: Some(CloseoutRecoveryDrift::Content),
                ..CloseoutRecoveryStateInput::default()
            }),
            CloseoutRecoveryState::DirectResponsePatchback
        );
    }

    #[test]
    fn recovery_state_classifier_splits_snapshot_and_visible_drift() {
        let cycle = committed_cycle();
        for (drift, state) in [
            (
                CloseoutRecoveryDrift::BoundaryOnly,
                CloseoutRecoveryState::BoundaryOnlyDrift,
            ),
            (
                CloseoutRecoveryDrift::MetadataOnly,
                CloseoutRecoveryState::QueueMetadataDrift,
            ),
            (
                CloseoutRecoveryDrift::Content,
                CloseoutRecoveryState::UnsafeUserContentDrift,
            ),
        ] {
            assert_eq!(
                classify_closeout_recovery_state_from_input(CloseoutRecoveryStateInput {
                    cycle: Some(cycle),
                    snapshot_head_drift: Some(drift),
                    ..CloseoutRecoveryStateInput::default()
                }),
                state,
                "unexpected snapshot-vs-head classification for {drift:?}"
            );
        }
        assert_eq!(
            classify_closeout_recovery_state_from_input(CloseoutRecoveryStateInput {
                cycle: Some(cycle),
                snapshot_visible_drift: Some(CloseoutRecoveryDrift::MetadataOnly),
                ..CloseoutRecoveryStateInput::default()
            }),
            CloseoutRecoveryState::SidecarVisibleDrift
        );
        assert_eq!(
            classify_closeout_recovery_state_from_input(CloseoutRecoveryStateInput {
                cycle: Some(cycle),
                snapshot_visible_drift: Some(CloseoutRecoveryDrift::Content),
                ..CloseoutRecoveryStateInput::default()
            }),
            CloseoutRecoveryState::UnsafeUserContentDrift
        );
    }

    #[test]
    fn content_component_signature_ignores_metadata_components() {
        let a = concat!(
            "---\nagent_doc_session: test\nqueue_active: true\n---\n\n",
            "<!-- agent:status -->\nactive\n<!-- /agent:status -->\n\n",
            "<!-- agent:exchange -->\n### Re: x\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let b = a
            .replace("queue_active: true", "queue_active: false")
            .replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        assert_eq!(
            closeout_content_component_signature(a),
            closeout_content_component_signature(&b)
        );
        let changed = b.replace("Done.", "Different response.");
        assert_ne!(
            closeout_content_component_signature(a),
            closeout_content_component_signature(&changed)
        );
    }

    #[test]
    fn recovery_decision_maps_states_to_typed_outcomes() {
        use CloseoutRecoveryDecision::*;
        use CloseoutRecoveryState::*;
        let command = Some("agent-doc recover tasks/doc.md");

        let default_cases = [
            (Clean, "already_committed"),
            (OpenCycle, "blocked"),
            (MissingResponseBody, "blocked"),
            (DirectResponsePatchback, "replay_safe"),
            (EscapedTemplatePatch, "blocked"),
            (BoundaryOnlyDrift, "replay_safe"),
            (NestedParentPointerStale, "replay_safe"),
            (OpenEmptyPreflight, "replay_safe"),
            (QueueMetadataDrift, "replay_safe"),
            (SidecarVisibleDrift, "reset_sidecars_from_visible"),
            (UnsafeUserContentDrift, "blocked"),
        ];
        assert_eq!(default_cases.len(), CloseoutRecoveryState::ALL.len());

        for (state, expected) in default_cases {
            let decision = closeout_recovery_decision_from_state(
                state,
                CloseoutRecoveryDecisionInput::default(),
                command,
            );
            assert_eq!(
                decision.as_str(),
                expected,
                "unexpected default decision for {state:?}: {decision:?}"
            );
            assert_eq!(
                decision.state(),
                if state == Clean { None } else { Some(state) },
                "decision should retain its source state for {state:?}: {decision:?}"
            );
            match decision {
                AlreadyCommitted => {}
                ReplaySafe {
                    command: rendered, ..
                }
                | ResetSidecarsFromVisible {
                    command: rendered, ..
                } => {
                    assert_eq!(rendered, command.unwrap());
                }
                Blocked {
                    missing_proof,
                    recommended,
                    ..
                } => {
                    assert!(
                        !missing_proof.is_empty(),
                        "blocked decision should name missing proof for {state:?}"
                    );
                    assert_eq!(recommended, command.unwrap());
                }
                other => panic!("default path unexpectedly produced {other:?} for {state:?}"),
            }
        }

        for state in CloseoutRecoveryState::ALL {
            assert_eq!(
                closeout_recovery_decision_from_state(
                    state,
                    CloseoutRecoveryDecisionInput {
                        prompt_context_available: true,
                        blocker_reason: Some("active closeout"),
                        stale_capture_supersession_proof: Some("superseded"),
                    },
                    command,
                ),
                QueuePromptForAfterCloseout {
                    state,
                    reason: "active closeout".to_string(),
                },
                "prompt context must take priority for {state:?}"
            );
            assert_eq!(
                closeout_recovery_decision_from_state(
                    state,
                    CloseoutRecoveryDecisionInput {
                        prompt_context_available: true,
                        blocker_reason: None,
                        stale_capture_supersession_proof: None,
                    },
                    None,
                ),
                QueuePromptForAfterCloseout {
                    state,
                    reason: state.as_str().to_string(),
                },
                "prompt context fallback reason should be the state name for {state:?}"
            );
        }

        for state in [MissingResponseBody, UnsafeUserContentDrift] {
            assert_eq!(
                closeout_recovery_decision_from_state(
                    state,
                    CloseoutRecoveryDecisionInput {
                        stale_capture_supersession_proof: Some("heading already answered"),
                        ..CloseoutRecoveryDecisionInput::default()
                    },
                    command,
                ),
                RetireStaleCapture {
                    state,
                    proof: "heading already answered".to_string(),
                }
            );
        }
    }
}
