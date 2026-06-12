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
//!   Template/CRDT documents always replay through the template write path
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

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

use crate::{frontmatter, snapshot, write};

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

fn first_response_heading_line(response: &str) -> Option<&str> {
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
    let exchange = if let Ok(components) = crate::component::parse(body) {
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
    let norm = |s: &str| crate::git::normalize_transient_agent_doc_markers(s);
    let snapshot_norm = norm(snapshot_doc);
    let current_norm = norm(current_doc);
    if current_norm == snapshot_norm
        || crate::session_check::detect_bypassed_response_write_between(
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
                if !collecting && !crate::session_check::is_exchange_response_heading(trimmed) {
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
                    || crate::diff::text_line_looks_like_prompt_target(trimmed)
                    || crate::session_check::is_exchange_response_heading(trimmed)
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
    if !matches!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    ) {
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
            crate::cycle_state::CyclePhase::ResponseCaptured
                | crate::cycle_state::CyclePhase::WriteApplied
        ) | None
    ) || crate::codex_hook::load_active_session_for_current_file(file)
        .ok()
        .flatten()
        .is_some()
}

fn head_already_matches_current_doc(file: &Path, doc_content: &str) -> Result<bool> {
    Ok(crate::git::show_head(file)?.as_deref().is_some_and(|head| {
        crate::git::normalize_transient_agent_doc_markers(head)
            == crate::git::normalize_transient_agent_doc_markers(doc_content)
    }))
}

fn normalized_content_hash(content: &str) -> String {
    // Compare-side normalization for stale-cycle replay matching. Neutralizes
    // transient markers AND the agent:queue component so queue-maintenance churn
    // alone does not block recovery of an already-materialized response
    // (#adoc-queue-ipc-buffer-divergence #4). Must match cycle_state.rs's
    // store-side normalization exactly.
    crate::ops_log::content_hash(&crate::git::normalize_for_replay_hash(content))
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
    if state.phase != crate::cycle_state::CyclePhase::PreflightStarted {
        return Ok(RepairOutcome::Noop);
    }

    let file_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for stale preflight repair",
            file.display()
        )
    })?;
    let snapshot_content = snapshot::load(file)?;
    let current_file_hash = crate::ops_log::content_hash(&file_content);
    let current_snapshot_hash = snapshot_content
        .as_deref()
        .map(crate::ops_log::content_hash);
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
                crate::flow::closeout::CloseoutGuardReason::ResponsePatchbackUncommitted,
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
            crate::flow::closeout::CloseoutGuardReason::StalePreflightLockRepaired,
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
            crate::flow::closeout::CloseoutGuardReason::StalePreflightLockRepaired,
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
            crate::flow::closeout::CloseoutGuardReason::OpenCycle,
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
                crate::flow::closeout::CloseoutGuardReason::StalePreflightCycleAbandoned,
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
            crate::flow::closeout::CloseoutGuardReason::OpenCycle,
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
            crate::flow::closeout::CloseoutGuardReason::StalePreflightLockRepaired,
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
            crate::cycle_state::CyclePhase::ResponseCaptured
                | crate::cycle_state::CyclePhase::WriteApplied
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
                crate::session_check::detect_bypassed_response_write_between(head, &current_doc)
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
        crate::flow::closeout::CloseoutGuardReason::CommitBoundaryRecovered,
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
    let components = crate::component::parse(&content).with_context(|| {
        format!(
            "failed to parse {} for completed backlog reap repair",
            file.display()
        )
    })?;
    let Some(backlog) = components
        .iter()
        .find(|component| crate::component::is_backlog_component(&component.name))
    else {
        return Ok(RepairOutcome::Noop);
    };

    let doc_id = crate::pending_cmd::doc_id_for(file);
    let (canonical_body, _) =
        crate::pending::backfill(backlog.content(&content), &doc_id, &HashSet::new());
    let (new_body, removed) = crate::pending::reap_with_items(&canonical_body)?;
    if removed.is_empty() {
        return Ok(RepairOutcome::Noop);
    }

    let mut repaired = backlog.replace_content(&content, &new_body);
    if let Some(archived) = crate::preflight::archive_pending_done(file, &repaired, &removed)? {
        repaired = archived;
    }
    if let Some(reconciled) = crate::status_cmd::reconcile_top_backlog_status_content(&repaired)? {
        repaired = reconciled;
    }

    write::atomic_write_pub(file, &repaired)?;

    let repaired_snapshot = if let Some(snap_content) = snapshot::load(file)? {
        let snap_components = crate::component::parse(&snap_content).with_context(|| {
            format!(
                "failed to parse snapshot for completed backlog reap repair {}",
                file.display()
            )
        })?;
        let snap_backlog = snap_components
            .iter()
            .find(|component| crate::component::is_backlog_component(&component.name))
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
            crate::status_cmd::reconcile_top_backlog_status_content(&new_snapshot)?
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
    while let Some(merged) = crate::template::repair_duplicate_exchange_opener(&dup_opener_input)? {
        dup_opener_input = merged;
        duplicate_opener_changed = true;
    }
    let duplicate_scaffold_repaired =
        crate::template::repair_duplicate_exchange_close_scaffold(&dup_opener_input)?
            .unwrap_or_else(|| dup_opener_input.clone());
    let duplicate_scaffold_changed = duplicate_scaffold_repaired != dup_opener_input;
    let duplicate_close_repaired =
        crate::template::repair_duplicate_exchange_close_tail(&duplicate_scaffold_repaired)?
            .unwrap_or_else(|| duplicate_scaffold_repaired.clone());
    let duplicate_close_changed = duplicate_close_repaired != duplicate_scaffold_repaired;
    let tail_repaired =
        crate::template::repair_conversation_tail_outside_exchange(&duplicate_close_repaired)?
            .unwrap_or_else(|| duplicate_close_repaired.clone());
    let tail_changed = tail_repaired != duplicate_close_repaired;
    let boundary_repaired = repair_answered_stale_boundary_if_safe(file, &tail_repaired)?;
    let boundary_changed = boundary_repaired.is_some();
    let mut repaired = boundary_repaired.unwrap_or_else(|| tail_repaired.clone());
    let order_repaired =
        write::repair_response_precedes_prompt_in_exchange(&repaired, known_response, file, None)?;
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
        if known_response.is_some()
            && let Some(stripped) =
                write::strip_prompt_prefix_from_response_body_first_lines(&repaired)
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

fn repair_duplicate_exchange_scaffold_if_needed(file: &Path, doc_content: &str) -> Result<String> {
    let repaired = crate::template::repair_duplicate_exchange_close_scaffold(doc_content)?
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
    let Some(diff_text) = crate::diff::unified_diff_from_contents(&norm_snapshot, &norm_repaired)
    else {
        return false;
    };
    let changes = crate::diff::classify_prompt_bearing_changes(&diff_text);
    let mut skip_answered_response_run = false;
    for (idx, change) in changes.iter().enumerate() {
        if change.kind != crate::diff::PromptBearingChangeKind::PromptTarget {
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
        if crate::diff::prompt_change_is_already_answered(&change.text)
            || crate::diff::prompt_change_is_answered_by_later_response(&changes, idx)
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

    let body = crate::frontmatter::parse(current_doc)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|_| current_doc.to_string());
    let Ok(components) = crate::component::parse(&body) else {
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
            return crate::session_check::is_exchange_response_heading(normalized);
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

    let components = crate::component::parse(doc_content).with_context(|| {
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
    let Some(boundary_id) = crate::boundary::find_boundary_id_in_component(doc_content, exchange)
    else {
        return Ok(None);
    };

    let exchange_body = exchange.content(doc_content);
    let marker = crate::boundary::format_marker(&boundary_id);
    let Some(marker_idx) = exchange_body.find(&marker) else {
        return Ok(None);
    };
    let tail_after_boundary = &exchange_body[marker_idx + marker.len()..];
    if tail_after_boundary.trim().is_empty()
        || !crate::diff::prompt_change_is_already_answered(tail_after_boundary)
        || crate::session_check::first_unstarted_prompt_bearing_change(file)?.is_some()
    {
        return Ok(None);
    }

    let repaired = crate::template::reposition_boundary_to_end_preserve_head_with_id(
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
    let root = crate::snapshot::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .context("resolve project root for blocked repair payload")?;
    let dir = root.join(".agent-doc/repair-blocked");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create blocked repair dir {}", dir.display()))?;
    let filename = format!(
        "{}-{}.json",
        crate::ops_log::content_hash(canonical.to_string_lossy().as_ref()),
        now_millis()
    );
    let path = dir.join(filename);
    let record = BlockedRepairPayloadRecord {
        captured_at: now_secs(),
        file: canonical.display().to_string(),
        reason,
        payload_sha256: crate::ops_log::content_hash(response),
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
    let pending_path = snapshot::pending_path_for(file)?;
    if pending_path.exists() {
        std::fs::remove_file(&pending_path).with_context(|| {
            format!(
                "failed to remove pending response after manual repair {}",
                pending_path.display()
            )
        })?;
    }
    if let Err(e) = snapshot::delete_pre_response(file) {
        eprintln!("[repair] warning: failed to delete pre-response: {}", e);
    }
    crate::capture::mark_discarded(file)?;
    snapshot::save(file, current_doc)?;
    crate::cycle_state::mark_committed(
        file,
        "repair_respect_manual_exchange_tail_removal",
        Some(current_doc),
        Some(current_doc),
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

    // Retire the stale capture body (Discarded — preserved on disk) and rebuild
    // snapshot + CRDT from the current document, matching the non-destructive
    // `reset --from-current --preserve-session` recovery.
    let pending_path = snapshot::pending_path_for(file)?;
    if pending_path.exists() {
        std::fs::remove_file(&pending_path).with_context(|| {
            format!(
                "failed to remove pending response while retiring stale capture {}",
                pending_path.display()
            )
        })?;
    }
    if let Err(e) = snapshot::delete_pre_response(file) {
        eprintln!("[repair] warning: failed to delete pre-response: {}", e);
    }
    crate::capture::mark_discarded(file)?;
    snapshot::save(file, doc_content)?;
    let crdt = crate::crdt::CrdtDoc::from_text(doc_content).encode_state();
    crate::snapshot::save_document_crdt(file, &crdt, doc_content)?;
    crate::cycle_state::mark_committed(
        file,
        "repair_retire_wedged_write_applied_capture",
        Some(doc_content),
        Some(doc_content),
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

    let pending_path = snapshot::pending_path_for(file)?;
    if pending_path.exists() {
        std::fs::remove_file(&pending_path).with_context(|| {
            format!(
                "failed to remove pending response while retiring superseded captured orphan {}",
                pending_path.display()
            )
        })?;
    }
    if let Err(e) = snapshot::delete_pre_response(file) {
        eprintln!("[repair] warning: failed to delete pre-response: {}", e);
    }
    crate::capture::mark_discarded(file)?;
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
fn live_exchange_answers_heading(doc_content: &str, heading: &str) -> bool {
    let target = normalize_replay_topic(heading);
    if target.is_empty() {
        return false;
    }
    let Ok(components) = crate::component::parse(doc_content) else {
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
    if capture.snapshot_hash != Some(crate::ops_log::content_hash(&snapshot_content)) {
        return Ok(false);
    }

    let Some(stripped_snapshot) =
        crate::template::strip_conversation_tail_outside_exchange(&snapshot_content)?
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

    eprintln!(
        "[repair] Found orphaned response for {} ({} bytes). Applying...",
        file.display(),
        response.len()
    );

    let (fm, _) = frontmatter::parse(&doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    let use_template_write = fm.resolve_mode().is_template() || response.contains("<!-- patch:");
    let response_to_write = if use_template_write {
        match crate::replay_guard::classify_replay_payload(&response) {
            crate::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
                fail_closed_on_blocked_template_replay(file, &response, &reason)?;
                response.clone()
            }
            crate::replay_guard::ReplayPayloadClassification::Replayable(response) => {
                response.into_owned()
            }
            crate::replay_guard::ReplayPayloadClassification::Empty => response.clone(),
        }
    } else {
        response.clone()
    };
    if use_template_write {
        write::apply_template_from_string(file, &response_to_write)?;
    } else {
        write::apply_append_from_string(file, &response_to_write)?;
    }

    // Remove the pending file after successful write
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
/// head (not a `do [#id]` / `do #id` directive).
fn first_queue_head_is_free_text(content: &str) -> bool {
    let Ok(components) = crate::component::parse(content) else {
        return false;
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return false;
    };
    let Ok(entries) = crate::queue::parse(queue.content(content)) else {
        return false;
    };
    match crate::queue::prompts(&entries).first() {
        Some(prompt) => {
            let t = prompt.text.trim().to_ascii_lowercase();
            !(t.starts_with("do [#") || t.starts_with("do #"))
        }
        None => false,
    }
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
            crate::flow::closeout::CloseoutGuardReason::SessionCheckInterrupted,
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
mod tests;
