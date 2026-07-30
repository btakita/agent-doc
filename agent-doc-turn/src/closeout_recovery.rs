//! Pure closeout recovery policy.
//!
//! Orchestration owns file, git, and recovery-projection effects. This module owns
//! action-independent turn recovery decisions that can be proven from document
//! content facts.

use crate::CyclePhase;

/// Return true when a repair mutation would leave a new, unanswered prompt-bearing
/// diff between the previous snapshot and the repaired document.
pub fn repair_leaves_unanswered_prompt_diff(
    snapshot_content: &str,
    repaired: &str,
    known_response: Option<&str>,
) -> bool {
    let norm_snapshot =
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
            snapshot_content,
        );
    let norm_repaired =
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(repaired);
    let Some(diff_text) =
        agent_doc_diff::unified_diff_from_contents(&norm_snapshot, &norm_repaired)
    else {
        return false;
    };
    let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff_text);
    let mut skip_answered_response_run = false;
    for (idx, change) in changes.iter().enumerate() {
        if change.kind != agent_doc_diff::PromptBearingChangeKind::PromptTarget {
            continue;
        }
        if skip_answered_response_run {
            let preview = change
                .text
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or(change.text.as_str())
                .trim();
            if !repair_line_looks_like_fresh_prompt_after_response(preview) {
                continue;
            }
        }
        if agent_doc_diff::prompt_change_is_already_answered(&change.text)
            || agent_doc_diff::prompt_change_is_answered_by_later_response(&changes, idx)
            || repair_prompt_target_immediately_before_existing_response(repaired, &change.text)
            || known_response
                .map(|response| {
                    crate::response_replay::prompt_change_is_known_response(&change.text, response)
                })
                .unwrap_or(false)
        {
            skip_answered_response_run = true;
            continue;
        }
        return true;
    }
    false
}

pub fn visible_response_recovery_is_adoptable(
    phase: Option<CyclePhase>,
    active_session_for_current_file: bool,
) -> bool {
    matches!(
        phase,
        Some(CyclePhase::ResponseCaptured | CyclePhase::WriteApplied) | None
    ) || active_session_for_current_file
}

pub fn stale_preflight_cycle_age_secs(started_at: u64, updated_at: u64, now_secs: u64) -> u64 {
    now_secs.saturating_sub(updated_at.max(started_at))
}

pub fn prompt_change_is_orchestration_handoff_marker(text: &str) -> bool {
    let mut meaningful = text
        .lines()
        .map(|line| line.trim().trim_start_matches('❯').trim())
        .filter(|line| !line.is_empty() && !line.starts_with("<!--"));
    let Some(line) = meaningful.next() else {
        return false;
    };
    if meaningful.next().is_some() {
        return false;
    }
    let normalized = line
        .trim_end_matches(':')
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "synchronous orchestra"
            | "synchronous orcestra"
            | "orchestra"
            | "orchestrate"
            | "sequential"
            | "sequentially"
            | "run these sequentially"
    )
}

pub fn content_matches_ignoring_trailing_newlines(left: &str, right: &str) -> bool {
    left.trim_end_matches('\n') == right.trim_end_matches('\n')
}

fn repair_line_looks_like_fresh_prompt_after_response(trimmed: &str) -> bool {
    let lower = trimmed.trim_start_matches('❯').trim().to_ascii_lowercase();
    trimmed.ends_with('?')
        || lower == "go"
        || lower == "continue"
        || lower.starts_with("do #")
        || lower.starts_with("do [#")
        || lower.starts_with("fix #")
        || lower.starts_with("run ")
        || lower.starts_with("rerun ")
        || lower.starts_with("build ")
        || lower.starts_with("test ")
        || lower.starts_with("commit ")
        || lower.starts_with("push ")
        || lower.starts_with("verify ")
        || lower.starts_with("investigate ")
}

fn repair_prompt_target_immediately_before_existing_response(
    current_doc: &str,
    change_text: &str,
) -> bool {
    let target = change_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().trim_start_matches('❯').trim().to_string());
    let Some(target) = target else {
        return false;
    };
    if target.is_empty() {
        return false;
    }

    let body = agent_doc_frontmatter::frontmatter::parse(current_doc)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|_| current_doc.to_string());
    let Ok(components) = agent_doc_element::element::parse(&body) else {
        return false;
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return false;
    };

    let lines: Vec<&str> = exchange.content(&body).lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let normalized = line.trim().trim_start_matches('❯').trim();
        if normalized != target {
            continue;
        }
        for next in lines.iter().skip(idx + 1) {
            let trimmed = next.trim();
            if trimmed.is_empty() || trimmed.starts_with("<!--") {
                continue;
            }
            let normalized = trimmed.strip_prefix("❯ ").unwrap_or(trimmed).trim();
            return crate::closeout_signal::is_exchange_response_heading(normalized);
        }
    }
    false
}

/// Which side of a metadata-only drift is authoritative for closeout recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataDriftAuthority {
    /// The local side (snapshot for queue metadata drift, visible file for
    /// projection-visible drift) is authoritative and can be committed forward.
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
    WholeDocumentReplayCoalescedBaseline,
    AuthoritativeReplayBaseline,
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
            Self::WholeDocumentReplayCoalescedBaseline => {
                "whole_document_replay_coalesced_baseline"
            }
            Self::AuthoritativeReplayBaseline => "authoritative_replay_baseline",
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
            Self::WholeDocumentReplayCoalescedBaseline => {
                "capture_baseline_refreshed_for_whole_document_replay_coalescence"
            }
            Self::AuthoritativeReplayBaseline => {
                "capture_baseline_refreshed_from_authoritative_current"
            }
            _ => "capture_baseline_refreshed_for_benign_drift",
        }
    }

    pub const fn capture_refresh_message(self) -> &'static str {
        match self {
            Self::QueueOnlyReplayBaseline => "queue-only drift detected",
            Self::WholeDocumentReplayCoalescedBaseline => {
                "whole-document replay coalescence detected"
            }
            Self::AuthoritativeReplayBaseline => {
                "newer authoritative document cut safely preserves the captured baseline"
            }
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
    /// The visible/working file is stale relative to its state projections (or vice versa)
    /// by metadata only, after an accepted metadata change.
    RecoveryProjectionVisibleDrift,
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

/// Open-cycle ledger facts needed to render a durable recovery command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCycleRecoveryCommandInput {
    pub cycle_id: String,
    pub phase: CyclePhase,
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
        Self::RecoveryProjectionVisibleDrift,
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
            Self::RecoveryProjectionVisibleDrift => "recovery_projection_visible_drift",
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
        CloseoutRecoveryState::RecoveryProjectionVisibleDrift => format!(
            "`agent-doc reset --from-current --preserve-session {f}` then `agent-doc commit {f}` to rebuild stale recovery projections from the visible file (metadata-only visible drift)"
        ),
        CloseoutRecoveryState::UnsafeUserContentDrift => format!(
            "preserve the user-authored content and finish through `agent-doc finalize {f}` (or `agent-doc write --commit {f}`) — do NOT `agent-doc commit`, which would commit unreviewed content drift as metadata"
        ),
    })
}

/// Pull the first `agent-doc <subcommand> <FILE>` invocation out of a
/// closeout-recovery recommendation so a surfaced recovery command stays short
/// and copy-pasteable.
///
/// Markdown backticks are stripped first so a command wrapped in `` `...` `` is
/// detected and the trailing backtick does not attach to the final path token.
/// Returns `None` if no `agent-doc` invocation is present.
pub fn short_recovery_command_from_recommendation(recommended: &str) -> Option<String> {
    let cleaned = recommended.replace('`', " ");
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let mut start = None;
    let mut end = 0;
    for (i, &tok) in words.iter().enumerate() {
        if start.is_none() {
            if tok == "agent-doc" {
                start = Some(i);
            }
            continue;
        }
        // Stop at the first token that is not part of an agent-doc CLI word
        // (subcommand, file path, or a known short flag). Keep the surface
        // tight: just the command + subcommand + file (or short flag + arg).
        let is_cli_word = tok
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '='));
        if !is_cli_word {
            break;
        }
        end = i + 1;
    }
    let start = start?;
    if end <= start {
        return None;
    }
    let command = words[start..end].join(" ");
    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

pub fn blocked_closeout_recovery_command(decision: &CloseoutRecoveryDecision) -> Option<String> {
    let CloseoutRecoveryDecision::Blocked { recommended, .. } = decision else {
        return None;
    };
    Some(
        short_recovery_command_from_recommendation(recommended)
            .unwrap_or_else(|| recommended.clone()),
    )
}

pub fn open_cycle_recovery_command(
    document: &str,
    state: Option<&OpenCycleRecoveryCommandInput>,
) -> String {
    let Some(state) = state else {
        return format!(
            "the active closeout already owns {document}; keep the new route queued until its durable checkpoint reaches a terminal commit, without resubmitting finalize or write"
        );
    };
    let phase = state.phase.as_str();
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
    let continuation = match state.phase {
        CyclePhase::PreflightStarted => {
            "the active response has not been captured yet; keep the new route queued behind its owner"
        }
        CyclePhase::ResponseCaptured => {
            "the response is already captured; retained write and commit recovery own the continuation"
        }
        CyclePhase::WriteApplied => {
            "the canonical write is already applied; retained commit recovery owns the terminal boundary"
        }
        CyclePhase::Committed | CyclePhase::Abandoned => {
            "the checkpoint is terminal; refresh the state projection instead of resubmitting response commands"
        }
    };
    format!(
        "resume durable checkpoint cycle={} phase={phase}{target}{pending}{capture}; {continuation}; do not resubmit finalize or write for {document}",
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
            return CloseoutRecoveryState::RecoveryProjectionVisibleDrift;
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

pub fn classify_snapshot_head_drift(snapshot: &str, head: &str) -> CloseoutRecoveryDrift {
    // Boundary / `(HEAD)` / answered-prompt-prefix artifacts only.
    if agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(snapshot)
        == agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(head)
    {
        return CloseoutRecoveryDrift::BoundaryOnly;
    }
    // User/response + tracked-item content is byte-identical, so the diff is
    // queue/status metadata and is safe to commit through closeout recovery.
    if closeout_content_signature_after_artifact_normalization(snapshot)
        == closeout_content_signature_after_artifact_normalization(head)
    {
        return CloseoutRecoveryDrift::MetadataOnly;
    }
    CloseoutRecoveryDrift::Content
}

pub fn classify_snapshot_visible_drift(snapshot: &str, visible: &str) -> CloseoutRecoveryDrift {
    if closeout_content_signature_after_artifact_normalization(snapshot)
        == closeout_content_signature_after_artifact_normalization(visible)
    {
        CloseoutRecoveryDrift::MetadataOnly
    } else {
        CloseoutRecoveryDrift::Content
    }
}

fn closeout_content_signature_after_artifact_normalization(doc: &str) -> String {
    let normalized =
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(doc);
    closeout_content_component_signature(&normalized)
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
    /// Recovery projections are stale relative to the visible markdown and can be rebuilt
    /// from the visible file.
    RefreshRecoveryProjectionsFromVisible {
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

/// Evidence for deriving `write_applied` from the durable captured-response
/// intent plus the observable editor-authority and disk projections. No
/// per-document capture-state compatibility file participates in this decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteAppliedReconciliationEvidence {
    pub cycle_phase: crate::CyclePhase,
    pub response_materialized_in_authority: bool,
    pub authority_matches_disk: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteAppliedReconciliationDecision {
    AlreadyProjected,
    PromoteCycleProjection,
    RetainUntilVisible,
    NotApplicable,
}

pub const fn reconcile_write_applied_evidence(
    evidence: WriteAppliedReconciliationEvidence,
) -> WriteAppliedReconciliationDecision {
    use WriteAppliedReconciliationDecision::{
        AlreadyProjected, NotApplicable, PromoteCycleProjection, RetainUntilVisible,
    };

    if matches!(evidence.cycle_phase, crate::CyclePhase::WriteApplied) {
        return AlreadyProjected;
    }
    if !matches!(evidence.cycle_phase, crate::CyclePhase::ResponseCaptured) {
        return NotApplicable;
    }
    if evidence.response_materialized_in_authority && evidence.authority_matches_disk {
        PromoteCycleProjection
    } else {
        RetainUntilVisible
    }
}

impl CloseoutRecoveryDecision {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AlreadyCommitted => "already_committed",
            Self::ReplaySafe { .. } => "replay_safe",
            Self::RetireStaleCapture { .. } => "retire_stale_capture",
            Self::RefreshRecoveryProjectionsFromVisible { .. } => {
                "refresh_recovery_projections_from_visible"
            }
            Self::QueuePromptForAfterCloseout { .. } => "queue_prompt_for_after_closeout",
            Self::Blocked { .. } => "blocked",
        }
    }

    pub const fn state(&self) -> Option<CloseoutRecoveryState> {
        match self {
            Self::AlreadyCommitted => None,
            Self::ReplaySafe { state, .. }
            | Self::RetireStaleCapture { state, .. }
            | Self::RefreshRecoveryProjectionsFromVisible { state, .. }
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
            Self::RefreshRecoveryProjectionsFromVisible { state, command } => format!(
                "closeout recovery refresh_recovery_projections_from_visible [{}]: {}",
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
            CloseoutRecoveryState::OpenCycle
                | CloseoutRecoveryState::MissingResponseBody
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
        CloseoutRecoveryState::RecoveryProjectionVisibleDrift => {
            CloseoutRecoveryDecision::RefreshRecoveryProjectionsFromVisible {
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
    fn repair_unanswered_prompt_diff_detects_new_prompt() {
        let snapshot = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: done\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        let repaired = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: done\n",
            "Done.\n",
            "What next?\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(repair_leaves_unanswered_prompt_diff(
            snapshot, repaired, None
        ));
    }

    #[test]
    fn repair_unanswered_prompt_diff_ignores_prompt_immediately_answered_in_repaired_doc() {
        let snapshot = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: done\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        let repaired = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: done\n",
            "Done.\n",
            "do #follow-up\n",
            "### Re: follow-up\n",
            "Already answered.\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(!repair_leaves_unanswered_prompt_diff(
            snapshot, repaired, None
        ));
    }

    #[test]
    fn visible_response_recovery_is_adoptable_only_for_agent_owned_phases_or_active_session() {
        assert!(visible_response_recovery_is_adoptable(None, false));
        assert!(visible_response_recovery_is_adoptable(
            Some(CyclePhase::ResponseCaptured),
            false
        ));
        assert!(visible_response_recovery_is_adoptable(
            Some(CyclePhase::WriteApplied),
            false
        ));
        assert!(!visible_response_recovery_is_adoptable(
            Some(CyclePhase::PreflightStarted),
            false
        ));
        assert!(visible_response_recovery_is_adoptable(
            Some(CyclePhase::PreflightStarted),
            true
        ));
    }

    #[test]
    fn write_applied_reconciliation_has_one_safe_forward_path() {
        use WriteAppliedReconciliationDecision::{
            AlreadyProjected, NotApplicable, PromoteCycleProjection, RetainUntilVisible,
        };

        let evidence = |cycle_phase, response_materialized_in_authority, authority_matches_disk| {
            reconcile_write_applied_evidence(WriteAppliedReconciliationEvidence {
                cycle_phase,
                response_materialized_in_authority,
                authority_matches_disk,
            })
        };

        assert_eq!(
            evidence(CyclePhase::WriteApplied, true, true),
            AlreadyProjected
        );
        assert_eq!(
            evidence(CyclePhase::ResponseCaptured, true, true),
            PromoteCycleProjection
        );
        assert_eq!(
            evidence(CyclePhase::ResponseCaptured, false, true),
            RetainUntilVisible
        );
        assert_eq!(
            evidence(CyclePhase::ResponseCaptured, true, false),
            RetainUntilVisible
        );
        assert_eq!(
            evidence(CyclePhase::PreflightStarted, true, true),
            NotApplicable
        );
    }

    #[test]
    fn stale_preflight_cycle_age_uses_newer_cycle_timestamp() {
        assert_eq!(stale_preflight_cycle_age_secs(10, 15, 20), 5);
        assert_eq!(stale_preflight_cycle_age_secs(15, 10, 20), 5);
        assert_eq!(stale_preflight_cycle_age_secs(15, 25, 20), 0);
    }

    #[test]
    fn orchestration_handoff_marker_accepts_single_handoff_line_only() {
        assert!(prompt_change_is_orchestration_handoff_marker(
            "❯ Orchestrate."
        ));
        assert!(prompt_change_is_orchestration_handoff_marker(
            "<!-- comment -->\nrun these sequentially:"
        ));
        assert!(!prompt_change_is_orchestration_handoff_marker(
            "orchestrate\nand then do more"
        ));
        assert!(!prompt_change_is_orchestration_handoff_marker("continue"));
    }

    #[test]
    fn content_match_ignores_only_trailing_newlines() {
        assert!(content_matches_ignoring_trailing_newlines("a\n", "a\n\n"));
        assert!(!content_matches_ignoring_trailing_newlines(
            "a\nb", "a\nb\nc"
        ));
    }

    #[test]
    fn metadata_drift_authority_head_when_local_drops_live_continuation() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto go -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n",
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
            "<!-- agent:queue auto go -->\n- do [#a]\n<!-- /agent:queue -->\n",
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
            "<!-- agent:queue auto go -->\n- do [#a]\n<!-- /agent:queue -->\n",
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
            CloseoutRecoveryMutationReason::WholeDocumentReplayCoalescedBaseline.as_str(),
            "whole_document_replay_coalesced_baseline"
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
            CloseoutRecoveryMutationReason::WholeDocumentReplayCoalescedBaseline
                .capture_refresh_event(),
            "capture_baseline_refreshed_for_whole_document_replay_coalescence"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::WholeDocumentReplayCoalescedBaseline
                .capture_refresh_message(),
            "whole-document replay coalescence detected"
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
            (
                RecoveryProjectionVisibleDrift,
                "recovery_projection_visible_drift",
            ),
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
            (OpenCycle, "open_cycle", "keep the new route queued"),
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
                RecoveryProjectionVisibleDrift,
                "recovery_projection_visible_drift",
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
        assert!(!cmd.contains("--baseline-file"), "{cmd}");
        assert!(!cmd.contains("agent-doc finalize"), "{cmd}");
        assert!(!cmd.contains("agent-doc write"), "{cmd}");
    }

    #[test]
    fn write_applied_recovery_never_recommends_resubmission() {
        let cmd = open_cycle_recovery_command(
            "tasks/doc.md",
            Some(&OpenCycleRecoveryCommandInput {
                cycle_id: "cycle-write-applied".to_string(),
                phase: CyclePhase::WriteApplied,
                target: Some("#noticedavailabledeals".to_string()),
                has_pending_mutations: true,
                capture_id: Some("cycle-write-applied".to_string()),
            }),
        );
        assert!(cmd.contains("canonical write is already applied"), "{cmd}");
        assert!(cmd.contains("retained commit recovery"), "{cmd}");
        assert!(cmd.contains("pending_mutations=true"), "{cmd}");
        assert!(!cmd.contains("agent-doc finalize"), "{cmd}");
        assert!(!cmd.contains("agent-doc write"), "{cmd}");
    }

    #[test]
    fn short_recovery_command_from_recommendation_picks_first_agent_doc_invocation() {
        let recommended = "finish the response, then `agent-doc finalize /abs/session.md` (or `agent-doc write --commit /abs/session.md` to absorb an already-visible response)";
        assert_eq!(
            short_recovery_command_from_recommendation(recommended).as_deref(),
            Some("agent-doc finalize /abs/session.md")
        );
        assert!(short_recovery_command_from_recommendation("just finish the response").is_none());
        let mixed = "Rebuild recovery projections: `agent-doc reset --from-current --preserve-session /path/session.md`";
        assert_eq!(
            short_recovery_command_from_recommendation(mixed).as_deref(),
            Some("agent-doc reset --from-current --preserve-session /path/session.md")
        );
    }

    #[test]
    fn blocked_closeout_recovery_command_extracts_only_blocked_recommendations() {
        let blocked = CloseoutRecoveryDecision::Blocked {
            state: CloseoutRecoveryState::OpenCycle,
            missing_proof: "response body".to_string(),
            recommended: "finish the response, then `agent-doc finalize /abs/session.md`"
                .to_string(),
        };
        assert_eq!(
            blocked_closeout_recovery_command(&blocked).as_deref(),
            Some("agent-doc finalize /abs/session.md")
        );

        let replay = CloseoutRecoveryDecision::ReplaySafe {
            state: CloseoutRecoveryState::BoundaryOnlyDrift,
            command: "agent-doc commit /abs/session.md".to_string(),
        };
        assert_eq!(blocked_closeout_recovery_command(&replay), None);
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
            CloseoutRecoveryState::RecoveryProjectionVisibleDrift
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
    fn snapshot_head_drift_classifier_distinguishes_boundary_metadata_and_content() {
        let head = concat!(
            "---\nagent_doc_session: test\nqueue_active: false\n---\n\n",
            "<!-- agent:status -->\nidle\n<!-- /agent:status -->\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Please rerun the deploy check.\n",
            "### Re: deploy check - gpt-5 (HEAD)\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let boundary_only = head.replace("Please rerun the", "❯ Please rerun the");
        assert_eq!(
            classify_snapshot_head_drift(&boundary_only, head),
            CloseoutRecoveryDrift::BoundaryOnly
        );

        let metadata_only = head
            .replace("queue_active: false", "queue_active: true")
            .replace(
                "<!-- agent:status -->\nidle",
                "<!-- agent:status -->\nactive",
            );
        assert_eq!(
            classify_snapshot_head_drift(&metadata_only, head),
            CloseoutRecoveryDrift::MetadataOnly
        );

        let content = head.replace("Done.", "Different response.");
        assert_eq!(
            classify_snapshot_head_drift(&content, head),
            CloseoutRecoveryDrift::Content
        );
    }

    #[test]
    fn snapshot_visible_drift_classifier_ignores_metadata_but_not_content() {
        let snapshot = concat!(
            "---\nagent_doc_session: test\nqueue_active: false\n---\n\n",
            "<!-- agent:status -->\nidle\n<!-- /agent:status -->\n\n",
            "<!-- agent:exchange -->\n### Re: x\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let metadata_only = snapshot
            .replace("queue_active: false", "queue_active: true")
            .replace(
                "<!-- agent:status -->\nidle",
                "<!-- agent:status -->\nactive",
            );
        assert_eq!(
            classify_snapshot_visible_drift(snapshot, &metadata_only),
            CloseoutRecoveryDrift::MetadataOnly
        );

        let content = snapshot.replace("Done.", "Different response.");
        assert_eq!(
            classify_snapshot_visible_drift(snapshot, &content),
            CloseoutRecoveryDrift::Content
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
        let command = "agent-doc recover tasks/doc.md";

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
            (
                RecoveryProjectionVisibleDrift,
                "refresh_recovery_projections_from_visible",
            ),
            (UnsafeUserContentDrift, "blocked"),
        ];
        assert_eq!(default_cases.len(), CloseoutRecoveryState::ALL.len());

        for (state, expected) in default_cases {
            let decision = closeout_recovery_decision_from_state(
                state,
                CloseoutRecoveryDecisionInput::default(),
                Some(command),
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
                | RefreshRecoveryProjectionsFromVisible {
                    command: rendered, ..
                } => {
                    assert_eq!(rendered, command);
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
                    assert_eq!(recommended, command);
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
                    Some(command),
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

        for state in [OpenCycle, MissingResponseBody, UnsafeUserContentDrift] {
            assert_eq!(
                closeout_recovery_decision_from_state(
                    state,
                    CloseoutRecoveryDecisionInput {
                        stale_capture_supersession_proof: Some("heading already answered"),
                        ..CloseoutRecoveryDecisionInput::default()
                    },
                    Some(command),
                ),
                RetireStaleCapture {
                    state,
                    proof: "heading already answered".to_string(),
                }
            );
        }
    }
}
