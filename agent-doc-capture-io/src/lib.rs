//! # Module: capture
//!
//! ## Spec
//! - Persists durable response captures and in-progress streamed checkpoints as
//!   typed facts in the project Lazily state ledger (`state.db`).
//! - Captures the final parsed response body plus cycle metadata before any
//!   document write or hook emission. The full replay baseline is retained in
//!   the Lazily state projection so recovery can reconcile partial editor
//!   materialization without restoring Git HEAD.
//! - `capture_response(file, response)` stores the response, marks the cycle as
//!   `response_captured`, and returns the saved record.
//! - `PartialCheckpointWriter` saves the first non-empty partial response and
//!   then checkpoints changed partial output at most once every 30 seconds.
//! - `load_active(file)` resolves the active capture from cycle state.
//! - `validate_replay(file, capture)` fail-closes replay when the current file
//!   or snapshot hashes no longer match the captured baseline.
//! - `mark_write_applied`, `mark_replayed`, `mark_committed`, and
//!   `mark_discarded` advance the shared turn lifecycle. Duplicate
//!   terminal updates are idempotent and do not re-log replay provenance.
//!
//! ## Agentic Contracts
//! - The capture ledger is document-scoped; one active capture is associated
//!   with one response cycle.
//! - Capture writes are idempotently keyed state-ledger events. There is no
//!   parallel filesystem capture representation.
//! - Recovery replays only the captured response body; it never regenerates
//!   content from hooks or history.
//! - Hash validation uses content SHA-256 for both the document and the
//!   snapshot baseline. The document hash is taken with the managed
//!   `agent_doc_pipeline:` frontmatter block stripped (#22a8) so the mid-cycle
//!   pipeline mirror never reads as replay-baseline drift.
//!
//! ## Evals
//! - `capture_response_persists_record_and_cycle_metadata`
//! - `partial_checkpoint_persists_without_advancing_cycle`
//! - `validate_replay_rejects_diverged_file_hash`
//! - `mark_committed_updates_capture_state`

use agent_doc_turn::closeout_recovery::CloseoutRecoveryMutationReason;
pub use agent_doc_workflow::capture::CaptureState;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PARTIAL_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureRecord {
    pub capture_id: String,
    pub cycle_id: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_strategy: Option<String>,
    pub captured_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_applied_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replayed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discarded_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_content: Option<String>,
    pub response_sha256: String,
    pub response_body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation_plan_json: Option<String>,
    pub state: CaptureState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartialCaptureRecord {
    pub checkpoint_id: String,
    pub cycle_id: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_strategy: Option<String>,
    pub captured_at: u64,
    pub updated_at: u64,
    pub checkpoint_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
    pub response_sha256: String,
    pub response_body: String,
}

#[derive(Debug, Default)]
struct CaptureMetadata {
    session_id: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    document_format: Option<String>,
    write_strategy: Option<String>,
}

pub struct PartialCheckpointWriter {
    file: PathBuf,
    cycle_id: String,
    interval: Duration,
    last_checkpoint: Option<Instant>,
    last_response_sha256: Option<String>,
    checkpoint_count: u64,
    stopped: bool,
}

impl PartialCheckpointWriter {
    pub fn new(file: &Path) -> Self {
        Self::with_interval(file, PARTIAL_CHECKPOINT_INTERVAL)
    }

    pub fn with_interval(file: &Path, interval: Duration) -> Self {
        let cycle_id = agent_doc_cycle_state_io::load_with_closeout_projection(file)
            .ok()
            .flatten()
            .map(|state| state.cycle_id)
            .unwrap_or_else(|| format!("partial-{}", now_millis()));
        Self {
            file: file.to_path_buf(),
            cycle_id,
            interval,
            last_checkpoint: None,
            last_response_sha256: None,
            checkpoint_count: 0,
            stopped: false,
        }
    }

    pub fn maybe_checkpoint(&mut self, response: &str) -> Result<Option<PartialCaptureRecord>> {
        self.maybe_checkpoint_inner(response, None)
    }

    pub fn maybe_checkpoint_with_current_content(
        &mut self,
        response: &str,
        current_content: &str,
    ) -> Result<Option<PartialCaptureRecord>> {
        self.maybe_checkpoint_inner(response, Some(current_content))
    }

    fn maybe_checkpoint_inner(
        &mut self,
        response: &str,
        current_content: Option<&str>,
    ) -> Result<Option<PartialCaptureRecord>> {
        if self.stopped || !self.active_cycle_accepts_checkpoint()? {
            return Ok(None);
        }
        if response.trim().is_empty() {
            return Ok(None);
        }

        let response_sha256 = agent_doc_hash::content_hash(response);
        if self.last_response_sha256.as_deref() == Some(response_sha256.as_str()) {
            return Ok(None);
        }

        let due = self
            .last_checkpoint
            .is_none_or(|last| last.elapsed() >= self.interval);
        if !due {
            return Ok(None);
        }

        self.checkpoint_count += 1;
        let record = match current_content {
            Some(current_content) => checkpoint_partial_response_for_cycle_with_current_content(
                &self.file,
                response,
                self.checkpoint_count,
                &self.cycle_id,
                current_content,
            )?,
            None => checkpoint_partial_response_for_cycle(
                &self.file,
                response,
                self.checkpoint_count,
                &self.cycle_id,
            )?,
        };
        self.last_checkpoint = Some(Instant::now());
        self.last_response_sha256 = Some(response_sha256);
        Ok(Some(record))
    }

    fn active_cycle_accepts_checkpoint(&mut self) -> Result<bool> {
        let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(&self.file)?
        else {
            return Ok(self.cycle_id.starts_with("partial-"));
        };
        if state.cycle_id == self.cycle_id && state.is_open() {
            return Ok(true);
        }

        self.stopped = true;
        let reason = if state.cycle_id != self.cycle_id {
            "cycle_changed"
        } else {
            "cycle_closed"
        };
        agent_doc_ops_log_io::log_op(
            &self.file,
            &format!(
                "partial_response_checkpoint_stopped file={} writer_cycle={} current_cycle={} phase={:?} reason={}",
                self.file.display(),
                self.cycle_id,
                state.cycle_id,
                state.phase,
                reason
            ),
        );
        Ok(false)
    }
}

pub fn capture_response(file: &Path, response: &str) -> Result<CaptureRecord> {
    let file_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for response capture", file.display()))?;
    capture_response_with_current_content_and_intent(file, response, &file_content, None)
}

pub fn capture_response_with_intent(
    file: &Path,
    response: &str,
    intent_body: &str,
) -> Result<CaptureRecord> {
    capture_response_with_intent_and_plan(file, response, intent_body, None)
}

pub fn capture_response_with_intent_and_plan(
    file: &Path,
    response: &str,
    intent_body: &str,
    mutation_plan_json: Option<&str>,
) -> Result<CaptureRecord> {
    let file_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for response capture", file.display()))?;
    capture_response_with_current_content_and_intent_and_plan(
        file,
        response,
        &file_content,
        Some(intent_body),
        mutation_plan_json,
    )
}

pub fn capture_response_with_current_content(
    file: &Path,
    response: &str,
    file_content: &str,
) -> Result<CaptureRecord> {
    capture_response_with_current_content_and_intent(file, response, file_content, None)
}

pub fn capture_response_with_current_content_and_intent(
    file: &Path,
    response: &str,
    file_content: &str,
    intent_body: Option<&str>,
) -> Result<CaptureRecord> {
    capture_response_with_current_content_and_intent_and_plan(
        file,
        response,
        file_content,
        intent_body,
        None,
    )
}

pub fn capture_response_with_current_content_and_intent_and_plan(
    file: &Path,
    response: &str,
    file_content: &str,
    intent_body: Option<&str>,
    mutation_plan_json: Option<&str>,
) -> Result<CaptureRecord> {
    let response_sha256 = agent_doc_hash::content_hash(response);
    let existing_cycle_id =
        agent_doc_cycle_state_io::load_with_closeout_projection(file)?.map(|s| s.cycle_id);
    let capture_id = existing_cycle_id.unwrap_or_else(|| format!("synthetic-{}", now_millis()));
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());

    let metadata = metadata_from_frontmatter(file_content);

    // Redact secrets from the response body before it lands in the state
    // ledger. The `response_sha256` keeps the original-bytes hash so
    // cycle-state correlation (which references the live in-memory response)
    // stays intact — only the persisted body changes.
    let redacted_response = agent_doc_secret_redact::redact(response);

    let record = CaptureRecord {
        capture_id: capture_id.clone(),
        cycle_id: capture_id.clone(),
        file: canonical.display().to_string(),
        session_id: metadata.session_id,
        agent: metadata.agent,
        model: metadata.model,
        document_format: metadata.document_format,
        write_strategy: metadata.write_strategy,
        captured_at: now_secs(),
        updated_at: now_secs(),
        write_applied_at: None,
        replayed_at: None,
        committed_at: None,
        discarded_at: None,
        snapshot_hash: None,
        file_hash: Some(replay_file_hash(file_content)),
        baseline_content: Some(file_content.to_string()),
        response_sha256: response_sha256.clone(),
        response_body: redacted_response,
        intent_body: intent_body.map(agent_doc_secret_redact::redact),
        mutation_plan_json: mutation_plan_json.map(agent_doc_secret_redact::redact),
        state: CaptureState::Captured,
    };
    agent_doc_cycle_state_io::mark_response_captured(
        file,
        "response_captured",
        None,
        Some(file_content),
        &response_sha256,
        Some(&capture_id),
    )?;
    agent_doc_cycle_state_io::append_response_captured_body(
        file,
        agent_doc_cycle_state_io::CapturedResponseFactInput {
            cycle_id: &record.cycle_id,
            capture_id: &record.capture_id,
            response_sha256: &record.response_sha256,
            response_body: &record.response_body,
            intent_body: record.intent_body.as_deref(),
            mutation_plan_json: record.mutation_plan_json.as_deref(),
            file_hash: record.file_hash.as_deref(),
            snapshot_hash: record.snapshot_hash.as_deref(),
            baseline_content: record.baseline_content.as_deref(),
        },
    )?;
    Ok(load_by_id(file, &capture_id)?.unwrap_or(record))
}

fn checkpoint_partial_response_for_cycle(
    file: &Path,
    response: &str,
    checkpoint_count: u64,
    cycle_id: &str,
) -> Result<PartialCaptureRecord> {
    let file_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for partial response capture",
            file.display()
        )
    })?;
    checkpoint_partial_response_for_cycle_with_current_content(
        file,
        response,
        checkpoint_count,
        cycle_id,
        &file_content,
    )
}

fn checkpoint_partial_response_for_cycle_with_current_content(
    file: &Path,
    response: &str,
    checkpoint_count: u64,
    cycle_id: &str,
    file_content: &str,
) -> Result<PartialCaptureRecord> {
    let response_sha256 = agent_doc_hash::content_hash(response);
    let checkpoint_id = format!("{cycle_id}-partial");
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let metadata = metadata_from_frontmatter(file_content);
    let existing = load_partial_by_cycle(file, cycle_id)?;
    let captured_at = existing
        .as_ref()
        .map_or_else(now_secs, |record| record.captured_at);

    let redacted_response = agent_doc_secret_redact::redact(response);
    let record = PartialCaptureRecord {
        checkpoint_id,
        cycle_id: cycle_id.to_string(),
        file: canonical.display().to_string(),
        session_id: metadata.session_id,
        agent: metadata.agent,
        model: metadata.model,
        document_format: metadata.document_format,
        write_strategy: metadata.write_strategy,
        captured_at,
        updated_at: now_secs(),
        checkpoint_count,
        snapshot_hash: None,
        file_hash: Some(replay_file_hash(file_content)),
        response_sha256,
        response_body: redacted_response,
    };
    agent_doc_cycle_state_io::checkpoint_response_draft(
        file,
        cycle_id,
        &record.checkpoint_id,
        checkpoint_count,
        &record.response_sha256,
        &record.response_body,
        record.file_hash.as_deref(),
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "partial_response_checkpoint file={} cycle={} count={} sha256={}",
            file.display(),
            cycle_id,
            checkpoint_count,
            record.response_sha256
        ),
    );
    load_partial_by_cycle(file, cycle_id)?.with_context(|| {
        format!(
            "partial response checkpoint {} was not visible in the state ledger for {}",
            record.checkpoint_id,
            file.display()
        )
    })
}

/// Read the one ownership predicate every retained-write refusal shares.
///
/// See `agent_doc_turn::write_ownership` for why this exists. This is the I/O
/// shell: it reads the two durable facts and hands them to the pure predicate,
/// so the three refusal sites cannot drift into three answers about the same
/// state.
///
/// A read error is **not** ownership. A site that cannot prove a holder must say
/// stranded and name a recovery, because the alternative — assuming a holder —
/// is the failure this predicate exists to remove.
pub fn retained_write_ownership(
    file: &Path,
) -> agent_doc_turn::write_ownership::RetainedWriteOwnership {
    // `#ownershipverdictdiverges`: carry the PHASE, not just "open". An
    // uncaptured `write_applied` cycle needs `agent-doc commit`; a retained
    // capture still owns the terminal boundary through captured-finalize.
    // Preserve both facts so phase cannot erase the stronger ownership proof.
    let mut write_applied = false;
    let cycle_open = match agent_doc_cycle_state_io::load_with_closeout_projection(file) {
        Ok(state) => state.is_some_and(|state| {
            write_applied = state.phase == agent_doc_turn::CyclePhase::WriteApplied;
            state.phase.is_open()
        }),
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "retained_write_ownership_cycle_read_failed file={} error={err}",
                    file.display()
                ),
            );
            false
        }
    };
    let retained_capture = match load_active(file) {
        Ok(capture) => capture.is_some(),
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "retained_write_ownership_capture_read_failed file={} error={err}",
                    file.display()
                ),
            );
            false
        }
    };
    agent_doc_turn::write_ownership::RetainedWriteOwnership::new_with_phase(
        cycle_open,
        retained_capture,
        write_applied,
    )
}

pub fn load_active(file: &Path) -> Result<Option<CaptureRecord>> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    if !state.phase.is_open() {
        return Ok(None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(None);
    };
    Ok(load_by_id(file, capture_id)?.filter(|capture| capture.state != CaptureState::Discarded))
}

pub fn load_by_id(file: &Path, capture_id: &str) -> Result<Option<CaptureRecord>> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    let Some(projected) =
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
    else {
        return Ok(None);
    };
    if projected.cycle_id != state.cycle_id
        || state
            .response_sha256
            .as_deref()
            .is_some_and(|sha| sha != projected.response_sha256)
    {
        return Ok(None);
    }
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let file_content = std::fs::read_to_string(file).unwrap_or_default();
    let metadata = metadata_from_frontmatter(&file_content);
    let closeout = agent_doc_cycle_state_io::load_closeout_projection(file)?;
    let capture_replayed = closeout.as_ref().is_some_and(|p| p.capture_replayed);
    let capture_retired = closeout.as_ref().is_some_and(|projection| {
        projection.captured_response_retired_reason.is_some()
            && projection.capture_id.as_deref() == Some(capture_id)
    });
    let capture_state = if capture_retired {
        CaptureState::Discarded
    } else if state.phase == agent_doc_turn::CyclePhase::Committed {
        CaptureState::Committed
    } else if capture_replayed {
        CaptureState::Replayed
    } else {
        match state.phase {
            agent_doc_turn::CyclePhase::PreflightStarted
            | agent_doc_turn::CyclePhase::ResponseCaptured => CaptureState::Captured,
            agent_doc_turn::CyclePhase::WriteApplied => CaptureState::WriteApplied,
            agent_doc_turn::CyclePhase::Committed => unreachable!("committed handled above"),
            agent_doc_turn::CyclePhase::Abandoned => CaptureState::Discarded,
        }
    };
    Ok(Some(CaptureRecord {
        capture_id: projected.capture_id,
        cycle_id: projected.cycle_id,
        file: canonical.display().to_string(),
        session_id: metadata.session_id,
        agent: metadata.agent,
        model: metadata.model,
        document_format: metadata.document_format,
        write_strategy: metadata.write_strategy,
        captured_at: state.started_at,
        updated_at: state.updated_at,
        write_applied_at: matches!(
            state.phase,
            agent_doc_turn::CyclePhase::WriteApplied | agent_doc_turn::CyclePhase::Committed
        )
        .then_some(state.updated_at),
        replayed_at: capture_replayed.then_some(state.updated_at),
        committed_at: (state.phase == agent_doc_turn::CyclePhase::Committed)
            .then_some(state.updated_at),
        discarded_at: (capture_retired || state.phase == agent_doc_turn::CyclePhase::Abandoned)
            .then_some(state.updated_at),
        snapshot_hash: projected.snapshot_hash,
        file_hash: projected.file_hash,
        baseline_content: projected.baseline_content,
        response_sha256: projected.response_sha256,
        response_body: projected.response_body,
        intent_body: projected.intent_body,
        mutation_plan_json: projected.mutation_plan_json,
        state: capture_state,
    }))
}

pub fn load_partial_by_cycle(file: &Path, cycle_id: &str) -> Result<Option<PartialCaptureRecord>> {
    let Some(projected) = agent_doc_cycle_state_io::load_projected_response_draft(file, cycle_id)?
    else {
        return Ok(None);
    };
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let file_content = std::fs::read_to_string(file).unwrap_or_default();
    let metadata = metadata_from_frontmatter(&file_content);
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    let timestamp = state
        .as_ref()
        .map_or_else(now_secs, |state| state.updated_at);
    Ok(Some(PartialCaptureRecord {
        checkpoint_id: projected.checkpoint_id,
        cycle_id: projected.cycle_id,
        file: canonical.display().to_string(),
        session_id: metadata.session_id,
        agent: metadata.agent,
        model: metadata.model,
        document_format: metadata.document_format,
        write_strategy: metadata.write_strategy,
        captured_at: state.as_ref().map_or(timestamp, |state| state.started_at),
        updated_at: timestamp,
        checkpoint_count: projected.checkpoint_count,
        snapshot_hash: None,
        file_hash: projected.file_hash,
        response_sha256: projected.response_sha256,
        response_body: projected.response_body,
    }))
}

#[allow(dead_code)]
pub fn latest_partial_checkpoint(file: &Path) -> Result<Option<PartialCaptureRecord>> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    load_partial_by_cycle(file, &state.cycle_id)
}

pub fn latest_committed(file: &Path) -> Result<Option<CaptureRecord>> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    if state.phase != agent_doc_turn::CyclePhase::Committed {
        return Ok(None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(None);
    };
    Ok(load_by_id(file, capture_id)?.filter(|record| record.state == CaptureState::Committed))
}

/// #22a8 (Phase 5b write-side): hash the document for capture replay/commit
/// validation with the managed `agent_doc_pipeline:` frontmatter block removed.
/// The block is mirrored onto disk mid-cycle (after response capture, cleared at
/// a terminal phase), so a raw hash would read that managed write as document
/// drift and fail the replay baseline. Stripping it keeps replay validation
/// invariant to the mirror, matching the diff layer.
pub fn replay_file_hash(content: &str) -> String {
    agent_doc_hash::content_hash(
        &agent_doc_frontmatter::frontmatter::strip_pipeline_block_lines(content),
    )
}

/// Result of removing only lines proven to be a partial materialization of a
/// captured response. The caller must publish `content` through document
/// authority before replaying the complete response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialResponseReconciliation {
    pub content: String,
    pub removed_nonblank_lines: usize,
}

/// Reconcile an editor buffer that contains a non-contiguous or reordered
/// subset of a captured response. Baseline line multiplicity protects text
/// that already existed before capture; response line multiplicity prevents
/// deleting more copies than the capture could have introduced. At least two
/// nonblank response lines are required so a coincidental one-line operator
/// edit cannot be consumed as recovery state.
pub fn reconcile_partial_response_lines(
    baseline: &str,
    current: &str,
    response: &str,
) -> Option<PartialResponseReconciliation> {
    if baseline == current
        || response.trim().is_empty()
        || agent_doc_turn::response_replay::response_already_applied(current, response)
        || agent_doc_turn::response_replay::response_already_applied_after_prefix_strip(
            current, response,
        )
        || agent_doc_turn::response_replay::response_materialized_in_content(response, current)
    {
        return None;
    }

    fn line_key(line: &str) -> &str {
        line.trim_end_matches(['\r', '\n'])
    }

    let mut baseline_counts = std::collections::HashMap::<&str, usize>::new();
    for line in baseline.split_inclusive('\n') {
        *baseline_counts.entry(line_key(line)).or_default() += 1;
    }
    let mut response_counts = std::collections::HashMap::<&str, usize>::new();
    for line in response.split_inclusive('\n') {
        let key = line_key(line);
        if !key.trim().is_empty() {
            *response_counts.entry(key).or_default() += 1;
        }
    }

    let mut reconciled = String::with_capacity(current.len());
    let mut removed_nonblank_lines = 0usize;
    for line in current.split_inclusive('\n') {
        let key = line_key(line);
        if let Some(remaining) = baseline_counts.get_mut(key)
            && *remaining > 0
        {
            *remaining -= 1;
            reconciled.push_str(line);
            continue;
        }
        if let Some(remaining) = response_counts.get_mut(key)
            && *remaining > 0
        {
            *remaining -= 1;
            removed_nonblank_lines += 1;
            continue;
        }
        reconciled.push_str(line);
    }

    (removed_nonblank_lines >= 2 && reconciled != current).then_some(
        PartialResponseReconciliation {
            content: reconciled,
            removed_nonblank_lines,
        },
    )
}

/// Fortify a hash-only ledger capture after a byte-identical baseline has been
/// recovered from an independently verified source. This never overwrites an
/// existing content-bearing baseline.
pub fn fortify_baseline_content(
    file: &Path,
    capture_id: &str,
    baseline_content: &str,
) -> Result<bool> {
    let Some(mut record) = load_by_id(file, capture_id)? else {
        return Ok(false);
    };
    if record.baseline_content.is_some() {
        return Ok(false);
    }
    let baseline_hash = replay_file_hash(baseline_content);
    if record.file_hash.as_deref() != Some(baseline_hash.as_str()) {
        return Ok(false);
    }
    record.baseline_content = Some(baseline_content.to_string());
    record.updated_at = now_secs();
    checkpoint_capture_projection(file, &record)?;
    Ok(true)
}

/// Refresh the ledger capture after Lazily-authoritative structural history
/// recovery rebases an open captured response. Identity and the old baseline
/// must still match, so stale recovery cannot overwrite a newer capture.
pub fn project_structural_recovery_baseline(
    file: &Path,
    capture: &CaptureRecord,
    expected_file_hash: Option<&str>,
    baseline_content: &str,
    snapshot_hash: &str,
) -> Result<bool> {
    let Some(mut record) = load_by_id(file, &capture.capture_id)? else {
        return Ok(false);
    };
    if record.cycle_id != capture.cycle_id
        || record.response_sha256 != capture.response_sha256
        || record.file_hash.as_deref() != expected_file_hash
    {
        return Ok(false);
    }
    record.file_hash = Some(replay_file_hash(baseline_content));
    record.snapshot_hash = Some(snapshot_hash.to_string());
    record.baseline_content = Some(baseline_content.to_string());
    record.updated_at = now_secs();
    checkpoint_capture_projection(file, &record)?;
    Ok(true)
}

/// Pure check (no side effects, no benign-drift refresh): returns true when the
/// capture's recorded baseline (file/snapshot content hashes) no longer matches
/// the current document — i.e. [`validate_replay`] would fail closed with
/// "captured response baseline no longer matches current document" unless the
/// response body is still intact in the document.
///
/// `#stale-capture-deadlock-autoretire`: repair uses this to recognize the
/// wedged-orphan deadlock signature (a `WriteApplied` capture whose response
/// vanished from the document AND whose baseline drifted) so it can retire the
/// stale capture non-destructively instead of deadlocking the document behind a
/// manual `reset --from-current --preserve-session`.
pub fn replay_baseline_drifted(file: &Path, capture: &CaptureRecord) -> Result<bool> {
    let current_file = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for capture replay drift check",
            file.display()
        )
    })?;
    replay_baseline_drifted_with_current_content(file, capture, &current_file)
}

pub fn replay_baseline_drifted_with_current_content(
    _file: &Path,
    capture: &CaptureRecord,
    current_file: &str,
) -> Result<bool> {
    let current_file_hash = replay_file_hash(current_file);
    let file_mismatch = capture.file_hash.as_deref() != Some(current_file_hash.as_str());
    Ok(file_mismatch)
}

pub fn validate_replay(file: &Path, capture: &CaptureRecord) -> Result<()> {
    let current_file = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for capture replay", file.display()))?;
    validate_replay_with_current_content(file, capture, &current_file)
}

pub fn validate_replay_with_current_content(
    file: &Path,
    capture: &CaptureRecord,
    current_file: &str,
) -> Result<()> {
    let current_file_hash = replay_file_hash(current_file);
    let file_mismatch = capture.file_hash.as_deref() != Some(current_file_hash.as_str());

    if !file_mismatch {
        return Ok(());
    }

    // Phase 2 of #adoc-baseline-drift-after-user-commit: when the captured
    // response body is still present in the working tree (intact), treat the
    // hash mismatch as benign drift — a user committed something on top of
    // the prior agent-doc commit but did not touch the response body. Refresh
    // the capture's file/snapshot hashes from the current state and proceed.
    //
    // Plan: tasks/agent-doc/plan-baseline-drift-after-user-commit.md
    if response_body_intact_in_current(file, &capture.response_body, current_file)? {
        refresh_replay_baseline_for_reason(
            file,
            capture,
            &current_file_hash,
            Some(&current_file_hash),
            Some(current_file),
            CloseoutRecoveryMutationReason::BenignReplayBaseline,
        )?;
        return Ok(());
    }

    // #queueeditcap: a captured response may be interrupted after capture but
    // before write/commit while the operator edits only `agent:queue`. In that
    // shape the content-bearing capture remains the baseline, so replaying the
    // response onto the current file preserves the queue edit and completes
    // closeout without consulting a filesystem snapshot.
    if file_mismatch
        && live_drift_is_queue_only_against_baseline(
            current_file,
            capture.baseline_content.as_deref(),
        )?
    {
        refresh_replay_baseline_for_reason(
            file,
            capture,
            &current_file_hash,
            Some(&current_file_hash),
            Some(current_file),
            CloseoutRecoveryMutationReason::QueueOnlyReplayBaseline,
        )?;
        return Ok(());
    }

    // A legacy editor could replay the complete session projection while a
    // response cell was in flight, then overwrite that response from its stale
    // cache. Preflight now coalesces that malformed dual projection through the
    // CRDT before interrupted-cycle recovery. The resulting byte hash is
    // intentionally different from the captured baseline even though no
    // operator text changed. Rebase only when the exact baseline used by the
    // matching open cycle independently coalesces to the current document.
    // This is a structural proof, not a generic drift waiver: divergent or
    // reordered projections still fail closed below.
    if file_mismatch
        && let Some((copies, retained_additions)) =
            captured_baseline_coalesces_to_current(file, capture, current_file)?
    {
        refresh_replay_baseline_for_reason(
            file,
            capture,
            &current_file_hash,
            Some(&current_file_hash),
            Some(current_file),
            CloseoutRecoveryMutationReason::WholeDocumentReplayCoalescedBaseline,
        )?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "capture_replay_baseline_coalesced file={} capture_id={} cycle_id={} copies={} retained_additions={} recovery=replay_captured_response",
                file.display(),
                capture.capture_id,
                capture.cycle_id,
                copies,
                retained_additions,
            ),
        );
        return Ok(());
    }

    anyhow::bail!(
        "captured response baseline no longer matches current document for {}. Rebuild the cold recovery projection without clearing session state: `agent-doc reset --from-current --preserve-session {}`",
        file.display(),
        file.display()
    );
}

fn captured_baseline_coalesces_to_current(
    file: &Path,
    capture: &CaptureRecord,
    current_file: &str,
) -> Result<Option<(usize, usize)>> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    if !state.is_open()
        || state.cycle_id != capture.cycle_id
        || state.capture_id.as_deref() != Some(capture.capture_id.as_str())
        || state.response_sha256.as_deref() != Some(capture.response_sha256.as_str())
    {
        return Ok(None);
    }
    let Some(captured_baseline) = agent_doc_snapshot_io::load_document_baseline(file)? else {
        return Ok(None);
    };
    let Some(replay) = agent_doc_document_realtime::write_policy::coalesce_exact_document_replay(
        &captured_baseline,
    ) else {
        return Ok(None);
    };
    let coalesced = agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
        replay.canonical,
    );
    let current =
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(current_file);
    Ok((coalesced == current).then_some((replay.copies, replay.retained_additions)))
}

/// Prove that the retained capture is still the response owned by the current
/// open cycle. Recovery adapters use this before adopting a newer authority cut.
pub fn capture_matches_open_cycle(file: &Path, capture: &CaptureRecord) -> Result<bool> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(false);
    };
    Ok(state.is_open()
        && state.cycle_id == capture.cycle_id
        && state.capture_id.as_deref() == Some(capture.capture_id.as_str())
        && state.response_sha256.as_deref() == Some(capture.response_sha256.as_str()))
}

fn replay_baseline_content_for_capture(
    file: &Path,
    capture: &CaptureRecord,
) -> Result<Option<String>> {
    if let Some(content) = capture.baseline_content.as_deref()
        && capture
            .file_hash
            .as_deref()
            .is_some_and(|expected| expected == replay_file_hash(content))
    {
        return Ok(Some(content.to_string()));
    }
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    if !state.is_open()
        || state.cycle_id != capture.cycle_id
        || state.capture_id.as_deref() != Some(capture.capture_id.as_str())
    {
        return Ok(None);
    }
    let Some(content) = agent_doc_snapshot_io::load_document_baseline(file)? else {
        return Ok(None);
    };
    Ok(capture
        .file_hash
        .as_deref()
        .is_some_and(|expected| expected == replay_file_hash(&content))
        .then_some(content))
}

/// Structural proof for automatic replay-baseline adoption: the authoritative
/// current cut must preserve every captured-baseline line in order. Transient
/// agent-doc markers are normalized before comparison so transport-only state
/// does not manufacture a conflict.
pub fn authoritative_current_monotonically_extends_capture_baseline(
    file: &Path,
    capture: &CaptureRecord,
    current_file: &str,
) -> Result<bool> {
    let Some(baseline) = replay_baseline_content_for_capture(file, capture)? else {
        return Ok(false);
    };
    let baseline =
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(&baseline);
    let current =
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(current_file);
    Ok(agent_doc_workflow::capture::current_monotonically_extends_baseline(&baseline, &current))
}

/// Adopt a newer authoritative, monotonic document cut as the replay baseline.
/// The capture remains active and the exact response body is unchanged.
pub fn rebase_replay_baseline_to_authoritative_current(
    file: &Path,
    capture: &CaptureRecord,
    current_file: &str,
) -> Result<bool> {
    if !capture_matches_open_cycle(file, capture)?
        || !authoritative_current_monotonically_extends_capture_baseline(
            file,
            capture,
            current_file,
        )?
    {
        return Ok(false);
    }
    let current_file_hash = replay_file_hash(current_file);
    refresh_replay_baseline_for_reason(
        file,
        capture,
        &current_file_hash,
        Some(&current_file_hash),
        Some(current_file),
        CloseoutRecoveryMutationReason::AuthoritativeReplayBaseline,
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "capture_replay_baseline_rebased_authoritative_current file={} capture_id={} cycle_id={} response_sha256={} recovery=replay_captured_response",
            file.display(),
            capture.capture_id,
            capture.cycle_id,
            capture.response_sha256,
        ),
    );
    Ok(true)
}

/// Returns true when the captured `response_body` is still contiguously
/// present in the current document (modulo blank-line / transient marker
/// normalization and a transient `❯ ` prompt prefix present on either side).
/// Materialization is semantic: capture transport wrappers/control markers and
/// exchange prompt-prefix normalization are not editor-visible response bytes.
fn response_body_intact_in_current(
    file: &Path,
    response_body: &str,
    current_file: &str,
) -> Result<bool> {
    if response_body.trim().is_empty() {
        return Ok(false);
    }
    let _ = file; // reserved for richer structural checks (#adoc-bdauc stretch goals)
    Ok(
        agent_doc_turn::response_replay::response_materialized_in_content(
            response_body,
            current_file,
        ),
    )
}

pub fn live_drift_is_queue_only_against_baseline(
    current_file: &str,
    captured_baseline: Option<&str>,
) -> Result<bool> {
    let Some(captured_baseline) = captured_baseline else {
        return Ok(false);
    };
    let current_file = agent_doc_frontmatter::frontmatter::strip_pipeline_block_lines(current_file);
    let captured_baseline =
        agent_doc_frontmatter::frontmatter::strip_pipeline_block_lines(captured_baseline);
    if current_file == captured_baseline {
        return Ok(false);
    }

    let current_components = agent_doc_element::element::parse(&current_file)?;
    let baseline_components = agent_doc_element::element::parse(&captured_baseline)?;
    let mut current_queues = current_components.iter().filter(|c| c.name == "queue");
    let mut baseline_queues = baseline_components.iter().filter(|c| c.name == "queue");
    let Some(current_queue) = current_queues.next() else {
        return Ok(false);
    };
    let Some(baseline_queue) = baseline_queues.next() else {
        return Ok(false);
    };
    if current_queues.next().is_some() || baseline_queues.next().is_some() {
        return Ok(false);
    }

    let restored =
        current_queue.replace_content(&current_file, baseline_queue.content(&captured_baseline));
    Ok(restored == captured_baseline)
}

/// Refresh the capture record's `file_hash` and `snapshot_hash` to match the
/// current state, after `response_body_intact_in_current` confirmed the
/// drift is benign. Logged via `ops_log` so the recovery is auditable.
pub fn refresh_replay_baseline_for_recovery(
    file: &Path,
    capture: &CaptureRecord,
    current_file_hash: &str,
    current_snapshot_hash: Option<&str>,
    current_baseline_content: Option<&str>,
    audit_event: &str,
    message: &str,
) -> Result<bool> {
    let mut record = capture.clone();
    let mut changed = false;
    if record.file_hash.as_deref() != Some(current_file_hash) {
        record.file_hash = Some(current_file_hash.to_string());
        changed = true;
    }
    if record.snapshot_hash.as_deref() != current_snapshot_hash {
        record.snapshot_hash = current_snapshot_hash.map(str::to_string);
        changed = true;
    }
    if let Some(current_baseline_content) = current_baseline_content
        && record.baseline_content.as_deref() != Some(current_baseline_content)
    {
        record.baseline_content = Some(current_baseline_content.to_string());
        changed = true;
    }
    if !changed {
        return Ok(false);
    }
    record.updated_at = now_secs();
    checkpoint_capture_projection(file, &record)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{} file={} capture_id={}",
            audit_event,
            file.display(),
            record.capture_id
        ),
    );
    eprintln!(
        "[capture] {} for {} — refreshed capture baseline (capture_id={})",
        message,
        file.display(),
        record.capture_id
    );
    Ok(true)
}

fn refresh_replay_baseline_for_reason(
    file: &Path,
    capture: &CaptureRecord,
    current_file_hash: &str,
    current_snapshot_hash: Option<&str>,
    current_baseline_content: Option<&str>,
    reason: CloseoutRecoveryMutationReason,
) -> Result<()> {
    let changed = refresh_replay_baseline_for_recovery(
        file,
        capture,
        current_file_hash,
        current_snapshot_hash,
        current_baseline_content,
        reason.capture_refresh_event(),
        reason.capture_refresh_message(),
    )?;
    if changed {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "closeout_recovery_mutation file={} action=refresh_replay_baseline reason={}",
                file.display(),
                reason.as_str()
            ),
        );
    }
    Ok(())
}

pub fn mark_write_applied(file: &Path) -> Result<()> {
    let current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for capture transition", file.display()))?;
    agent_doc_cycle_state_io::mark_write_applied(
        file,
        "capture_write_applied",
        None,
        Some(&current),
    )?;
    Ok(())
}

pub fn mark_replayed(file: &Path) -> Result<()> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(());
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(());
    };
    let Some(capture) = load_by_id(file, capture_id)? else {
        return Ok(());
    };
    agent_doc_cycle_state_io::record_response_replay(file, &capture.cycle_id, &capture.capture_id)?;
    Ok(())
}

pub fn mark_committed(file: &Path) -> Result<()> {
    let current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for capture commit", file.display()))?;
    mark_committed_with_current_content(file, &current)
}

pub fn mark_committed_with_current_content(file: &Path, current_content: &str) -> Result<()> {
    let committed_after_replay =
        load_active(file)?.is_some_and(|capture| capture.state == CaptureState::Replayed);
    agent_doc_cycle_state_io::mark_write_applied(
        file,
        "capture_commit_write_applied",
        None,
        Some(current_content),
    )?;
    agent_doc_cycle_state_io::mark_committed(
        file,
        "capture_committed",
        None,
        Some(current_content),
    )?;
    if committed_after_replay {
        agent_doc_ops_log_io::log_op(
            file,
            &format!("capture_committed_after_replay file={}", file.display()),
        );
    }
    Ok(())
}

pub fn mark_discarded(file: &Path) -> Result<()> {
    let Some(capture) = load_active(file)? else {
        return Ok(());
    };
    agent_doc_cycle_state_io::retire_projected_captured_response(
        file,
        &capture.cycle_id,
        &capture.capture_id,
        "capture_discarded",
    )?;
    Ok(())
}

/// Reactivate only the exact capture that the stale-capture repair path
/// discarded while its retained CRDT write was still in flight. The caller
/// must separately prove response materialization and restore the matching
/// cycle projection; ordinary discarded captures remain terminal.
pub fn reactivate_false_stale_retirement(
    file: &Path,
    capture_id: &str,
    expected_response_sha256: &str,
) -> Result<bool> {
    let Some(record) = load_by_id(file, capture_id)? else {
        return Ok(false);
    };
    if record.capture_id != capture_id
        || record.response_sha256 != expected_response_sha256
        || record.response_body.trim().is_empty()
    {
        return Ok(false);
    }
    if record.state == CaptureState::Captured && record.discarded_at.is_none() {
        return Ok(true);
    }
    if record.state != CaptureState::Discarded || record.discarded_at.is_none() {
        return Ok(false);
    }

    if !agent_doc_cycle_state_io::reactivate_false_stale_capture_retirement(
        file,
        capture_id,
        expected_response_sha256,
    )? {
        return Ok(false);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "capture_false_stale_retirement_reactivated file={} capture_id={} cycle_id={} response_sha256={}",
            file.display(),
            record.capture_id,
            record.cycle_id,
            record.response_sha256,
        ),
    );
    Ok(true)
}

/// Retire the ledger capture whose response body was just archived out of the
/// live document by `compact`.
///
/// Compaction is the authority that decided to archive those responses, so a
/// capture whose `response_body` is contained in `archived_text` (intact or
/// after prompt-prefix normalization) is provably terminal — the response was
/// committed before and is now intentionally gone from the document. Discarding
/// it prevents a later `route` drain / [`validate_replay`] from fail-closing
/// with "captured response baseline no longer matches current document" (which
/// otherwise forces a manual `reset --from-current --preserve-session`).
///
/// Containment against the *archived* text — not mere absence from the current
/// document — is the safe discriminator: a never-applied / genuinely pending
/// capture's response would not appear in the committed content being archived,
/// so it is left untouched. Returns the number of captures discarded.
pub fn discard_captures_for_archived_responses(file: &Path, archived_text: &str) -> Result<usize> {
    if archived_text.trim().is_empty() {
        return Ok(0);
    }
    let Some(record) = load_active(file)? else {
        return Ok(0);
    };
    if record.response_body.trim().is_empty() {
        return Ok(0);
    }
    let response_archived = agent_doc_turn::response_replay::response_already_applied(
        archived_text,
        &record.response_body,
    )
        || agent_doc_turn::response_replay::response_already_applied_after_prefix_strip(
            archived_text,
            &record.response_body,
        );
    if !response_archived {
        return Ok(0);
    }
    agent_doc_cycle_state_io::retire_projected_captured_response(
        file,
        &record.cycle_id,
        &record.capture_id,
        "response_archived_by_compaction",
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "capture_retired_for_archived_response file={} capture_id={}",
            file.display(),
            record.capture_id
        ),
    );
    Ok(1)
}

fn metadata_from_frontmatter(file_content: &str) -> CaptureMetadata {
    let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(file_content) else {
        return CaptureMetadata::default();
    };
    let resolved = fm.resolve_mode();
    let harness = agent_doc_model_tier::detect_harness();
    let model_config = agent_doc_model_tier::ModelConfig::default();
    let resolved_model = fm
        .resolve_harness_model(&harness)
        .map(|s| agent_doc_model_tier::canonical_model_name(s, &harness, &model_config));
    CaptureMetadata {
        session_id: fm.session,
        agent: fm.agent,
        model: resolved_model,
        document_format: Some(resolved.format.to_string()),
        write_strategy: Some(resolved.write.to_string()),
    }
}

fn checkpoint_capture_projection(file: &Path, record: &CaptureRecord) -> Result<()> {
    agent_doc_cycle_state_io::append_response_captured_body(
        file,
        agent_doc_cycle_state_io::CapturedResponseFactInput {
            cycle_id: &record.cycle_id,
            capture_id: &record.capture_id,
            response_sha256: &record.response_sha256,
            response_body: &record.response_body,
            intent_body: record.intent_body.as_deref(),
            mutation_plan_json: record.mutation_plan_json.as_deref(),
            file_hash: record.file_hash.as_deref(),
            snapshot_hash: record.snapshot_hash.as_deref(),
            baseline_content: record.baseline_content.as_deref(),
        },
    )?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    #[test]
    fn capture_response_persists_record_and_cycle_metadata() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nsession: sid\nagent: codex\nmodel: gpt-5\n---\n\n## User\n\nHello\n",
        )
        .unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &std::fs::read_to_string(&doc).unwrap(),
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let record = capture_response(&doc, "response body").unwrap();
        let active = load_active(&doc).unwrap().unwrap();
        let cycle = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();

        assert_eq!(record, active);
        assert_eq!(record.state, CaptureState::Captured);
        assert_eq!(record.session_id.as_deref(), Some("sid"));
        assert_eq!(cycle.phase, agent_doc_turn::CyclePhase::ResponseCaptured);
        assert_eq!(
            cycle.capture_id.as_deref(),
            Some(record.capture_id.as_str())
        );
        let projected =
            agent_doc_cycle_state_io::load_projected_captured_response(&doc, &record.capture_id)
                .unwrap()
                .expect("captured response projection");
        assert_eq!(projected.response_sha256, record.response_sha256);
        assert_eq!(projected.response_body, "response body");
        assert_eq!(projected.file_hash, record.file_hash);
        assert_eq!(projected.snapshot_hash, record.snapshot_hash);
        let captured_baseline = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            record.baseline_content.as_deref(),
            Some(captured_baseline.as_str())
        );
        assert_eq!(projected.baseline_content, record.baseline_content);
    }

    #[test]
    fn exact_false_stale_retirement_can_reactivate_same_capture() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nsession: sid\nagent: codex\nmodel: gpt-5\n---\n\n## User\n\nHello\n",
        )
        .unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &std::fs::read_to_string(&doc).unwrap(),
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let record = capture_response(&doc, "response body").unwrap();
        mark_discarded(&doc).unwrap();
        assert!(
            reactivate_false_stale_retirement(&doc, &record.capture_id, &record.response_sha256,)
                .unwrap()
        );
        let reactivated = load_active(&doc).unwrap().unwrap();
        assert_eq!(reactivated.capture_id, record.capture_id);
        assert_eq!(reactivated.state, CaptureState::Captured);
        assert!(reactivated.discarded_at.is_none());
        assert!(
            reactivate_false_stale_retirement(&doc, &record.capture_id, &record.response_sha256,)
                .unwrap(),
            "ledger recovery must remain idempotent"
        );
        assert!(
            !reactivate_false_stale_retirement(&doc, "wrong", &record.response_sha256).unwrap()
        );
    }

    #[test]
    fn reconciles_reordered_partial_response_lines_without_losing_operator_edits() {
        let baseline = "before\nanchor\nafter\n";
        let response = concat!(
            "### Re: topic — gpt-5\n\n",
            "- editor-buffer save uses the live target;\n",
            "- snapshot staging uses the committed target;\n",
            "- relay convergence stays live.\n",
        );
        let current = concat!(
            "before\n",
            "operator note\n",
            "- relay convergence stays live.\n",
            "- snapshot staging uses the committed target;\n",
            "anchor\n",
            "- editor-buffer save uses the live target;\n",
            "after\n",
        );

        let reconciled = reconcile_partial_response_lines(baseline, current, response)
            .expect("partial response should reconcile");
        assert_eq!(reconciled.removed_nonblank_lines, 3);
        assert_eq!(reconciled.content, "before\noperator note\nanchor\nafter\n");
    }

    #[test]
    fn partial_response_reconciliation_requires_multi_line_proof() {
        assert!(
            reconcile_partial_response_lines(
                "before\nafter\n",
                "before\n- possibly operator text\nafter\n",
                "### Re: topic — gpt-5\n\n- possibly operator text\n",
            )
            .is_none()
        );
    }

    #[test]
    fn partial_response_reconciliation_keeps_semantically_materialized_response() {
        let baseline = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n❯ topic\n<!-- /agent:exchange -->\n";
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — gpt-5\n\n",
            "Line one.\nLine two.\n",
            "<!-- no-pending-capture -->\n",
            "<!-- /patch:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ topic\n",
            "### Re: topic — gpt-5\n\n",
            "Line one.\nLine two.\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(
            agent_doc_turn::response_replay::response_materialized_in_content(response, current)
        );
        assert!(reconcile_partial_response_lines(baseline, current, response).is_none());
    }

    #[test]
    fn partial_checkpoint_persists_without_advancing_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: sid\nagent: codex\ncodex_model: gpt-5\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange -->\n❯ Do work\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &std::fs::read_to_string(&doc).unwrap(),
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(
            &doc,
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .as_deref(),
            Some(&std::fs::read_to_string(&doc).unwrap()),
        )
        .unwrap();

        let mut writer = PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
        let checkpoint = writer
            .maybe_checkpoint("partial streamed response")
            .unwrap()
            .unwrap();
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        let loaded = latest_partial_checkpoint(&doc).unwrap().unwrap();

        assert_eq!(checkpoint, loaded);
        assert_eq!(loaded.response_body, "partial streamed response");
        assert_eq!(loaded.checkpoint_count, 1);
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
        assert!(state.capture_id.is_none());
    }

    #[test]
    fn partial_checkpoint_stops_after_cycle_commits() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();

        let mut writer = PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
        assert!(writer.maybe_checkpoint("first").unwrap().is_some());
        agent_doc_cycle_state_io::mark_committed(&doc, "test", Some("body"), Some("body")).unwrap();

        assert!(writer.maybe_checkpoint("second").unwrap().is_none());
        let loaded = latest_partial_checkpoint(&doc).unwrap().unwrap();
        assert_eq!(loaded.response_body, "first");
    }

    #[test]
    fn partial_checkpoint_stops_after_committed_ledger_phase() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();
        let mut writer = PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
        assert!(writer.maybe_checkpoint("first").unwrap().is_some());
        agent_doc_cycle_state_io::mark_committed(&doc, "test", Some("body"), Some("body")).unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::Committed,
            "the committed ledger phase is authoritative"
        );
        assert!(writer.maybe_checkpoint("second").unwrap().is_none());
        let loaded = latest_partial_checkpoint(&doc).unwrap().unwrap();
        assert_eq!(loaded.response_body, "first");
    }

    #[test]
    fn partial_checkpoint_stops_after_cycle_is_abandoned() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();

        let mut writer = PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
        assert!(writer.maybe_checkpoint("first").unwrap().is_some());
        agent_doc_cycle_state_io::mark_abandoned(&doc, "test", Some("body"), Some("body")).unwrap();

        assert!(writer.maybe_checkpoint("second").unwrap().is_none());
        let loaded = latest_partial_checkpoint(&doc).unwrap().unwrap();
        assert_eq!(loaded.response_body, "first");

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("partial_response_checkpoint_stopped"),
            "abandoned-cycle checkpoint stop should be logged:\n{log}"
        );
        assert!(
            log.contains("reason=cycle_closed"),
            "same-cycle abandoned state should be reported as a closed cycle:\n{log}"
        );
    }

    #[test]
    fn discard_captures_for_archived_responses_discards_only_archived() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();

        // The active capture is a committed response that compaction is about to archive.
        let archived =
            capture_response(&doc, "### Re: old — opus-4-8\n\nArchived answer A.\n").unwrap();

        let archived_text =
            "Preamble.\n### Re: old — opus-4-8\n\nArchived answer A.\nmore archive\n";
        let discarded = discard_captures_for_archived_responses(&doc, archived_text).unwrap();
        assert_eq!(
            discarded, 1,
            "the archived active capture should be retired"
        );

        assert_eq!(
            load_by_id(&doc, &archived.capture_id)
                .unwrap()
                .unwrap()
                .state,
            CaptureState::Discarded,
        );
        assert!(load_active(&doc).unwrap().is_none());

        // Idempotent: a second pass discards nothing new.
        assert_eq!(
            discard_captures_for_archived_responses(&doc, archived_text).unwrap(),
            0,
        );
    }

    #[test]
    fn terminal_cycle_never_exposes_retained_capture_as_active() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();
        let capture = capture_response(&doc, "### Re: topic — gpt-5\n\nDone.\n").unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "test_write",
            Some("body"),
            Some("body"),
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(&doc, "test_commit", Some("body"), Some("body"))
            .unwrap();

        assert!(load_by_id(&doc, &capture.capture_id).unwrap().is_some());
        assert!(load_active(&doc).unwrap().is_none());
        assert_eq!(
            latest_committed(&doc).unwrap().unwrap().capture_id,
            capture.capture_id
        );
    }

    #[test]
    fn partial_checkpoint_skips_unchanged_response() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some("body"), Some("body")).unwrap();

        let mut writer = PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
        assert!(writer.maybe_checkpoint("same").unwrap().is_some());
        assert!(writer.maybe_checkpoint("same").unwrap().is_none());
        assert!(writer.maybe_checkpoint("changed").unwrap().is_some());

        let loaded = latest_partial_checkpoint(&doc).unwrap().unwrap();
        assert_eq!(loaded.response_body, "changed");
        assert_eq!(loaded.checkpoint_count, 2);
    }

    #[test]
    fn replay_baseline_drifted_tracks_file_divergence() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = capture_response(&doc, "response body").unwrap();

        assert!(
            !replay_baseline_drifted(&doc, &capture).unwrap(),
            "matching baseline must not be reported as drifted"
        );

        std::fs::write(&doc, "body changed").unwrap();
        assert!(
            replay_baseline_drifted(&doc, &capture).unwrap(),
            "diverged file content must be reported as drifted"
        );
    }

    #[test]
    fn validate_replay_rejects_diverged_file_hash() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = capture_response(&doc, "response body").unwrap();

        std::fs::write(&doc, "body changed").unwrap();
        let err = validate_replay(&doc, &capture).unwrap_err();
        assert!(err.to_string().contains("baseline no longer matches"));
        assert!(
            err.to_string()
                .contains("agent-doc reset --from-current --preserve-session")
        );
    }

    #[test]
    fn validate_replay_rebases_after_proven_whole_document_replay_coalescence() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        let canonical = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "Operator prompt\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
        );
        let replayed = format!("{canonical}{canonical}");
        std::fs::write(&doc, &replayed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &replayed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&replayed), Some(&replayed)).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &replayed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::record_turn_checkpoint(
            &doc,
            &["Operator prompt".to_string()],
            None,
            None,
        )
        .unwrap();
        let capture = capture_response(
            &doc,
            "<!-- patch:exchange -->\n### Re: prompt — gpt-5\n\nRecovered.\n<!-- /patch:exchange -->\n",
        )
        .unwrap();

        let divergent = canonical.replace("Operator prompt", "Operator changed prompt");
        assert_eq!(
            captured_baseline_coalesces_to_current(&doc, &capture, &divergent).unwrap(),
            None,
            "structurally divergent operator text must remain fail-closed",
        );

        std::fs::write(&doc, canonical).unwrap();
        validate_replay(&doc, &capture)
            .expect("proven whole-document replay coalescence should safely rebase replay");

        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .as_deref(),
            Some(replayed.as_str()),
            "hot replay validation must not rewrite the cold snapshot projection",
        );
        let refreshed = load_active(&doc).unwrap().unwrap();
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(replay_file_hash(canonical).as_str()),
        );
        assert_eq!(refreshed.baseline_content.as_deref(), Some(canonical));
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("capture_replay_baseline_coalesced")
                && log.contains("reason=whole_document_replay_coalesced_baseline"),
            "coalesced replay recovery must leave a specific audit trail:\n{log}",
        );
    }

    #[test]
    fn validate_replay_refreshes_baseline_for_queue_only_drift() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        let original = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "user prompt\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- first head\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = capture_response(
            &doc,
            "<!-- patch:exchange -->\n### Re: first head — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n",
        )
        .unwrap();

        let current = original.replace(
            "- first head\n",
            "- first head\n- user typed a new queue note during closeout\n",
        );
        std::fs::write(&doc, &current).unwrap();

        validate_replay(&doc, &capture).expect("queue-only drift should allow replay");

        let refreshed = load_active(&doc).unwrap().unwrap();
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(replay_file_hash(&current).as_str()),
            "file_hash should be refreshed to the live queue-edited document"
        );
        assert_eq!(
            refreshed.snapshot_hash.as_deref(),
            Some(agent_doc_hash::content_hash(&current).as_str()),
            "snapshot_hash should identify the logical baseline without becoming live authority"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("capture_baseline_refreshed_for_queue_only_drift"),
            "queue-only refresh must be logged for audit:\n{log}"
        );
        assert!(
            log.contains("closeout_recovery_mutation")
                && log.contains("reason=queue_only_replay_baseline"),
            "queue-only replay refresh must go through the shared recovery mutation primitive:\n{log}"
        );
    }

    /// Phase 2 of #adoc-baseline-drift-after-user-commit: when the captured
    /// `response_body` is still intact in the current document, `validate_replay`
    /// auto-refreshes the capture's `file_hash` / `snapshot_hash` instead of
    /// bailing — this is the user-committed-on-top benign-drift recovery path.
    #[test]
    fn validate_replay_refreshes_baseline_when_response_intact_after_user_commit() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        let original = "## Exchange\n\nold body\n";
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let response = "### Re: topic — gpt-5\n\nIntact response body.\n";
        let capture = capture_response(&doc, response).unwrap();

        // User committed a benign edit: added an unrelated backlog item but
        // left the response body untouched.
        let after_user_commit = "## Exchange\n\nold body\n### Re: topic — gpt-5\n\nIntact response body.\n\n## Backlog\n\n- new item added by user\n";
        std::fs::write(&doc, after_user_commit).unwrap();
        assert_ne!(
            agent_doc_hash::content_hash(after_user_commit),
            capture.file_hash.clone().unwrap()
        );

        validate_replay(&doc, &capture).expect("benign drift must auto-refresh");

        let refreshed = load_active(&doc).unwrap().unwrap();
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(agent_doc_hash::content_hash(after_user_commit).as_str()),
            "file_hash should be refreshed to current"
        );
        assert_eq!(
            refreshed.snapshot_hash.as_deref(),
            Some(agent_doc_hash::content_hash(after_user_commit).as_str())
        );
        assert_eq!(
            refreshed.baseline_content.as_deref(),
            Some(after_user_commit),
            "the ledger baseline should advance without a snapshot sidecar"
        );
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .as_deref(),
            Some(original),
            "hot validation must not rewrite the cold snapshot projection"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("capture_baseline_refreshed_for_benign_drift"),
            "refresh must be logged for audit:\n{log}"
        );
        assert!(
            log.contains("closeout_recovery_mutation")
                && log.contains("reason=benign_replay_baseline"),
            "benign replay refresh must go through the shared recovery mutation primitive:\n{log}"
        );
    }

    /// Phase 2/3 of #adoc-prefix-strip-uncommitted: when the captured
    /// response_body had spurious `❯ ` markers on response prose lines
    /// (e.g. from the JB cache-conflict prefix spill) and the user has
    /// since stripped them with sed, the `response_already_applied_after_prefix_strip`
    /// match accepts the cleaned working tree and refreshes the baseline.
    #[test]
    fn validate_replay_adopts_user_prefix_stripped_response_body() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        let original = "## Exchange\n\nuser prompt\n";
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        // Captured body simulates the JB cache-conflict spill: a stray "❯ "
        // accidentally prepended to one of the agent's response prose lines.
        let captured_response =
            "### Re: topic — gpt-5\n\nImplemented and verified.\n❯ Submodule pointer updated.\n";
        let capture = capture_response(&doc, captured_response).unwrap();

        // User ran sed (or equivalent) to strip the spurious ❯ markers.
        let cleaned_doc = "## Exchange\n\nuser prompt\n### Re: topic — gpt-5\n\nImplemented and verified.\nSubmodule pointer updated.\n";
        std::fs::write(&doc, cleaned_doc).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            cleaned_doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        validate_replay(&doc, &capture)
            .expect("user-normalized prefix strip must auto-refresh the baseline");

        let refreshed = load_active(&doc).unwrap().unwrap();
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(agent_doc_hash::content_hash(cleaned_doc).as_str()),
            "file_hash should reflect the user-cleaned document"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("capture_baseline_refreshed_for_benign_drift"),
            "prefix-strip refresh should use the same audit event:\n{log}"
        );
    }

    #[test]
    fn validate_replay_adopts_materialized_patch_body_with_visible_prefix_normalization() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        let original = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\nuser prompt\n<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let captured_response = concat!(
            "<!-- patch:exchange -->\n",
            "Implemented and verified.\n",
            "- Preserved operator content.\n",
            "<!-- no-pending-capture -->\n",
            "<!-- /patch:exchange -->\n",
        );
        let capture = capture_response(&doc, captured_response).unwrap();

        let materialized = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\nuser prompt\n\n",
            "### Re: user prompt — gpt-5\n\n",
            "❯ Implemented and verified.\n",
            "- Preserved operator content.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, materialized).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            materialized,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        validate_replay(&doc, &capture)
            .expect("materialized patch body must refresh the stale capture baseline");

        let refreshed = load_active(&doc).unwrap().unwrap();
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(agent_doc_hash::content_hash(materialized).as_str())
        );
    }

    /// Even with benign-drift refresh enabled, a divergence that actually
    /// removes or rewrites the captured response body must still fail closed.
    #[test]
    fn validate_replay_still_bails_when_response_body_removed_from_current() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        let original = "## Exchange\n\nold body\n### Re: topic — gpt-5\n\nOriginal response.\n";
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture =
            capture_response(&doc, "### Re: topic — gpt-5\n\nOriginal response.\n").unwrap();

        // User clobbered the response body — true corruption, not benign.
        let damaged = "## Exchange\n\nold body\n";
        std::fs::write(&doc, damaged).unwrap();

        let err = validate_replay(&doc, &capture).unwrap_err();
        assert!(
            err.to_string().contains("baseline no longer matches")
                || err.to_string().contains("snapshot no longer matches"),
            "expected the existing fail-closed message; got: {err}"
        );
    }

    #[test]
    fn mark_committed_updates_capture_state() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = capture_response(&doc, "response body").unwrap();

        mark_committed(&doc).unwrap();
        let committed = load_by_id(&doc, &capture.capture_id).unwrap().unwrap();
        assert_eq!(committed.state, CaptureState::Committed);
        assert!(committed.committed_at.is_some());
    }

    #[test]
    fn mark_write_applied_does_not_regress_committed_capture() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = capture_response(&doc, "response body").unwrap();

        mark_committed(&doc).unwrap();
        mark_write_applied(&doc).unwrap();

        let committed = load_by_id(&doc, &capture.capture_id).unwrap().unwrap();
        assert_eq!(committed.state, CaptureState::Committed);
    }

    #[test]
    fn mark_replayed_records_terminal_provenance_without_reopening_capture() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = capture_response(&doc, "response body").unwrap();

        mark_committed(&doc).unwrap();
        mark_replayed(&doc).unwrap();

        let committed = load_by_id(&doc, &capture.capture_id).unwrap().unwrap();
        assert_eq!(committed.state, CaptureState::Committed);
        assert!(committed.replayed_at.is_some());
        assert!(committed.committed_at.is_some());
    }

    #[test]
    fn committed_capture_preserves_replay_provenance() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = capture_response(&doc, "response body").unwrap();

        mark_replayed(&doc).unwrap();
        mark_committed(&doc).unwrap();

        let committed = load_by_id(&doc, &capture.capture_id).unwrap().unwrap();
        assert_eq!(committed.state, CaptureState::Committed);
        assert!(committed.replayed_at.is_some());
        assert!(committed.committed_at.is_some());
    }

    #[test]
    fn repeated_mark_committed_does_not_relog_replay_provenance() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let capture = capture_response(&doc, "response body").unwrap();

        mark_replayed(&doc).unwrap();
        mark_committed(&doc).unwrap();
        let first = load_by_id(&doc, &capture.capture_id).unwrap().unwrap();
        mark_committed(&doc).unwrap();
        let second = load_by_id(&doc, &capture.capture_id).unwrap().unwrap();

        assert_eq!(second, first);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert_eq!(
            log.matches("capture_committed_after_replay").count(),
            1,
            "terminal replay provenance should be logged once:\n{log}"
        );
    }
}
