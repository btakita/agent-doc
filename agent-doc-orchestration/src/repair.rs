//! # Module: repair
//!
//! ## Spec
//! - Guards against response loss caused by context compaction interrupting the write-back phase (between agent respond and `agent-doc write`).
//! - Pending responses are stored in `.agent-doc/pending/<hash>.md` before the write attempt, and
//!   the same response is also captured in `.agent-doc/captures/<doc-hash>/<cycle-id>.json`.
//! - `run(file)` — canonicalizes the path, checks for a pending file or a recoverable durable
//!   capture, and applies it if found. Terminal captures (`committed`, `discarded`) are ignored for
//!   replay so later preflights do not repeatedly enter the dedup path after a successful closeout.
//!   Before applying, reads the current document and checks if the response is already present
//!   (dedup guard). If already present, template docs still run binary-owned transcript/tail
//!   normalization before pending cleanup, then `run(file)` returns `RepairOutcome::AlreadyApplied`.
//!   When replaying from a durable capture, requires the current document and snapshot hashes to
//!   still match the captured baseline; otherwise fails closed.
//!   Template/CRDT patchback responses replay through the normal strict stream
//!   write path so recovery reuses the same response capture, materialization,
//!   queue-consumption, snapshot, and commit closeout as `finalize`.
//!   Other template documents replay through the template repair write path
//!   (`write::apply_template_from_string`) even when the captured response is raw text
//!   without `<!-- patch:... -->` fences (for example `compact exchange` closeouts).
//!   Template replay first passes through `replay_guard`; blocked transcript/full-document
//!   payloads are captured under `.agent-doc/repair-blocked`, and sanitized replayable
//!   payloads such as patch bodies extracted from leading commentary are what get written.
//!   Non-template documents use plain append (`write::apply_append_from_string`).
//!   Removes the pending file on successful write.
//! - Empty pending files are cleaned up without triggering a write; `run` returns `RepairOutcome::Noop`.
//! - `repair(file)` — runs the same recovery logic as `run(file)` and, when recovery work happened
//!   inside a git repo, immediately attempts `git::commit(file)` so the repaired response crosses
//!   the normal commit boundary instead of waiting for a later `preflight`.
//! - When there is no pending response/capture to replay and a stale open
//!   `preflight_started` cycle contains unresolved prompt-bearing drift, `run(file)` abandons
//!   that empty cycle without committing a placeholder response so the next preflight can start
//!   a fresh cycle for the still-visible prompt. Recent empty cycles still fail closed so a
//!   concurrent live preflight is not stolen.
//! - When there is no pending response/capture to replay, `run(file)` also reaps stale completed
//!   backlog items (`- [x] ...`) that should already have been removed, synchronizing the reap
//!   into the snapshot and `agent:done` archive when present.
//! - When there is no pending response/capture to replay, `run(file)` also normalizes safe
//!   template drift such as a stale `agent:boundary` marker left before an already-answered
//!   exchange turn; the repair repositions the boundary to the true end of the completed turn
//!   and advances the snapshot through the same binary-owned path.
//! - `save_pending(file, response)` — writes the response to the pending store, creating parent directories as needed.
//! - `clear_pending(file)` — removes the pending file; no-op if it does not exist.
//! - `normalized_response_lines(response)` — extracts the response's non-empty, non-marker lines for dedup checking and normalizes transient ` (HEAD)` response-heading churn.
//!
//! ## Agentic Contracts
//! - `run(file)` — returns a `RepairOutcome` describing whether nothing happened, the response was replayed, the response was already present, manual tail cleanup was respected, or a stale `preflight_started` lock was repaired. Returns `Err` on I/O failure or if the write-back itself fails.
//! - `repair(file)` — preserves `run(file)` behavior and additionally attempts `git::commit(file)` when the document lives in git and the outcome was not `Noop`.
//! - Pending file is removed only after a fully successful write (or dedup detection); a failed write leaves the pending file intact for retry.
//! - `save_pending` and `clear_pending` are idempotent with respect to directory creation and missing files respectively.
//! - Callers (e.g., `preflight`) invoke `run` at session start to surface any orphaned responses before proceeding.
//!
//! ## Evals
//! - no_pending_returns_false: document with no pending file or capture → run returns Ok(false)
//! - save_and_clear_pending: save then clear → pending file created then removed
//! - recover_append_response: pending plain text response → applied as Assistant section, file updated, pending file removed, run returns Ok(true)
//! - empty_pending_cleaned_up: pending file with only whitespace → run returns Ok(false), pending file removed
//! - recover_skips_duplicate_apply: pending response already present in document → run returns Ok(false), pending file removed, document unchanged
//! - recover_already_applied_template_canonicalizes_prompt_prefixes: template dedup still restores missing `❯ ` transcript prefixes before cleanup
//! - repair_repositions_stale_boundary_after_answered_turn: no pending response, stale boundary left before an answered turn → boundary moved to tail, snapshot advanced
//! - recover_replays_capture_without_pending: durable capture with no pending file → run returns Ok(true)
//! - recover_fails_closed_on_capture_hash_mismatch: durable capture baseline mismatch → run returns Err

use agent_doc_turn::closeout_recovery::CloseoutRecoveryMutationReason;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

use agent_doc_frontmatter::frontmatter;

use crate::{snapshot, write};

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
    pub fn repaired(self) -> bool {
        !matches!(self, Self::Noop)
    }

    pub fn replayed_response(self) -> bool {
        matches!(self, Self::ReplayedResponse)
    }
}

fn capture_is_repairable(capture: &crate::capture::CaptureRecord) -> bool {
    matches!(
        capture.state,
        crate::capture::CaptureState::Captured
            | crate::capture::CaptureState::WriteApplied
            | crate::capture::CaptureState::Replayed
    )
}

pub(crate) fn first_response_heading_line(response: &str) -> Option<&str> {
    response
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("### Re:"))
}

fn normalize_replay_topic(text: &str) -> String {
    let trimmed = text.trim();
    let trimmed = trimmed
        .strip_prefix("❯ ")
        .unwrap_or(trimmed)
        .strip_prefix("### Re:")
        .unwrap_or(trimmed)
        .trim();
    let trimmed = trimmed
        .split_once(" — ")
        .map(|(topic, _)| topic)
        .unwrap_or(trimmed)
        .trim();
    let trimmed = trimmed
        .strip_prefix("do ")
        .unwrap_or(trimmed)
        .trim_start_matches('#')
        .trim();

    let mut normalized = String::new();
    let mut last_was_space = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_space = false;
        } else if !last_was_space {
            normalized.push(' ');
            last_was_space = true;
        }
    }
    normalized.trim().to_string()
}

fn line_matches_historical_prompt(line: &str, topic: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("## Assistant")
        || trimmed.starts_with("<!--")
    {
        return false;
    }
    if !(trimmed.starts_with("❯ ")
        || trimmed.starts_with('#')
        || trimmed.starts_with("do #")
        || trimmed.starts_with("preset #"))
    {
        return false;
    }

    let normalized_line = normalize_replay_topic(trimmed);
    !normalized_line.is_empty()
        && (normalized_line == topic
            || normalized_line.contains(topic)
            || topic.contains(&normalized_line))
}

fn has_matching_orphan_prompt_for_committed_capture(
    doc_content: &str,
    response_heading: &str,
) -> bool {
    let topic = normalize_replay_topic(response_heading);
    if topic.is_empty() {
        return false;
    }

    let body = frontmatter::parse(doc_content)
        .map(|(_, body)| body)
        .unwrap_or(doc_content);
    let exchange = if let Ok(components) = agent_doc_element::element::parse(body) {
        components
            .iter()
            .find(|component| component.name == "exchange")
            .map(|component| component.content(body).to_string())
            .unwrap_or_else(|| body.to_string())
    } else {
        body.to_string()
    };

    let mut saw_match = false;
    for line in exchange.lines() {
        let trimmed = line.trim();
        if trimmed == response_heading.trim() {
            return false;
        }
        if saw_match && trimmed.starts_with("### Re:") {
            return false;
        }
        if line_matches_historical_prompt(trimmed, &topic) {
            saw_match = true;
        }
    }

    saw_match
}

fn historical_committed_capture_replay(
    file: &Path,
    doc_content: &str,
) -> Result<Option<crate::capture::CaptureRecord>> {
    let Some(capture) = crate::capture::latest_committed(file)? else {
        return Ok(None);
    };
    if response_already_applied(doc_content, &capture.response_body) {
        return Ok(None);
    }
    let Some(response_heading) = first_response_heading_line(&capture.response_body) else {
        return Ok(None);
    };
    if !has_matching_orphan_prompt_for_committed_capture(doc_content, response_heading) {
        return Ok(None);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_replay_committed_capture file={} capture_id={}",
            file.display(),
            capture.capture_id
        ),
    );
    Ok(Some(capture))
}

fn wrap_template_exchange_patch(body: &str) -> String {
    let mut patch = String::from("<!-- patch:exchange -->\n");
    patch.push_str(body);
    if !body.ends_with('\n') {
        patch.push('\n');
    }
    patch.push_str("<!-- /patch:exchange -->\n");
    patch
}

fn extract_visible_response_patch_between(
    snapshot_doc: &str,
    current_doc: &str,
    template_mode: bool,
) -> Option<String> {
    let norm =
        |s: &str| agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(s);
    let snapshot_norm = norm(snapshot_doc);
    let current_norm = norm(current_doc);
    if current_norm == snapshot_norm
        || agent_doc_turn::document_drift::detect_bypassed_response_write_between(
            &snapshot_norm,
            &current_norm,
        )
        .is_none()
    {
        return None;
    }

    let diff = similar::TextDiff::from_lines(&snapshot_norm, &current_norm);
    let mut collected = String::new();
    let mut collecting = false;
    for change in diff.iter_all_changes() {
        let line = change.value();
        let trimmed = line.trim_end_matches('\n').trim();
        match change.tag() {
            similar::ChangeTag::Insert => {
                if !collecting
                    && !agent_doc_turn::closeout_signal::is_exchange_response_heading(trimmed)
                {
                    continue;
                }
                collecting = true;
                collected.push_str(line);
            }
            similar::ChangeTag::Equal if collecting => {
                if trimmed.is_empty() {
                    collected.push_str(line);
                    continue;
                }
                if trimmed.starts_with("<!-- agent:boundary:")
                    || trimmed == "<!-- /agent:exchange -->"
                    || trimmed == "<!-- /patch:exchange -->"
                    || agent_doc_diff::text_line_looks_like_prompt_target(trimmed)
                    || agent_doc_turn::closeout_signal::is_exchange_response_heading(trimmed)
                {
                    break;
                }
                break;
            }
            _ => {}
        }
    }

    if collected.trim().is_empty() {
        return None;
    }

    Some(if template_mode {
        wrap_template_exchange_patch(&collected)
    } else {
        collected
    })
}

fn visible_response_patch_from_document(file: &Path, doc_content: &str) -> Result<Option<String>> {
    let Some(snapshot_doc) = snapshot::load(file)? else {
        return Ok(None);
    };
    let template_mode = frontmatter::parse(doc_content)
        .map(|(fm, _)| fm.resolve_mode().is_template())
        .unwrap_or(false);
    Ok(extract_visible_response_patch_between(
        &snapshot_doc,
        doc_content,
        template_mode,
    ))
}

pub const AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR: &str =
    "ambiguous preflight_started patchback";
pub const RESPONSE_PATCHBACK_UNCOMMITTED_ERROR: &str = "response_patchback_uncommitted";
pub const EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR: &str =
    "empty preflight_started cycle has no response capture";
pub const STALE_EMPTY_PREFLIGHT_TTL_SECS: u64 = 60;

/// Outcome of an explicit run-cancel reclaim ([`cancel_preflight_cycle`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelOutcome {
    /// An empty `preflight_started` cycle with no response capture was
    /// abandoned so the next dispatch can start a fresh cycle immediately.
    Abandoned,
    /// Nothing to reclaim — no open cycle for this document.
    NoOpenCycle,
    /// The open cycle is protected: it advanced past `preflight_started` or it
    /// already owns a response capture, so an explicit cancel must NOT discard
    /// it (that would risk dropping real work). Reclaim waits for the normal
    /// closeout / staleness path instead.
    Protected,
}

/// `#cancel-orphans-preflight-cycle`: explicit run-cancel reclaim.
///
/// When the user cancels an in-progress run, the JB plugin (thin reporter)
/// calls this so the orphaned `preflight_started` cycle is abandoned *now*
/// instead of blocking the next `Run Agent Doc` until the
/// [`STALE_EMPTY_PREFLIGHT_TTL_SECS`] window elapses. The abandon decision is a
/// pure, fail-safe `cycle_state` operation: it only abandons an open cycle that
/// is still `preflight_started` **and** owns no response capture. Any cycle
/// that advanced past preflight or already captured a response is left intact
/// (`Protected`) so a cancel can never discard real in-flight work.
pub fn cancel_preflight_cycle(file: &Path) -> Result<CancelOutcome> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(CancelOutcome::NoOpenCycle);
    };
    if !state.is_open() {
        return Ok(CancelOutcome::NoOpenCycle);
    }
    if !matches!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted) {
        return Ok(CancelOutcome::Protected);
    }
    if crate::capture::load_by_id(file, &state.cycle_id)?.is_some() {
        return Ok(CancelOutcome::Protected);
    }
    let snapshot_content = crate::snapshot::load(file)?;
    let file_content = std::fs::read_to_string(file).ok();
    crate::cycle_state::mark_abandoned(
        file,
        "cancel_preflight_cycle_abandoned",
        snapshot_content.as_deref(),
        file_content.as_deref(),
    )?;
    crate::ops_log::log_op(
        file,
        &format!(
            "cancel_preflight_cycle_abandoned file={} cycle_id={}",
            file.display(),
            state.cycle_id
        ),
    );
    eprintln!(
        "[cancel] abandoned empty preflight_started cycle {} for {} on explicit run cancel; next dispatch starts fresh",
        state.cycle_id,
        file.display()
    );
    Ok(CancelOutcome::Abandoned)
}

fn agent_owned_visible_response_is_adoptable(
    file: &Path,
    state: Option<&crate::cycle_state::CycleState>,
) -> bool {
    matches!(
        state.map(|state| state.phase),
        Some(
            agent_doc_turn::CyclePhase::ResponseCaptured | agent_doc_turn::CyclePhase::WriteApplied
        ) | None
    ) || crate::codex_hook::load_active_session_for_current_file(file)
        .ok()
        .flatten()
        .is_some()
}

fn head_already_matches_current_doc(file: &Path, doc_content: &str) -> Result<bool> {
    Ok(crate::git::show_head(file)?.as_deref().is_some_and(|head| {
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(head)
            == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                doc_content,
            )
    }))
}

fn normalized_content_hash(content: &str) -> String {
    // Compare-side normalization for stale-cycle replay matching. Neutralizes
    // transient markers AND the agent:queue component so queue-maintenance churn
    // alone does not block recovery of an already-materialized response
    // (#adoc-queue-ipc-buffer-divergence #4). Must match cycle_state.rs's
    // store-side normalization exactly.
    agent_doc_hash::content_hash(
        &agent_doc_document::transient_markers::normalize_for_replay_hash(content),
    )
}

fn preflight_cycle_age_secs(state: &crate::cycle_state::CycleState) -> u64 {
    now_secs().saturating_sub(state.updated_at.max(state.started_at))
}

fn prompt_change_is_orchestration_handoff_marker(text: &str) -> bool {
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

pub fn repair_stale_preflight_started_cycle(file: &Path) -> Result<RepairOutcome> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(RepairOutcome::Noop);
    };
    if state.phase != agent_doc_turn::CyclePhase::PreflightStarted {
        return Ok(RepairOutcome::Noop);
    }

    let file_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for stale preflight repair",
            file.display()
        )
    })?;
    let snapshot_content = snapshot::load(file)?;
    let current_file_hash = agent_doc_hash::content_hash(&file_content);
    let current_snapshot_hash = snapshot_content
        .as_deref()
        .map(agent_doc_hash::content_hash);
    let current_normalized_file_hash = normalized_content_hash(&file_content);
    let current_normalized_snapshot_hash = snapshot_content.as_deref().map(normalized_content_hash);

    let raw_hashes_match = state.file_hash.as_deref() == Some(current_file_hash.as_str())
        && state.snapshot_hash == current_snapshot_hash;
    let normalized_hashes_match = state.normalized_file_hash.as_deref()
        == Some(current_normalized_file_hash.as_str())
        && state.normalized_snapshot_hash == current_normalized_snapshot_hash;

    if raw_hashes_match || normalized_hashes_match {
        if !head_already_matches_current_doc(file, &file_content)?
            && let Some(marker) = crate::session_check::detect_bypassed_response_write(file)?
        {
            crate::flow::closeout::log_closeout_guard_event(
                file,
                crate::flow::types::FlowStage::TerminalGuard,
                crate::flow::types::FlowOutcome::FailedClosed,
                agent_doc_turn::closeout_guard::CloseoutGuardReason::ResponsePatchbackUncommitted,
            );
            anyhow::bail!(
                "{} for {}: stale preflight_started cycle `{}` has visible response patchback drift ({marker}) that is not committed in HEAD. Run `agent-doc write --commit {}` or `agent-doc finalize {}` through the normal closeout path; recovery will not report an already-committed cycle while this response is still only in the working tree.",
                RESPONSE_PATCHBACK_UNCOMMITTED_ERROR,
                file.display(),
                state.cycle_id,
                file.display(),
                file.display(),
            );
        }
        crate::cycle_state::mark_committed(
            file,
            "repair_preflight_stale_lock",
            snapshot_content.as_deref(),
            Some(&file_content),
        )?;
        crate::ops_log::log_op(
            file,
            &format!(
                "repair_preflight_stale_lock file={} cycle_id={}",
                file.display(),
                state.cycle_id
            ),
        );
        crate::flow::closeout::log_closeout_guard_event(
            file,
            crate::flow::types::FlowStage::TerminalGuard,
            crate::flow::types::FlowOutcome::Completed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::StalePreflightLockRepaired,
        );
        eprintln!(
            "[repair] repaired stale preflight_started cycle {} for {}",
            state.cycle_id,
            file.display()
        );
        return Ok(RepairOutcome::StalePreflightLockRepaired);
    }

    if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
        let repaired_snapshot = snapshot::load(file)?;
        crate::cycle_state::mark_committed(
            file,
            "repair_preflight_committed_historical",
            repaired_snapshot.as_deref(),
            Some(&file_content),
        )?;
        crate::capture::mark_committed(file)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "repair_preflight_committed_historical file={} cycle_id={} reason={}",
                file.display(),
                state.cycle_id,
                reason
            ),
        );
        crate::flow::closeout::log_closeout_guard_event(
            file,
            crate::flow::types::FlowStage::TerminalGuard,
            crate::flow::types::FlowOutcome::Completed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::StalePreflightLockRepaired,
        );
        eprintln!(
            "[repair] closed stale preflight_started cycle {} for {} after repairing committed historical {} drift",
            state.cycle_id,
            file.display(),
            reason
        );
        return Ok(RepairOutcome::StalePreflightLockRepaired);
    }

    if let Some(marker) = crate::session_check::detect_bypassed_response_write(file)? {
        crate::flow::closeout::log_closeout_guard_event(
            file,
            crate::flow::types::FlowStage::TerminalGuard,
            crate::flow::types::FlowOutcome::FailedClosed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::OpenCycle,
        );
        anyhow::bail!(
            "{} for {}: found visible response patchback ({marker}) but no pending/capture artifact exists and HEAD cannot prove the patchback was already committed",
            AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR,
            file.display(),
        );
    }

    let cycle_capture_exists = crate::capture::load_by_id(file, &state.cycle_id)?.is_some();
    let age_secs = preflight_cycle_age_secs(&state);
    if !cycle_capture_exists
        && let Some(change) = crate::session_check::first_unstarted_prompt_bearing_change(file)?
        && !prompt_change_is_orchestration_handoff_marker(&change.text)
    {
        let preview = change
            .text
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or(change.text.as_str())
            .trim();
        if age_secs >= STALE_EMPTY_PREFLIGHT_TTL_SECS {
            crate::cycle_state::mark_abandoned(
                file,
                "repair_preflight_stale_prompt_cycle_abandoned",
                snapshot_content.as_deref(),
                Some(&file_content),
            )?;
            crate::ops_log::log_op(
                file,
                &format!(
                    "repair_preflight_stale_prompt_cycle_abandoned file={} cycle_id={} age_secs={} prompt_preview={}",
                    file.display(),
                    state.cycle_id,
                    age_secs,
                    preview
                ),
            );
            crate::flow::closeout::log_closeout_guard_event(
                file,
                crate::flow::types::FlowStage::TerminalGuard,
                crate::flow::types::FlowOutcome::FailedClosed,
                agent_doc_turn::closeout_guard::CloseoutGuardReason::StalePreflightCycleAbandoned,
            );
            eprintln!(
                "[repair] abandoned stale empty preflight_started cycle {} for {} after {}s; unresolved prompt remains visible for the next preflight",
                state.cycle_id,
                file.display(),
                age_secs
            );
            return Ok(RepairOutcome::StalePreflightCycleAbandoned);
        }
        crate::flow::closeout::log_closeout_guard_event(
            file,
            crate::flow::types::FlowStage::TerminalGuard,
            crate::flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::OpenCycle,
        );
        anyhow::bail!(
            "{} for {}: previous cycle `{}` is still `preflight_started`, the live document has unresolved prompt_target: {preview}, and no response exists to replay. The cycle is only {}s old; wait until it is stale or restart the harness pane and rerun `agent-doc {}` (or use `agent-doc start {}` from a fresh pane) so the prompt is handled by a new response cycle.",
            EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR,
            file.display(),
            state.cycle_id,
            age_secs,
            file.display(),
            file.display(),
        );
    }

    if age_secs >= STALE_EMPTY_PREFLIGHT_TTL_SECS && !cycle_capture_exists {
        crate::cycle_state::mark_committed(
            file,
            "repair_preflight_stale_empty_cycle",
            snapshot_content.as_deref(),
            Some(&file_content),
        )?;
        crate::ops_log::log_op(
            file,
            &format!(
                "repair_preflight_stale_empty_cycle file={} cycle_id={} age_secs={}",
                file.display(),
                state.cycle_id,
                age_secs
            ),
        );
        crate::flow::closeout::log_closeout_guard_event(
            file,
            crate::flow::types::FlowStage::TerminalGuard,
            crate::flow::types::FlowOutcome::Completed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::StalePreflightLockRepaired,
        );
        eprintln!(
            "[repair] closed stale empty preflight_started cycle {} for {} after {}s without a capture",
            state.cycle_id,
            file.display(),
            age_secs
        );
        return Ok(RepairOutcome::StalePreflightLockRepaired);
    }

    Ok(RepairOutcome::Noop)
}

pub fn recover_missing_commit_boundary(file: &Path, event: &str) -> Result<Option<&'static str>> {
    let state = crate::cycle_state::load(file)?;
    let has_open_commit_boundary = state.as_ref().is_some_and(|state| {
        matches!(
            state.phase,
            agent_doc_turn::CyclePhase::ResponseCaptured | agent_doc_turn::CyclePhase::WriteApplied
        )
    });
    let has_missing_commit_event = if has_open_commit_boundary {
        false
    } else {
        crate::session_check::detect_write_completed_commit_missing(file)?.is_some()
    };
    if !has_open_commit_boundary && !has_missing_commit_event {
        return Ok(None);
    }

    let current_doc = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for commit-boundary recovery",
            file.display()
        )
    })?;
    let head_doc = crate::git::show_head(file)?;
    let reason = match crate::git::verify_snapshot_committed(file)? {
        crate::git::SnapshotCommitStatus::Committed => head_doc
            .as_deref()
            .filter(|head| {
                agent_doc_turn::document_drift::detect_bypassed_response_write_between(
                    head,
                    &current_doc,
                )
                .is_none()
            })
            .map(|_| "already-committed HEAD"),
        _ => crate::git::repair_committed_historical_snapshot_drift(file)?
            .map(|_| "committed historical exchange snapshot drift"),
    };
    let Some(reason) = reason else {
        return Ok(None);
    };

    let repaired_snapshot = snapshot::load(file)?;
    crate::cycle_state::mark_committed(
        file,
        event,
        repaired_snapshot.as_deref(),
        Some(&current_doc),
    )?;
    crate::capture::mark_committed(file)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_commit_boundary_recovered file={} event={} reason={}",
            file.display(),
            event,
            reason
        ),
    );
    crate::flow::closeout::log_closeout_guard_event(
        file,
        crate::flow::types::FlowStage::TerminalGuard,
        crate::flow::types::FlowOutcome::Completed,
        agent_doc_turn::closeout_guard::CloseoutGuardReason::CommitBoundaryRecovered,
    );
    Ok(Some(reason))
}

fn repair_completed_backlog_items(file: &Path) -> Result<RepairOutcome> {
    let content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for completed backlog reap repair",
            file.display()
        )
    })?;
    let components = agent_doc_element::element::parse(&content).with_context(|| {
        format!(
            "failed to parse {} for completed backlog reap repair",
            file.display()
        )
    })?;
    let Some(backlog) = components
        .iter()
        .find(|component| agent_doc_element::element::is_backlog_component(&component.name))
    else {
        return Ok(RepairOutcome::Noop);
    };

    let doc_id = agent_doc_hash::document_id_for_path(file);
    let (canonical_body, _) = agent_doc_element_backlog::backlog::backfill(
        backlog.content(&content),
        &doc_id,
        &HashSet::new(),
    );
    let (new_body, removed) = agent_doc_element_backlog::backlog::reap_with_items(&canonical_body)?;
    if removed.is_empty() {
        return Ok(RepairOutcome::Noop);
    }

    let mut repaired = backlog.replace_content(&content, &new_body);
    if let Some(archived) = crate::preflight::archive_pending_done(file, &repaired, &removed)? {
        repaired = archived;
    }
    if let Some(reconciled) =
        agent_doc_document::status_projection::reconcile_top_backlog_status_content(&repaired)?
    {
        repaired = reconciled;
    }

    write::atomic_write_pub(file, &repaired)?;

    let repaired_snapshot = if let Some(snap_content) = snapshot::load(file)? {
        let snap_components =
            agent_doc_element::element::parse(&snap_content).with_context(|| {
                format!(
                    "failed to parse snapshot for completed backlog reap repair {}",
                    file.display()
                )
            })?;
        let snap_backlog = snap_components
            .iter()
            .find(|component| agent_doc_element::element::is_backlog_component(&component.name))
            .with_context(|| {
                format!(
                    "completed backlog reap repair requires the snapshot backlog component in {}",
                    file.display()
                )
            })?;

        let mut new_snapshot = snap_backlog.replace_content(&snap_content, &new_body);
        if let Some(archived) =
            crate::preflight::archive_pending_done(file, &new_snapshot, &removed)?
        {
            new_snapshot = archived;
        }
        if let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_top_backlog_status_content(
                &new_snapshot,
            )?
        {
            new_snapshot = reconciled;
        }
        snapshot::save(file, &new_snapshot)?;
        Some(new_snapshot)
    } else {
        None
    };

    if repaired_snapshot.as_deref() == Some(repaired.as_str()) {
        let _ = crate::cycle_state::mark_committed(
            file,
            "repair_completed_backlog_reap",
            Some(&repaired),
            Some(&repaired),
        );
    }

    let refs = removed
        .iter()
        .map(|item| format!("#{}", item.id))
        .collect::<Vec<_>>()
        .join(", ");
    let removed_ids: Vec<String> = removed.iter().map(|item| item.id.clone()).collect();
    let _ = crate::cycle_state::record_reaped_pending_ids(file, &removed_ids);
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_completed_backlog_reap file={} count={} ids={}",
            file.display(),
            removed.len(),
            refs
        ),
    );
    eprintln!(
        "[repair] reaped stale completed backlog item(s) in {}: {}",
        file.display(),
        refs
    );

    Ok(RepairOutcome::CompletedBacklogReaped)
}

fn repair_template_doc_if_needed(
    file: &Path,
    doc_content: &str,
    known_response: Option<&str>,
) -> Result<String> {
    let mut dup_opener_input = doc_content.to_string();
    let mut duplicate_opener_changed = false;
    while let Some(merged) =
        agent_doc_template::repair_duplicate_exchange_opener(&dup_opener_input)?
    {
        dup_opener_input = merged;
        duplicate_opener_changed = true;
    }
    let duplicate_scaffold_repaired =
        agent_doc_template::repair_duplicate_exchange_close_scaffold(&dup_opener_input)?
            .unwrap_or_else(|| dup_opener_input.clone());
    let duplicate_scaffold_changed = duplicate_scaffold_repaired != dup_opener_input;
    let duplicate_close_repaired =
        agent_doc_template::repair_duplicate_exchange_close_tail(&duplicate_scaffold_repaired)?
            .unwrap_or_else(|| duplicate_scaffold_repaired.clone());
    let duplicate_close_changed = duplicate_close_repaired != duplicate_scaffold_repaired;
    let tail_repaired =
        agent_doc_template::repair_conversation_tail_outside_exchange(&duplicate_close_repaired)?
            .unwrap_or_else(|| duplicate_close_repaired.clone());
    let tail_changed = tail_repaired != duplicate_close_repaired;
    let boundary_repaired = repair_answered_stale_boundary_if_safe(file, &tail_repaired)?;
    let boundary_changed = boundary_repaired.is_some();
    let mut repaired = boundary_repaired.unwrap_or_else(|| tail_repaired.clone());
    let order_repaired =
        write::repair_response_prompt_order_for_file(&repaired, known_response, file, None)?;
    let order_changed = order_repaired.is_some();
    if let Some(ordered) = order_repaired {
        repaired = ordered;
    }

    let (fm, _) = frontmatter::parse(&repaired)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;

    let prompt_input = repaired.clone();
    if fm.resolve_mode().is_template()
        && let Some(snapshot_content) = snapshot::load(file)?
    {
        repaired = write::normalize_user_prompts_in_exchange_safe(
            &repaired,
            &repaired,
            &snapshot_content,
            file,
        );
        if let Some(stripped) = write::strip_prompt_prefix_from_response_body_first_lines(&repaired)
        {
            crate::ops_log::log_op(
                file,
                &format!(
                    "repair_response_body_prompt_prefix_stripped file={}",
                    file.display()
                ),
            );
            repaired = stripped;
        }
        repaired = write::normalize_template_structure_or_fail_preserving(
            &repaired,
            file,
            Some(&prompt_input),
        )?;
    }
    let prompt_changed = repaired != prompt_input;

    if duplicate_opener_changed
        || duplicate_close_changed
        || duplicate_scaffold_changed
        || tail_changed
        || boundary_changed
        || order_changed
        || prompt_changed
    {
        let save_repaired_snapshot = match snapshot::load(file)? {
            Some(snapshot_content) => {
                !repair_leaves_unanswered_prompt_diff(&snapshot_content, &repaired, known_response)
            }
            None => true,
        };
        write::atomic_write_pub(file, &repaired)?;
        if save_repaired_snapshot {
            snapshot::save(file, &repaired)?;
        }
        if duplicate_opener_changed {
            crate::ops_log::log_op(
                file,
                &format!("repair_duplicate_exchange_opener file={}", file.display()),
            );
            eprintln!(
                "[repair] merged duplicate exchange opener(s) in {}",
                file.display()
            );
        }
        if duplicate_close_changed {
            crate::ops_log::log_op(
                file,
                &format!("repair_duplicate_exchange_close file={}", file.display()),
            );
            eprintln!(
                "[repair] removed duplicate exchange close and restored escaped content in {}",
                file.display()
            );
        }
        if duplicate_scaffold_changed {
            crate::ops_log::log_op(
                file,
                &format!("repair_duplicate_exchange_scaffold file={}", file.display()),
            );
            eprintln!(
                "[repair] removed duplicate template scaffold after exchange close in {}",
                file.display()
            );
        }
        if tail_changed {
            crate::ops_log::log_op(
                file,
                &format!("repair_exchange_tail file={}", file.display()),
            );
            eprintln!(
                "[repair] repaired escaped conversation tail in {}",
                file.display()
            );
        }
        if boundary_changed {
            crate::ops_log::log_op(
                file,
                &format!("repair_completed_turn_boundary file={}", file.display()),
            );
            eprintln!(
                "[repair] moved stale boundary to the end of the completed exchange turn in {}",
                file.display()
            );
        }
        if order_changed {
            crate::ops_log::log_op(
                file,
                &format!("repair_response_prompt_order file={}", file.display()),
            );
            eprintln!(
                "[repair] repaired response/prompt ordering in {}",
                file.display()
            );
        }
        if prompt_changed {
            crate::ops_log::log_op(
                file,
                &format!("repair_prompt_prefixes file={}", file.display()),
            );
            eprintln!(
                "[repair] repaired transcript prompt prefixes in {}",
                file.display()
            );
        }
    }

    Ok(repaired)
}

fn repair_response_body_prompt_prefixes_if_needed(
    file: &Path,
    doc_content: &str,
) -> Result<String> {
    let (fm, _) = frontmatter::parse(doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    if !fm.resolve_mode().is_template() {
        return Ok(doc_content.to_string());
    }

    let Some(repaired) = write::strip_prompt_prefix_from_response_body_first_lines(doc_content)
    else {
        return Ok(doc_content.to_string());
    };

    let save_repaired_snapshot = match snapshot::load(file)? {
        Some(snapshot_content) => {
            !repair_leaves_unanswered_prompt_diff(&snapshot_content, &repaired, None)
        }
        None => true,
    };
    write::atomic_write_pub(file, &repaired)?;
    if save_repaired_snapshot {
        snapshot::save(file, &repaired)?;
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_response_body_prompt_prefix_stripped file={}",
            file.display()
        ),
    );
    eprintln!(
        "[repair] stripped leaked response-body prompt prefixes in {}",
        file.display()
    );
    Ok(repaired)
}

fn repair_duplicate_exchange_scaffold_if_needed(file: &Path, doc_content: &str) -> Result<String> {
    let repaired = agent_doc_template::repair_duplicate_exchange_close_scaffold(doc_content)?
        .unwrap_or_else(|| doc_content.to_string());
    if repaired == doc_content {
        return Ok(repaired);
    }

    let save_repaired_snapshot = match snapshot::load(file)? {
        Some(snapshot_content) => {
            !repair_leaves_unanswered_prompt_diff(&snapshot_content, &repaired, None)
        }
        None => true,
    };
    write::atomic_write_pub(file, &repaired)?;
    if save_repaired_snapshot {
        snapshot::save(file, &repaired)?;
    }
    crate::ops_log::log_op(
        file,
        &format!("repair_duplicate_exchange_scaffold file={}", file.display()),
    );
    eprintln!(
        "[repair] removed duplicate template scaffold after exchange close in {}",
        file.display()
    );
    Ok(repaired)
}

fn repair_leaves_unanswered_prompt_diff(
    snapshot_content: &str,
    repaired: &str,
    known_response: Option<&str>,
) -> bool {
    let norm_snapshot = crate::git::normalize_committed_exchange_artifacts(snapshot_content);
    let norm_repaired = crate::git::normalize_committed_exchange_artifacts(repaired);
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
                .map(|response| prompt_change_is_known_response(&change.text, response))
                .unwrap_or(false)
        {
            skip_answered_response_run = true;
            continue;
        }
        return true;
    }
    false
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
            return agent_doc_turn::closeout_signal::is_exchange_response_heading(normalized);
        }
    }
    false
}

fn prompt_change_is_known_response(change_text: &str, response: &str) -> bool {
    let response_lines: HashSet<String> = normalized_response_lines(response)
        .into_iter()
        .map(|line| line.trim().trim_start_matches('❯').trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    if response_lines.is_empty() {
        return false;
    }
    change_text
        .lines()
        .map(|line| line.trim().trim_start_matches('❯').trim())
        .filter(|line| !line.is_empty())
        .all(|line| response_lines.contains(line))
}

fn repair_answered_stale_boundary_if_safe(
    file: &Path,
    doc_content: &str,
) -> Result<Option<String>> {
    let (fm, _) = frontmatter::parse(doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    if !fm.resolve_mode().is_template() || snapshot::load(file)?.is_none() {
        return Ok(None);
    }

    let components = agent_doc_element::element::parse(doc_content).with_context(|| {
        format!(
            "failed to parse {} for completed-turn boundary repair",
            file.display()
        )
    })?;
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(None);
    };
    let Some(boundary_id) =
        agent_doc_element_boundary::boundary::find_boundary_id_in_component(doc_content, exchange)
    else {
        return Ok(None);
    };

    let exchange_body = exchange.content(doc_content);
    let marker = agent_doc_element_boundary::boundary::format_marker(&boundary_id);
    let Some(marker_idx) = exchange_body.find(&marker) else {
        return Ok(None);
    };
    let tail_after_boundary = &exchange_body[marker_idx + marker.len()..];
    if tail_after_boundary.trim().is_empty()
        || !agent_doc_diff::prompt_change_is_already_answered(tail_after_boundary)
        || crate::session_check::first_unstarted_prompt_bearing_change(file)?.is_some()
    {
        return Ok(None);
    }

    let repaired = agent_doc_template::reposition_boundary_to_end_preserve_head_with_id(
        doc_content,
        Some(boundary_id.as_str()),
    );
    if repaired == doc_content {
        return Ok(None);
    }
    Ok(Some(repaired))
}

fn same_content_ignoring_trailing_newlines(left: &str, right: &str) -> bool {
    left.trim_end_matches('\n') == right.trim_end_matches('\n')
}

#[derive(Debug, Serialize)]
struct BlockedRepairPayloadRecord<'a> {
    captured_at: u64,
    file: String,
    reason: &'a str,
    payload_sha256: String,
    response_body: &'a str,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn save_blocked_repair_payload(
    file: &Path,
    response: &str,
    reason: &str,
) -> Result<std::path::PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = agent_doc_fs::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .context("resolve project root for blocked repair payload")?;
    let dir = root.join(".agent-doc/repair-blocked");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create blocked repair dir {}", dir.display()))?;
    let filename = format!(
        "{}-{}.json",
        agent_doc_hash::content_hash(canonical.to_string_lossy().as_ref()),
        now_millis()
    );
    let path = dir.join(filename);
    let record = BlockedRepairPayloadRecord {
        captured_at: now_secs(),
        file: canonical.display().to_string(),
        reason,
        payload_sha256: agent_doc_hash::content_hash(response),
        response_body: response,
    };
    let json = serde_json::to_string_pretty(&record)?;
    std::fs::write(&path, json)
        .with_context(|| format!("write blocked repair payload {}", path.display()))?;
    Ok(path)
}

fn fail_closed_on_blocked_template_replay(file: &Path, response: &str, reason: &str) -> Result<()> {
    match save_blocked_repair_payload(file, response, reason) {
        Ok(path) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "repair_blocked_replay path={} reason={}",
                    path.display(),
                    reason
                ),
            );
            anyhow::bail!(
                "refused to replay pending response for {} because {}; blocked payload captured at {}",
                file.display(),
                reason,
                path.display()
            );
        }
        Err(err) => {
            anyhow::bail!(
                "refused to replay pending response for {} because {}; additionally failed to save blocked payload: {}",
                file.display(),
                reason,
                err
            );
        }
    }
}

fn discard_pending_capture_for_manual_repair(file: &Path, current_doc: &str) -> Result<()> {
    crate::flow::closeout::apply_closeout_recovery_mutation(
        file,
        crate::flow::closeout::CloseoutRecoveryMutation::RetireStaleCapture {
            content: Some(current_doc),
            clear_pending_response: true,
            delete_pre_response: true,
            mark_cycle_committed_event: Some("repair_respect_manual_exchange_tail_removal"),
            reason: CloseoutRecoveryMutationReason::RespectManualTailRemoval,
        },
    )?;
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_discard_stale_capture_after_manual_tail_removal file={}",
            file.display()
        ),
    );
    eprintln!(
        "[repair] respected manual removal of escaped conversation tail in {}",
        file.display()
    );
    Ok(())
}

/// `#stale-capture-deadlock-autoretire`: retire a wedged `WriteApplied` capture
/// whose written response has vanished from the document and whose baseline has
/// drifted irreconcilably, instead of fail-closing with "captured response
/// baseline no longer matches current document".
///
/// **Deadlock signature.** A `finalize` advanced the cycle to `write_applied`
/// (the response was written to disk), but a concurrent editor edit / CRDT
/// intermix then removed or fragmented that response body *and* drifted the
/// captured baseline, so by the time `run` reaches this point the caller has
/// already proved `response_already_applied*` is false (the body is not
/// contiguously present in `doc_content`) and
/// [`crate::capture::replay_baseline_drifted`] is true (the captured
/// file/snapshot hashes no longer match), so [`crate::capture::validate_replay`]
/// is about to fail closed. That combination wedges every later `commit` /
/// `write --commit` / route closeout drain behind a manual
/// `reset --from-current --preserve-session` — the compounding deadlock
/// operators kept hitting in dogfooding.
///
/// **Why retiring is safe and strictly better than deadlocking.**
///   1. Scoped to `WriteApplied` captures — the write was already attempted, so
///      this is not a never-written pending body. A `Captured`-only orphan stays
///      on the conservative `validate_replay` fail-closed path.
///   2. Replaying a drifted baseline is exactly the duplicate / reorder
///      corruption `validate_replay` exists to prevent; re-running the turn fresh
///      on the rebuilt baseline is the correct recovery, not a regression. Queue
///      prompts survive in `agent:queue` for the next drain to process.
///   3. The captured body is preserved on disk as a `Discarded` record for
///      forensics — never deleted — mirroring `reset --preserve-session`.
///
/// Mirrors `reset --from-current --preserve-session`: discards the capture and
/// rebuilds the snapshot + CRDT sidecars from the current document so the merge
/// layer cannot re-diverge, then closes the cycle as committed. Returns `true`
/// when it retired a capture, `false` when the signature does not match (so the
/// caller falls through to the normal `validate_replay` guard).
fn retire_wedged_write_applied_capture_if_drifted(
    file: &Path,
    doc_content: &str,
    capture: &crate::capture::CaptureRecord,
) -> Result<bool> {
    if capture.state != crate::capture::CaptureState::WriteApplied {
        return Ok(false);
    }
    if !crate::capture::replay_baseline_drifted(file, capture)? {
        return Ok(false);
    }

    crate::flow::closeout::apply_closeout_recovery_mutation(
        file,
        crate::flow::closeout::CloseoutRecoveryMutation::RetireStaleCapture {
            content: Some(doc_content),
            clear_pending_response: true,
            delete_pre_response: true,
            mark_cycle_committed_event: Some("repair_retire_wedged_write_applied_capture"),
            reason: CloseoutRecoveryMutationReason::RetireWedgedWriteAppliedCapture,
        },
    )?;
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_retire_wedged_write_applied_capture file={} capture_id={} cycle_id={}",
            file.display(),
            capture.capture_id,
            capture.cycle_id
        ),
    );
    eprintln!(
        "[repair] retired wedged write-applied capture for {} (response missing from document + baseline drifted); rebuilt snapshot/CRDT from current and preserved the captured body for forensics",
        file.display()
    );
    Ok(true)
}

/// `#stale-capture-captured-only-drift`: extend the stale-capture retire path to
/// a `Captured`-only orphan (write never attempted) whose baseline drifted — but
/// ONLY when there is positive superseding-turn evidence.
///
/// [`retire_wedged_write_applied_capture_if_drifted`] is deliberately scoped to
/// `WriteApplied` captures because the write was already attempted, so re-running
/// fresh is safe. A bare `Captured` orphan could still be a legitimate pending
/// response (captured but not yet written — e.g. a crash between capture and
/// write), so blindly retiring it would lose that response. The conservative
/// default therefore stays on the `validate_replay` fail-closed path.
///
/// The one safe exception: the captured response's prompt has **already been
/// answered** in the live document. `run` only ever loads the *current* cycle's
/// capture (`load_active` keys off `cycle_state.capture_id`), so the deadlock
/// orphan is the current cycle's Captured body. Positive superseding-turn
/// evidence is therefore that the captured response's `### Re:` heading already
/// appears in the live `agent:exchange` — a later turn's answer to the same
/// prompt landed — so this never-written body is a stale duplicate, not the only
/// answer. (The `response_already_applied*` checks earlier in `run` already
/// handle the case where the captured *body* itself is present; this covers the
/// heading-present-but-body-differs supersession.) When that holds, retire the
/// orphan — discard the body as a `Discarded` record (preserved on disk for
/// forensics) and clear the pending sidecar — breaking the `validate_replay`
/// deadlock that otherwise needs a manual `reset --from-current --preserve-session`.
/// Without the evidence, stay on the conservative fail-closed path. Returns
/// `true` when it retired the orphan.
fn retire_superseded_captured_only_orphan_if_drifted(
    file: &Path,
    doc_content: &str,
    capture: &crate::capture::CaptureRecord,
) -> Result<bool> {
    if capture.state != crate::capture::CaptureState::Captured {
        return Ok(false);
    }
    if !crate::capture::replay_baseline_drifted(file, capture)? {
        return Ok(false);
    }
    // Positive superseding-turn evidence: the captured response's heading is
    // already answered in the live exchange.
    let Some(heading) = first_response_heading_line(&capture.response_body) else {
        return Ok(false);
    };
    if !live_exchange_answers_heading(doc_content, heading) {
        return Ok(false);
    }

    crate::flow::closeout::apply_closeout_recovery_mutation(
        file,
        crate::flow::closeout::CloseoutRecoveryMutation::RetireStaleCapture {
            content: None,
            clear_pending_response: true,
            delete_pre_response: true,
            mark_cycle_committed_event: None,
            reason: CloseoutRecoveryMutationReason::RetireSupersededCapturedOnlyOrphan,
        },
    )?;
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_retire_superseded_captured_only_orphan file={} capture_id={} cycle_id={}",
            file.display(),
            capture.capture_id,
            capture.cycle_id
        ),
    );
    eprintln!(
        "[repair] retired superseded Captured-only orphan for {} (captured response's heading already answered in the live exchange + baseline drifted); preserved the captured body for forensics",
        file.display()
    );
    Ok(true)
}

/// True when the live document's `agent:exchange` already contains a `### Re:`
/// response heading whose normalized topic matches `heading` — i.e. the prompt
/// the orphan answered is already answered by a landed response.
pub(crate) fn live_exchange_answers_heading(doc_content: &str, heading: &str) -> bool {
    let target = normalize_replay_topic(heading);
    if target.is_empty() {
        return false;
    }
    let Ok(components) = agent_doc_element::element::parse(doc_content) else {
        return false;
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return false;
    };
    exchange
        .content(doc_content)
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("### Re:"))
        .any(|line| normalize_replay_topic(line) == target)
}

fn respect_manual_exchange_tail_removal_if_safe(
    file: &Path,
    doc_content: &str,
    capture: &crate::capture::CaptureRecord,
) -> Result<bool> {
    let (fm, _) = frontmatter::parse(doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    if !fm.resolve_mode().is_template() {
        return Ok(false);
    }

    let Some(snapshot_content) = snapshot::load(file)? else {
        return Ok(false);
    };
    if capture.snapshot_hash != Some(agent_doc_hash::content_hash(&snapshot_content)) {
        return Ok(false);
    }

    let Some(stripped_snapshot) =
        agent_doc_template::strip_conversation_tail_outside_exchange(&snapshot_content)?
    else {
        return Ok(false);
    };
    if !same_content_ignoring_trailing_newlines(&stripped_snapshot, doc_content) {
        return Ok(false);
    }

    discard_pending_capture_for_manual_repair(file, doc_content)?;
    Ok(true)
}

/// Check for a pending response and apply it if found.
pub fn run(file: &Path) -> Result<RepairOutcome> {
    run_with_queue_completion_ids(file, &[])
}

pub(crate) fn run_with_queue_completion_ids(
    file: &Path,
    queue_completion_ids: &[String],
) -> Result<RepairOutcome> {
    // Canonicalize first to handle CWD drift (e.g., when CWD is in a submodule)
    let canonical = file
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("file not found: {}", file.display()))?;

    let pending_path = snapshot::pending_path_for(&canonical)?;
    let capture = crate::capture::load_active(&canonical)?.filter(capture_is_repairable);
    let doc_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read document for repair {}", file.display()))?;
    let cycle_state = crate::cycle_state::load(file)?;
    let historical_capture = if !pending_path.exists() && capture.is_none() {
        historical_committed_capture_replay(&canonical, &doc_content)?
    } else {
        None
    };
    let visible_response_recovery = if !pending_path.exists()
        && capture.is_none()
        && historical_capture.is_none()
        && agent_owned_visible_response_is_adoptable(file, cycle_state.as_ref())
        && crate::git::is_in_git_repo(file)
        && !head_already_matches_current_doc(file, &doc_content)?
    {
        visible_response_patch_from_document(file, &doc_content)?
    } else {
        None
    };
    if !pending_path.exists()
        && capture.is_none()
        && historical_capture.is_none()
        && visible_response_recovery.is_none()
    {
        let outcome = repair_stale_preflight_started_cycle(file)?;
        if outcome != RepairOutcome::Noop {
            let refreshed_content = std::fs::read_to_string(file).with_context(|| {
                format!(
                    "failed to read document after stale preflight repair {}",
                    file.display()
                )
            })?;
            let response_prefix_repaired_doc =
                repair_response_body_prompt_prefixes_if_needed(file, &refreshed_content)?;
            if response_prefix_repaired_doc != refreshed_content {
                return Ok(RepairOutcome::TemplateNormalized);
            }
            return Ok(outcome);
        }
        if recover_missing_commit_boundary(file, "repair_commit_boundary_recovered")?.is_some() {
            return Ok(RepairOutcome::CommitBoundaryRecovered);
        }
        let scaffold_repaired_doc =
            repair_duplicate_exchange_scaffold_if_needed(file, &doc_content)?;
        if scaffold_repaired_doc != doc_content {
            return Ok(RepairOutcome::TemplateNormalized);
        }
        let response_prefix_repaired_doc =
            repair_response_body_prompt_prefixes_if_needed(file, &doc_content)?;
        if response_prefix_repaired_doc != doc_content {
            return Ok(RepairOutcome::TemplateNormalized);
        }
        let has_live_prompt =
            crate::session_check::first_unstarted_prompt_bearing_change(file)?.is_some();
        if !has_live_prompt {
            let repaired_doc = repair_template_doc_if_needed(file, &doc_content, None)?;
            if repaired_doc != doc_content {
                return Ok(RepairOutcome::TemplateNormalized);
            }
        }
        return repair_completed_backlog_items(file);
    }

    let pending_response = if pending_path.exists() {
        Some(std::fs::read_to_string(&pending_path).with_context(|| {
            format!("failed to read pending response {}", pending_path.display())
        })?)
    } else {
        None
    };
    let response = capture
        .as_ref()
        .map(|r| r.response_body.clone())
        .or_else(|| historical_capture.as_ref().map(|r| r.response_body.clone()))
        .or_else(|| visible_response_recovery.clone())
        .or(pending_response.clone())
        .unwrap_or_default();

    if response.trim().is_empty() {
        // Empty pending file — just clean up
        let _ = std::fs::remove_file(&pending_path);
        let _ = crate::capture::mark_discarded(&canonical);
        return Ok(RepairOutcome::Noop);
    }

    // Dedup guard: check if the response content is already present in the document.
    // This prevents double-apply when the pending file was left behind after a successful
    // IPC write (e.g., IPC timeout path exits with code 75 without calling clear_pending,
    // but the plugin already applied the content via the IPC patch file).
    let response_already_present = response_already_applied(&doc_content, &response)
        || response_already_applied_after_prefix_strip(&doc_content, &response);
    if response_already_present {
        if let Some(ref capture) = capture {
            crate::capture::validate_replay(&canonical, capture)?;
        }
        eprintln!(
            "[repair] Response already present in document — skipping apply, cleaning up pending file"
        );
        let repaired_doc = repair_template_doc_if_needed(file, &doc_content, Some(&response))?;
        let state_is_open = crate::cycle_state::load(file)?
            .map(|state| state.is_open())
            .unwrap_or(true);
        let snapshot_missing_response = snapshot::load(file)?
            .as_deref()
            .map(|snapshot_doc| {
                !response_already_applied(snapshot_doc, &response)
                    && !response_already_applied_after_prefix_strip(snapshot_doc, &response)
            })
            .unwrap_or(true);
        if (state_is_open || visible_response_recovery.is_some()) && snapshot_missing_response {
            snapshot::save(file, &repaired_doc)?;
            crate::ops_log::log_op(
                file,
                &format!(
                    "repair_adopt_existing_response file={} reason=snapshot_missing_response",
                    file.display()
                ),
            );
            eprintln!(
                "[repair] advanced snapshot to the already-present response for {}",
                file.display()
            );
        }
        if state_is_open
            && let Err(e) = crate::cycle_state::mark_write_applied(
                file,
                "repair_already_applied",
                Some(&repaired_doc),
                Some(&repaired_doc),
            )
        {
            eprintln!("[repair] cycle-state update failed: {} (non-fatal)", e);
        }
        clear_pending(&canonical)?;
        return Ok(RepairOutcome::AlreadyApplied);
    }

    if let Some(ref capture) = capture {
        if respect_manual_exchange_tail_removal_if_safe(&canonical, &doc_content, capture)? {
            return Ok(RepairOutcome::ManualTailRemovalRespected);
        }
        if retire_wedged_write_applied_capture_if_drifted(&canonical, &doc_content, capture)? {
            return Ok(RepairOutcome::StaleCaptureRetired);
        }
        if retire_superseded_captured_only_orphan_if_drifted(&canonical, &doc_content, capture)? {
            return Ok(RepairOutcome::StaleCaptureRetired);
        }
        crate::capture::validate_replay(&canonical, capture)?;
    }

    if replay_crdt_patchback_through_strict_write(
        file,
        &doc_content,
        &response,
        queue_completion_ids,
    )? {
        return Ok(RepairOutcome::ReplayedResponse);
    }

    eprintln!(
        "[repair] Found orphaned response for {} ({} bytes). Applying...",
        file.display(),
        response.len()
    );

    let (fm, _) = frontmatter::parse(&doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    let use_template_write = fm.resolve_mode().is_template() || response.contains("<!-- patch:");
    let response_to_write = if use_template_write {
        match agent_doc_template::replay_guard::classify_replay_payload(&response) {
            agent_doc_template::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
                fail_closed_on_blocked_template_replay(file, &response, &reason)?;
                response.clone()
            }
            agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
                response.into_owned()
            }
            agent_doc_template::replay_guard::ReplayPayloadClassification::Empty => {
                response.clone()
            }
        }
    } else {
        response.clone()
    };
    if use_template_write {
        if crate::git::is_in_git_repo(file) {
            replay_orphaned_response_through_strict_write(
                file,
                &response_to_write,
                true,
                queue_completion_ids,
            )?;
        } else {
            write::apply_template_from_string_with_options(
                file,
                &response_to_write,
                write::TemplateApplyOptions {
                    force_disk: repair_replay_force_disk(file),
                },
            )?;
        }
    } else {
        if crate::git::is_in_git_repo(file) {
            replay_orphaned_response_through_strict_write(
                file,
                &response_to_write,
                false,
                queue_completion_ids,
            )?;
        } else {
            write::apply_append_from_string(file, &response_to_write)?;
        }
    }

    let final_doc_after_write = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read recovered document {}", file.display()))?;
    ensure_repair_materialized_response(file, &final_doc_after_write, &response_to_write)?;

    // Remove the pending file only after the repaired document proves the
    // captured response materialized. A malformed/partial replay must leave the
    // capture available for retry instead of closing a false-success cycle.
    clear_pending(&canonical)?;

    // #repair-strike-consumed-head: finalize strikes the consumed queue head, but
    // repair's recovery path historically left it live. `do [#id]` heads are
    // struck by preflight's reap path once their backlog item is done; a
    // free-text head has no backlog id to reap, so a recovered free-text-head
    // response leaves the head unstruck and preflight re-presents it. Strike it
    // here via a guard-skipping consume — repair already wrote the response
    // straight to disk, so the matching strike must bypass the visible-write
    // guard a live IDE buffer would otherwise trip. Best-effort: never fail the
    // recovery on the strike.
    strike_recovered_free_text_queue_head(file);

    eprintln!(
        "[repair] Response repaired and written to {}",
        file.display()
    );
    let final_doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read recovered document {}", file.display()))?;
    if let Err(e) = crate::cycle_state::mark_write_applied(
        file,
        "repair_applied",
        Some(&final_doc),
        Some(&final_doc),
    ) {
        eprintln!("[repair] cycle-state update failed: {} (non-fatal)", e);
    }
    if historical_capture.is_none()
        && let Err(e) = crate::capture::mark_replayed(&canonical)
    {
        eprintln!("[repair] capture-state update failed: {} (non-fatal)", e);
    }
    Ok(RepairOutcome::ReplayedResponse)
}

fn ensure_repair_materialized_response(file: &Path, final_doc: &str, response: &str) -> Result<()> {
    if crate::write::response_materialized_in_content(response, final_doc) {
        return Ok(());
    }
    anyhow::bail!(
        "orphaned response replay did not materialize captured response in {}; refusing to clear capture",
        file.display()
    )
}

/// Strike the active queue head after a repaired response IF it is a free-text
/// head (`#repair-strike-consumed-head`). Scoped to free-text heads only: `do
/// [#id]` heads are struck by preflight's reap path once their backlog item is
/// resolved, and striking one here without resolving its id would desync the
/// head from its still-open backlog item. Best-effort — logs but never fails the
/// recovery.
fn strike_recovered_free_text_queue_head(file: &Path) {
    let Ok(content) = std::fs::read_to_string(file) else {
        return;
    };
    let Ok((fm, _)) = frontmatter::parse(&content) else {
        return;
    };
    if fm.queue_active != Some(true) {
        return;
    }
    if !first_queue_head_is_free_text(&content) {
        return;
    }
    match crate::write::consume_queue_prompt_force_disk(file) {
        Ok(Some(outcome)) => eprintln!(
            "[repair] struck consumed free-text queue head (remaining: {})",
            outcome.remaining
        ),
        Ok(None) => {}
        Err(e) => eprintln!("[repair] queue-head strike after replay failed: {e} (non-fatal)"),
    }
}

/// True when the first prompt in the document's `agent:queue` is a free-text
/// head — i.e. NOT an id-backed directive (`#id`, `[#id]`, `do [#id]`, with or
/// without a leading `:pushpin:` / `:round_pushpin:` / bullet pin marker).
///
/// `#qmisstrike`: the free-text strike repair may consume (strike) a head only
/// when it is genuinely free text. An id-backed head is struck solely via the
/// `--done` / `--pending-gate` / `queue consume` reap paths, never by this
/// positional free-text heuristic. Delegate to the authoritative classifier
/// (`queue_head_is_free_text_prompt`) so this guard agrees with the finalize
/// path. The previous brittle `do [#` / `do #` prefix check mis-classified a
/// pin-prefixed (`:pushpin: [#id]`) or bare-`[#id]` head as free text, so after
/// the operator deleted struck items (or a free-text head was consumed) the
/// repair wrongly struck the next open id-backed head by position.
fn first_queue_head_is_free_text(content: &str) -> bool {
    agent_doc_queue::queue_response::queue_head_is_free_text_prompt(content).unwrap_or(false)
}

fn repair_replay_command_options(
    file: &Path,
    is_template: bool,
    is_stream: bool,
    force_disk: bool,
    queue_completion_ids: &[String],
) -> write::CommandOptions {
    write::CommandOptions {
        file: file.to_path_buf(),
        baseline_file: None,
        is_template,
        is_stream,
        is_ipc: false,
        force_disk,
        origin: Some("repair_replay".to_string()),
        pending_add: Vec::new(),
        pending_add_to: Vec::new(),
        pending_add_gated: Vec::new(),
        pending_add_after: Vec::new(),
        pending_add_before: Vec::new(),
        pending_add_back: Vec::new(),
        icebox_add: Vec::new(),
        icebox_add_after: Vec::new(),
        icebox_add_before: Vec::new(),
        icebox_add_back: Vec::new(),
        icebox_edit: Vec::new(),
        icebox_clear: false,
        icebox_reorder: None,
        pending_done: Vec::new(),
        pending_edit: Vec::new(),
        pending_clear: false,
        pending_reorder: None,
        pending_gate: Vec::new(),
        pending_ungate: Vec::new(),
        pending_resolve_gate: Vec::new(),
        pending_set_gate_type: Vec::new(),
        pending_set_verify: Vec::new(),
        review_add: Vec::new(),
        review_edit: Vec::new(),
        review_remove: Vec::new(),
        review_resolve: Vec::new(),
        queue_completion_ids: queue_completion_ids.to_vec(),
        allow_replace_pending: false,
        pending_only: false,
        status: None,
        lint_override: None,
        commit_sibling: Vec::new(),
        commit_sibling_message: Vec::new(),
    }
}

fn replay_orphaned_response_through_strict_write(
    file: &Path,
    response: &str,
    is_template: bool,
    queue_completion_ids: &[String],
) -> Result<()> {
    let force_disk = repair_replay_force_disk(file);
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_replay_via_strict_write file={} mode={} force_disk={} response_hash={}",
            file.display(),
            if is_template { "template" } else { "append" },
            force_disk,
            agent_doc_hash::content_hash(response)
        ),
    );
    write::run_command_with_response(
        repair_replay_command_options(file, is_template, false, force_disk, queue_completion_ids),
        repair_replay_commit_mode(file),
        response.to_string(),
    )
}

fn repair_replay_force_disk(file: &Path) -> bool {
    !crate::crdt_authority::authority_for_file(&file.display().to_string()).editor_attached()
}

fn repair_replay_commit_mode(file: &Path) -> write::CommitMode {
    if crate::git::is_in_git_repo(file) {
        write::CommitMode::Required
    } else {
        write::CommitMode::None
    }
}

fn replay_crdt_patchback_through_strict_write(
    file: &Path,
    doc_content: &str,
    response: &str,
    queue_completion_ids: &[String],
) -> Result<bool> {
    if !response.contains("<!-- patch:") {
        return Ok(false);
    }
    if !crate::git::is_in_git_repo(file) {
        return Ok(false);
    }
    let (fm, _) = frontmatter::parse(doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    if !fm.resolve_mode().is_crdt() {
        return Ok(false);
    }

    let force_disk = repair_replay_force_disk(file);
    crate::ops_log::log_op(
        file,
        &format!(
            "repair_replay_via_strict_write file={} mode=crdt force_disk={} response_hash={}",
            file.display(),
            force_disk,
            agent_doc_hash::content_hash(response)
        ),
    );
    eprintln!(
        "[repair] replaying captured CRDT patchback through strict write closeout for {}",
        file.display()
    );

    write::run_command_with_response(
        repair_replay_command_options(file, false, true, force_disk, queue_completion_ids),
        repair_replay_commit_mode(file),
        response.to_string(),
    )?;
    Ok(true)
}

pub fn repair(file: &Path) -> Result<RepairOutcome> {
    let outcome = run(file)?;
    if outcome.repaired()
        && outcome != RepairOutcome::StalePreflightCycleAbandoned
        && crate::git::is_in_git_repo(file)
    {
        crate::write::complete_required_closeout(file)?;
    } else if !outcome.repaired()
        && let crate::session_check::SessionCheckStatus::Interrupted(message) =
            crate::session_check::inspect(file)?
    {
        crate::flow::closeout::log_closeout_guard_event(
            file,
            crate::flow::types::FlowStage::SessionCheck,
            crate::flow::types::FlowOutcome::FailedClosed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::SessionCheckInterrupted,
        );
        anyhow::bail!(message);
    }
    Ok(outcome)
}

/// Returns true if the pending response content appears to already be applied to the document.
///
/// Checks whether the document contains the response's normalized visible lines
/// as one contiguous block. This tolerates blank-line separation and transient
/// ` (HEAD)` suffixes on response headings without treating scattered matching
/// phrases elsewhere in the document as an already-applied replay.
pub fn response_already_applied(doc: &str, response: &str) -> bool {
    let response_lines = normalized_response_lines(response);
    if response_lines.is_empty() {
        return false;
    }
    let doc_lines = normalized_response_lines(doc);
    doc_lines
        .windows(response_lines.len())
        .any(|window| window == response_lines.as_slice())
}

/// Phase 2/3 of `#adoc-prefix-strip-uncommitted`: accepts the response as
/// applied when the captured `response` had spurious leading `❯ ` markers
/// (e.g. JB cache-conflict applied prompt-prefix markers to agent response
/// body lines) that the user has since stripped from the document. Compares
/// the response's normalized lines against the document after also stripping
/// a single leading `❯ ` from response lines. Returns true on a strict
/// contiguous match — non-prefixed response lines therefore behave the same
/// as `response_already_applied`.
pub fn response_already_applied_after_prefix_strip(doc: &str, response: &str) -> bool {
    let response_lines: Vec<String> = response
        .lines()
        .filter_map(normalize_response_line)
        .map(|line| {
            let trimmed = line.trim_start();
            if let Some(stripped) = trimmed.strip_prefix("❯ ") {
                let indent_len = line.len() - trimmed.len();
                format!("{}{}", &line[..indent_len], stripped)
            } else {
                line
            }
        })
        .collect();
    if response_lines.is_empty() {
        return false;
    }
    let doc_lines = normalized_response_lines(doc);
    doc_lines
        .windows(response_lines.len())
        .any(|window| window == response_lines.as_slice())
}

fn normalized_response_lines(content: &str) -> Vec<String> {
    // #stuck-capture-queue-echo-false-positive: skip the binary-inserted
    // `> **Queue prompt:**` echo blockquote (#queue-prompt-echo-in-response).
    // The echo is decoration written between the response heading and body at
    // queue-consume time — it is never part of the agent's captured response —
    // so leaving it in would break the contiguous-block match in
    // `response_already_applied` and make `response_materialized_in_content`
    // (and therefore `stuck_captured_cycle`) report a committed response as
    // missing from HEAD. Stripping it on both the response and document side
    // keeps "is this response already applied" detection accurate.
    let mut out = Vec::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "> **Queue prompt:**" {
            while let Some(next) = lines.peek() {
                if next.trim_start().starts_with('>') {
                    lines.next();
                } else {
                    break;
                }
            }
            continue;
        }
        if let Some(normalized) = normalize_response_line(line) {
            out.push(normalized);
        }
    }
    out
}

fn normalize_response_line(line: &str) -> Option<String> {
    let raw = line.trim_end_matches('\r');
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!-- patch:")
        || trimmed.starts_with("<!-- /patch:")
        || trimmed.starts_with("<!-- agent:")
        || trimmed.starts_with("<!-- /agent:")
    {
        return None;
    }
    Some(strip_transient_response_head_marker(raw))
}

fn strip_transient_response_head_marker(line: &str) -> String {
    if let Some(stripped) = line.strip_suffix(" (HEAD)") {
        let trimmed = stripped.trim_start();
        let is_re_heading = trimmed.starts_with("### Re:");
        let is_bold_re_heading = trimmed.starts_with("**Re:") && trimmed.ends_with("**");
        if is_re_heading || is_bold_re_heading {
            return stripped.to_string();
        }
    }
    line.to_string()
}

/// Save a response to the pending store before attempting write-back.
/// This makes the response durable across context compaction.
pub fn save_pending(file: &Path, response: &str) -> Result<()> {
    let response = write::canonicalize_response_for_capture(file, response)?;
    crate::capture::capture_response(file, &response)?;
    let pending_path = snapshot::pending_path_for(file)?;
    if let Some(parent) = pending_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pending_path, &response)
        .with_context(|| format!("failed to save pending response {}", pending_path.display()))?;
    Ok(())
}

/// Remove the pending file after a successful write-back.
pub fn clear_pending(file: &Path) -> Result<()> {
    let pending_path = snapshot::pending_path_for(file)?;
    if pending_path.exists() {
        std::fs::remove_file(&pending_path)?;
    }
    // Also clean up the pre-response snapshot (saved before write for undo support).
    // Without this, pre-response files accumulate indefinitely after successful writes.
    if let Err(e) = snapshot::delete_pre_response(file) {
        eprintln!("[repair] warning: failed to delete pre-response: {}", e);
    }
    if let Err(e) = crate::capture::mark_write_applied(file) {
        eprintln!("[repair] warning: failed to update capture state: {}", e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command as ProcessCommand;
    use std::time::Duration;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        dir
    }

    fn init_git_repo(root: &Path, tracked: &Path) {
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["init"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", tracked.file_name().unwrap().to_str().unwrap()])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .status()
            .unwrap();
    }

    fn age_cycle_state(file: &Path, age_secs: u64) {
        let canonical = file.canonicalize().unwrap();
        let root = agent_doc_fs::find_project_root(&canonical).unwrap();
        let hash = crate::snapshot::doc_hash(&canonical).unwrap();
        let path = root
            .join(".agent-doc/state/cycles")
            .join(format!("{hash}.json"));
        let mut state: crate::cycle_state::CycleState =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        state.started_at = state.started_at.saturating_sub(age_secs);
        state.updated_at = state.updated_at.saturating_sub(age_secs);
        std::fs::write(path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    }

    #[test]
    fn no_pending_returns_false() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "# Doc\n\n## User\n\nHello\n").unwrap();
        assert_eq!(run(&doc).unwrap(), RepairOutcome::Noop);
    }

    #[test]
    fn first_queue_head_free_text_check_excludes_id_backed_head() {
        // #qmisstrike: the free-text strike repair must classify every id-backed
        // head spelling as NOT free text so it never strikes the next open
        // id-backed head by position. The previous `do [#`/`do #` prefix check
        // missed the bare `[#id]` and pin-prefixed `:pushpin: [#id]` spellings,
        // which the operator's queues use after manual edits / pin promotion.
        let id_backed_heads = [
            "- do [#qchurn]\n",
            "- [#qchurn]\n",
            "- :pushpin: [#qchurn]\n",
            "- :round_pushpin: [#qchurn]\n",
            "- :pushpin: do [#qchurn]\n",
            "- #qchurn\n",
        ];
        for head in id_backed_heads {
            let doc = format!(
                concat!(
                    "---\nqueue_active: true\n---\n\n",
                    "<!-- agent:queue auto -->\n",
                    "{}",
                    "- do [#tail]\n",
                    "<!-- /agent:queue -->\n",
                ),
                head
            );
            assert!(
                !first_queue_head_is_free_text(&doc),
                "id-backed head {head:?} must not be classified as free text (would be wrongly struck)"
            );
        }

        // A genuine free-text head is still recognized so the repair can strike
        // the head this turn actually answered.
        let free_text = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Is the parser masking fenced regions correctly?\n",
            "- do [#tail]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            first_queue_head_is_free_text(free_text),
            "a no-#id queue head is free text and strikeable by being answered"
        );

        // Inactive queue → nothing to strike.
        let inactive = free_text.replace("queue_active: true", "queue_active: false");
        assert!(!first_queue_head_is_free_text(&inactive));
    }

    #[test]
    fn response_already_applied_tolerates_queue_prompt_echo_between_heading_and_body() {
        // #stuck-capture-queue-echo-false-positive: the captured response is the
        // raw heading+body. At queue-consume time the binary inserts a
        // `> **Queue prompt:**` echo blockquote between the heading and the body
        // (#queue-prompt-echo-in-response). The contiguous-block match must still
        // recognize the response as applied, otherwise stuck_captured_cycle fires
        // a false positive after every queue-consumed cycle.
        let captured_response = "### Re: do [#thing] — opus-4-8\n\nShipped the fix.\n";
        let materialized_doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do [#thing] — opus-4-8\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> do [#thing]\n\n",
            "Shipped the fix.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(
            response_already_applied(materialized_doc, captured_response),
            "queue-prompt echo between heading and body must not defeat the applied-response match"
        );

        // A genuinely absent response must still report not-applied.
        let other_doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do [#other] — opus-4-8\n\nUnrelated.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(
            !response_already_applied(other_doc, captured_response),
            "an unrelated document must not match the captured response"
        );
    }

    #[test]
    fn repair_materialization_requires_captured_response_block() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: build and install status — gpt-5\n\n",
            "- Reinstalled the CLI from this checkout.\n",
            "<!-- /patch:exchange -->\n",
        );
        let malformed = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Verification:\n",
            "- Reinstalled the CLI from this checkout.\n",
            "<!-- /agent:exchange -->\n",
        );

        let err = ensure_repair_materialized_response(
            std::path::Path::new("session.md"),
            malformed,
            response,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("orphaned response replay did not materialize captured response"),
            "malformed body-only materialization must fail closed: {err}"
        );
    }

    #[test]
    fn cancel_preflight_cycle_abandons_empty_preflight_immediately() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "# Doc\n\n## User\n\nDo the thing\n";
        std::fs::write(&doc, content).unwrap();
        // Fresh empty preflight_started cycle (no capture), age irrelevant.
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        assert_eq!(
            cancel_preflight_cycle(&doc).unwrap(),
            CancelOutcome::Abandoned
        );
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Abandoned);
    }

    #[test]
    fn cancel_preflight_cycle_protects_cycle_with_capture() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "# Doc\n\n## User\n\nDo the thing\n";
        std::fs::write(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        // A response capture exists for this cycle → cancel must not discard it.
        crate::capture::capture_response(&doc, "### Re: do — opus-4-8\n\nDone.\n").unwrap();

        assert_eq!(
            cancel_preflight_cycle(&doc).unwrap(),
            CancelOutcome::Protected
        );
        assert!(
            crate::cycle_state::load(&doc).unwrap().unwrap().is_open(),
            "a cycle that owns a capture must stay open after cancel"
        );
    }

    #[test]
    fn cancel_preflight_cycle_protects_advanced_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "# Doc\n\n## User\n\nDo the thing\n";
        std::fs::write(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_write_applied(&doc, "write_applied", Some(content), Some(content))
            .unwrap();

        assert_eq!(
            cancel_preflight_cycle(&doc).unwrap(),
            CancelOutcome::Protected
        );
    }

    #[test]
    fn cancel_preflight_cycle_noop_without_open_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "# Doc\n\nNothing\n").unwrap();
        assert_eq!(
            cancel_preflight_cycle(&doc).unwrap(),
            CancelOutcome::NoOpenCycle
        );
    }

    #[test]
    fn repair_repositions_stale_boundary_after_answered_turn() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:keep-this-id -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:keep-this-id -->\n",
            "Can we run specific rubrics for fine tuning?\n",
            "### Re: specific rubrics — gpt-5\n\n",
            "Yes.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::TemplateNormalized);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("Can we run specific rubrics for fine tuning?"));
        assert!(repaired.contains("### Re: specific rubrics — gpt-5"));
        assert!(
            repaired
                .contains("Yes.\n<!-- agent:boundary:keep-this-id -->\n<!-- /agent:exchange -->"),
            "boundary should move to the true end of the answered turn:\n{repaired}"
        );

        let repaired_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(repaired_snapshot, repaired);
    }

    #[test]
    fn repair_reorders_response_before_prompt_tail_when_pending_response_is_visible() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let response = "### Re: timeout fallback — gpt-5\n\nDone.\n";
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please handle the timeout fallback.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please handle the timeout fallback.\n",
            "### Re: timeout fallback — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:new -->\n",
            "Can you preserve the second paragraph too?\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();
        save_pending(&doc, response).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        let prompt_tail = repaired
            .find("Can you preserve the second paragraph too?")
            .unwrap();
        let response_heading = repaired.find("### Re: timeout fallback").unwrap();
        let boundary = repaired.find("<!-- agent:boundary:").unwrap();
        let close = repaired.find("<!-- /agent:exchange -->").unwrap();
        assert!(
            prompt_tail < response_heading,
            "repair should move prompt tail before response:\n{repaired}"
        );
        assert!(
            response_heading < boundary && boundary < close,
            "boundary should close the repaired response turn:\n{repaired}"
        );
    }

    #[test]
    fn repair_does_not_move_boundary_past_unanswered_prompt() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:keep-this-id -->\n",
            "❯ Can we run specific rubrics for fine tuning?\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::Noop);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }

    #[test]
    fn save_and_clear_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();

        save_pending(&doc, "response text").unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(pending.exists());

        clear_pending(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn repair_reaps_completed_backlog_without_pending_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#aaaa] keep\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("- [ ] [#aaaa] keep"));
        assert!(!repaired.contains("- [x] [#bbbb] drop"));
        assert!(repaired.contains("[#bbbb] drop"));

        let repaired_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert!(repaired_snapshot.contains("- [ ] [#aaaa] keep"));
        assert!(!repaired_snapshot.contains("- [x] [#bbbb] drop"));
        assert!(repaired_snapshot.contains("[#bbbb] drop"));
    }

    #[test]
    fn repair_backfills_legacy_done_ids_before_reaping_completed_backlog() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "- [x] legacy drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        let pending_body = repaired
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .expect("pending component");
        assert!(
            repaired.contains("- [ ] [#"),
            "open legacy item should be backfilled: {repaired}"
        );
        assert!(repaired.contains("keep"));
        assert!(!pending_body.contains("legacy drop"));
        assert!(repaired.contains("legacy drop"));

        let repaired_snapshot = snapshot::load(&doc).unwrap().unwrap();
        let snapshot_pending_body = repaired_snapshot
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .expect("snapshot pending component");
        assert!(repaired_snapshot.contains("- [ ] [#"));
        assert!(!snapshot_pending_body.contains("legacy drop"));
        assert!(repaired_snapshot.contains("legacy drop"));
        assert!(repaired_snapshot.contains("agent:done"));
    }

    #[test]
    fn repair_commits_reaped_completed_backlog_in_git_repo() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#aaaa] keep\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(dir.path(), &doc);

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(_) => {}
            other => panic!("expected clean closeout after repair, got {other:?}"),
        }

        let head = ProcessCommand::new("git")
            .current_dir(dir.path())
            .args(["show", "HEAD:test.md"])
            .output()
            .unwrap();
        let head_text = String::from_utf8_lossy(&head.stdout);
        assert!(head_text.contains("- [ ] [#aaaa] keep"));
        assert!(!head_text.contains("- [x] [#bbbb] drop"));
        assert!(head_text.contains("[#bbbb] drop"));
    }

    #[test]
    fn repair_completed_backlog_reap_preserves_live_prompt_outside_snapshot() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        let live_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "do #statusws. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, live_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("do #statusws. spec-test-build-install-commit-push"));
        assert!(!repaired.contains("- [x] [#bbbb] drop"));

        let repaired_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !repaired_snapshot.contains("do #statusws. spec-test-build-install-commit-push"),
            "snapshot must not absorb the live prompt"
        );
        assert!(!repaired_snapshot.contains("- [x] [#bbbb] drop"));

        let diff = crate::diff_io::compute(&doc).unwrap().unwrap();
        assert!(diff.contains("do #statusws. spec-test-build-install-commit-push"));
    }

    #[test]
    fn recover_append_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\nagent_doc_format: append\nagent_doc_write: merge\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();

        // Save a pending response
        save_pending(&doc, "This is the recovered response.").unwrap();

        // Recover it
        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        // Verify the response was written
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("This is the recovered response."));
        assert!(result.contains("## Assistant"));

        // Pending file should be cleaned up
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn repair_strikes_consumed_free_text_queue_head() {
        // #repair-strike-consumed-head: a recovered free-text-head response must
        // strike its queue head (finalize does; repair historically left it live,
        // so preflight re-presented the answered head).
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- improve the docs please\n",
            "- a second queued item\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(
            &doc,
            "<!-- patch:exchange -->\n### Re: improve the docs — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n",
        )
        .unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        // The answered head is struck in place; the next item stays live.
        assert!(
            result.contains("~improve the docs please~"),
            "free-text queue head must be struck after repair replay:\n{result}"
        );
        assert!(
            result.contains("- a second queued item"),
            "the next queue item must remain live:\n{result}"
        );
    }

    #[test]
    fn repair_replay_preserves_response_leading_code_fence_after_prompt_fence() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ show fenced prompt\n",
            "```\n",
            "prompt body\n",
            "```\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(
            &doc,
            "<!-- patch:exchange -->\n```\nresponse body\n```\n<!-- /patch:exchange -->\n",
        )
        .unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        let exchange = agent_doc_element::element::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();
        assert_eq!(
            exchange.matches("```").count(),
            4,
            "repair replay must preserve prompt and response fences:\n{exchange}"
        );
        assert!(
            exchange.contains("```\n```\nresponse body\n```"),
            "repair replay stripped the response opening fence:\n{exchange}"
        );
    }

    #[test]
    fn repair_leaves_do_id_queue_head_for_reap_path() {
        // do[#id] heads are struck by preflight's reap path once their backlog
        // item resolves; the repair strike must NOT touch them, or the head
        // desyncs from its still-open backlog id.
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#widget]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(
            &doc,
            "<!-- patch:exchange -->\n### Re: do [#widget] — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n",
        )
        .unwrap();

        run(&doc).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- do [#widget]") && !result.contains("~do [#widget]~"),
            "do[#id] head must remain for the reap path:\n{result}"
        );
    }

    #[test]
    fn recover_plain_response_uses_template_path_for_template_docs() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "## User\n",
            "compact exchange\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(
            &doc,
            "Exchange compacted. No new work was run in this turn.",
        )
        .unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = result.find("<!-- /agent:exchange -->").unwrap();
        let summary = result
            .find("Exchange compacted. No new work was run in this turn.")
            .unwrap();
        assert!(
            summary < exchange_close,
            "plain recovery for template docs should stay inside exchange:\n{result}"
        );
        assert!(
            !result[exchange_close..].contains("## Assistant"),
            "template recovery must not append inline assistant blocks after exchange:\n{result}"
        );
    }

    #[test]
    fn recover_normalizes_captured_replace_pending_patch() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#aaaa] existing\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — gpt-5\n\n",
            "Recovered.\n",
            "<!-- /patch:exchange -->\n",
            "<!-- replace:pending -->\n",
            "- [x] [#aaaa] existing\n",
            "- [ ] [#bbbb] add regression coverage\n",
            "<!-- /replace:pending -->\n"
        );
        save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("### Re: topic — gpt-5"));
        assert!(result.contains("- [x] [#aaaa] existing"));
        assert!(result.contains("- [ ] [#bbbb] add regression coverage"));
        assert!(!result.contains("replace:pending"));
    }

    #[test]
    fn empty_pending_cleaned_up() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();

        save_pending(&doc, "").unwrap();
        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::Noop);

        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn recover_skips_duplicate_apply() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        // Document already contains the response content (as if IPC applied it)
        let response = "This is the response that was already applied.\nSecond line.\nThird line.";
        let content = format!(
            "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\n{}\n\n## User\n\n",
            response
        );
        std::fs::write(&doc, &content).unwrap();

        // Pending file still exists (clear_pending was never called after IPC write)
        save_pending(&doc, response).unwrap();

        // run should detect the content is already present and skip
        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::AlreadyApplied);

        // Document should be unchanged
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, content);

        // Pending file should be cleaned up
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn recover_replays_capture_without_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\nagent_doc_format: append\nagent_doc_write: merge\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(dir.path(), &doc);

        save_pending(&doc, "Recovered from capture.").unwrap();
        clear_pending(&doc).unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
        // Re-arm capture as if the write never happened.
        crate::capture::capture_response(&doc, "Recovered from capture.").unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("Recovered from capture."));
    }

    #[test]
    fn recover_already_applied_template_canonicalizes_prompt_prefixes() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Prior question?\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let response =
            "<!-- patch:exchange -->\n### Re: topic — gpt-5\n\nBody\n<!-- /patch:exchange -->";
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Prior question?\n",
            "Why was this missed?\n",
            "### Re: topic — gpt-5\n\n",
            "Body\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, snapshot).unwrap();

        save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::AlreadyApplied);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(
            repaired.contains("❯ Why was this missed?"),
            "repair should restore the missing prompt prefix:\n{repaired}"
        );
        assert!(
            !repaired.contains("\nWhy was this missed?\n"),
            "bare prompt target should not remain after repair:\n{repaired}"
        );

        let saved_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            saved_snapshot, repaired,
            "snapshot should advance to the canonicalized repaired document"
        );
    }

    #[test]
    fn recover_already_applied_template_keeps_response_body_unprefixed() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do the repair.\n",
            "### Re: repair — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: repair — gpt-5\n\n",
            "First response paragraph.\n\n",
            "Second response paragraph.\n",
            "- Proof line.\n",
            "<!-- /patch:exchange -->"
        );
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do the repair.\n",
            "### Re: repair — gpt-5\n\n",
            "First response paragraph.\n\n",
            "Second response paragraph.\n",
            "- Proof line.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, snapshot).unwrap();
        save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::AlreadyApplied);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("\nFirst response paragraph.\n"));
        assert!(repaired.contains("\nSecond response paragraph.\n- Proof line.\n"));
        assert!(
            !repaired.contains("❯ First response paragraph.")
                && !repaired.contains("❯ Second response paragraph.")
                && !repaired.contains("❯ - Proof line."),
            "already-applied response body lines must not be prompt-prefixed:\n{repaired}"
        );

        let saved_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(saved_snapshot, repaired);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("repair_response_body_prompt_prefix_stripped"));
    }

    #[test]
    fn repair_without_pending_strips_response_body_prompt_prefixes() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do the repair.\n",
            "### Re: repair — gpt-5\n\n",
            "Response intro.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do the repair.\n",
            "### Re: repair — gpt-5\n\n",
            "Response intro.\n\n",
            "Verification passed:\n",
            "❯ - `make check`\n",
            "❯ - `agent-doc write --commit`\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, snapshot).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::TemplateNormalized);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("\nVerification passed:\n"));
        assert!(repaired.contains("\n- `make check`\n- `agent-doc write --commit`\n"));
        assert!(
            !repaired.contains("❯ - `make check`")
                && !repaired.contains("❯ - `agent-doc write --commit`"),
            "no-pending response tails must not remain prompt-prefixed:\n{repaired}"
        );

        let saved_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(saved_snapshot, repaired);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("repair_response_body_prompt_prefix_stripped"));
    }

    #[test]
    fn repair_without_pending_canonicalizes_bare_prompt_before_existing_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "Why was this missed?\n",
            "### Re: topic — gpt-5\n\n",
            "Body\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, snapshot).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::TemplateNormalized);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(
            repaired.contains("❯ Why was this missed?"),
            "repair should restore the missing prompt prefix even without pending replay:\n{repaired}"
        );
        assert!(
            !repaired.contains("\nWhy was this missed?\n"),
            "bare prompt target should not remain after repair:\n{repaired}"
        );

        let saved_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            saved_snapshot, repaired,
            "snapshot should advance to the canonicalized repaired document"
        );
    }

    #[test]
    fn recover_fails_closed_on_capture_hash_mismatch() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(&doc, "Recovered from capture.").unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        std::fs::remove_file(&pending).unwrap();
        std::fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello again\n").unwrap();

        let err = run(&doc).unwrap_err();
        assert!(
            err.to_string().contains("baseline no longer matches"),
            "unexpected error: {err}"
        );
    }

    // `#stale-capture-deadlock-autoretire`: a wedged WRITE-APPLIED capture whose
    // response vanished from the document and whose baseline drifted must be
    // retired non-destructively (rebuild sidecars from current, preserve the
    // captured body as Discarded) instead of fail-closing with "captured
    // response baseline no longer matches current document" — which otherwise
    // deadlocks every later commit / write --commit / route closeout drain
    // behind a manual `reset --from-current --preserve-session`.
    #[test]
    fn retires_wedged_write_applied_capture_on_baseline_drift() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let v1 = concat!(
            "---\nagent_doc_format: template\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — opus-4-8\n\n",
            "Prior answer.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do something new\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, v1).unwrap();
        snapshot::save(&doc, v1).unwrap();

        // A response that was captured + write-applied but never landed
        // contiguously in the document (the CRDT-intermix / concurrent-edit
        // class). It is absent from both v1 and the drifted v2.
        let lost_response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: do something new — opus-4-8\n\n",
            "The new answer that got lost.\n",
            "<!-- /patch:exchange -->",
        );
        crate::capture::capture_response(&doc, lost_response).unwrap();
        crate::capture::mark_write_applied(&doc).unwrap();
        crate::cycle_state::mark_write_applied(&doc, "write_applied", Some(v1), Some(v1)).unwrap();

        // Concurrent user edit drifts the live file off the captured baseline.
        let v2 = v1.replace(
            "- do something new\n",
            "- do something new\n- another unrelated edit\n",
        );
        std::fs::write(&doc, &v2).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::StaleCaptureRetired,
            "wedged write-applied capture on baseline drift must be retired, not fail-closed"
        );

        // Document is left as the user's current edit — the lost response is NOT
        // replayed onto the drifted baseline (that would duplicate/reorder).
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, v2, "current document must be preserved verbatim");
        assert!(
            !result.contains("The new answer that got lost"),
            "stale captured body must not be replayed onto the drifted document:\n{result}"
        );

        // Sidecars rebuilt from current; cycle closed; capture retired (body
        // preserved on disk as Discarded for forensics, not deleted).
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(snap, v2, "snapshot must follow the current document");
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert!(!state.is_open(), "cycle must be closed after retire");
        let capture = crate::capture::load_active(&doc).unwrap().unwrap();
        assert_eq!(capture.state, crate::capture::CaptureState::Discarded);
        assert_eq!(
            capture.response_body, lost_response,
            "captured body must be preserved for forensics"
        );

        // Session-check accepts the recovered state (no open cycle, no drift).
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(_) => {}
            crate::session_check::SessionCheckStatus::Interrupted(msg) => {
                panic!("session-check must accept the retired-capture recovery: {msg}")
            }
        }
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("closeout_recovery_mutation")
                && log.contains("retire_wedged_write_applied_capture"),
            "wedged capture retirement must go through the shared recovery mutation primitive:\n{log}"
        );
    }

    // A `Captured`-only orphan (write never attempted) must STAY on the
    // conservative fail-closed path even when the baseline drifts — the retire
    // path is scoped to `WriteApplied` captures only.
    #[test]
    fn captured_only_orphan_on_drift_still_fails_closed() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let v1 = "---\nagent_doc_format: template\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: prior — opus-4-8\n\nPrior.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, v1).unwrap();
        snapshot::save(&doc, v1).unwrap();

        let lost =
            "<!-- patch:exchange -->\n### Re: new — opus-4-8\n\nLost.\n<!-- /patch:exchange -->";
        crate::capture::capture_response(&doc, lost).unwrap();
        // NOTE: no mark_write_applied — capture stays `Captured`.

        let v2 = v1.replace("Prior.", "Prior, edited.");
        std::fs::write(&doc, &v2).unwrap();

        let err = run(&doc).unwrap_err();
        assert!(
            err.to_string().contains("baseline no longer matches"),
            "captured-only orphan must keep failing closed without superseding evidence: {err}"
        );
    }

    // `#stale-capture-captured-only-drift`: a `Captured`-only orphan whose
    // baseline drifted IS retired (non-destructively) once there is positive
    // superseding-turn evidence — the captured response's `### Re:` heading is
    // already answered in the live exchange, so the never-written body is a stale
    // duplicate, not the only answer.
    #[test]
    fn retires_superseded_captured_only_orphan_on_drift() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        // State when the orphan was captured: the prompt is NOT yet answered.
        let v1 = "---\nagent_doc_format: template\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: prior — opus-4-8\n\nPrior.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, v1).unwrap();
        snapshot::save(&doc, v1).unwrap();

        // Capture a response (never written) answering `### Re: new`.
        let lost = "<!-- patch:exchange -->\n### Re: new — opus-4-8\n\nLost duplicate.\n<!-- /patch:exchange -->";
        let capture = crate::capture::capture_response(&doc, lost).unwrap();
        assert_eq!(capture.state, crate::capture::CaptureState::Captured);

        // A superseding turn answered the SAME prompt with a DIFFERENT body and
        // drifted the live document off the capture's recorded baseline.
        let v2 = "---\nagent_doc_format: template\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: prior — opus-4-8\n\nPrior.\n### Re: new — opus-4-8\n\nThe real landed answer.\n<!-- agent:boundary:def -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, v2).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::StaleCaptureRetired,
            "a Captured-only orphan whose heading is already answered + drifted must be retired"
        );
        // Current document preserved verbatim; the stale body is not replayed.
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, v2, "current document must be preserved verbatim");
        assert!(
            !result.contains("Lost duplicate."),
            "stale captured body must not be replayed:\n{result}"
        );
        // Orphan retired (Discarded); body preserved on disk for forensics.
        let capture = crate::capture::load_active(&doc).unwrap().unwrap();
        assert_eq!(capture.state, crate::capture::CaptureState::Discarded);
        assert_eq!(
            capture.response_body, lost,
            "captured body must be preserved for forensics"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("closeout_recovery_mutation")
                && log.contains("retire_superseded_captured_only_orphan"),
            "superseded captured orphan retirement must go through the shared recovery mutation primitive:\n{log}"
        );
    }

    #[test]
    fn recover_respects_manual_removal_of_escaped_exchange_tail() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let malformed = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "[//]: # (leave this note outside exchange)\n\n",
            "## Assistant\n\n",
            "Escaped answer.\n"
        );
        std::fs::write(&doc, malformed).unwrap();
        snapshot::save(&doc, malformed).unwrap();

        save_pending(&doc, "Escaped answer.").unwrap();

        let repaired = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "[//]: # (leave this note outside exchange)\n"
        );
        std::fs::write(&doc, repaired).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::ManualTailRemovalRespected,
            "manual deletion of the escaped tail should be treated as a repair"
        );

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, repaired);
        assert!(
            !result.contains("## Assistant"),
            "stale assistant tail must not be re-added:\n{result}"
        );

        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists(), "pending file should be cleared");

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(
            state.last_event,
            "repair_respect_manual_exchange_tail_removal"
        );

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(snap, repaired, "snapshot should follow the user repair");

        let capture = crate::capture::load_active(&doc).unwrap().unwrap();
        assert_eq!(capture.state, crate::capture::CaptureState::Discarded);
    }

    #[test]
    fn recover_dedup_with_blank_lines_and_boundary() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        // Response has template patch with content lines
        let response = "<!-- patch:exchange -->\n### Re: topic — opus-4-6\n\n**Details:**\n- Item one\n<!-- /patch:exchange -->";
        // Document has the content with blank lines and (HEAD) boundary suffix
        let content = "---\nsession: test\n---\n\n<!-- agent:exchange -->\n### Re: topic — opus-4-6 (HEAD)\n\n**Details:**\n- Item one\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();

        save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::AlreadyApplied,
            "should detect content as already applied despite (HEAD) suffix and blank lines"
        );

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn dedup_requires_contiguous_normalized_response_block() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — opus-4-6\n",
            "Implemented in `src/agent-doc`.\n",
            "- `cargo test`\n",
            "<!-- /patch:exchange -->\n"
        );
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: topic — opus-4-6 (HEAD)\n",
            "Earlier answer.\n\n",
            "Implemented in `src/agent-doc`.\n\n",
            "Unrelated text.\n",
            "- `cargo test`\n",
            "<!-- /agent:exchange -->\n"
        );

        assert!(
            !response_already_applied(doc, response),
            "scattered matching lines should not trigger dedup"
        );
    }

    #[test]
    fn dedup_short_response_still_requires_contiguous_match() {
        let response = "Implemented.\nDone.\n";
        let doc = "Implemented.\nOther line.\nDone.\n";

        assert!(
            !response_already_applied(doc, response),
            "short responses should not dedup from non-contiguous matches"
        );
    }

    #[test]
    fn recover_repairs_stale_preflight_started_cycle_when_hashes_match() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(
            repaired,
            RepairOutcome::StalePreflightLockRepaired,
            "stale preflight lock should be repaired"
        );
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "repair_preflight_stale_lock");
    }

    #[test]
    fn recover_stale_preflight_cycle_strips_response_body_prompt_prefixes() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do the repair.\n",
            "### Re: repair — gpt-5\n\n",
            "Verification passed:\n",
            "❯ - `make check`\n",
            "❯ - `agent-doc write --commit`\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::TemplateNormalized);

        let doc_after = std::fs::read_to_string(&doc).unwrap();
        assert!(doc_after.contains("\n- `make check`\n- `agent-doc write --commit`\n"));
        assert!(
            !doc_after.contains("❯ - `make check`")
                && !doc_after.contains("❯ - `agent-doc write --commit`"),
            "stale-preflight repair must canonicalize response-owned proof lines:\n{doc_after}"
        );
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("repair_preflight_stale_lock"));
        assert!(log.contains("repair_response_body_prompt_prefix_stripped"));
    }

    #[test]
    fn recover_repairs_stale_empty_preflight_started_cycle_with_frontmatter_only_drift() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(root, &doc);
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let live = base.replace(
            "agent_doc_session: test",
            "agent_doc_session: test\nagent: codex",
        );
        std::fs::write(&doc, &live).unwrap();
        age_cycle_state(&doc, STALE_EMPTY_PREFLIGHT_TTL_SECS + 1);

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::StalePreflightLockRepaired);

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "repair_preflight_stale_empty_cycle");
        assert_eq!(snapshot::load(&doc).unwrap().as_deref(), Some(base));
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), live);
    }

    #[test]
    fn recover_abandons_stale_empty_preflight_started_cycle_with_prompt_drift() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(root, &doc);
        let state = crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let live = base.replace(
            "<!-- agent:boundary:abc123 -->\n",
            "do [#root-empty-preflight]. spec-test-build-install-commit-push\n<!-- agent:boundary:abc123 -->\n",
        );
        std::fs::write(&doc, &live).unwrap();
        age_cycle_state(&doc, STALE_EMPTY_PREFLIGHT_TTL_SECS + 1);

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::StalePreflightCycleAbandoned);

        let after = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(after.phase, agent_doc_turn::CyclePhase::Abandoned);
        assert_eq!(after.cycle_id, state.cycle_id);
        assert_eq!(
            after.last_event,
            "repair_preflight_stale_prompt_cycle_abandoned"
        );
        assert_eq!(snapshot::load(&doc).unwrap().as_deref(), Some(base));
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), live);

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("repair_preflight_stale_prompt_cycle_abandoned file="),
            "abandon event should be logged for diagnostics:\n{log}"
        );
    }

    #[test]
    fn stale_preflight_abandonment_stops_original_partial_checkpoint_writer() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(root, &doc);
        let state = crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let mut writer =
            crate::capture::PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
        assert!(writer.maybe_checkpoint("first partial").unwrap().is_some());

        let live = base.replace(
            "<!-- agent:boundary:abc123 -->\n",
            "do [#staleckpt]. spec-test-build-install-commit-push\n<!-- agent:boundary:abc123 -->\n",
        );
        std::fs::write(&doc, &live).unwrap();
        age_cycle_state(&doc, STALE_EMPTY_PREFLIGHT_TTL_SECS + 1);

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::StalePreflightCycleAbandoned);

        let after = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(after.phase, agent_doc_turn::CyclePhase::Abandoned);
        assert_eq!(after.cycle_id, state.cycle_id);
        assert!(writer.maybe_checkpoint("second partial").unwrap().is_none());

        let loaded = crate::capture::latest_partial_checkpoint(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.response_body, "first partial");
        assert_eq!(loaded.checkpoint_count, 1);

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("repair_preflight_stale_prompt_cycle_abandoned file="),
            "repair abandonment should be logged:\n{log}"
        );
        assert!(
            log.contains("partial_response_checkpoint_stopped"),
            "stale checkpoint writer should stop after repair abandonment:\n{log}"
        );
        assert!(
            log.contains("reason=cycle_closed"),
            "abandoned same-cycle checkpoint stop should be classified as a closed cycle:\n{log}"
        );
    }

    #[test]
    fn recover_fails_closed_on_recent_empty_preflight_started_cycle_with_prompt_drift() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let live = base.replace(
            "<!-- agent:boundary:abc123 -->\n",
            "do [#staleflt]. spec-test-build-install-commit-push\n<!-- agent:boundary:abc123 -->\n",
        );
        std::fs::write(&doc, &live).unwrap();

        let err = run(&doc).unwrap_err();
        let message = err.to_string();
        assert!(message.contains(EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR));
        assert!(message.contains("prompt_target: do [#staleflt]"));
        assert!(message.contains("no response exists to replay"));

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
        assert_eq!(state.last_event, "preflight_started");
    }

    #[test]
    fn recover_does_not_treat_orchestration_handoff_marker_as_missing_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let live = base.replace(
            "❯ Please reply\n",
            "❯ Please reply\n\nSynchronous orchestra:\n",
        );
        std::fs::write(&doc, &live).unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::Noop);

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
    }

    #[test]
    fn recover_repairs_preflight_started_cycle_when_committed_patchback_is_visible() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(root, &doc);
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let updated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .status()
            .unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::StalePreflightLockRepaired);

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "repair_preflight_committed_historical");
        assert_eq!(snapshot::load(&doc).unwrap().as_deref(), Some(updated));
    }

    #[test]
    fn recover_repairs_stale_preflight_cycle_despite_queue_only_churn() {
        // #adoc-queue-ipc-buffer-divergence root cause #4: a committed cycle
        // whose only working-tree drift since preflight start is queue-component
        // churn (auto strip + queue_active toggle from queue maintenance) must
        // still recover via the normalized replay-hash match instead of staying
        // wedged in PreflightStarted (the recurring stuck_captured_cycle symptom).
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test\n- do [#qchurn]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(root, &doc);
        let state = crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();

        // Only the queue churned (halt: auto stripped + queue_active cleared +
        // body drained). The exchange/response is byte-identical. Commit it so
        // HEAD matches the working tree (the committed steady state).
        let churned = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\nqueue_active: false\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, churned).unwrap();
        snapshot::save(&doc, churned).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "queue churn", "--no-verify"])
            .status()
            .unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(
            repaired,
            RepairOutcome::StalePreflightLockRepaired,
            "queue-only churn must not block stale-lock recovery"
        );

        let after = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(after.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(after.cycle_id, state.cycle_id);
    }

    #[test]
    fn recover_closes_write_applied_cycle_when_head_already_has_exchange_patchback() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(root, &doc);

        let updated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .status()
            .unwrap();

        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(updated)).unwrap();
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(content),
            Some(updated),
        )
        .unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::CommitBoundaryRecovered);

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "repair_commit_boundary_recovered");
        assert_eq!(snapshot::load(&doc).unwrap().as_deref(), Some(updated));
    }

    #[test]
    fn recover_fails_closed_on_ambiguous_preflight_started_patchback_without_artifact() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let updated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();

        let err = run(&doc).unwrap_err();
        let message = err.to_string();
        assert!(message.contains(AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR));
        assert!(message.contains("### Re: topic — gpt-5"));
    }

    #[test]
    fn recover_ignores_committed_capture_without_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        crate::capture::capture_response(&doc, "Recovered answer.").unwrap();
        crate::capture::mark_committed(&doc).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::Noop,
            "committed captures should not trigger replay/dedup on later preflights"
        );
    }

    #[test]
    fn recover_replays_latest_committed_capture_when_matching_prompt_was_left_orphaned() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted.*\n\n",
            "❯ #code-review\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: code review — gpt-5\n\n",
            "Recovered body.\n",
            "<!-- /patch:exchange -->\n"
        );
        crate::capture::capture_response(&doc, response).unwrap();
        crate::capture::mark_committed(&doc).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("❯ #code-review"));
        assert!(result.contains("### Re: code review — gpt-5"));
        assert!(result.contains("Recovered body."));
    }

    #[test]
    fn recover_repairs_escaped_exchange_tail_when_response_already_present() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "## Assistant\n\n",
            "Recovered answer.\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        save_pending(&doc, "Recovered answer.").unwrap();
        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::AlreadyApplied,
            "dedup path should skip replay"
        );

        let repaired = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let assistant = repaired.find("## Assistant").unwrap();
        assert!(
            assistant < exchange_close,
            "escaped assistant block should move back inside exchange:\n{repaired}"
        );
    }

    #[test]
    fn recover_fails_closed_on_transcript_shaped_template_replay() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let transcript_dump = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "### Re: topic — gpt-5\n",
            "Body\n",
            "<!-- agent:boundary:def456 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        save_pending(&doc, transcript_dump).unwrap();

        let err = run(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("refused to replay pending response"),
            "unexpected error: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            content,
            "blocked replay must not mutate the document"
        );

        let blocked_dir = dir.path().join(".agent-doc/repair-blocked");
        let captures: Vec<_> = std::fs::read_dir(&blocked_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .collect();
        assert_eq!(captures.len(), 1, "expected one blocked repair capture");
        let blocked_payload = std::fs::read_to_string(captures[0].path()).unwrap();
        assert!(blocked_payload.contains("agent component markers"));
        assert!(blocked_payload.contains("response_body"));
    }

    #[test]
    fn recover_replays_guard_prefixed_template_patch() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let response = concat!(
            "<!-- no-pending-capture -->\n",
            "<!-- patch:exchange -->\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- /patch:exchange -->\n"
        );
        save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("### Re: topic — gpt-5"));
        assert!(result.contains("Recovered body."));
        assert!(
            !dir.path().join(".agent-doc/repair-blocked").exists(),
            "guard-prefixed patch payload should not be parked as blocked"
        );
    }

    #[test]
    fn repair_crosses_commit_boundary_for_git_backed_replay() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\nagent_doc_format: append\nagent_doc_write: merge\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(dir.path(), &doc);

        save_pending(&doc, "This is the recovered response.").unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::ReplayedResponse);

        let head = crate::git::show_head(&doc).unwrap().unwrap();
        assert!(
            head.contains("This is the recovered response."),
            "HEAD should contain the recovered response:\n{head}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);

        let capture = crate::capture::load_active(&doc).unwrap().unwrap();
        assert_eq!(capture.state, crate::capture::CaptureState::Committed);
        assert!(
            capture.replayed_at.is_some(),
            "recovered patchback should retain replay provenance"
        );
        assert!(
            capture.committed_at.is_some(),
            "recovered patchback should record the later commit boundary"
        );
    }

    #[test]
    fn repair_crosses_commit_boundary_when_response_already_present() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "## Assistant\n\n",
            "Recovered answer.\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(dir.path(), &doc);

        save_pending(&doc, "Recovered answer.").unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let head = crate::git::show_head(&doc).unwrap().unwrap();
        assert!(
            head.contains("Recovered answer."),
            "HEAD should contain the deduped recovered response:\n{head}"
        );
        let exchange_close = head.find("<!-- /agent:exchange -->").unwrap();
        let assistant = head.find("## Assistant").unwrap();
        assert!(
            assistant < exchange_close,
            "HEAD should keep the repaired assistant content inside exchange:\n{head}"
        );

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("Recovered answer."),
            "snapshot should be advanced to the recovered response:\n{snap}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
    }

    #[test]
    fn repair_adopts_visible_response_without_pending_when_cycle_never_started() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do [#8zjh]. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(dir.path(), &doc);

        let current = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do [#8zjh]. spec-test-build-install-commit-push\n",
            "### Re: #8zjh — gpt-5\n\n",
            "Recovered from the visible exchange tail.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, current).unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let head = crate::git::show_head(&doc).unwrap().unwrap();
        assert!(
            head.contains("Recovered from the visible exchange tail."),
            "HEAD should contain the adopted response:\n{head}"
        );
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("Recovered from the visible exchange tail."),
            "snapshot should advance to the visible response:\n{snap}"
        );
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
    }

    #[test]
    fn repair_adopts_visible_response_for_open_agent_doc_write_cycle() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please recover the partial patchback\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(root, &doc);
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let current = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please recover the partial patchback\n",
            "### Re: partial patchback — gpt-5\n\n",
            "Recovered from an agent-doc-owned visible response.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        crate::cycle_state::mark_write_applied(&doc, "write_template", Some(base), Some(current))
            .unwrap();
        snapshot::save(&doc, base).unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let head = crate::git::show_head(&doc).unwrap().unwrap();
        assert!(
            head.contains("Recovered from an agent-doc-owned visible response."),
            "HEAD should contain the adopted partial patchback:\n{head}"
        );
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("Recovered from an agent-doc-owned visible response."),
            "snapshot should advance to the adopted response:\n{snap}"
        );
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
    }

    #[test]
    fn repair_commits_already_present_response_when_snapshot_lags_committed_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Why did the patchback miss?\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(dir.path(), &doc);

        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(base), Some(base)).unwrap();

        let direct_patch = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Why did the patchback miss?\n",
            "### Re: missed patchback — gpt-5\n\n",
            "Recovered through direct patch.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, direct_patch).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: missed patchback — gpt-5\n\n",
            "Recovered through direct patch.\n",
            "<!-- /patch:exchange -->\n"
        );
        save_pending(&doc, response).unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let head = crate::git::show_head(&doc).unwrap().unwrap();
        assert!(
            head.contains("Recovered through direct patch."),
            "HEAD should own the already-present response after repair:\n{head}"
        );

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("Recovered through direct patch."),
            "snapshot should advance to the already-present response:\n{snap}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
    }

    #[test]
    fn repair_run_does_not_rewind_committed_cycle_when_replaying_after_commit() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(root, &doc);

        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(base), Some(base)).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: replay after commit — gpt-5\n\n",
            "Recovered answer.\n",
            "<!-- /patch:exchange -->\n"
        );
        save_pending(&doc, response).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::ReplayedResponse);

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_success");
        let doc_after = std::fs::read_to_string(&doc).unwrap();
        assert!(doc_after.contains("### Re: replay after commit — gpt-5"));
    }

    #[test]
    fn repair_fails_closed_when_only_later_prompt_drift_remains_after_committed_patchback() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Why did repair miss the pending response?\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        snapshot::save(&doc, base).unwrap();
        init_git_repo(root, &doc);

        let committed_patchback = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Why did repair miss the pending response?\n",
            "### Re: missed patchback — gpt-5\n\n",
            "Recovered through a direct patchback.\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, committed_patchback).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .status()
            .unwrap();

        snapshot::save(&doc, base).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(base), Some(base)).unwrap();

        let current = committed_patchback.replace(
            "<!-- /agent:exchange -->\n",
            "do [#followup]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, &current).unwrap();

        let err = repair(&doc).expect_err(
            "repair should fail closed when only later prompt drift remains after adopting the committed patchback",
        );
        let message = err.to_string();
        assert!(message.contains("unresolved prompt-bearing user changes"));
        assert!(message.contains("do [#followup]. spec-test-build-install-commit-push"));

        let repaired_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            repaired_snapshot.contains("Recovered through a direct patchback."),
            "snapshot should advance to the committed patchback:\n{repaired_snapshot}"
        );
        assert!(
            !repaired_snapshot.contains("do [#followup]. spec-test-build-install-commit-push"),
            "snapshot must not absorb the later prompt drift:\n{repaired_snapshot}"
        );

        let head = crate::git::show_head(&doc).unwrap().unwrap();
        assert!(
            head.contains("Recovered through a direct patchback."),
            "HEAD should keep the committed patchback:\n{head}"
        );
        assert!(
            !head.contains("do [#followup]. spec-test-build-install-commit-push"),
            "repair must not commit the later prompt drift:\n{head}"
        );
    }
}
