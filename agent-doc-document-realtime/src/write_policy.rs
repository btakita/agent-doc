//! Deterministic write/reconnect policy for realtime document mutations.
//!
//! The caller owns IO, editor IPC, git inspection, and flow logging. This
//! module owns only pure decisions about when a visible document mutation is
//! allowed to proceed.

use agent_doc_document::commit_normalization::{
    normalize_component_content_for_absorb, redact_component_contents_for_absorb,
};
use agent_doc_document::transient_markers::strip_boundary_markers;
use agent_doc_document::write_normalization::strip_boundary_for_dedup;
use agent_doc_element::element;
use agent_doc_element_exchange::{
    normalization_target_counts, normalize_exchange_prefixes_for_targets,
};
use agent_doc_flow::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};
use agent_doc_prompt_lines::{
    line_looks_like_plain_response_after_prompt, text_line_looks_like_prompt_target,
};
use agent_doc_queue::queue_prompt_drift::queue_prompt_deletions_between;
use agent_doc_turn::closeout_signal::line_is_carry_forward_signal;
use agent_doc_turn::response_replay::response_materialized_in_content;
use anyhow::Result;
use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleWriteTypingFacts {
    pub idle_reached: bool,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleWriteDecision {
    Apply,
    DeferActiveTyping,
}

/// Admission policy for legacy editor patch transports under reliable document
/// liveness. Payload delivery is allowed only when both authorities agree that
/// an editor is live. A disagreement is fail-closed; an agreed detached state
/// lets the caller use its normal disk-authority path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorDeliveryAdmission {
    DeliverToLiveEditor,
    Detached,
    RefuseAuthorityMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorDeliveryAdmissionFacts {
    pub reliable_editor_live: bool,
    pub legacy_endpoint_live: bool,
}

pub const fn decide_editor_delivery_admission(
    facts: EditorDeliveryAdmissionFacts,
) -> EditorDeliveryAdmission {
    match (facts.reliable_editor_live, facts.legacy_endpoint_live) {
        (true, true) => EditorDeliveryAdmission::DeliverToLiveEditor,
        (false, false) => EditorDeliveryAdmission::Detached,
        _ => EditorDeliveryAdmission::RefuseAuthorityMismatch,
    }
}

impl VisibleWriteDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::DeferActiveTyping => "defer_active_typing",
        }
    }
}

pub fn decide_visible_write_after_typing(facts: VisibleWriteTypingFacts) -> VisibleWriteDecision {
    let _timeout_ms = facts.timeout_ms;
    if facts.idle_reached {
        VisibleWriteDecision::Apply
    } else {
        VisibleWriteDecision::DeferActiveTyping
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleWriteCommitCandidateFacts {
    pub commit_candidate_hash: String,
    pub applied_commit_candidate_hash: Option<String>,
    pub model_revision: Option<u64>,
    pub editor_applied_observed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleWriteCommitCandidateDecision {
    Proven,
    MissingProof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleWriteCommitCandidateState {
    AwaitingProof,
    ModelRevisionObserved,
    EditorAppliedObserved,
    ModelRevisionAndAppliedObserved,
    Proven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleWriteCommitCandidateEvent {
    ModelRevisionSeen,
    EditorAppliedSeen,
    CommitCandidateHashMatched,
}

pub struct VisibleWriteCommitCandidateMachine {
    ctx: ThreadSafeContext,
    machine:
        ThreadSafeStateMachine<VisibleWriteCommitCandidateState, VisibleWriteCommitCandidateEvent>,
}

impl VisibleWriteCommitCandidateMachine {
    pub fn new(initial: VisibleWriteCommitCandidateState) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine =
            ThreadSafeStateMachine::new(&ctx, initial, transition_visible_write_commit_candidate);
        Self { ctx, machine }
    }

    pub fn send(&self, event: VisibleWriteCommitCandidateEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> VisibleWriteCommitCandidateState {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: VisibleWriteCommitCandidateState,
        event: VisibleWriteCommitCandidateEvent,
    ) -> Option<VisibleWriteCommitCandidateState> {
        let machine = Self::new(initial);
        machine.send(event).then(|| machine.state())
    }
}

pub fn transition_visible_write_commit_candidate(
    current: &VisibleWriteCommitCandidateState,
    event: &VisibleWriteCommitCandidateEvent,
) -> Option<VisibleWriteCommitCandidateState> {
    use VisibleWriteCommitCandidateEvent::*;
    use VisibleWriteCommitCandidateState::*;

    match (*current, *event) {
        (AwaitingProof, ModelRevisionSeen) => Some(ModelRevisionObserved),
        (AwaitingProof, EditorAppliedSeen) => Some(EditorAppliedObserved),
        (ModelRevisionObserved, EditorAppliedSeen) => Some(ModelRevisionAndAppliedObserved),
        (EditorAppliedObserved, ModelRevisionSeen) => Some(ModelRevisionAndAppliedObserved),
        (ModelRevisionAndAppliedObserved, CommitCandidateHashMatched) => Some(Proven),
        (Proven, ModelRevisionSeen | EditorAppliedSeen | CommitCandidateHashMatched) => {
            Some(Proven)
        }
        _ => None,
    }
}

pub fn visible_write_commit_candidate_state(
    facts: &VisibleWriteCommitCandidateFacts,
) -> VisibleWriteCommitCandidateState {
    let machine =
        VisibleWriteCommitCandidateMachine::new(VisibleWriteCommitCandidateState::AwaitingProof);
    if facts.model_revision.is_some_and(|revision| revision > 0) {
        machine.send(VisibleWriteCommitCandidateEvent::ModelRevisionSeen);
    }
    if facts.editor_applied_observed {
        machine.send(VisibleWriteCommitCandidateEvent::EditorAppliedSeen);
    }
    if !facts.commit_candidate_hash.trim().is_empty()
        && facts
            .applied_commit_candidate_hash
            .as_deref()
            .is_some_and(|applied_hash| {
                applied_hash.eq_ignore_ascii_case(&facts.commit_candidate_hash)
            })
    {
        machine.send(VisibleWriteCommitCandidateEvent::CommitCandidateHashMatched);
    }
    machine.state()
}

pub fn decide_visible_write_commit_candidate(
    facts: &VisibleWriteCommitCandidateFacts,
) -> VisibleWriteCommitCandidateDecision {
    match visible_write_commit_candidate_state(facts) {
        VisibleWriteCommitCandidateState::Proven => VisibleWriteCommitCandidateDecision::Proven,
        VisibleWriteCommitCandidateState::AwaitingProof
        | VisibleWriteCommitCandidateState::ModelRevisionObserved
        | VisibleWriteCommitCandidateState::EditorAppliedObserved
        | VisibleWriteCommitCandidateState::ModelRevisionAndAppliedObserved => {
            VisibleWriteCommitCandidateDecision::MissingProof
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleWriteMaterializedCarryForwardFacts {
    pub commit_candidate_hash: String,
    pub proven_commit_candidate_hash: Option<String>,
    pub file_content_hash: String,
    pub proven_file_content_hash: Option<String>,
    pub live_buffer_hash: String,
    pub proven_live_buffer_hash: Option<String>,
    pub model_revision: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleWriteMaterializedCarryForwardDecision {
    Proven,
    MissingProof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleWriteMaterializedCarryForwardState {
    AwaitingProof,
    ModelRevisionObserved,
    LiveBufferMatched,
    FileContentMatched,
    CandidateMatched,
    Proven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleWriteMaterializedCarryForwardEvent {
    ModelRevisionSeen,
    LiveBufferHashMatched,
    FileContentHashMatched,
    CommitCandidateHashMatched,
}

pub struct VisibleWriteMaterializedCarryForwardMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<
        VisibleWriteMaterializedCarryForwardState,
        VisibleWriteMaterializedCarryForwardEvent,
    >,
}

impl VisibleWriteMaterializedCarryForwardMachine {
    pub fn new(initial: VisibleWriteMaterializedCarryForwardState) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(
            &ctx,
            initial,
            transition_visible_write_materialized_carry_forward,
        );
        Self { ctx, machine }
    }

    pub fn send(&self, event: VisibleWriteMaterializedCarryForwardEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> VisibleWriteMaterializedCarryForwardState {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: VisibleWriteMaterializedCarryForwardState,
        event: VisibleWriteMaterializedCarryForwardEvent,
    ) -> Option<VisibleWriteMaterializedCarryForwardState> {
        let machine = Self::new(initial);
        machine.send(event).then(|| machine.state())
    }
}

pub fn transition_visible_write_materialized_carry_forward(
    current: &VisibleWriteMaterializedCarryForwardState,
    event: &VisibleWriteMaterializedCarryForwardEvent,
) -> Option<VisibleWriteMaterializedCarryForwardState> {
    use VisibleWriteMaterializedCarryForwardEvent::*;
    use VisibleWriteMaterializedCarryForwardState::*;

    match (*current, *event) {
        (AwaitingProof, ModelRevisionSeen) => Some(ModelRevisionObserved),
        (ModelRevisionObserved, LiveBufferHashMatched) => Some(LiveBufferMatched),
        (LiveBufferMatched, FileContentHashMatched) => Some(FileContentMatched),
        (FileContentMatched, CommitCandidateHashMatched) => Some(Proven),
        (
            Proven,
            ModelRevisionSeen
            | LiveBufferHashMatched
            | FileContentHashMatched
            | CommitCandidateHashMatched,
        ) => Some(Proven),
        _ => None,
    }
}

pub fn visible_write_materialized_carry_forward_state(
    facts: &VisibleWriteMaterializedCarryForwardFacts,
) -> VisibleWriteMaterializedCarryForwardState {
    let machine = VisibleWriteMaterializedCarryForwardMachine::new(
        VisibleWriteMaterializedCarryForwardState::AwaitingProof,
    );
    if facts.model_revision.is_some_and(|revision| revision > 0) {
        machine.send(VisibleWriteMaterializedCarryForwardEvent::ModelRevisionSeen);
    }
    if !facts.live_buffer_hash.trim().is_empty()
        && facts
            .proven_live_buffer_hash
            .as_deref()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(&facts.live_buffer_hash))
    {
        machine.send(VisibleWriteMaterializedCarryForwardEvent::LiveBufferHashMatched);
    }
    if !facts.file_content_hash.trim().is_empty()
        && facts
            .proven_file_content_hash
            .as_deref()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(&facts.file_content_hash))
    {
        machine.send(VisibleWriteMaterializedCarryForwardEvent::FileContentHashMatched);
    }
    if !facts.commit_candidate_hash.trim().is_empty()
        && facts
            .proven_commit_candidate_hash
            .as_deref()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(&facts.commit_candidate_hash))
    {
        machine.send(VisibleWriteMaterializedCarryForwardEvent::CommitCandidateHashMatched);
    }
    machine.state()
}

pub fn decide_visible_write_materialized_carry_forward(
    facts: &VisibleWriteMaterializedCarryForwardFacts,
) -> VisibleWriteMaterializedCarryForwardDecision {
    match visible_write_materialized_carry_forward_state(facts) {
        VisibleWriteMaterializedCarryForwardState::Proven => {
            VisibleWriteMaterializedCarryForwardDecision::Proven
        }
        VisibleWriteMaterializedCarryForwardState::AwaitingProof
        | VisibleWriteMaterializedCarryForwardState::ModelRevisionObserved
        | VisibleWriteMaterializedCarryForwardState::LiveBufferMatched
        | VisibleWriteMaterializedCarryForwardState::FileContentMatched
        | VisibleWriteMaterializedCarryForwardState::CandidateMatched => {
            VisibleWriteMaterializedCarryForwardDecision::MissingProof
        }
    }
}

pub fn visible_write_guard_event(decision: VisibleWriteDecision, source: &str) -> FlowEvent {
    let outcome = match decision {
        VisibleWriteDecision::Apply => FlowOutcome::Completed,
        VisibleWriteDecision::DeferActiveTyping => FlowOutcome::Blocked,
    };
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PreWriteGuard,
        outcome,
    )
    .with_reason(format!(
        "visible_write_typing_{}:{source}",
        decision.as_str()
    ))
}

pub fn visible_write_current_changed_event(source: &str) -> FlowEvent {
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PreWriteGuard,
        FlowOutcome::Blocked,
    )
    .with_reason(format!("visible_write_current_changed:{source}"))
}

/// Outcome of reconciling the visible-write guard with the on-disk state.
///
/// The caller owns IO and live-buffer inspection. This policy value only
/// distinguishes a clean write from reconcilable disk drift that should trigger
/// a re-merge before retrying the visible write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibleWriteReconcile {
    /// Disk and the live editor buffer agree with the expected content; the
    /// caller may write its computed payload.
    Clean,
    /// The on-disk file drifted after the response merge was computed, but the
    /// live editor buffer did not diverge. Carries the fresh disk content so the
    /// caller can re-merge the captured response against it and retry.
    DiskDrifted { fresh_current: String },
}

/// Drive the visible-write reconcile policy.
///
/// Starting from `initial_current`/`initial_payload`, repeatedly consult
/// `guard`. On [`VisibleWriteReconcile::Clean`], the current payload is safe to
/// write and `(current, payload)` is returned. On reconcilable disk drift,
/// `recompute` re-merges against the fresh disk content and the guard is retried
/// up to `max_attempts`. If drift never settles, `fail_closed` is invoked so the
/// orchestration layer can return its normal retry/error path.
pub fn reconcile_visible_write<C: ?Sized, T>(
    context: &C,
    initial_current: String,
    initial_payload: T,
    max_attempts: usize,
    mut guard: impl FnMut(&C, &str, &T) -> Result<VisibleWriteReconcile>,
    mut recompute: impl FnMut(&str) -> Result<T>,
    fail_closed: impl FnOnce(&C, &str, &T) -> Result<()>,
) -> Result<(String, T)> {
    let mut current = initial_current;
    let mut payload = initial_payload;
    for _ in 0..max_attempts {
        match guard(context, &current, &payload)? {
            VisibleWriteReconcile::Clean => return Ok((current, payload)),
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                current = fresh_current;
                payload = recompute(&current)?;
            }
        }
    }
    fail_closed(context, &current, &payload)?;
    Ok((current, payload))
}

/// Decide whether a direct disk fallback must be refused after an editor IPC
/// delivery attempt already failed.
///
/// An active editor authority means the editor buffer, not the disk replica, owns
/// the current document text — but only while its IPC transport is answering.
/// The durable reliable-sync liveness projection is the sole editor-authority
/// signal. It still requires a live, answering listener to force a refusal: an
/// open fact with a dead socket means the editor cannot receive or re-save the
/// write, so refusing disk would wedge the document forever
/// (`#staleattachdemote`, stale attached-editor demotion). Plugin-owner leases are
/// deliberately absent from this decision; P4 retired that divergent cold path.
pub fn should_refuse_disk_fallback(
    editor_authority_active: bool,
    listener_answering: bool,
) -> bool {
    listener_answering && editor_authority_active
}

/// Apply `❯ ` prefix to lines in `content` that appear in `prefix_lines`.
///
/// IPC patch content is normalized before delivery so newly-appended lines
/// already carry the prompt prefix when the editor applies them.
pub fn normalize_patch_content(content: &str, prefix_lines: &[String]) -> String {
    if prefix_lines.is_empty() {
        return content.to_string();
    }
    let mut remaining = normalization_target_counts(prefix_lines);
    let mut result = String::with_capacity(content.len() + 2 * prefix_lines.len());
    for line in content.lines() {
        let bare = line
            .trim_end()
            .strip_prefix("\u{276f} ")
            .unwrap_or(line.trim_end());
        if line_looks_like_plain_response_after_prompt(bare) {
            result.push_str(line);
            result.push('\n');
            continue;
        }
        if !line.starts_with("\u{276f} ")
            && let Some(remaining_count) = remaining.get_mut(bare)
            && *remaining_count > 0
        {
            result.push_str("\u{276f} ");
            *remaining_count -= 1;
        }
        result.push_str(line);
        result.push('\n');
    }
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.truncate(result.len() - 1);
    }
    result
}

fn normalize_component_content_for_delta(content: &str) -> String {
    agent_doc_diff::strip_comments(&strip_boundary_markers(content))
}

fn containment_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn base_prompt_prefix_equivalents(base: &str) -> HashSet<String> {
    base.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            Some(
                trimmed
                    .strip_prefix('❯')
                    .unwrap_or(trimmed)
                    .trim()
                    .to_string(),
            )
        })
        .collect()
}

fn inserted_delta_hunks(base: &str, ours: &str) -> Vec<Vec<String>> {
    let base_prefix_equivalents = base_prompt_prefix_equivalents(base);
    let base_lines = base
        .lines()
        .filter_map(containment_line)
        .collect::<HashSet<_>>();
    let diff = similar::TextDiff::from_lines(base, ours);
    let mut hunks = Vec::<Vec<String>>::new();
    let mut current = Vec::<String>::new();

    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => {
                let line = change.to_string();
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if base_lines.contains(trimmed) {
                    continue;
                }
                if let Some(unprefixed) = trimmed.strip_prefix('❯') {
                    let unprefixed = unprefixed.trim();
                    if base_prefix_equivalents.contains(unprefixed) {
                        continue;
                    }
                }
                current.push(trimmed.to_string());
            }
            similar::ChangeTag::Delete | similar::ChangeTag::Equal => {
                if !current.is_empty() {
                    hunks.push(std::mem::take(&mut current));
                }
            }
        }
    }
    if !current.is_empty() {
        hunks.push(current);
    }

    hunks
        .into_iter()
        .filter(|hunk| response_delta_hunk_is_actionable(hunk))
        .collect()
}

fn response_delta_hunk_is_actionable(hunk: &[String]) -> bool {
    hunk.iter().any(|line| {
        line.starts_with("### Re:")
            || line.starts_with("## Assistant")
            || line.starts_with("## User")
    }) || hunk.len() >= 2
}

fn contains_contiguous_hunk(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Detect whether the current document already contains the response delta.
///
/// On IPC sidecar ack timeout, socket/file delivery may have succeeded while
/// confirmation did not arrive in time. If the editor applied the patches, the
/// exchange component in `content_current` already contains the response delta
/// from `base -> content_ours`; applying it again would duplicate the response.
///
/// Detection computes normalized insertion hunks from `base -> content_ours` in
/// `agent:exchange`, ignores boundary/comment churn and prompt-prefix-only
/// normalization lines, and requires each actionable response hunk to appear
/// contiguously in `content_current`.
pub fn response_already_in_current(base: &str, content_ours: &str, content_current: &str) -> bool {
    let base_comps = element::parse(base).unwrap_or_default();
    let ours_comps = element::parse(content_ours).unwrap_or_default();
    let current_comps = element::parse(content_current).unwrap_or_default();

    let base_exc = base_comps
        .iter()
        .find(|component| component.name == "exchange");
    let ours_exc = ours_comps
        .iter()
        .find(|component| component.name == "exchange");
    let current_exc = current_comps
        .iter()
        .find(|component| component.name == "exchange");

    let (Some(base_e), Some(ours_e), Some(current_e)) = (base_exc, ours_exc, current_exc) else {
        return false;
    };

    let base_content = normalize_component_content_for_delta(base_e.content(base));
    let ours_content = normalize_component_content_for_delta(ours_e.content(content_ours));
    let current_content = normalize_component_content_for_delta(current_e.content(content_current));

    if ours_content.trim() == base_content.trim() {
        return false;
    }

    let response_hunks = inserted_delta_hunks(&base_content, &ours_content);
    if response_hunks.is_empty() {
        return false;
    }

    let current_lines = current_content
        .lines()
        .filter_map(containment_line)
        .collect::<Vec<_>>();
    response_hunks
        .iter()
        .all(|hunk| contains_contiguous_hunk(&current_lines, hunk))
}

/// The `### Re:` response heading lines present in the agent's `candidate`
/// exchange component but absent from `base`.
pub fn new_agent_response_headings(base: &str, candidate: &str) -> Vec<String> {
    let base_ex = exchange_component_text(base);
    let candidate_ex = exchange_component_text(candidate);
    let base_headings: HashSet<&str> = base_ex
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("### Re:"))
        .collect();
    candidate_ex
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("### Re:"))
        .filter(|line| !base_headings.contains(line))
        .map(str::to_string)
        .collect()
}

/// Return true when visible-write content already contains `target`'s latest exchange
/// response block.
fn visible_write_contains_latest_response(visible_write_content: &str, target: &str) -> bool {
    let Some(response) = latest_exchange_response_block(target) else {
        return true;
    };
    response_materialized_in_content(&response, visible_write_content)
}

/// Operator-edit-tolerant convergence predicate for the `live_prompt_drift`
/// closeout path (`#adoc-live-prompt-drift-operator-edit`).
///
/// Returns true when `target` — the reconciled snapshot about to be adopted —
/// already presents every `### Re:` response heading the agent authored this
/// cycle (`base` → `candidate`) with a non-empty body. That means the response
/// converged into the operator-visible document even though the operator may
/// have edited the body before the write landed, so the editor already shows
/// the response and NO redelivery/repair is required — closeout must not wedge.
///
/// This is the reconcile-aware counterpart to
/// [`visible_write_contains_latest_response`], which requires the agent's *exact*
/// response bytes and therefore misreads any operator body-edit as
/// "response missing", forcing a needless editor redelivery that cannot prove
/// against a lagging disk (the observed live_prompt_drift wedge). When the cycle
/// authored no new `### Re:` heading, it falls back to the exact materialization
/// check so non-heading turns keep their prior proof semantics unchanged.
pub fn response_converged_in_visible_target(base: &str, candidate: &str, target: &str) -> bool {
    let headings = new_agent_response_headings(base, candidate);
    if headings.is_empty() {
        return visible_write_contains_latest_response(candidate, target);
    }
    let target_exchange = exchange_component_text(target);
    headings
        .iter()
        .all(|heading| response_heading_has_body(&target_exchange, heading))
}

/// One decision step of the bounded reconcile-before-accept loop (Phase 2,
/// `#adoc-live-prompt-drift-operator-edit`). The realtime model owns the decision;
/// the caller owns the IO (reading the operator live buffer and settling between
/// rounds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatorReconcileStep {
    /// The operator buffer is stable (unchanged since the previous read) and still
    /// presents the response — adopt this content as the reconciled snapshot.
    Accept(String),
    /// The operator is still editing (buffer changed since the previous read) —
    /// keep looping until it settles or the caller's bound is hit.
    Continue,
    /// The settled buffer no longer carries the response — fail closed; the
    /// existing point-in-time proof stays authoritative rather than committing a
    /// buffer that dropped the agent's turn.
    FailClosed,
}

/// Decide one reconcile step. Given the proven response `reference` and two
/// consecutive operator live-buffer reads (`prev`, `curr`): an unchanged buffer
/// (`curr == prev`) that still presents the response is accepted; an unchanged
/// buffer that dropped the response fails closed; a changed buffer means the
/// operator is still editing, so continue the loop.
pub fn operator_reconcile_step(
    reference: &str,
    prev: Option<&str>,
    curr: &str,
) -> OperatorReconcileStep {
    if prev != Some(curr) {
        return OperatorReconcileStep::Continue;
    }
    if buffer_presents_reference_response(reference, curr) {
        OperatorReconcileStep::Accept(curr.to_string())
    } else {
        OperatorReconcileStep::FailClosed
    }
}

/// True when `buffer` still presents the latest `### Re:` response block found in
/// `reference` — matched by heading with a non-empty body — even if the operator
/// edited the body. `reference` is the proven visible-write content (which carries
/// the response); `buffer` is a *newer* operator live buffer that has moved ahead
/// of it. Used to reconcile a stale lazily-visible disk write FORWARD to the
/// operator's newer buffer instead of wedging on a `stale_source_buffer` mismatch
/// (`#adoc-live-prompt-drift-operator-edit`). When `reference` carries no response
/// block there is nothing to preserve, so it returns true.
pub fn buffer_presents_reference_response(reference: &str, buffer: &str) -> bool {
    let Some(response) = latest_exchange_response_block(reference) else {
        return true;
    };
    match first_response_heading(&response) {
        Some(heading) => response_heading_has_body(&exchange_component_text(buffer), &heading),
        None => visible_write_contains_latest_response(reference, buffer),
    }
}

/// True when `exchange` contains `heading` on its own trimmed line followed by
/// at least one non-empty content line before the next `### Re:` heading,
/// `<!-- agent:boundary:` marker, or the end of the component — i.e. the
/// response body was kept (possibly operator-edited), not emptied. An emptied or
/// missing body returns false so the fail-closed repair path still runs for that
/// degenerate case.
fn response_heading_has_body(exchange: &str, heading: &str) -> bool {
    let mut lines = exchange.lines();
    while let Some(line) = lines.next() {
        if line.trim() != heading {
            continue;
        }
        for body in lines.by_ref() {
            let trimmed = body.trim();
            if trimmed.starts_with("### Re:") || trimmed.starts_with("<!-- agent:boundary:") {
                return false;
            }
            if !trimmed.is_empty() {
                return true;
            }
        }
        return false;
    }
    false
}

fn latest_exchange_response_block(content: &str) -> Option<String> {
    let exchange = exchange_content(content);
    let lines = exchange
        .split_inclusive('\n')
        .scan(0usize, |offset, text| {
            let start = *offset;
            *offset += text.len();
            Some((start, text))
        })
        .collect::<Vec<_>>();
    let start = lines
        .iter()
        .rposition(|(_, line)| line.trim_start().starts_with("### Re:"))?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(idx, (_, line))| {
            (line.trim_start().starts_with("### Re:")
                || line.trim_start().starts_with("<!-- agent:boundary:"))
            .then_some(idx)
        })
        .unwrap_or(lines.len());
    let block_start = lines[start].0;
    let block_end = lines
        .get(end)
        .map(|(offset, _)| *offset)
        .unwrap_or(exchange.len());
    Some(exchange[block_start..block_end].to_string())
}

fn exchange_content(content: &str) -> &str {
    element::parse(content)
        .ok()
        .and_then(|components| {
            components
                .into_iter()
                .find(|component| component.name == "exchange")
        })
        .map(|component| component.content(content))
        .unwrap_or(content)
}

pub fn first_response_heading(response: &str) -> Option<String> {
    response
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("### Re:"))
        .map(str::to_string)
}

/// True when a rejected editor state can be repaired by applying only exchange
/// prompt-prefix normalization to the targeted lines.
pub fn normalization_repair_candidate_matches(
    expected_bad_state: &str,
    repaired_content: &str,
    normalize_prefix_lines: &[String],
) -> bool {
    if normalize_prefix_lines.is_empty() {
        return false;
    }
    let normalized =
        normalize_exchange_prefixes_for_targets(expected_bad_state, normalize_prefix_lines);
    strip_boundary_for_dedup(&normalized) == strip_boundary_for_dedup(repaired_content)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullContentSourceProof {
    pub expected_content_hash: String,
    pub expected_content_len: usize,
}

impl FullContentSourceProof {
    pub fn from_content(content: &str) -> Self {
        Self {
            expected_content_hash: content_hash(content),
            expected_content_len: content.len(),
        }
    }

    pub fn matches_current(&self, current_content: &str) -> bool {
        current_content.len() == self.expected_content_len
            && content_hash(current_content) == self.expected_content_hash
    }
}

pub fn full_content_source_proof(before_content: Option<&str>) -> Option<FullContentSourceProof> {
    before_content.map(FullContentSourceProof::from_content)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotPersistMode {
    FinalContent,
    ContentOurs,
}

pub fn snapshot_persist_mode(
    baseline: Option<&str>,
    content_ours: &str,
    final_content: &str,
) -> SnapshotPersistMode {
    if baseline.is_none() {
        return SnapshotPersistMode::FinalContent;
    }

    let ours_norm = strip_boundary_markers(content_ours);
    let final_norm = strip_boundary_markers(final_content);
    if ours_norm == final_norm {
        return SnapshotPersistMode::FinalContent;
    }

    if crate::baseline_comparison::detect_bypassed_response_write_between(&ours_norm, &final_norm)
        .is_some()
    {
        return SnapshotPersistMode::FinalContent;
    }

    let ours_prompt_norm = agent_doc_diff::strip_comments(&ours_norm);
    let final_prompt_norm = agent_doc_diff::strip_comments(&final_norm);
    let Some(diff_text) =
        agent_doc_diff::unified_diff_from_contents(&ours_prompt_norm, &final_prompt_norm)
    else {
        return SnapshotPersistMode::FinalContent;
    };
    let has_prompt_bearing_user_drift = agent_doc_diff::classify_prompt_bearing_changes(&diff_text)
        .iter()
        .any(|change| {
            matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::PromptTarget
                    | agent_doc_diff::PromptBearingChangeKind::ContentEdit
            )
        });

    if has_prompt_bearing_user_drift {
        SnapshotPersistMode::ContentOurs
    } else {
        SnapshotPersistMode::FinalContent
    }
}

pub fn snapshot_persist_mode_with_current(
    baseline: Option<&str>,
    base: &str,
    content_current: &str,
    content_ours: &str,
    final_content: &str,
) -> SnapshotPersistMode {
    if baseline.is_some()
        && post_exchange_ordinary_comment_carry_forward_drift(content_ours, content_current)
    {
        return SnapshotPersistMode::ContentOurs;
    }

    if baseline.is_some()
        && strip_boundary_markers(base) != strip_boundary_markers(content_current)
        && (has_prompt_bearing_user_drift(base, content_current)
            || non_exchange_drift_carries_directive(base, content_current))
    {
        return SnapshotPersistMode::ContentOurs;
    }

    snapshot_persist_mode(baseline, content_ours, final_content)
}

pub fn snapshot_content_to_persist<'a>(
    mode: SnapshotPersistMode,
    content_ours: &'a str,
    final_content: &'a str,
) -> &'a str {
    match mode {
        SnapshotPersistMode::FinalContent => final_content,
        SnapshotPersistMode::ContentOurs => content_ours,
    }
}

fn non_exchange_drift_carries_directive(base: &str, current: &str) -> bool {
    let base_norm = strip_boundary_markers(base);
    let current_norm = strip_boundary_markers(current);
    if base_norm == current_norm {
        return false;
    }
    if !outside_component_content_changed(&base_norm, &current_norm, "exchange") {
        return false;
    }
    added_nonblank_lines(&base_norm, &current_norm)
        .iter()
        .any(|line| line_is_carry_forward_signal(line))
}

fn outside_component_content_changed(left: &str, right: &str, component_name: &str) -> bool {
    let left_component = match agent_doc_element::element::parse(left) {
        Ok(components) => components.into_iter().find(|c| c.name == component_name),
        Err(_) => return left != right,
    };
    let right_component = match agent_doc_element::element::parse(right) {
        Ok(components) => components.into_iter().find(|c| c.name == component_name),
        Err(_) => return left != right,
    };

    let Some(left_component) = left_component else {
        return left != right;
    };
    let Some(right_component) = right_component else {
        return true;
    };

    left[..left_component.open_end] != right[..right_component.open_end]
        || left[left_component.close_start..] != right[right_component.close_start..]
}

fn has_prompt_bearing_user_drift(base: &str, current: &str) -> bool {
    !prompt_bearing_user_changes_between(base, current).is_empty()
}

pub fn prompt_bearing_user_changes_between(
    base: &str,
    current: &str,
) -> Vec<agent_doc_diff::PromptBearingChange> {
    let base_norm = strip_boundary_markers(base);
    let current_norm = strip_boundary_markers(current);
    let base_prompt_norm = agent_doc_diff::strip_comments(&base_norm);
    let current_prompt_norm = agent_doc_diff::strip_comments(&current_norm);
    let Some(diff_text) =
        agent_doc_diff::unified_diff_from_contents(&base_prompt_norm, &current_prompt_norm)
    else {
        return Vec::new();
    };
    let mut changes: Vec<_> = agent_doc_diff::classify_prompt_bearing_changes(&diff_text)
        .into_iter()
        .filter(|change| {
            matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::PromptTarget
                    | agent_doc_diff::PromptBearingChangeKind::ContentEdit
            )
        })
        .collect();
    if diff_text.lines().any(|line| {
        let Some(added) = line.strip_prefix('+') else {
            return false;
        };
        if line.starts_with("+++") {
            return false;
        }
        let trimmed = added.trim();
        trimmed.starts_with('❯') || text_line_looks_like_prompt_target(trimmed)
    }) {
        for line in diff_text.lines() {
            let Some(added) = line.strip_prefix('+') else {
                continue;
            };
            if line.starts_with("+++") {
                continue;
            }
            let trimmed = added.trim();
            if trimmed.starts_with('❯') || text_line_looks_like_prompt_target(trimmed) {
                let text = trimmed
                    .strip_prefix('❯')
                    .unwrap_or(trimmed)
                    .trim()
                    .to_string();
                if !changes.iter().any(|change| {
                    change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget
                        && change.text.trim() == text
                }) {
                    changes.push(agent_doc_diff::PromptBearingChange {
                        kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
                        text,
                    });
                }
            }
        }
    }
    changes
}

fn prompt_bearing_change_owned_by_content_ours(
    change: &agent_doc_diff::PromptBearingChange,
    owned_changes: &[agent_doc_diff::PromptBearingChange],
) -> bool {
    let text = normalized_prompt_line(&change.text);
    owned_changes
        .iter()
        .any(|owned| owned.kind == change.kind && normalized_prompt_line(&owned.text) == text)
}

pub fn ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
    baseline: &str,
    snapshot_candidate: &str,
    content_ours: &str,
) -> bool {
    let baseline_norm = strip_boundary_markers(baseline);
    let candidate_norm = strip_boundary_markers(snapshot_candidate);
    let ours_norm = strip_boundary_markers(content_ours);
    if outside_component_content_changed(&baseline_norm, &candidate_norm, "exchange")
        && outside_component_content_changed(&ours_norm, &candidate_norm, "exchange")
    {
        return true;
    }
    if candidate_has_unowned_prompt_target_line(snapshot_candidate, content_ours) {
        return true;
    }
    if content_ours_drops_operator_text(baseline, snapshot_candidate, content_ours) {
        return true;
    }

    let candidate_changes = prompt_bearing_user_changes_between(baseline, snapshot_candidate);
    if candidate_changes.is_empty() {
        return false;
    }
    let owned_changes = prompt_bearing_user_changes_between(baseline, content_ours);
    candidate_changes
        .iter()
        .any(|change| !prompt_bearing_change_owned_by_content_ours(change, &owned_changes))
}

fn candidate_has_unowned_prompt_target_line(candidate: &str, content_ours: &str) -> bool {
    let owned = prompt_target_line_set(content_ours);
    prompt_target_line_set(candidate)
        .into_iter()
        .any(|line| !owned.contains(&line))
}

fn prompt_target_line_set(doc: &str) -> HashSet<String> {
    exchange_component_or_document_text(doc)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| line.starts_with('❯') || text_line_looks_like_prompt_target(line))
        .map(normalized_prompt_line)
        .collect()
}

fn exchange_component_or_document_text(doc: &str) -> String {
    let exchange = exchange_component_text(doc);
    if exchange.is_empty() {
        doc.to_string()
    } else {
        exchange
    }
}

fn added_nonblank_lines(baseline: &str, candidate: &str) -> Vec<String> {
    let base: HashSet<&str> = baseline
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    candidate
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !base.contains(line))
        .map(|line| line.to_string())
        .collect()
}

pub fn response_target_disjoint_from_user_edit(
    baseline: &str,
    content_ours: &str,
    candidate: &str,
    merge_contents: impl FnOnce(&str, &str, &str) -> Option<String>,
) -> bool {
    if strip_boundary_markers(candidate) == strip_boundary_markers(content_ours) {
        return false;
    }
    let user_added = added_nonblank_lines(baseline, candidate);
    if user_added.is_empty() {
        return false;
    }
    if !queue_prompt_deletions_between(baseline, candidate).is_empty() {
        return false;
    }

    let baseline_ex = exchange_component_text(baseline);
    let candidate_ex = exchange_component_text(candidate);
    let ours_ex = exchange_component_text(content_ours);
    let response_ex_added: HashSet<String> = added_nonblank_lines(&baseline_ex, &ours_ex)
        .into_iter()
        .collect();
    let user_ex_added = added_nonblank_lines(&baseline_ex, &candidate_ex)
        .into_iter()
        .any(|line| !response_ex_added.contains(&line));
    if user_ex_added {
        return false;
    }

    let response_added_set: HashSet<String> = added_nonblank_lines(baseline, content_ours)
        .into_iter()
        .collect();
    let user_carries_directive = user_added
        .iter()
        .filter(|line| !response_added_set.contains(*line))
        .any(|line| line_is_carry_forward_signal(line));
    if user_carries_directive {
        return false;
    }

    let Some(merged) = merge_contents(baseline, content_ours, candidate) else {
        return false;
    };
    if merged.contains("<<<<<<<") || merged.contains(">>>>>>>") {
        return false;
    }
    let merged_lines: HashSet<&str> = merged
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let response_added = added_nonblank_lines(baseline, content_ours);
    response_added
        .iter()
        .all(|line| merged_lines.contains(line.as_str()))
        && user_added
            .iter()
            .all(|line| merged_lines.contains(line.as_str()))
}

pub fn exchange_component_text(doc: &str) -> String {
    let Ok(components) = agent_doc_element::element::parse(doc) else {
        return String::new();
    };
    components
        .iter()
        .find(|component| component.name == "exchange")
        .map(|component| component.content(doc).to_string())
        .unwrap_or_default()
}

pub fn dropped_prompt_lines_after_content_ours(
    baseline: &str,
    candidate: &str,
    content_ours: &str,
) -> Vec<String> {
    let baseline_ex = exchange_component_text(baseline);
    let candidate_ex = exchange_component_text(candidate);
    let content_ours_ex = exchange_component_text(content_ours);

    let candidate_changes = prompt_bearing_user_changes_between(&baseline_ex, &candidate_ex);
    if candidate_changes.is_empty() {
        return Vec::new();
    }
    let owned_changes = prompt_bearing_user_changes_between(&baseline_ex, &content_ours_ex);
    candidate_changes
        .into_iter()
        .filter(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget)
        .filter(|change| !prompt_bearing_change_owned_by_content_ours(change, &owned_changes))
        .map(|change| change.text.trim().to_string())
        .filter(|text| !text.is_empty() && !text.contains('\n'))
        .collect()
}

/// True when persisting `content_ours` over `candidate` (the live/final buffer)
/// would drop operator-authored exchange text present in `candidate` but absent
/// from `content_ours` — covering both new prompts (`PromptTarget`) and edits to
/// existing operator text (`ContentEdit`).
///
/// This is the trigger for a durable recovery sidecar so concurrent operator
/// text is never silently lost when the operator-wins merge selects the agent
/// candidate (`#qftlossdelta`): broader than
/// [`dropped_prompt_lines_after_content_ours`], which only enumerates single-line
/// prompt targets for display.
pub fn content_ours_drops_operator_text(
    baseline: &str,
    candidate: &str,
    content_ours: &str,
) -> bool {
    let baseline_ex = exchange_component_text(baseline);
    let candidate_ex = exchange_component_text(candidate);
    let content_ours_ex = exchange_component_text(content_ours);

    let candidate_changes = prompt_bearing_user_changes_between(&baseline_ex, &candidate_ex);
    if candidate_changes.is_empty() {
        return false;
    }
    let owned_changes = prompt_bearing_user_changes_between(&baseline_ex, &content_ours_ex);
    candidate_changes
        .into_iter()
        .filter(|change| {
            matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::PromptTarget
                    | agent_doc_diff::PromptBearingChangeKind::ContentEdit
            )
        })
        .any(|change| !prompt_bearing_change_owned_by_content_ours(&change, &owned_changes))
}

/// Build the committed-snapshot content when the merge would otherwise choose
/// `content_ours` (deeper root cause A). `final_content` is the full union already
/// written to disk (agent response + operator edits + any concurrently-typed
/// prompt). Returns that union with only the **carry-forward** prompt lines
/// stripped — the concurrently-typed operator prompts that must stay UNCOMMITTED
/// for the next cycle (`#fintol/#pcwc`). The result keeps every operator *edit*
/// (so nothing is lost, `#qftlossdelta`) while still excluding the new prompt (so
/// it carries forward). When no carry-forward prompt exists this is `final_content`
/// verbatim.
///
/// Precise line stripping (normalized prompt identity) rather than a re-merge:
/// `final_content` is already the correct union, so we only subtract the exact
/// carry-forward prompt lines instead of risking conflict markers from a fresh
/// 3-way merge of the committed authority.
pub fn committed_snapshot_union_excluding_carry_forward(
    base: &str,
    content_ours: &str,
    content_current: &str,
    final_content: &str,
) -> String {
    let carry_forward =
        dropped_prompt_lines_after_content_ours(base, content_current, content_ours);
    let snapshot = strip_prompt_lines(final_content, &carry_forward);
    if post_exchange_ordinary_comment_carry_forward_drift(content_ours, content_current) {
        restore_post_exchange_gap_from_content_ours(content_ours, &snapshot).unwrap_or(snapshot)
    } else {
        snapshot
    }
}

fn post_exchange_ordinary_comment_carry_forward_drift(
    content_ours: &str,
    content_current: &str,
) -> bool {
    let ours = agent_doc_diff::post_exchange_ordinary_html_comments(content_ours);
    let current = agent_doc_diff::post_exchange_ordinary_html_comments(content_current);
    if current.len() > ours.len() {
        return true;
    }
    current
        .iter()
        .zip(ours.iter())
        .any(|(current, ours)| current != ours && ordinary_comment_carries_prompt_work(current))
}

fn ordinary_comment_carries_prompt_work(comment: &str) -> bool {
    !agent_doc_diff::post_exchange_comment_directive_signals(comment).is_empty()
        || comment.lines().any(|line| {
            let trimmed = line.trim().trim_start_matches('❯').trim();
            !trimmed.is_empty()
                && (text_line_looks_like_prompt_target(trimmed)
                    || starts_with_prompt_preset_reference(trimmed))
        })
}

fn post_exchange_gap_range(doc: &str) -> Option<std::ops::Range<usize>> {
    let components = element::parse(doc).ok()?;
    let exchange = components
        .iter()
        .filter(|component| component.name == "exchange")
        .max_by_key(|component| component.close_end)?;
    let start = exchange.close_end;
    let end = components
        .iter()
        .filter(|component| component.open_start >= start)
        .map(|component| component.open_start)
        .min()
        .unwrap_or(doc.len());
    Some(start..end)
}

fn restore_post_exchange_gap_from_content_ours(
    content_ours: &str,
    snapshot: &str,
) -> Option<String> {
    let ours_gap = post_exchange_gap_range(content_ours)?;
    let snapshot_gap = post_exchange_gap_range(snapshot)?;
    let mut restored = String::with_capacity(snapshot.len() - snapshot_gap.len() + ours_gap.len());
    restored.push_str(&snapshot[..snapshot_gap.start]);
    restored.push_str(&content_ours[ours_gap]);
    restored.push_str(&snapshot[snapshot_gap.end..]);
    Some(restored)
}

/// Remove lines from `content` whose normalized prompt identity matches one of
/// `prompt_lines`. Preserves the trailing-newline shape of `content`.
fn strip_prompt_lines(content: &str, prompt_lines: &[String]) -> String {
    if prompt_lines.is_empty() {
        return content.to_string();
    }
    let strip: std::collections::HashSet<String> = prompt_lines
        .iter()
        .map(|line| normalized_prompt_line(line))
        .filter(|line| !line.is_empty())
        .collect();
    if strip.is_empty() {
        return content.to_string();
    }
    let mut out = content
        .lines()
        .filter(|line| !strip.contains(&normalized_prompt_line(line)))
        .collect::<Vec<_>>()
        .join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn normalized_prompt_line(line: &str) -> String {
    line.trim()
        .strip_prefix('❯')
        .unwrap_or_else(|| line.trim())
        .trim()
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullContentVisibleReplacementDecision {
    Apply,
    RejectStaleSourceBuffer,
}

impl FullContentVisibleReplacementDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::RejectStaleSourceBuffer => "reject_stale_source_buffer",
        }
    }
}

pub fn decide_full_content_visible_replacement(
    current_content: &str,
    proof: Option<&FullContentSourceProof>,
) -> FullContentVisibleReplacementDecision {
    match proof {
        Some(proof) if !proof.matches_current(current_content) => {
            FullContentVisibleReplacementDecision::RejectStaleSourceBuffer
        }
        _ => FullContentVisibleReplacementDecision::Apply,
    }
}

pub fn full_content_visible_replacement_event(
    decision: FullContentVisibleReplacementDecision,
    source: &str,
) -> FlowEvent {
    let outcome = match decision {
        FullContentVisibleReplacementDecision::Apply => FlowOutcome::Completed,
        FullContentVisibleReplacementDecision::RejectStaleSourceBuffer => FlowOutcome::Blocked,
    };
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PreWriteGuard,
        outcome,
    )
    .with_reason(format!(
        "full_content_source_buffer_{}:{source}",
        decision.as_str()
    ))
}

#[cfg(test)]
mod write_flow_event_tests {
    use super::*;

    #[test]
    fn visible_write_guard_defers_when_typing_never_settles() {
        let decision = VisibleWriteDecision::DeferActiveTyping;
        let event = visible_write_guard_event(decision, "socket_ipc");

        assert_eq!(decision, VisibleWriteDecision::DeferActiveTyping);
        assert_eq!(event.flow, FlowName::DocumentMutation);
        assert_eq!(event.stage, FlowStage::PreWriteGuard);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(
            event.reason.as_deref(),
            Some("visible_write_typing_defer_active_typing:socket_ipc")
        );
    }

    #[test]
    fn visible_write_guard_allows_idle_writes() {
        let decision = VisibleWriteDecision::Apply;
        let event = visible_write_guard_event(decision, "socket_ipc");

        assert_eq!(decision, VisibleWriteDecision::Apply);
        assert_eq!(event.outcome, FlowOutcome::Completed);
    }

    #[test]
    fn full_content_visible_replacement_blocks_stale_source_buffer() {
        let decision = FullContentVisibleReplacementDecision::RejectStaleSourceBuffer;
        let event = full_content_visible_replacement_event(decision, "compact_exchange");

        assert_eq!(
            decision,
            FullContentVisibleReplacementDecision::RejectStaleSourceBuffer
        );
        assert_eq!(event.flow, FlowName::DocumentMutation);
        assert_eq!(event.stage, FlowStage::PreWriteGuard);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(
            event.reason.as_deref(),
            Some("full_content_source_buffer_reject_stale_source_buffer:compact_exchange")
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullContentScopeRejection {
    TemplateFrontmatter,
    AgentComponentMarkers,
}

impl FullContentScopeRejection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TemplateFrontmatter => "template_frontmatter",
            Self::AgentComponentMarkers => "agent_component_markers",
        }
    }
}

fn frontmatter_mode_is_explicit_template(mode: &str) -> bool {
    matches!(
        mode.trim().to_ascii_lowercase().as_str(),
        "template" | "stream"
    )
}

fn content_declares_template_frontmatter(content: &str) -> bool {
    agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .is_some_and(|(fm, _)| {
            fm.format == Some(agent_doc_frontmatter::frontmatter::AgentDocFormat::Template)
                || fm
                    .mode
                    .as_deref()
                    .is_some_and(frontmatter_mode_is_explicit_template)
        })
}

fn content_has_agent_components(content: &str) -> bool {
    agent_doc_element::element::parse(content)
        .ok()
        .is_some_and(|components| !components.is_empty())
}

pub fn full_content_scope_rejection_reason(
    contents: &[Option<&str>],
) -> Option<FullContentScopeRejection> {
    for content in contents.iter().flatten() {
        if content_declares_template_frontmatter(content) {
            return Some(FullContentScopeRejection::TemplateFrontmatter);
        }
        if content_has_agent_components(content) {
            return Some(FullContentScopeRejection::AgentComponentMarkers);
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WholeBufferDelivery {
    FullContentEditorIpc,
    VisibleWriteDiskWriteThrough,
    EditorRepairRedelivery,
}

impl WholeBufferDelivery {
    const fn requires_source_buffer_match(self) -> bool {
        matches!(
            self,
            Self::FullContentEditorIpc
                | Self::VisibleWriteDiskWriteThrough
                | Self::EditorRepairRedelivery
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullContentEditorIpc => "full_content_editor_ipc",
            Self::VisibleWriteDiskWriteThrough => "visible_write_disk_write_through",
            Self::EditorRepairRedelivery => "editor_repair_redelivery",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WholeBufferAuthority {
    OperatorTextAuthority,
    FileRead,
    ContentOurs,
    None,
}

impl WholeBufferAuthority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OperatorTextAuthority => "operator_text_authority",
            Self::FileRead => "file_read",
            Self::ContentOurs => "content_ours",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WholeBufferDeliveryAction {
    Apply,
    ObserveOnly,
    Reject,
}

impl WholeBufferDeliveryAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::ObserveOnly => "observe_only",
            Self::Reject => "reject",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WholeBufferAuthorityFacts {
    pub delivery: WholeBufferDelivery,
    pub authority: WholeBufferAuthority,
    pub source_buffer_matches: bool,
    pub scope_rejection: Option<FullContentScopeRejection>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WholeBufferAuthorityDecision {
    pub action: WholeBufferDeliveryAction,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WholeBufferAuthorityRule {
    delivery: WholeBufferDelivery,
    authority: WholeBufferAuthority,
    action: WholeBufferDeliveryAction,
    reason: &'static str,
}

const WHOLE_BUFFER_AUTHORITY_TABLE: &[WholeBufferAuthorityRule] = &[
    WholeBufferAuthorityRule {
        delivery: WholeBufferDelivery::FullContentEditorIpc,
        authority: WholeBufferAuthority::OperatorTextAuthority,
        action: WholeBufferDeliveryAction::Apply,
        reason: "operator_text_authority_source_buffer",
    },
    WholeBufferAuthorityRule {
        delivery: WholeBufferDelivery::VisibleWriteDiskWriteThrough,
        authority: WholeBufferAuthority::OperatorTextAuthority,
        action: WholeBufferDeliveryAction::Apply,
        reason: "operator_text_authority",
    },
    WholeBufferAuthorityRule {
        delivery: WholeBufferDelivery::EditorRepairRedelivery,
        authority: WholeBufferAuthority::OperatorTextAuthority,
        action: WholeBufferDeliveryAction::Apply,
        reason: "operator_text_authority_source_buffer",
    },
];

pub fn decide_whole_buffer_delivery(
    facts: WholeBufferAuthorityFacts,
) -> WholeBufferAuthorityDecision {
    if let Some(scope_rejection) = facts.scope_rejection {
        return WholeBufferAuthorityDecision {
            action: WholeBufferDeliveryAction::Reject,
            reason: scope_rejection.as_str(),
        };
    }

    if facts.delivery.requires_source_buffer_match() && !facts.source_buffer_matches {
        return WholeBufferAuthorityDecision {
            action: WholeBufferDeliveryAction::Reject,
            reason: "stale_source_buffer",
        };
    }

    if facts.delivery == WholeBufferDelivery::FullContentEditorIpc && !facts.enabled {
        return WholeBufferAuthorityDecision {
            action: WholeBufferDeliveryAction::ObserveOnly,
            reason: "disabled_by_default",
        };
    }

    WHOLE_BUFFER_AUTHORITY_TABLE
        .iter()
        .find(|rule| rule.delivery == facts.delivery && rule.authority == facts.authority)
        .map(|rule| WholeBufferAuthorityDecision {
            action: rule.action,
            reason: rule.reason,
        })
        .unwrap_or(WholeBufferAuthorityDecision {
            action: WholeBufferDeliveryAction::Reject,
            reason: "missing_operator_text_authority",
        })
}

/// Decision for reconciling an editor buffer against disk when the plugin
/// reconnects its IPC listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectBufferDecision {
    /// Buffer already matches disk; nothing to do.
    InSync,
    /// Buffer equals a prior commit of the file and disk is clean HEAD; re-read
    /// disk into the buffer.
    RereadDisk,
    /// Buffer diverges from disk but is not a known prior commit; keep it.
    KeepBuffer,
}

impl ReconnectBufferDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InSync => "in_sync",
            Self::RereadDisk => "reread_disk",
            Self::KeepBuffer => "keep_buffer",
        }
    }
}

/// Re-read disk only when the buffer is provably stale committed content.
pub fn decide_reconnect_buffer(
    buffer_matches_disk: bool,
    disk_is_committed_head: bool,
    buffer_matches_prior_commit: bool,
) -> ReconnectBufferDecision {
    if buffer_matches_disk {
        return ReconnectBufferDecision::InSync;
    }
    if disk_is_committed_head && buffer_matches_prior_commit {
        return ReconnectBufferDecision::RereadDisk;
    }
    ReconnectBufferDecision::KeepBuffer
}

/// Decision for a finalize/converge write when an editor-IPC socket may be
/// absent, controller-hosted, or backed by a live editor endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorlessDiskFallbackDecision {
    /// A live editor endpoint is present or possible and delivery is unproven.
    FailClosed,
    /// No editor endpoint owns the document; a guarded direct disk write is
    /// allowed.
    DetachedDisk,
    /// Explicit operator force routes to controller-hosted disk write.
    ForceDiskNoEditor,
    /// A live editor endpoint is present and reachable; converge through it.
    ConvergeViaEditor,
}

impl EditorlessDiskFallbackDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FailClosed => "fail_closed",
            Self::DetachedDisk => "detached_disk",
            Self::ForceDiskNoEditor => "force_disk_no_editor",
            Self::ConvergeViaEditor => "converge_via_editor",
        }
    }
}

pub fn decide_editorless_disk_fallback(
    socket_connectable: bool,
    editor_endpoint_proven: bool,
    consecutive_no_ack: usize,
    threshold: usize,
    force_disk_requested: bool,
) -> EditorlessDiskFallbackDecision {
    if force_disk_requested {
        return EditorlessDiskFallbackDecision::ForceDiskNoEditor;
    }
    if editor_endpoint_proven {
        return if consecutive_no_ack >= threshold && threshold > 0 {
            EditorlessDiskFallbackDecision::FailClosed
        } else {
            EditorlessDiskFallbackDecision::ConvergeViaEditor
        };
    }
    if !socket_connectable || (threshold > 0 && consecutive_no_ack >= threshold) {
        return EditorlessDiskFallbackDecision::DetachedDisk;
    }
    EditorlessDiskFallbackDecision::FailClosed
}

const AGENT_RESPONSE_COMPONENT: &str = "exchange";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckMismatchRecovery {
    RevertUntrustedAckToCurrent,
    ReplayMissingAgentResponseToTarget,
}

fn blank_components_named(doc: &str, names: &[&str]) -> Option<String> {
    let comps = agent_doc_element::element::parse(doc).ok()?;
    let mut spans: Vec<(usize, usize)> = comps
        .iter()
        .filter(|c| names.contains(&c.name.as_str()))
        .map(|c| (c.open_end, c.close_start))
        .collect();
    spans.sort_by_key(|(start, _)| *start);
    let mut out = doc.to_string();
    for (start, end) in spans.into_iter().rev() {
        if start <= end
            && end <= out.len()
            && out.is_char_boundary(start)
            && out.is_char_boundary(end)
        {
            out.replace_range(start..end, "");
        }
    }
    Some(out)
}

fn missing_agent_response_block<'a>(target_body: &'a str, recovered_body: &str) -> Option<&'a str> {
    if target_body.len() <= recovered_body.len() {
        return None;
    }
    let missing = if let Some(missing) = target_body.strip_prefix(recovered_body) {
        missing
    } else if let Some(missing) = target_body.strip_suffix(recovered_body) {
        missing
    } else {
        let start = target_body.find(recovered_body)?;
        let end = start + recovered_body.len();
        let before = &target_body[..start];
        let after = &target_body[end..];
        if before.trim().is_empty() {
            after
        } else if after.trim().is_empty() {
            before
        } else {
            return None;
        }
    };
    let trimmed = missing.trim_start();
    if trimmed.starts_with("### Re:") || trimmed.contains("\n### Re:") {
        Some(missing)
    } else {
        None
    }
}

fn stale_queue_prompt_exchange_artifact(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('>') || trimmed == "❯ >"
}

pub fn classify_socket_receipt_mismatch_recovery(
    target: &str,
    recovered: &str,
    normalize_transient_markers: impl Fn(&str) -> String,
) -> Option<AckMismatchRecovery> {
    let (Some(target_without_exchange), Some(recovered_without_exchange)) = (
        blank_components_named(target, &[AGENT_RESPONSE_COMPONENT]),
        blank_components_named(recovered, &[AGENT_RESPONSE_COMPONENT]),
    ) else {
        return None;
    };
    if normalize_transient_markers(&target_without_exchange)
        != normalize_transient_markers(&recovered_without_exchange)
    {
        return None;
    }

    let (Ok(target_comps), Ok(recovered_comps)) = (
        agent_doc_element::element::parse(target),
        agent_doc_element::element::parse(recovered),
    ) else {
        return None;
    };
    let target_exchange = target_comps
        .iter()
        .find(|c| c.name == AGENT_RESPONSE_COMPONENT);
    let recovered_exchange = recovered_comps
        .iter()
        .find(|c| c.name == AGENT_RESPONSE_COMPONENT);
    let (Some(target_exchange), Some(recovered_exchange)) = (target_exchange, recovered_exchange)
    else {
        return None;
    };
    let target_body = normalize_transient_markers(target_exchange.content(target));
    let recovered_body = normalize_transient_markers(recovered_exchange.content(recovered));
    if target_body == recovered_body {
        return None;
    }
    if recovered_body.len() < target_body.len()
        && missing_agent_response_block(&target_body, &recovered_body).is_some()
    {
        return Some(AckMismatchRecovery::ReplayMissingAgentResponseToTarget);
    }
    let target_lines: HashSet<&str> = target_body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let recovered_lines: HashSet<&str> = recovered_body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if !target_lines
        .iter()
        .all(|line| recovered_lines.contains(line))
    {
        return None;
    }
    let recovered_only: Vec<&str> = recovered_lines.difference(&target_lines).copied().collect();
    if !recovered_only.is_empty()
        && recovered_only
            .iter()
            .all(|line| stale_queue_prompt_exchange_artifact(line))
        && recovered_only
            .iter()
            .any(|line| line.trim().starts_with("> **Queue prompt:**"))
    {
        return Some(AckMismatchRecovery::RevertUntrustedAckToCurrent);
    }
    None
}

pub fn exchange_change_is_complete_response_block_trim(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return false;
    }
    let blocks = exchange_response_block_ranges(snapshot);
    if blocks.is_empty() {
        return false;
    }

    let mut snapshot_pos = 0usize;
    let mut current_pos = 0usize;
    let mut removed = 0usize;
    for block in blocks {
        let prefix = &snapshot[snapshot_pos..block.start];
        if !current[current_pos..].starts_with(prefix) {
            return false;
        }
        current_pos += prefix.len();

        let block_text = &snapshot[block.clone()];
        if current[current_pos..].starts_with(block_text) {
            current_pos += block_text.len();
        } else {
            removed += 1;
        }
        snapshot_pos = block.end;
    }

    removed > 0 && current[current_pos..] == snapshot[snapshot_pos..]
}

pub fn exchange_change_is_safe_historical_reduction(snapshot: &str, current: &str) -> bool {
    exchange_change_is_complete_response_block_trim(snapshot, current)
        || exchange_change_is_compact_summary_replacement(snapshot, current)
}

pub fn exchange_change_is_compact_summary_replacement(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return false;
    }
    let current_trimmed = current.trim_start();
    if !current_trimmed.starts_with("### Session Summary") {
        return false;
    }
    if !current.contains("*Compacted. Content archived to `")
        && !current.contains("Compacted content:")
    {
        return false;
    }

    let snapshot_headings = exchange_response_heading_lines(snapshot);
    if snapshot_headings.is_empty() {
        return false;
    }
    let current_headings = exchange_response_heading_lines(current);
    if current_headings
        .iter()
        .any(|heading| !snapshot_headings.contains(heading))
    {
        return false;
    }

    current_headings.len() < snapshot_headings.len()
}

pub fn exchange_response_heading_lines(exchange: &str) -> Vec<String> {
    exchange
        .lines()
        .filter(|line| is_exchange_response_heading(line))
        .map(|line| line.trim().to_string())
        .collect()
}

pub fn exchange_response_block_ranges(exchange: &str) -> Vec<std::ops::Range<usize>> {
    #[derive(Clone, Copy)]
    struct Line<'a> {
        start: usize,
        end: usize,
        text: &'a str,
    }

    let mut lines = Vec::new();
    let mut offset = 0usize;
    for line in exchange.split_inclusive('\n') {
        let end = offset + line.len();
        lines.push(Line {
            start: offset,
            end,
            text: line,
        });
        offset = end;
    }
    if offset < exchange.len() {
        lines.push(Line {
            start: offset,
            end: exchange.len(),
            text: &exchange[offset..],
        });
    }

    let heading_indices: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| is_exchange_response_heading(line.text).then_some(idx))
        .collect();
    let mut ranges = Vec::new();
    for (pos, &heading_idx) in heading_indices.iter().enumerate() {
        let mut end_idx = heading_indices.get(pos + 1).copied().unwrap_or(lines.len());
        for (idx, line) in lines.iter().enumerate().take(end_idx).skip(heading_idx + 1) {
            if is_exchange_boundary(line.text) {
                end_idx = idx;
                break;
            }
        }
        ranges.push(lines[heading_idx].start..lines[end_idx - 1].end);
    }
    ranges
}

fn is_exchange_response_heading(line: &str) -> bool {
    line.trim_start().starts_with("### Re:")
}

fn is_exchange_boundary(line: &str) -> bool {
    line.trim_start().starts_with("<!-- agent:boundary:")
}

/// `#exch-intermix`: realtime resolver for the `live_prompt_drift_after_preflight`
/// closeout wedge. After the IPC drift guard carries the agent response in the
/// snapshot candidate, the visible document may still be missing that response
/// while carrying newer operator-visible edits. Recovery must rebase only the
/// missing response block onto the current document; it must not adopt the
/// snapshot as a whole-document authority.
///
/// This returns true only when the current realtime document can preserve the
/// operator-visible state and accept the missing agent response as a delta. It
/// never authorizes wholesale snapshot adoption: queue/backlog/frontmatter and
/// other disjoint operator edits stay as they are in `file_content`, while only
/// the newest missing `### Re:` block from `snapshot` may be appended to
/// `agent:exchange`. Prompt-target edits inside the visible file still fail
/// closed because the resolver cannot prove where the response should land
/// relative to a newly typed prompt.
pub fn live_prompt_drift_auto_recovery_safe(
    snapshot: &str,
    file_content: &str,
    normalize_visible_recovery_compare: impl Fn(&str) -> String + Copy,
) -> bool {
    live_prompt_drift_recovery_target(snapshot, file_content, normalize_visible_recovery_compare)
        .is_some()
}

pub fn normalize_visible_recovery_compare(content: &str) -> String {
    agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
        &strip_boundary_for_dedup(content),
    )
}

pub fn live_prompt_drift_recovery_target(
    snapshot: &str,
    file_content: &str,
    normalize_visible_recovery_compare: impl Fn(&str) -> String + Copy,
) -> Option<String> {
    // A newly typed prompt inside `agent:exchange` makes response placement
    // ambiguous. Queue/backlog prompt text is disjoint operator state and is
    // preserved by the merged target below.
    if exchange_has_disk_only_prompt_target(snapshot, file_content) {
        return None;
    }

    let response_block = latest_missing_snapshot_response_block(
        snapshot,
        file_content,
        normalize_visible_recovery_compare,
    )?;
    let components = agent_doc_element::element::parse(file_content).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == AGENT_RESPONSE_COMPONENT)?;
    let mut exchange_body = exchange.content(file_content).to_string();
    agent_doc_template::response_materialization::push_materialization_segment(
        &mut exchange_body,
        &response_block,
    );
    let recovered = exchange.replace_content(file_content, &exchange_body);
    (normalize_visible_recovery_compare(&recovered)
        != normalize_visible_recovery_compare(file_content))
    .then_some(recovered)
}

fn exchange_has_disk_only_prompt_target(snapshot: &str, file_content: &str) -> bool {
    let (Ok(snapshot_components), Ok(file_components)) = (
        agent_doc_element::element::parse(snapshot),
        agent_doc_element::element::parse(file_content),
    ) else {
        return true;
    };
    let (Some(snapshot_exchange), Some(file_exchange)) = (
        snapshot_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
        file_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
    ) else {
        return true;
    };
    let snapshot_counts = exchange_prompt_target_counts(snapshot_exchange.content(snapshot));
    let mut seen: HashMap<String, usize> = HashMap::new();
    for prompt in exchange_prompt_target_lines(file_exchange.content(file_content)) {
        let count = seen.entry(prompt.clone()).or_insert(0);
        *count += 1;
        if *count > snapshot_counts.get(&prompt).copied().unwrap_or(0) {
            return true;
        }
    }
    false
}

fn exchange_prompt_target_counts(exchange_body: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for prompt in exchange_prompt_target_lines(exchange_body) {
        *counts.entry(prompt).or_insert(0) += 1;
    }
    counts
}

fn exchange_prompt_target_lines(exchange_body: &str) -> Vec<String> {
    exchange_body
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('❯') || text_line_looks_like_prompt_target(trimmed) {
                Some(
                    trimmed
                        .strip_prefix('❯')
                        .unwrap_or(trimmed)
                        .trim()
                        .to_string(),
                )
            } else {
                None
            }
        })
        .collect()
}

fn latest_missing_snapshot_response_block(
    snapshot: &str,
    file_content: &str,
    normalize_visible_recovery_compare: impl Fn(&str) -> String + Copy,
) -> Option<String> {
    let (Ok(snapshot_components), Ok(file_components)) = (
        agent_doc_element::element::parse(snapshot),
        agent_doc_element::element::parse(file_content),
    ) else {
        return None;
    };
    let (Some(snapshot_exchange), Some(file_exchange)) = (
        snapshot_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
        file_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
    ) else {
        return None;
    };
    let snapshot_body = snapshot_exchange.content(snapshot);
    let file_body = file_exchange.content(file_content);
    let file_norm = normalize_visible_recovery_compare(file_body);
    for range in exchange_response_block_ranges(snapshot_body)
        .into_iter()
        .rev()
    {
        let block = &snapshot_body[range];
        let block_norm = normalize_visible_recovery_compare(block);
        let block_trimmed = block_norm.trim();
        if block_trimmed.is_empty() {
            continue;
        }
        if !file_norm.contains(block_trimmed) {
            return Some(block.to_string());
        }
    }
    None
}

/// `#exch-intermix-falsedrop`: true when a recorded dropped prompt is still
/// present in the response candidate - as an active line, a
/// struck/consumed queue item (`~~...~~`), or echoed in a `### Re:` heading - so
/// response recovery loses nothing. The drift-time dropped-prompt record
/// compares the divergent IPC candidate against `content_ours` and therefore
/// false-positives on prompts that `content_ours` consumed or preserved; this
/// containment check reconciles those against the response candidate text.
/// Returns false only when the prompt text genuinely does not appear in the
/// candidate (real user-content loss -> fail closed). Strike markers are
/// stripped from both sides so a consumed item still matches its recorded prompt
/// text.
pub fn snapshot_contains_dropped_prompt(snapshot: &str, prompt: &str) -> bool {
    let stripped = prompt.replace("~~", "");
    let needle = stripped.trim();
    if needle.is_empty() {
        return true;
    }
    snapshot.replace("~~", "").contains(needle)
}

fn is_safe_out_of_band_exchange_growth(snapshot_content: &str, file_content: &str) -> bool {
    if !file_content.starts_with(snapshot_content) {
        return false;
    }
    let suffix = file_content[snapshot_content.len()..].trim();
    !suffix.is_empty() && suffix.starts_with("### Re:")
}

fn is_safe_exchange_user_prompt_insert(snapshot_exchange: &str, file_exchange: &str) -> bool {
    let snap_lines: Vec<&str> = snapshot_exchange.lines().collect();
    let file_lines: Vec<&str> = file_exchange.lines().collect();

    if snap_lines.len() >= file_lines.len() {
        return false;
    }

    let prefix_len = snap_lines
        .iter()
        .zip(file_lines.iter())
        .take_while(|(s, f)| s.trim() == f.trim())
        .count();

    let suffix_len = snap_lines
        .iter()
        .rev()
        .zip(file_lines.iter().rev())
        .take_while(|(s, f)| s.trim() == f.trim())
        .count();

    if suffix_len == 0 {
        return false;
    }

    let suffix_start_in_snap = snap_lines.len().saturating_sub(suffix_len);
    let suffix_has_response = snap_lines[suffix_start_in_snap..]
        .iter()
        .any(|line| line.trim().starts_with("### Re:"));

    if !suffix_has_response {
        return false;
    }

    let insert_start = prefix_len;
    let insert_end = file_lines.len().saturating_sub(suffix_len);

    if insert_start >= insert_end {
        return false;
    }

    let inserted_lines = &file_lines[insert_start..insert_end];

    for line in inserted_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("### Re:")
            || trimmed.starts_with("#### Re:")
            || trimmed.starts_with("<!-- agent:")
            || trimmed.starts_with("<!-- /agent:")
        {
            return false;
        }
    }

    true
}

fn flush_exchange_insert_block(block: &mut String) -> bool {
    let trimmed = block.trim();
    if trimmed.is_empty() {
        block.clear();
        return true;
    }
    let ok = is_safe_historical_exchange_insert_block(trimmed);
    block.clear();
    ok
}

fn is_safe_historical_exchange_insert_block(block: &str) -> bool {
    let non_blank: Vec<&str> = block
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if non_blank.is_empty() {
        return true;
    }

    let Some(first_response_idx) = non_blank.iter().position(|line| {
        line.starts_with("### Re:") || line.starts_with("#### Re:") || line.starts_with("##### Re:")
    }) else {
        return false;
    };
    if first_response_idx == 0 {
        return true;
    }

    non_blank[..first_response_idx]
        .iter()
        .all(|line| historical_exchange_prelude_looks_like_prompt_target(line))
}

fn historical_exchange_prelude_looks_like_prompt_target(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with("<!--")
        && !trimmed.starts_with("```")
        && !trimmed.starts_with("~~~")
        && !trimmed.starts_with("### Re:")
        && !trimmed.starts_with("#### Re:")
        && !trimmed.starts_with("##### Re:")
        && (trimmed.starts_with('❯')
            || trimmed.ends_with('?')
            || historical_exchange_prelude_looks_like_imperative(trimmed))
}

fn historical_exchange_prelude_looks_like_imperative(line: &str) -> bool {
    let compact = line.trim_start_matches('>').trim().to_ascii_lowercase();
    compact == "go"
        || compact == "continue"
        || compact.starts_with("do #")
        || compact.starts_with("run ")
        || compact.starts_with("rerun ")
        || compact.starts_with("build ")
        || compact.starts_with("test ")
        || compact.starts_with("commit ")
        || compact.starts_with("push ")
        || compact.starts_with("fix ")
        || compact.starts_with("complete ")
}

fn is_safe_historical_exchange_growth(snapshot_content: &str, file_content: &str) -> bool {
    let diff = similar::TextDiff::from_lines(snapshot_content, file_content);
    let mut insert_block = String::new();
    let mut saw_insert = false;

    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Equal => {
                if !flush_exchange_insert_block(&mut insert_block) {
                    return false;
                }
            }
            similar::ChangeTag::Delete => return false,
            similar::ChangeTag::Insert => {
                saw_insert = true;
                insert_block.push_str(change.value());
            }
        }
    }

    saw_insert && flush_exchange_insert_block(&mut insert_block)
}

pub fn is_safe_user_follow_up_exchange_growth(head_content: &str, current_content: &str) -> bool {
    if head_content == current_content || !current_content.starts_with(head_content) {
        return false;
    }

    let suffix = current_content[head_content.len()..].trim();
    !suffix.is_empty()
        && suffix != "## Assistant"
        && !suffix.starts_with("### Re:")
        && !suffix.starts_with("#### Re:")
}

fn is_safe_out_of_band_pending_mutation(snapshot_content: &str, file_content: &str) -> bool {
    let (snap_prelude, snap_items, snap_postlude) =
        agent_doc_element_backlog::backlog::parse_items(snapshot_content);
    let (file_prelude, file_items, file_postlude) =
        agent_doc_element_backlog::backlog::parse_items(file_content);

    if snap_prelude.trim() != file_prelude.trim() || snap_postlude.trim() != file_postlude.trim() {
        return false;
    }
    if file_items.is_empty() {
        return false;
    }

    let file_ids: HashSet<&str> = file_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .map(|item| item.id.as_str())
        .collect();
    if file_ids.is_empty() {
        return false;
    }

    snap_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .all(|item| file_ids.contains(item.id.as_str()))
}

pub fn detect_reintroduced_reaped_pending_ids(
    doc: &str,
    reaped_ids: &HashSet<String>,
) -> Result<Vec<String>> {
    if reaped_ids.is_empty() {
        return Ok(Vec::new());
    }

    let components = agent_doc_element::element::parse(doc)?;
    let mut seen = HashSet::new();
    let mut reintroduced = Vec::new();
    for component in components
        .iter()
        .filter(|component| agent_doc_element::element::is_tracked_work_component(&component.name))
    {
        let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(component.content(doc));
        for item in items {
            if !item.id.is_empty() && reaped_ids.contains(&item.id) && seen.insert(item.id.clone())
            {
                reintroduced.push(item.id);
            }
        }
    }

    reintroduced.sort();
    Ok(reintroduced)
}

fn strip_promptish_list_prefix(line: &str) -> &str {
    let mut trimmed = line.trim();

    if let Some(rest) = trimmed.strip_prefix('❯') {
        trimmed = rest.trim_start();
    }

    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return rest.trim_start();
    }

    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return rest.trim_start();
        }
    }

    trimmed
}

fn starts_with_prompt_preset_reference(line: &str) -> bool {
    let trimmed = strip_promptish_list_prefix(line);
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    let token_len = rest
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .map(|(idx, ch)| idx + ch.len_utf8())
        .last()
        .unwrap_or(0);
    if token_len == 0 {
        return false;
    }
    let remainder = &rest[token_len..];
    remainder.is_empty()
        || remainder
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace())
}

fn status_mutation_introduces_prompt_work(snapshot_content: &str, file_content: &str) -> bool {
    let diff = similar::TextDiff::from_lines(snapshot_content, file_content);
    let mut added = String::new();

    for change in diff.iter_all_changes() {
        if change.tag() == similar::ChangeTag::Insert {
            added.push_str(change.value());
        }
    }

    if added.trim().is_empty() {
        return false;
    }

    if !agent_doc_diff::extract_prompt_preset_requests_from_text(&added).is_empty() {
        return true;
    }

    added.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && (text_line_looks_like_prompt_target(trimmed)
                || starts_with_prompt_preset_reference(trimmed))
    })
}

fn is_safe_out_of_band_status_mutation(snapshot_content: &str, file_content: &str) -> bool {
    snapshot_content.trim() != file_content.trim()
        && !status_mutation_introduces_prompt_work(snapshot_content, file_content)
}

pub fn is_empty_template_scaffold_snapshot(snapshot_doc: &str) -> bool {
    let body = agent_doc_frontmatter::frontmatter::parse(snapshot_doc)
        .map(|(_, body)| body)
        .unwrap_or(snapshot_doc);
    let Ok(components) = agent_doc_element::element::parse(body) else {
        return false;
    };

    let has_status = components.iter().any(|c| c.name == "status");
    let has_exchange = components.iter().any(|c| c.name == "exchange");
    let has_pending = components
        .iter()
        .any(|c| agent_doc_element::element::is_backlog_component(&c.name));
    if !(has_status && has_exchange && has_pending) {
        return false;
    }

    components.iter().all(|component| {
        (matches!(component.name.as_str(), "status" | "exchange" | "queue")
            || agent_doc_element::element::is_backlog_component(&component.name))
            && normalize_component_content_for_absorb(component.content(body)).is_empty()
    })
}

fn classify_safe_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
    allow_historical_exchange_growth: bool,
) -> Option<&'static str> {
    if snapshot_doc == file_doc {
        return None;
    }

    let snap_body = agent_doc_frontmatter::frontmatter::parse(snapshot_doc)
        .map(|(_, body)| body)
        .unwrap_or(snapshot_doc);
    let file_body = agent_doc_frontmatter::frontmatter::parse(file_doc)
        .map(|(_, body)| body)
        .unwrap_or(file_doc);

    if redact_component_contents_for_absorb(snap_body)?
        != redact_component_contents_for_absorb(file_body)?
    {
        return None;
    }

    let snap_components = agent_doc_element::element::parse(snap_body).ok()?;
    let file_components = agent_doc_element::element::parse(file_body).ok()?;
    if snap_components.len() != file_components.len() {
        return None;
    }

    let mut saw_exchange = false;
    let mut saw_pending = false;
    let mut saw_status = false;

    for (snap_comp, file_comp) in snap_components.iter().zip(file_components.iter()) {
        if snap_comp.name != file_comp.name {
            return None;
        }
        if !agent_doc_element::element::is_backlog_component(&snap_comp.name)
            && snap_comp.patch_mode() != file_comp.patch_mode()
        {
            return None;
        }

        let snap_content = normalize_component_content_for_absorb(snap_comp.content(snap_body));
        let file_content = normalize_component_content_for_absorb(file_comp.content(file_body));
        if snap_content == file_content {
            continue;
        }

        match snap_comp.name.as_str() {
            "exchange" => {
                let safe_exchange =
                    is_safe_out_of_band_exchange_growth(&snap_content, &file_content)
                        || (allow_historical_exchange_growth
                            && is_safe_historical_exchange_growth(&snap_content, &file_content))
                        || is_safe_exchange_user_prompt_insert(&snap_content, &file_content);
                if !safe_exchange {
                    return None;
                }
                saw_exchange = true;
            }
            name if agent_doc_element::element::is_backlog_component(name) => {
                if !is_safe_out_of_band_pending_mutation(&snap_content, &file_content) {
                    return None;
                }
                saw_pending = true;
            }
            "status" => {
                if !is_safe_out_of_band_status_mutation(&snap_content, &file_content) {
                    return None;
                }
                saw_status = true;
            }
            _ => return None,
        }
    }

    match (saw_status, saw_exchange, saw_pending) {
        (true, true, true) => Some("status+exchange+pending"),
        (true, true, false) => Some("status+exchange"),
        (true, false, true) => Some("status+pending"),
        (true, false, false) => Some("status"),
        (false, true, true) => Some("exchange+pending"),
        (false, true, false) => Some("exchange"),
        (false, false, true) => Some("pending"),
        (false, false, false) => None,
    }
}

pub fn classify_safe_out_of_band_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
) -> Option<&'static str> {
    classify_safe_agent_doc_mutation(snapshot_doc, file_doc, false)
}

pub fn classify_committed_historical_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
) -> Option<&'static str> {
    classify_safe_agent_doc_mutation(snapshot_doc, file_doc, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_disk_fallback_refusal_tracks_live_editor_authority() {
        assert!(
            !should_refuse_disk_fallback(false, false),
            "no reliable-sync authority + dead socket must allow the disk fallback"
        );
        assert!(
            should_refuse_disk_fallback(true, true),
            "durably live editor + answering socket must fail closed"
        );
        assert!(
            !should_refuse_disk_fallback(false, true),
            "answering socket without reliable-sync authority must allow the disk fallback"
        );
    }

    #[test]
    fn stale_attached_editor_on_dead_socket_demotes_instead_of_wedging() {
        // #staleattachdemote: a retained open fact with a dead socket cannot
        // receive or re-save the write, so refusing disk here would wedge the
        // document forever.
        assert!(
            !should_refuse_disk_fallback(true, false),
            "durably live editor + dead socket must demote and allow the disk fallback"
        );
        // The same authority with a live, answering transport still protects the
        // editor buffer — demotion is keyed off the dead transport, not off the
        // authority signal itself.
        assert!(
            should_refuse_disk_fallback(true, true),
            "durably live editor + answering socket must still fail closed"
        );
    }

    fn identity_normalize(text: &str) -> String {
        text.to_string()
    }

    fn drift_baseline() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    fn drift_content_ours() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "### Re: do #fix — opus-4-8\n\n",
            "Implemented the fix and verified it end to end. The response body is long\n",
            "enough to make the missing-response wedge shape unambiguous for the\n",
            "recovery discriminator under test here.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    fn doc_with_exchange(exchange: &str, queue: &str) -> String {
        format!(
            "# Plan\n\n<!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn normalize_patch_content_empty_prefix_lines_passthrough() {
        let patch_content = "some line\nanother line\n";

        assert_eq!(normalize_patch_content(patch_content, &[]), patch_content);
    }

    #[test]
    fn normalize_patch_content_counts_duplicate_targets() {
        let patch_content = "repeat target\nrepeat target\nrepeat target\n";
        let prefix_lines = vec!["repeat target".to_string(), "repeat target".to_string()];

        assert_eq!(
            normalize_patch_content(patch_content, &prefix_lines),
            "❯ repeat target\n❯ repeat target\nrepeat target\n"
        );
    }

    #[test]
    fn normalize_patch_content_keeps_already_prefixed_lines_idempotent() {
        let patch_content = "❯ already prefixed\nnot prefixed\n";
        let prefix_lines = vec!["already prefixed".to_string(), "not prefixed".to_string()];

        assert_eq!(
            normalize_patch_content(patch_content, &prefix_lines),
            "❯ already prefixed\n❯ not prefixed\n"
        );
    }

    #[test]
    fn normalize_patch_content_preserves_plain_response_lines_after_prompt() {
        let patch_content = "Verification:\nuser directive\n";
        let prefix_lines = vec!["Verification:".to_string(), "user directive".to_string()];

        assert_eq!(
            normalize_patch_content(patch_content, &prefix_lines),
            "Verification:\n❯ user directive\n"
        );
    }

    #[test]
    fn normalize_patch_content_preserves_missing_trailing_newline() {
        let patch_content = "target line";
        let prefix_lines = vec!["target line".to_string()];

        assert_eq!(
            normalize_patch_content(patch_content, &prefix_lines),
            "❯ target line"
        );
    }

    #[test]
    fn response_already_in_current_detects_plugin_applied() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
User added this line.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        assert!(
            response_already_in_current(base, content_ours, content_current),
            "should detect plugin-applied response"
        );
    }

    #[test]
    fn response_already_in_current_rejects_partial_line_overlap() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — gpt-5
Done.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
Done.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "a shared response body line is not proof that the response delta was applied"
        );
    }

    #[test]
    fn response_already_in_current_accepts_normalized_delta_with_bare_prompt() {
        let base = "\
<!-- agent:exchange patch=append -->
do #ipcd
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #ipcd
### Re: #ipcd — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
do #ipcd
while typing next prompt
### Re: #ipcd — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        assert!(
            response_already_in_current(base, content_ours, content_current),
            "the response hunk should be detected even when prompt-prefix normalization differs"
        );
    }

    #[test]
    fn response_already_in_current_false_when_not_applied() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
User typed something new.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "should not detect when plugin hasn't applied"
        );
    }

    #[test]
    fn response_already_in_current_false_when_no_exchange() {
        let base = "No components here.";
        let content_ours = "No components here either.";
        let content_current = "Still no components.";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "should return false when no exchange components"
        );
    }

    #[test]
    fn response_already_in_current_false_when_no_changes() {
        let base = "\
<!-- agent:exchange patch=append -->
Same content.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, base, base),
            "should return false when ours equals base"
        );
    }

    #[test]
    fn new_agent_response_headings_returns_candidate_exchange_headings_missing_from_base() {
        let base = doc_with_exchange(
            "❯ do #old\n### Re: do #old - gpt-5\n\nOld response.\n",
            "### Re: queue text is not exchange\n",
        );
        let candidate = doc_with_exchange(
            concat!(
                "❯ do #old\n",
                "### Re: do #old - gpt-5\n\n",
                "Old response.\n",
                "  ### Re: do #new - gpt-5\n\n",
                "New response.\n",
                "### Re: do #second - gpt-5\n\n",
                "Second response.\n",
            ),
            "### Re: queue text is not exchange\n",
        );

        assert_eq!(
            new_agent_response_headings(&base, &candidate),
            vec![
                "### Re: do #new - gpt-5".to_string(),
                "### Re: do #second - gpt-5".to_string(),
            ]
        );
    }

    #[test]
    fn visible_write_contains_latest_response_uses_latest_exchange_block() {
        let target = doc_with_exchange(
            concat!(
                "❯ do #old\n",
                "### Re: do #old - gpt-5\n\n",
                "Old response.\n",
                "### Re: do #latest - gpt-5\n\n",
                "Latest response.\n",
                "<!-- agent:boundary: next -->\n",
                "❯ do #next\n",
            ),
            "",
        );
        let ack_with_latest =
            doc_with_exchange("### Re: do #latest - gpt-5\n\nLatest response.\n", "");
        let ack_with_old = doc_with_exchange("### Re: do #old - gpt-5\n\nOld response.\n", "");

        assert!(visible_write_contains_latest_response(
            &ack_with_latest,
            &target
        ));
        assert!(!visible_write_contains_latest_response(
            &ack_with_old,
            &target
        ));
    }

    #[test]
    fn visible_write_contains_latest_response_allows_targets_without_response_heading() {
        let target = doc_with_exchange("❯ do #pending\nstill typing\n", "");

        assert!(visible_write_contains_latest_response("", &target));
    }

    #[test]
    fn visible_write_contains_latest_response_falls_back_to_whole_content_without_exchange() {
        let target = "### Re: loose response - gpt-5\n\nLoose response body.\n";

        assert!(visible_write_contains_latest_response(target, target));
        assert!(!visible_write_contains_latest_response(
            "### Re: other response - gpt-5\n\nOther body.\n",
            target,
        ));
    }

    #[test]
    fn response_converged_true_when_operator_edited_body_but_kept_heading() {
        // #adoc-live-prompt-drift-operator-edit: the operator edited the response
        // body before commit. The heading is present with a body, so the target is
        // converged — no editor repair required (must NOT wedge), even though the
        // agent's exact bytes are gone.
        let base = doc_with_exchange("❯ fold in the line\n", "");
        let candidate = doc_with_exchange(
            "❯ fold in the line\n### Re: folded draft — opus\n\nAgent's exact draft body.\n",
            "",
        );
        let operator_edited = doc_with_exchange(
            "❯ fold in the line\n### Re: folded draft — opus\n\nOperator's reworded body.\n",
            "",
        );

        assert!(
            response_converged_in_visible_target(&base, &candidate, &operator_edited),
            "operator body-edit with heading + body present is converged"
        );
        // The strict byte check is what used to wedge here.
        assert!(!visible_write_contains_latest_response(
            &candidate,
            &operator_edited
        ));
    }

    #[test]
    fn response_converged_false_when_heading_present_but_body_emptied() {
        let base = doc_with_exchange("❯ do #x\n", "");
        let candidate = doc_with_exchange("❯ do #x\n### Re: do #x — opus\n\nAgent body.\n", "");
        // Operator emptied the body (kept only the heading) → not converged, the
        // fail-closed repair path must still run.
        let emptied = doc_with_exchange("❯ do #x\n### Re: do #x — opus\n\n", "");

        assert!(!response_converged_in_visible_target(
            &base, &candidate, &emptied
        ));
    }

    #[test]
    fn response_converged_false_when_heading_missing() {
        let base = doc_with_exchange("❯ do #x\n", "");
        let candidate = doc_with_exchange("❯ do #x\n### Re: do #x — opus\n\nAgent body.\n", "");
        // Response never landed in the target (editor still shows only the prompt).
        let no_response = doc_with_exchange("❯ do #x\n", "");

        assert!(!response_converged_in_visible_target(
            &base,
            &candidate,
            &no_response
        ));
    }

    #[test]
    fn response_converged_falls_back_to_exact_check_without_new_heading() {
        // No NEW response heading this cycle → preserve prior exact-materialization
        // semantics.
        let base = doc_with_exchange("### Re: pre — opus\n\nOld.\n", "");
        let candidate = doc_with_exchange("### Re: pre — opus\n\nOld.\n", "");
        let target = doc_with_exchange("### Re: pre — opus\n\nOld.\n", "");

        assert!(response_converged_in_visible_target(
            &base, &candidate, &target
        ));
    }

    #[test]
    fn buffer_presents_reference_response_true_for_newer_operator_edit() {
        // Reference = proven visible-write content (carries the response). Buffer = a NEWER
        // operator buffer whose response body was edited. It still presents the
        // response heading with a body, so the stale ack can reconcile forward.
        let reference = doc_with_exchange("❯ prompt\n### Re: topic — opus\n\nAck body.\n", "");
        let newer = doc_with_exchange(
            "❯ prompt\n### Re: topic — opus\n\nOperator edited body, plus a happy-4th line.\n",
            "",
        );

        assert!(buffer_presents_reference_response(&reference, &newer));
    }

    #[test]
    fn buffer_presents_reference_response_false_when_response_dropped_from_newer_buffer() {
        let reference = doc_with_exchange("❯ prompt\n### Re: topic — opus\n\nAck body.\n", "");
        // Operator (or a bad merge) dropped the response entirely in the newer
        // buffer → must NOT reconcile forward (fail closed).
        let newer = doc_with_exchange("❯ prompt\n", "");

        assert!(!buffer_presents_reference_response(&reference, &newer));
    }

    #[test]
    fn operator_reconcile_step_continues_while_buffer_still_changing() {
        let reference = doc_with_exchange("❯ p\n### Re: t — opus\n\nBody.\n", "");
        let prev = doc_with_exchange("❯ p\n### Re: t — opus\n\nBod.\n", "");
        let curr = doc_with_exchange("❯ p\n### Re: t — opus\n\nBody edited.\n", "");
        assert_eq!(
            operator_reconcile_step(&reference, Some(&prev), &curr),
            OperatorReconcileStep::Continue,
            "a changed buffer means the operator is still editing"
        );
        // First read (no prev) is never stable.
        assert_eq!(
            operator_reconcile_step(&reference, None, &curr),
            OperatorReconcileStep::Continue
        );
    }

    #[test]
    fn operator_reconcile_step_accepts_stable_buffer_presenting_response() {
        let reference = doc_with_exchange("❯ p\n### Re: t — opus\n\nAgent body.\n", "");
        let settled = doc_with_exchange("❯ p\n### Re: t — opus\n\nOperator body, 4th.\n", "");
        assert_eq!(
            operator_reconcile_step(&reference, Some(&settled), &settled),
            OperatorReconcileStep::Accept(settled.clone()),
            "a stable buffer still presenting the response is adopted"
        );
    }

    #[test]
    fn operator_reconcile_step_fails_closed_when_stable_buffer_dropped_response() {
        let reference = doc_with_exchange("❯ p\n### Re: t — opus\n\nAgent body.\n", "");
        let dropped = doc_with_exchange("❯ p\n", "");
        assert_eq!(
            operator_reconcile_step(&reference, Some(&dropped), &dropped),
            OperatorReconcileStep::FailClosed,
            "a stable buffer that dropped the response must not be adopted"
        );
    }

    #[test]
    fn first_response_heading_returns_trimmed_first_re_heading() {
        let response = "preface\n  ### Re: do #fix - gpt-5\n\nBody.\n### Re: later\n";

        assert_eq!(
            first_response_heading(response).as_deref(),
            Some("### Re: do #fix - gpt-5")
        );
        assert_eq!(first_response_heading("no response heading"), None);
    }

    #[test]
    fn normalization_repair_candidate_matches_prefix_only_boundary_equivalent_repair() {
        let bad_state = "\
<!-- agent:exchange patch=append -->
do #norm
<!-- agent:boundary: old -->
### Re: do #norm — gpt-5

Working.
<!-- /agent:exchange -->
";
        let repaired = "\
<!-- agent:exchange patch=append -->
❯ do #norm
<!-- agent:boundary: new -->
### Re: do #norm — gpt-5

Working.
<!-- /agent:exchange -->
";
        let targets = vec!["do #norm".to_string()];

        assert!(normalization_repair_candidate_matches(
            bad_state, repaired, &targets
        ));
        assert!(!normalization_repair_candidate_matches(
            bad_state,
            repaired,
            &[]
        ));
        assert!(!normalization_repair_candidate_matches(
            bad_state,
            &repaired.replace("Working.", "Changed."),
            &targets
        ));
    }

    #[test]
    fn editorless_detached_disk_after_no_delivery_or_no_listener() {
        assert_eq!(
            decide_editorless_disk_fallback(true, false, 3, 3, false),
            EditorlessDiskFallbackDecision::DetachedDisk
        );
        assert_eq!(
            decide_editorless_disk_fallback(false, false, 0, 3, false),
            EditorlessDiskFallbackDecision::DetachedDisk
        );
        assert_eq!(
            decide_editorless_disk_fallback(true, true, 0, 3, true),
            EditorlessDiskFallbackDecision::ForceDiskNoEditor
        );
    }

    #[test]
    fn editorless_fail_closed_protects_live_editor_buffer() {
        assert_eq!(
            decide_editorless_disk_fallback(true, true, 5, 3, false),
            EditorlessDiskFallbackDecision::FailClosed
        );
        assert_eq!(
            decide_editorless_disk_fallback(true, false, 1, 3, false),
            EditorlessDiskFallbackDecision::FailClosed
        );
    }

    #[test]
    fn editorless_converges_via_healthy_editor() {
        assert_eq!(
            decide_editorless_disk_fallback(true, true, 0, 3, false),
            EditorlessDiskFallbackDecision::ConvergeViaEditor
        );
    }

    #[test]
    fn reconnect_buffer_in_sync_when_buffer_matches_disk() {
        assert_eq!(
            decide_reconnect_buffer(true, true, true),
            ReconnectBufferDecision::InSync
        );
        assert_eq!(
            decide_reconnect_buffer(true, false, false),
            ReconnectBufferDecision::InSync
        );
    }

    #[test]
    fn reconnect_buffer_rereads_provably_stale_committed_buffer() {
        assert_eq!(
            decide_reconnect_buffer(false, true, true),
            ReconnectBufferDecision::RereadDisk
        );
    }

    #[test]
    fn reconnect_buffer_keeps_unproven_divergent_buffer() {
        assert_eq!(
            decide_reconnect_buffer(false, true, false),
            ReconnectBufferDecision::KeepBuffer
        );
        assert_eq!(
            decide_reconnect_buffer(false, false, true),
            ReconnectBufferDecision::KeepBuffer
        );
    }

    #[test]
    fn visible_write_guard_defers_when_typing_never_settles() {
        let decision = decide_visible_write_after_typing(VisibleWriteTypingFacts {
            idle_reached: false,
            timeout_ms: 5_000,
        });

        assert_eq!(decision, VisibleWriteDecision::DeferActiveTyping);
    }

    #[test]
    fn visible_write_guard_allows_idle_writes() {
        let decision = decide_visible_write_after_typing(VisibleWriteTypingFacts {
            idle_reached: true,
            timeout_ms: 5_000,
        });

        assert_eq!(decision, VisibleWriteDecision::Apply);
    }

    #[test]
    fn visible_write_commit_candidate_requires_revision_applied_and_matching_hash() {
        let proven = VisibleWriteCommitCandidateFacts {
            commit_candidate_hash: "abc".into(),
            applied_commit_candidate_hash: Some("ABC".into()),
            model_revision: Some(7),
            editor_applied_observed: true,
        };
        assert_eq!(
            visible_write_commit_candidate_state(&proven),
            VisibleWriteCommitCandidateState::Proven
        );
        assert_eq!(
            decide_visible_write_commit_candidate(&proven),
            VisibleWriteCommitCandidateDecision::Proven
        );

        for facts in [
            VisibleWriteCommitCandidateFacts {
                commit_candidate_hash: "abc".into(),
                applied_commit_candidate_hash: None,
                model_revision: Some(7),
                editor_applied_observed: true,
            },
            VisibleWriteCommitCandidateFacts {
                commit_candidate_hash: "abc".into(),
                applied_commit_candidate_hash: Some("abc".into()),
                model_revision: Some(0),
                editor_applied_observed: true,
            },
            VisibleWriteCommitCandidateFacts {
                commit_candidate_hash: "abc".into(),
                applied_commit_candidate_hash: Some("def".into()),
                model_revision: Some(7),
                editor_applied_observed: true,
            },
            VisibleWriteCommitCandidateFacts {
                commit_candidate_hash: "abc".into(),
                applied_commit_candidate_hash: Some("abc".into()),
                model_revision: Some(7),
                editor_applied_observed: false,
            },
        ] {
            assert_eq!(
                decide_visible_write_commit_candidate(&facts),
                VisibleWriteCommitCandidateDecision::MissingProof
            );
        }
    }

    #[test]
    fn visible_write_commit_candidate_chart_requires_ordered_proof_facts() {
        assert_eq!(
            VisibleWriteCommitCandidateMachine::transition(
                VisibleWriteCommitCandidateState::AwaitingProof,
                VisibleWriteCommitCandidateEvent::CommitCandidateHashMatched,
            ),
            None
        );
        assert_eq!(
            VisibleWriteCommitCandidateMachine::transition(
                VisibleWriteCommitCandidateState::AwaitingProof,
                VisibleWriteCommitCandidateEvent::ModelRevisionSeen,
            ),
            Some(VisibleWriteCommitCandidateState::ModelRevisionObserved)
        );
        assert_eq!(
            VisibleWriteCommitCandidateMachine::transition(
                VisibleWriteCommitCandidateState::ModelRevisionObserved,
                VisibleWriteCommitCandidateEvent::EditorAppliedSeen,
            ),
            Some(VisibleWriteCommitCandidateState::ModelRevisionAndAppliedObserved)
        );
        assert_eq!(
            VisibleWriteCommitCandidateMachine::transition(
                VisibleWriteCommitCandidateState::ModelRevisionAndAppliedObserved,
                VisibleWriteCommitCandidateEvent::CommitCandidateHashMatched,
            ),
            Some(VisibleWriteCommitCandidateState::Proven)
        );
    }

    #[test]
    fn visible_write_materialized_carry_forward_requires_revision_and_matching_hashes() {
        let proven = VisibleWriteMaterializedCarryForwardFacts {
            commit_candidate_hash: "candidate".into(),
            proven_commit_candidate_hash: Some("CANDIDATE".into()),
            file_content_hash: "file".into(),
            proven_file_content_hash: Some("FILE".into()),
            live_buffer_hash: "live".into(),
            proven_live_buffer_hash: Some("LIVE".into()),
            model_revision: Some(3),
        };
        assert_eq!(
            visible_write_materialized_carry_forward_state(&proven),
            VisibleWriteMaterializedCarryForwardState::Proven
        );
        assert_eq!(
            decide_visible_write_materialized_carry_forward(&proven),
            VisibleWriteMaterializedCarryForwardDecision::Proven
        );

        for facts in [
            VisibleWriteMaterializedCarryForwardFacts {
                model_revision: Some(0),
                ..proven.clone()
            },
            VisibleWriteMaterializedCarryForwardFacts {
                proven_commit_candidate_hash: Some("other".into()),
                ..proven.clone()
            },
            VisibleWriteMaterializedCarryForwardFacts {
                proven_file_content_hash: Some("other".into()),
                ..proven.clone()
            },
            VisibleWriteMaterializedCarryForwardFacts {
                proven_live_buffer_hash: Some("other".into()),
                ..proven.clone()
            },
        ] {
            assert_eq!(
                decide_visible_write_materialized_carry_forward(&facts),
                VisibleWriteMaterializedCarryForwardDecision::MissingProof
            );
        }
    }

    #[test]
    fn reconcile_visible_write_remerges_foreign_append_then_lands_clean() {
        let base = "BASE";
        let foreign = "BASE+FOREIGN";
        let guard_calls = std::cell::RefCell::new(0usize);
        let recompute_calls = std::cell::RefCell::new(0usize);

        let guard =
            |_context: &&str, expected: &str, _payload: &String| -> Result<VisibleWriteReconcile> {
                let mut n = guard_calls.borrow_mut();
                *n += 1;
                if *n == 1 {
                    assert_eq!(expected, base);
                    Ok(VisibleWriteReconcile::DiskDrifted {
                        fresh_current: foreign.to_string(),
                    })
                } else {
                    assert_eq!(expected, foreign);
                    Ok(VisibleWriteReconcile::Clean)
                }
            };
        let recompute = |current: &str| -> Result<String> {
            *recompute_calls.borrow_mut() += 1;
            Ok(format!("{current}+RESPONSE"))
        };
        let fail_closed = |_context: &&str, _current: &str, _payload: &String| -> Result<()> {
            panic!("must not fail closed on reconcilable disk drift");
        };

        let (current, payload) = reconcile_visible_write(
            &"doc",
            base.to_string(),
            format!("{base}+RESPONSE"),
            3,
            guard,
            recompute,
            fail_closed,
        )
        .unwrap();

        assert_eq!(current, foreign);
        assert_eq!(payload, "BASE+FOREIGN+RESPONSE");
        assert_eq!(*guard_calls.borrow(), 2);
        assert_eq!(*recompute_calls.borrow(), 1);
    }

    #[test]
    fn reconcile_visible_write_falls_back_to_fail_closed_when_drift_never_settles() {
        let counter = std::cell::RefCell::new(0usize);
        let guard = |_context: &&str,
                     _expected: &str,
                     _payload: &String|
         -> Result<VisibleWriteReconcile> {
            let mut n = counter.borrow_mut();
            *n += 1;
            Ok(VisibleWriteReconcile::DiskDrifted {
                fresh_current: format!("drift-{n}"),
            })
        };
        let recompute = |current: &str| -> Result<String> { Ok(current.to_string()) };
        let fail_closed = |_context: &&str, _current: &str, _payload: &String| -> Result<()> {
            anyhow::bail!("document still changing");
        };

        let err = reconcile_visible_write(
            &"doc",
            "start".to_string(),
            "start".to_string(),
            3,
            guard,
            recompute,
            fail_closed,
        )
        .unwrap_err();

        assert!(err.to_string().contains("document still changing"));
        assert_eq!(*counter.borrow(), 3);
    }

    #[test]
    fn full_content_source_proof_matches_original_buffer_only() {
        let proof = FullContentSourceProof::from_content("before");
        let utf8_proof = FullContentSourceProof::from_content("before ❯");

        assert!(proof.matches_current("before"));
        assert!(!proof.matches_current("before\nlive prompt"));
        assert!(!proof.matches_current("beforE"));
        assert_eq!(utf8_proof.expected_content_len, "before ❯".len());
        assert!(utf8_proof.expected_content_len > "before ❯".chars().count());
    }

    #[test]
    fn full_content_visible_replacement_blocks_stale_source_buffer() {
        let proof = FullContentSourceProof::from_content("before");
        let decision = decide_full_content_visible_replacement("before\nlive prompt", Some(&proof));

        assert_eq!(
            decision,
            FullContentVisibleReplacementDecision::RejectStaleSourceBuffer
        );
    }

    #[test]
    fn full_content_scope_rejects_template_frontmatter() {
        let template_format = "---\nagent_doc_format: template\n---\nplain\n";
        let stream_mode = "---\nagent_doc_mode: stream\n---\nplain\n";

        assert_eq!(
            full_content_scope_rejection_reason(&[Some(template_format)]),
            Some(FullContentScopeRejection::TemplateFrontmatter)
        );
        assert_eq!(
            full_content_scope_rejection_reason(&[Some(stream_mode)]),
            Some(FullContentScopeRejection::TemplateFrontmatter)
        );
    }

    #[test]
    fn full_content_scope_rejects_agent_component_markers() {
        let target = "plain\n";
        let source = "<!-- agent:exchange -->\nbody\n<!-- /agent:exchange -->\n";

        assert_eq!(
            full_content_scope_rejection_reason(&[Some(target), Some(source), None]),
            Some(FullContentScopeRejection::AgentComponentMarkers)
        );
    }

    #[test]
    fn full_content_scope_allows_plain_documents() {
        assert_eq!(
            full_content_scope_rejection_reason(&[Some("plain\n"), None, Some("other\n")]),
            None
        );
    }

    #[test]
    fn whole_buffer_table_observes_disabled_full_content() {
        let decision = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
            delivery: WholeBufferDelivery::FullContentEditorIpc,
            authority: WholeBufferAuthority::OperatorTextAuthority,
            source_buffer_matches: true,
            scope_rejection: None,
            enabled: false,
        });

        assert_eq!(decision.action, WholeBufferDeliveryAction::ObserveOnly);
        assert_eq!(decision.reason, "disabled_by_default");
    }

    #[test]
    fn whole_buffer_table_rejects_stale_source_before_authority() {
        let decision = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
            delivery: WholeBufferDelivery::FullContentEditorIpc,
            authority: WholeBufferAuthority::OperatorTextAuthority,
            source_buffer_matches: false,
            scope_rejection: None,
            enabled: false,
        });

        assert_eq!(decision.action, WholeBufferDeliveryAction::Reject);
        assert_eq!(decision.reason, "stale_source_buffer");
    }

    #[test]
    fn whole_buffer_table_allows_visible_write_through_only_with_operator_authority() {
        let allowed = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
            delivery: WholeBufferDelivery::VisibleWriteDiskWriteThrough,
            authority: WholeBufferAuthority::OperatorTextAuthority,
            source_buffer_matches: true,
            scope_rejection: None,
            enabled: true,
        });
        let blocked = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
            delivery: WholeBufferDelivery::VisibleWriteDiskWriteThrough,
            authority: WholeBufferAuthority::None,
            source_buffer_matches: true,
            scope_rejection: None,
            enabled: true,
        });

        assert_eq!(allowed.action, WholeBufferDeliveryAction::Apply);
        assert_eq!(blocked.action, WholeBufferDeliveryAction::Reject);
        assert_eq!(blocked.reason, "missing_operator_text_authority");
    }

    #[test]
    fn whole_buffer_table_rejects_visible_write_through_stale_source() {
        let decision = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
            delivery: WholeBufferDelivery::VisibleWriteDiskWriteThrough,
            authority: WholeBufferAuthority::OperatorTextAuthority,
            source_buffer_matches: false,
            scope_rejection: None,
            enabled: true,
        });

        assert_eq!(decision.action, WholeBufferDeliveryAction::Reject);
        assert_eq!(decision.reason, "stale_source_buffer");
    }

    #[test]
    fn ack_mismatch_classifies_stale_queue_prompt_artifact_as_revert() {
        let exchange = "### Re: do [#head]\n\nAnswered from the agent.\n";
        let target = doc_with_exchange(exchange, "- do [#head]\n");
        let recovered = doc_with_exchange(
            &format!("{exchange}> **Queue prompt:** stale leftover from failed queue consume\n"),
            "- do [#head]\n",
        );

        assert_eq!(
            classify_socket_receipt_mismatch_recovery(&target, &recovered, identity_normalize),
            Some(AckMismatchRecovery::RevertUntrustedAckToCurrent)
        );
    }

    #[test]
    fn ack_mismatch_classifies_missing_agent_response_as_target_replay() {
        let target = doc_with_exchange(
            "❯ do [#head]\n\n### Re: do [#head]\n\nAnswered from the agent.\n",
            "- do [#head]\n",
        );
        let recovered = doc_with_exchange("❯ do [#head]\n", "- do [#head]\n");

        assert_eq!(
            classify_socket_receipt_mismatch_recovery(&target, &recovered, identity_normalize),
            Some(AckMismatchRecovery::ReplayMissingAgentResponseToTarget)
        );
    }

    #[test]
    fn ack_mismatch_rejects_user_prompt_drift() {
        let exchange = "### Re: do [#head]\n\nAnswered from the agent.\n";
        let target = doc_with_exchange(exchange, "- do [#head]\n");
        let recovered =
            doc_with_exchange(&format!("{exchange}❯ do [#followup]\n"), "- do [#head]\n");

        assert_eq!(
            classify_socket_receipt_mismatch_recovery(&target, &recovered, identity_normalize),
            None
        );
    }

    #[test]
    fn exchange_change_is_safe_historical_reduction_accepts_response_block_trim() {
        let snapshot = concat!(
            "❯ do [#old]\n",
            "### Re: do [#old]\n\nOld response body.\n",
            "❯ do [#new]\n",
            "### Re: do [#new]\n\nNew response body.\n",
        );
        let current = concat!(
            "❯ do [#old]\n",
            "### Re: do [#old]\n\nOld response body.\n",
            "❯ do [#new]\n",
        );

        assert!(exchange_change_is_safe_historical_reduction(
            snapshot, current
        ));
    }

    #[test]
    fn exchange_change_is_safe_historical_reduction_accepts_compact_summary_replacement() {
        let snapshot = concat!(
            "### Re: archived 0 - gpt-5\n\nArchived response body.\n",
            "### Re: archived 1 - gpt-5\n\nArchived response body.\n",
        );
        let current = concat!(
            "### Session Summary\n\n",
            "*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\n",
            "Compacted content:\n",
            "- Archived 2 response topic(s): archived 0; archived 1\n",
        );

        assert!(exchange_change_is_safe_historical_reduction(
            snapshot, current
        ));
    }

    #[test]
    fn exchange_change_is_safe_historical_reduction_rejects_unproven_rewrite() {
        let snapshot = "### Re: archived 0 - gpt-5\n\nArchived response body.\n";
        let current =
            "### Session Summary\n\nOperator-authored replacement without compact archive proof.\n";

        assert!(!exchange_change_is_safe_historical_reduction(
            snapshot, current
        ));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_accepts_benign_wedge() {
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline();

        assert!(live_prompt_drift_auto_recovery_safe(
            &snapshot,
            &fragmented,
            identity_normalize
        ));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_no_wedge() {
        let snapshot = drift_content_ours();

        assert!(!live_prompt_drift_auto_recovery_safe(
            &snapshot,
            &snapshot,
            identity_normalize
        ));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_disk_only_exchange_prompt() {
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline().replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\n❯ do #brand-new-user-prompt-typed-after-preflight\n<!-- /agent:exchange -->",
        );

        assert!(!live_prompt_drift_auto_recovery_safe(
            &snapshot,
            &fragmented,
            identity_normalize
        ));
    }

    #[test]
    fn visible_recovery_compare_normalizes_boundary_and_transient_markers() {
        let left = "<!-- agent:boundary:HEAD -->\n### Re: do #fix\nbody\n";
        let right = "<!-- agent:boundary:OURS -->\n### Re: do #fix\nbody\n";

        assert_eq!(
            normalize_visible_recovery_compare(left),
            normalize_visible_recovery_compare(right)
        );
    }

    #[test]
    fn visible_recovery_compare_preserves_response_text_difference() {
        let left = "<!-- agent:boundary:HEAD -->\n### Re: do #fix\nbody\n";
        let right = "<!-- agent:boundary:HEAD -->\n### Re: do #fix\nchanged\n";

        assert_ne!(
            normalize_visible_recovery_compare(left),
            normalize_visible_recovery_compare(right)
        );
    }

    #[test]
    fn live_prompt_drift_recovery_target_preserves_disk_only_queue_item() {
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline().replace(
            "- do [#fix]\n<!-- /agent:queue -->",
            "- do [#fix]\n- do [#user-added-queue-item]\n<!-- /agent:queue -->",
        );

        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented, identity_normalize)
            .expect("queue edits should be preserved while the response lands");

        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- do [#user-added-queue-item]"));
    }

    #[test]
    fn live_prompt_drift_recovery_target_preserves_partial_exchange_word() {
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline().replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\noperator-partial-wo\n<!-- /agent:exchange -->",
        );

        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented, identity_normalize)
            .expect("partial exchange text should be preserved while the response lands");

        assert!(target.contains("### Re: do #fix"));
        assert!(
            target.contains("operator-partial-wo"),
            "operator-typed partial word must survive recovery:\n{target}"
        );
    }

    #[test]
    fn live_prompt_drift_recovery_target_preserves_operator_edited_backlog_text() {
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- original backlog wording\n<!-- /agent:backlog -->\n",
            drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- edited backlog wording\n<!-- /agent:backlog -->\n",
            drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented, identity_normalize)
            .expect("backlog edits should be preserved while the response lands");

        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- edited backlog wording"));
        assert!(!target.contains("- original backlog wording"));
    }

    #[test]
    fn snapshot_contains_dropped_prompt_matches_consumed_and_active() {
        let snapshot = concat!(
            "<!-- agent:queue go -->\n",
            "- ~~do [#consumed]~~\n",
            "- do [#active]\n",
            "<!-- /agent:queue -->\n",
        );

        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#consumed]"));
        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#active]"));
        assert!(!snapshot_contains_dropped_prompt(snapshot, "do [#gone]"));
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_exchange_and_pending() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#c3d4] new pending\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            Some("exchange+pending")
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_user_prompt_append() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_status_and_exchange() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Older status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Newer status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            Some("status+exchange")
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_status_prompt_preset_reference() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Compacted.\n\
            <!-- /agent:status -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            #next-steps\n\
            <!-- /agent:status -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_status_prompt_preset_reference_with_guidance()
     {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Compacted.\n\
            <!-- /agent:status -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            #next-steps for calibrating session benchmarks with expected scores\n\
            <!-- /agent:status -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn is_safe_historical_exchange_growth_allows_prompt_target_before_response() {
        let snapshot = "### Re: older\nold body\n";
        let head = "### Re: older\nold body\n\ndo #7mqc. spec-test-news-commit-push\n### Re: do `#7mqc` - codex\nCompleted.\n";

        assert!(is_safe_historical_exchange_insert_block(
            "do #7mqc. spec-test-news-commit-push\n### Re: do `#7mqc` - codex\nCompleted."
        ));
        assert!(is_safe_historical_exchange_growth(snapshot, head));
    }

    #[test]
    fn classify_safe_committed_historical_agent_doc_mutation_exchange() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: historical\n\
            repaired body\n\
            #### #next-steps\n\
            Follow up.\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_committed_historical_agent_doc_mutation(snapshot, file),
            Some("exchange")
        );
        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn safe_exchange_user_prompt_insert_basic() {
        let snapshot = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        let file = "### Re: prev - model\nprev response\nUSER PROMPT\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_rejects_after_response() {
        let snapshot = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        let file = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response\nEXTRA TEXT";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_rejects_deletions() {
        let snapshot = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        let file =
            "### Re: prev - model\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_rejects_agent_markers() {
        let snapshot = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        let file = "### Re: prev - model\nprev response\n### Re: injected - model\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_no_boundary() {
        let snapshot = "### Re: new - model\nnew response";
        let file = "USER PROMPT\n### Re: new - model\nnew response";
        assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_identical() {
        let snapshot = "### Re: prev - model\nprev response\n### Re: new - model\nnew response";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, snapshot));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_multiline_prompts() {
        let snapshot = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        let file = "### Re: prev - model\nprev response\nline one\nline two\nline three\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_classify_integration() {
        let snapshot_doc = "---\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: prev - model\nprev response\n\
            <!-- agent:boundary:abc -->\n\
            ### Re: new - model\nnew response\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] item\n\
            <!-- /agent:backlog -->\n";

        let file_doc = "---\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: prev - model\nprev response\n\
            USER PROMPT\n\
            <!-- agent:boundary:abc -->\n\
            ### Re: new - model\nnew response\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] item\n\
            <!-- /agent:backlog -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot_doc, file_doc),
            Some("exchange")
        );
    }

    #[test]
    fn dropped_prompt_lines_after_content_ours_captures_unowned_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let candidate = baseline.replace(
            "<!-- agent:boundary:b0 -->",
            "go\n<!-- agent:boundary:b0 -->",
        );

        let dropped = dropped_prompt_lines_after_content_ours(baseline, &candidate, baseline);
        assert_eq!(dropped, vec!["go".to_string()]);
    }

    #[test]
    fn dropped_prompt_lines_after_content_ours_empty_when_content_ours_owns_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let with_go = baseline.replace(
            "<!-- agent:boundary:b0 -->",
            "go\n<!-- agent:boundary:b0 -->",
        );

        let dropped = dropped_prompt_lines_after_content_ours(baseline, &with_go, &with_go);
        assert!(dropped.is_empty());
    }

    #[test]
    fn content_ours_drops_operator_text_true_when_candidate_has_unowned_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        // final buffer carries a concurrent operator prompt; content_ours does not.
        let candidate = baseline.replace(
            "<!-- agent:boundary:b0 -->",
            "❯ concurrent operator prompt\n<!-- agent:boundary:b0 -->",
        );
        assert!(content_ours_drops_operator_text(
            baseline, &candidate, baseline
        ));
    }

    #[test]
    fn committed_snapshot_excludes_carry_forward_prompt_keeps_operator_edit() {
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        // Agent applied a new response; NO new operator prompt in the agent candidate.
        let content_ours = base.replace(
            "\nAnswered.\n",
            "\nAnswered EDITED BY OPERATOR.\n### Re: now — gpt-5\n\nFresh answer.\n",
        );
        // Operator concurrently edited existing text AND typed a new prompt.
        let content_current = base.replace(
            "\nAnswered.\n<!-- agent:boundary:b0 -->",
            "\nAnswered EDITED BY OPERATOR.\n❯ carry me forward\n<!-- agent:boundary:b0 -->",
        );
        // The union already written to disk = agent response + operator edit + prompt.
        let final_content = base.replace(
            "\nAnswered.\n<!-- agent:boundary:b0 -->",
            "\nAnswered EDITED BY OPERATOR.\n### Re: now — gpt-5\n\nFresh answer.\n❯ carry me forward\n<!-- agent:boundary:b0 -->",
        );

        let snapshot = committed_snapshot_union_excluding_carry_forward(
            base,
            &content_ours,
            &content_current,
            &final_content,
        );
        // Operator edit to existing text is retained (no #qftlossdelta loss).
        assert!(
            snapshot.contains("Answered EDITED BY OPERATOR."),
            "operator edit must be committed, not dropped: {snapshot}"
        );
        // Agent response retained.
        assert!(
            snapshot.contains("Fresh answer."),
            "agent response retained"
        );
        // Carry-forward prompt excluded from the committed snapshot (stays uncommitted).
        assert!(
            !snapshot.contains("carry me forward"),
            "carry-forward prompt must NOT be committed (it carries forward): {snapshot}"
        );
    }

    #[test]
    fn committed_snapshot_union_is_verbatim_final_without_carry_forward() {
        let base = "<!-- agent:exchange -->\n### Re: a\nDone.\n<!-- /agent:exchange -->\n";
        let content_ours = base;
        let content_current = base;
        let final_content = "<!-- agent:exchange -->\n### Re: a\nDone.\n### Re: b\nMore.\n<!-- /agent:exchange -->\n";
        // No concurrently-typed prompt → snapshot is the union verbatim.
        assert_eq!(
            committed_snapshot_union_excluding_carry_forward(
                base,
                content_ours,
                content_current,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn content_ours_drops_operator_text_false_when_owned_or_unchanged() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let with_prompt = baseline.replace(
            "<!-- agent:boundary:b0 -->",
            "❯ concurrent operator prompt\n<!-- agent:boundary:b0 -->",
        );
        // content_ours already carries the same operator prompt → nothing dropped.
        assert!(!content_ours_drops_operator_text(
            baseline,
            &with_prompt,
            &with_prompt
        ));
        // candidate identical to baseline → no operator drift at all.
        assert!(!content_ours_drops_operator_text(
            baseline, baseline, baseline
        ));
    }

    #[test]
    fn explicit_baseline_preserves_concurrent_user_edits_for_next_cycle() {
        let baseline = Some("baseline");
        let content_ours =
            "<!-- agent:exchange -->\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: answer\nDone.\n❯ late follow-up\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(baseline, content_ours, final_content),
            SnapshotPersistMode::ContentOurs
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(baseline, content_ours, final_content),
                content_ours,
                final_content
            ),
            content_ours
        );
    }

    #[test]
    fn explicit_baseline_forward_merges_concurrent_comment_tail_into_this_cycle() {
        let baseline = Some("baseline");
        let base = "<!-- agent:exchange -->\n❯ prompt\n<!-- /agent:exchange -->\n###\n\n<!--\nold note\n-->\n";
        let content_current = "<!-- agent:exchange -->\n❯ prompt\n<!-- /agent:exchange -->\n###\n\n<!--\nedited note\n-->\n";
        let content_ours = "<!-- agent:exchange -->\n❯ prompt\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n###\n\n<!--\nold note\n-->\n";
        let final_content = "<!-- agent:exchange -->\n❯ prompt\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n###\n\n<!--\nedited note\n-->\n";

        assert_eq!(
            snapshot_persist_mode_with_current(
                baseline,
                base,
                content_current,
                content_ours,
                final_content
            ),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode_with_current(
                    baseline,
                    base,
                    content_current,
                    content_ours,
                    final_content
                ),
                content_ours,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn committed_snapshot_union_restores_new_post_exchange_gap_from_content_ours() {
        let base = concat!(
            "<!-- agent:exchange -->\n",
            "❯ prompt\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let content_current = base.replace(
            "<!-- agent:backlog -->",
            "###\n\n<!--\nnew scratch note\n-->\n\n<!-- agent:backlog -->",
        );
        let content_ours = base.replace("❯ prompt\n", "❯ prompt\n### Re: answer\nDone.\n");
        let final_content = content_ours.replace(
            "<!-- agent:backlog -->",
            "###\n\n<!--\nnew scratch note\n-->\n\n<!-- agent:backlog -->",
        );

        assert_eq!(
            snapshot_persist_mode_with_current(
                Some(base),
                base,
                &content_current,
                &content_ours,
                &final_content,
            ),
            SnapshotPersistMode::ContentOurs
        );

        let snapshot = committed_snapshot_union_excluding_carry_forward(
            base,
            &content_ours,
            &content_current,
            &final_content,
        );

        assert!(
            snapshot.contains("### Re: answer"),
            "agent response should still be committed: {snapshot}"
        );
        assert!(
            !snapshot.contains("new scratch note"),
            "new post-exchange scratch comments should carry forward: {snapshot}"
        );
    }

    #[test]
    fn committed_snapshot_union_restores_prompt_edit_in_existing_post_exchange_comment() {
        let base = concat!(
            "<!-- agent:exchange -->\n",
            "❯ prompt\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let live_prompt = "Does the operator need a follow-up email?";
        let content_current = base.replace("<!--\n-->", &format!("<!--\n{live_prompt}\n-->"));
        let content_ours = base.replace("❯ prompt\n", "❯ prompt\n### Re: answer\nDone.\n");
        let final_content = content_ours.replace("<!--\n-->", &format!("<!--\n{live_prompt}\n-->"));

        assert_eq!(
            snapshot_persist_mode_with_current(
                Some(base),
                base,
                &content_current,
                &content_ours,
                &final_content,
            ),
            SnapshotPersistMode::ContentOurs
        );

        let snapshot = committed_snapshot_union_excluding_carry_forward(
            base,
            &content_ours,
            &content_current,
            &final_content,
        );

        assert!(
            snapshot.contains("### Re: answer"),
            "agent response should still be committed: {snapshot}"
        );
        assert!(
            !snapshot.contains(live_prompt),
            "prompt text typed into an existing post-exchange comment should carry forward: {snapshot}"
        );
    }

    #[test]
    fn implicit_baseline_still_persists_final_merged_disk_state() {
        let content_ours =
            "<!-- agent:exchange -->\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: answer\nDone.\n❯ late follow-up\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(None, content_ours, final_content),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(None, content_ours, final_content),
                content_ours,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn explicit_baseline_keeps_final_content_when_delta_is_prior_streamed_agent_prefix() {
        let baseline = Some("baseline");
        let content_ours = "<!-- agent:exchange -->\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: orchestrate streaming — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(baseline, content_ours, final_content),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(baseline, content_ours, final_content),
                content_ours,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn response_target_disjoint_from_user_edit_carries_queue_directives_forward() {
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        );
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented.\n<!-- /agent:exchange -->",
        );
        let candidate = ours.replace(
            "- do [#fix]\n<!-- /agent:queue -->",
            "- do [#fix]\n- do [#user-added-directive]\n<!-- /agent:queue -->",
        );

        assert!(!response_target_disjoint_from_user_edit(
            baseline,
            &ours,
            &candidate,
            |_, _, candidate| Some(candidate.to_string())
        ));
    }

    #[test]
    fn response_target_disjoint_from_user_edit_accepts_plain_outside_edit() {
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\nold parked note body\n-->\n",
        )
        .to_string();
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented and verified with a long-enough response body to matter.\n<!-- /agent:exchange -->",
        );
        let candidate = ours.replace("old parked note body", "edited parked note body");

        assert!(response_target_disjoint_from_user_edit(
            &baseline,
            &ours,
            &candidate,
            |_, _, candidate| Some(candidate.to_string())
        ));
    }

    #[test]
    fn response_target_disjoint_from_user_edit_blocks_unproven_queue_deletion() {
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#keep]\n",
            "<!-- /agent:queue -->\n\n",
            "<!--\nold parked note body\n-->\n",
        )
        .to_string();
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented and verified with a long-enough response body to matter.\n<!-- /agent:exchange -->",
        );
        let candidate = ours
            .replace("- do [#keep]\n", "")
            .replace("old parked note body", "edited parked note body");

        assert!(!response_target_disjoint_from_user_edit(
            &baseline,
            &ours,
            &candidate,
            |_, _, candidate| Some(candidate.to_string())
        ));
    }

    #[test]
    fn response_target_disjoint_from_user_edit_blocks_response_rewrite_and_new_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n",
        );
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented the fix and verified it end to end.\n<!-- /agent:exchange -->",
        );
        let rewritten = ours.replace(
            "Implemented the fix and verified it end to end.",
            "User rewrote the committed response body inside the live buffer.",
        );
        let new_prompt = ours.replace(
            "<!-- /agent:exchange -->",
            "❯ a brand new prompt typed during closeout\n<!-- /agent:exchange -->",
        );

        for candidate in [rewritten, new_prompt, ours.clone()] {
            assert!(!response_target_disjoint_from_user_edit(
                baseline,
                &ours,
                &candidate,
                |_, _, candidate| Some(candidate.to_string())
            ));
        }
    }

    #[test]
    fn response_target_disjoint_from_user_edit_requires_clean_merge() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\nold note\n-->\n",
        );
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented.\n<!-- /agent:exchange -->",
        );
        let candidate = ours.replace("old note", "edited note");

        assert!(!response_target_disjoint_from_user_edit(
            baseline,
            &ours,
            &candidate,
            |_, _, _| None
        ));
        assert!(!response_target_disjoint_from_user_edit(
            baseline,
            &ours,
            &candidate,
            |_, _, _| Some("<<<<<<< conflict\n>>>>>>>".to_string())
        ));
    }
}

#[test]
fn editor_delivery_admission_fails_closed_on_authority_mismatch() {
    assert_eq!(
        decide_editor_delivery_admission(EditorDeliveryAdmissionFacts {
            reliable_editor_live: true,
            legacy_endpoint_live: true,
        }),
        EditorDeliveryAdmission::DeliverToLiveEditor,
    );
    assert_eq!(
        decide_editor_delivery_admission(EditorDeliveryAdmissionFacts {
            reliable_editor_live: false,
            legacy_endpoint_live: false,
        }),
        EditorDeliveryAdmission::Detached,
    );
    assert_eq!(
        decide_editor_delivery_admission(EditorDeliveryAdmissionFacts {
            reliable_editor_live: false,
            legacy_endpoint_live: true,
        }),
        EditorDeliveryAdmission::RefuseAuthorityMismatch,
        "a process-live legacy endpoint must not receive recovery payloads after the reliable open-set reports zero live editors",
    );
}
