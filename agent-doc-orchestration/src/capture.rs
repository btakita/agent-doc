//! # Module: capture
//!
//! ## Spec
//! - Persists a durable response-capture ledger under
//!   `.agent-doc/captures/<doc-hash>/<cycle-id>.json`.
//! - Persists the latest in-progress streamed response checkpoint under
//!   `.agent-doc/captures/<doc-hash>/<cycle-id>.partial.json`.
//! - Captures the final parsed response body plus cycle metadata before any
//!   document write or hook emission.
//! - `capture_response(file, response)` stores the response, marks the cycle as
//!   `response_captured`, and returns the saved record.
//! - `PartialCheckpointWriter` saves the first non-empty partial response and
//!   then checkpoints changed partial output at most once every 30 seconds.
//! - `load_active(file)` resolves the active capture from cycle state.
//! - `validate_replay(file, capture)` fail-closes replay when the current file
//!   or snapshot hashes no longer match the captured baseline.
//! - `mark_write_applied`, `mark_replayed`, `mark_committed`, and
//!   `mark_discarded` advance the capture lifecycle in-place. Duplicate
//!   terminal updates are idempotent and do not re-log replay provenance.
//!
//! ## Agentic Contracts
//! - The capture ledger is document-scoped; one active capture is associated
//!   with one response cycle.
//! - Capture writes are atomic JSON replacements.
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

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PARTIAL_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    Captured,
    WriteApplied,
    Replayed,
    Committed,
    Discarded,
}

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
    pub response_sha256: String,
    pub response_body: String,
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
        let cycle_id = crate::cycle_state::load(file)
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
        if self.stopped || !self.active_cycle_accepts_checkpoint()? {
            return Ok(None);
        }
        if response.trim().is_empty() {
            return Ok(None);
        }

        let response_sha256 = crate::ops_log::content_hash(response);
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
        let record = checkpoint_partial_response_for_cycle(
            &self.file,
            response,
            self.checkpoint_count,
            &self.cycle_id,
        )?;
        self.last_checkpoint = Some(Instant::now());
        self.last_response_sha256 = Some(response_sha256);
        Ok(Some(record))
    }

    fn active_cycle_accepts_checkpoint(&mut self) -> Result<bool> {
        let Some(state) = crate::cycle_state::load(&self.file)? else {
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
        crate::ops_log::log_op(
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
    let snapshot_content = crate::snapshot::load(file)?;
    let response_sha256 = crate::ops_log::content_hash(response);
    let existing_cycle_id = crate::cycle_state::load(file)?.map(|s| s.cycle_id);
    let capture_id = existing_cycle_id.unwrap_or_else(|| format!("synthetic-{}", now_millis()));
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());

    let metadata = metadata_from_frontmatter(&file_content);

    // Redact secrets from the response body before it lands in the capture
    // sidecar JSON. The `response_sha256` keeps the original-bytes hash so
    // cycle-state correlation (which references the live in-memory response)
    // stays intact — only the persisted body changes.
    let redacted_response = crate::secret_redact::redact(response);

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
        snapshot_hash: snapshot_content
            .as_deref()
            .map(crate::ops_log::content_hash),
        file_hash: Some(replay_file_hash(&file_content)),
        response_sha256: response_sha256.clone(),
        response_body: redacted_response,
        state: CaptureState::Captured,
    };
    write_record(file, &record)?;
    crate::cycle_state::mark_response_captured(
        file,
        "response_captured",
        snapshot_content.as_deref(),
        Some(&file_content),
        &response_sha256,
        Some(&capture_id),
    )?;
    Ok(record)
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
    let snapshot_content = crate::snapshot::load(file)?;
    let response_sha256 = crate::ops_log::content_hash(response);
    let checkpoint_id = format!("{cycle_id}-partial");
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let metadata = metadata_from_frontmatter(&file_content);
    let existing = load_partial_by_cycle(file, cycle_id)?;
    let captured_at = existing
        .as_ref()
        .map_or_else(now_secs, |record| record.captured_at);

    let redacted_response = crate::secret_redact::redact(response);
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
        snapshot_hash: snapshot_content
            .as_deref()
            .map(crate::ops_log::content_hash),
        file_hash: Some(replay_file_hash(&file_content)),
        response_sha256,
        response_body: redacted_response,
    };
    write_partial_record(file, &record)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "partial_response_checkpoint file={} cycle={} count={} sha256={}",
            file.display(),
            cycle_id,
            checkpoint_count,
            record.response_sha256
        ),
    );
    Ok(record)
}

pub fn load_active(file: &Path) -> Result<Option<CaptureRecord>> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(None);
    };
    load_by_id(file, capture_id)
}

pub fn load_by_id(file: &Path, capture_id: &str) -> Result<Option<CaptureRecord>> {
    let path = capture_path_for(file, capture_id)?;
    let Some(content) = crate::fs_util::read_optional_text(&path)
        .with_context(|| format!("failed to read capture {}", path.display()))?
    else {
        return Ok(None);
    };
    let record: CaptureRecord = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse capture {}", path.display()))?;
    Ok(Some(record))
}

pub fn load_partial_by_cycle(file: &Path, cycle_id: &str) -> Result<Option<PartialCaptureRecord>> {
    let path = partial_capture_path_for(file, cycle_id)?;
    let Some(content) = crate::fs_util::read_optional_text(&path)
        .with_context(|| format!("failed to read partial capture {}", path.display()))?
    else {
        return Ok(None);
    };
    let record: PartialCaptureRecord = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse partial capture {}", path.display()))?;
    Ok(Some(record))
}

#[allow(dead_code)]
pub fn latest_partial_checkpoint(file: &Path) -> Result<Option<PartialCaptureRecord>> {
    let canonical = file.canonicalize()?;
    let hash = crate::snapshot::doc_hash(&canonical)?;
    let project_root = crate::snapshot::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let dir = project_root.join(".agent-doc/captures").join(hash);
    if !dir.exists() {
        return Ok(None);
    }

    let mut latest: Option<PartialCaptureRecord> = None;
    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("failed to read capture directory {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        if !file_name.to_string_lossy().ends_with(".partial.json") {
            continue;
        }
        let content = std::fs::read_to_string(entry.path()).with_context(|| {
            format!("failed to read partial capture {}", entry.path().display())
        })?;
        let record: PartialCaptureRecord = serde_json::from_str(&content).with_context(|| {
            format!("failed to parse partial capture {}", entry.path().display())
        })?;
        if latest
            .as_ref()
            .is_none_or(|current| record.updated_at > current.updated_at)
        {
            latest = Some(record);
        }
    }

    Ok(latest)
}

pub fn latest_committed(file: &Path) -> Result<Option<CaptureRecord>> {
    let canonical = file.canonicalize()?;
    let hash = crate::snapshot::doc_hash(&canonical)?;
    let project_root = crate::snapshot::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let dir = project_root.join(".agent-doc/captures").join(hash);
    if !dir.exists() {
        return Ok(None);
    }

    let mut latest: Option<CaptureRecord> = None;
    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("failed to read capture directory {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        if entry
            .file_name()
            .to_string_lossy()
            .ends_with(".partial.json")
        {
            continue;
        }
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(entry.path())
            .with_context(|| format!("failed to read capture {}", entry.path().display()))?;
        let record: CaptureRecord = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse capture {}", entry.path().display()))?;
        if record.state != CaptureState::Committed {
            continue;
        }
        let is_newer = latest.as_ref().is_none_or(|current| {
            record.committed_at.unwrap_or(record.updated_at)
                > current.committed_at.unwrap_or(current.updated_at)
        });
        if is_newer {
            latest = Some(record);
        }
    }

    Ok(latest)
}

/// #22a8 (Phase 5b write-side): hash the document for capture replay/commit
/// validation with the managed `agent_doc_pipeline:` frontmatter block removed.
/// The block is mirrored onto disk mid-cycle (after response capture, cleared at
/// a terminal phase), so a raw hash would read that managed write as document
/// drift and fail the replay baseline. Stripping it keeps replay validation
/// invariant to the mirror, matching the diff layer.
pub(crate) fn replay_file_hash(content: &str) -> String {
    crate::ops_log::content_hash(&agent_doc_core::frontmatter::strip_pipeline_block_lines(
        content,
    ))
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
    let current_file_hash = replay_file_hash(&current_file);
    let current_snapshot = crate::snapshot::load(file)?;
    let current_snapshot_hash = current_snapshot
        .as_deref()
        .map(crate::ops_log::content_hash);
    let file_mismatch = capture.file_hash.as_deref() != Some(current_file_hash.as_str());
    let snapshot_mismatch = capture.snapshot_hash != current_snapshot_hash;
    Ok(file_mismatch || snapshot_mismatch)
}

pub fn validate_replay(file: &Path, capture: &CaptureRecord) -> Result<()> {
    let current_file = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for capture replay", file.display()))?;
    let current_file_hash = replay_file_hash(&current_file);
    let current_snapshot = crate::snapshot::load(file)?;
    let current_snapshot_hash = current_snapshot
        .as_deref()
        .map(crate::ops_log::content_hash);

    let file_mismatch = capture.file_hash.as_deref() != Some(current_file_hash.as_str());
    let snapshot_mismatch = capture.snapshot_hash != current_snapshot_hash;

    if !file_mismatch && !snapshot_mismatch {
        return Ok(());
    }

    // Phase 2 of #adoc-baseline-drift-after-user-commit: when the captured
    // response body is still present in the working tree (intact), treat the
    // hash mismatch as benign drift — a user committed something on top of
    // the prior agent-doc commit but did not touch the response body. Refresh
    // the capture's file/snapshot hashes from the current state and proceed.
    //
    // Plan: tasks/agent-doc/plan-baseline-drift-after-user-commit.md
    if response_body_intact_in_current(file, &capture.response_body, &current_file)? {
        crate::flow::closeout::apply_closeout_recovery_mutation(
            file,
            crate::flow::closeout::CloseoutRecoveryMutation::RefreshReplayBaseline {
                capture,
                current_file_hash: &current_file_hash,
                current_snapshot_hash: current_snapshot_hash.as_deref(),
                reason: crate::flow::closeout::CloseoutRecoveryMutationReason::BenignReplayBaseline,
            },
        )?;
        return Ok(());
    }

    // #queueeditcap: a captured response may be interrupted after capture but
    // before write/commit while the operator edits only `agent:queue`. In that
    // shape the snapshot is still the captured baseline, so replaying the
    // response onto the current file preserves the queue edit and completes the
    // closeout instead of deadlocking route/JB Run Agent Doc behind a manual
    // reset.
    if file_mismatch
        && !snapshot_mismatch
        && live_drift_is_queue_only_against_snapshot(&current_file, current_snapshot.as_deref())?
    {
        crate::flow::closeout::apply_closeout_recovery_mutation(
            file,
            crate::flow::closeout::CloseoutRecoveryMutation::RefreshReplayBaseline {
                capture,
                current_file_hash: &current_file_hash,
                current_snapshot_hash: current_snapshot_hash.as_deref(),
                reason:
                    crate::flow::closeout::CloseoutRecoveryMutationReason::QueueOnlyReplayBaseline,
            },
        )?;
        return Ok(());
    }

    if file_mismatch {
        anyhow::bail!(
            "captured response baseline no longer matches current document for {}. Rebuild sidecars without clearing session state: `agent-doc reset --from-current --preserve-session {}`",
            file.display(),
            file.display()
        );
    }
    anyhow::bail!(
        "captured response snapshot no longer matches current baseline for {}. Rebuild sidecars without clearing session state: `agent-doc reset --from-current --preserve-session {}`",
        file.display(),
        file.display()
    );
}

/// Returns true when the captured `response_body` is still contiguously
/// present in the current document (modulo blank-line / transient marker
/// normalization, and modulo `❯ ` prompt-prefix markers stripped by the
/// user after a JB cache-conflict-induced prefix spill). Defers to
/// `repair::response_already_applied` for the strict match and falls back
/// to `repair::response_already_applied_after_prefix_strip` for the
/// post-strip recovery path covered by `#adoc-prefix-strip-uncommitted`.
fn response_body_intact_in_current(
    file: &Path,
    response_body: &str,
    current_file: &str,
) -> Result<bool> {
    if response_body.trim().is_empty() {
        return Ok(false);
    }
    let _ = file; // reserved for richer structural checks (#adoc-bdauc stretch goals)
    if crate::repair::response_already_applied(current_file, response_body) {
        return Ok(true);
    }
    Ok(crate::repair::response_already_applied_after_prefix_strip(
        current_file,
        response_body,
    ))
}

pub(crate) fn live_drift_is_queue_only_against_snapshot(
    current_file: &str,
    current_snapshot: Option<&str>,
) -> Result<bool> {
    let Some(current_snapshot) = current_snapshot else {
        return Ok(false);
    };
    let current_file = agent_doc_core::frontmatter::strip_pipeline_block_lines(current_file);
    let current_snapshot =
        agent_doc_core::frontmatter::strip_pipeline_block_lines(current_snapshot);
    if current_file == current_snapshot {
        return Ok(false);
    }

    let current_components = agent_doc_element::element::parse(&current_file)?;
    let snapshot_components = agent_doc_element::element::parse(&current_snapshot)?;
    let mut current_queues = current_components.iter().filter(|c| c.name == "queue");
    let mut snapshot_queues = snapshot_components.iter().filter(|c| c.name == "queue");
    let Some(current_queue) = current_queues.next() else {
        return Ok(false);
    };
    let Some(snapshot_queue) = snapshot_queues.next() else {
        return Ok(false);
    };
    if current_queues.next().is_some() || snapshot_queues.next().is_some() {
        return Ok(false);
    }

    let restored =
        current_queue.replace_content(&current_file, snapshot_queue.content(&current_snapshot));
    Ok(restored == current_snapshot)
}

/// Refresh the capture record's `file_hash` and `snapshot_hash` to match the
/// current state, after `response_body_intact_in_current` confirmed the
/// drift is benign. Logged via `ops_log` so the recovery is auditable.
pub(crate) fn refresh_replay_baseline_for_recovery(
    file: &Path,
    capture: &CaptureRecord,
    current_file_hash: &str,
    current_snapshot_hash: Option<&str>,
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
    if !changed {
        return Ok(false);
    }
    record.updated_at = now_secs();
    write_record(file, &record)?;
    crate::ops_log::log_op(
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

pub fn mark_write_applied(file: &Path) -> Result<()> {
    update_active_state(file, CaptureState::WriteApplied)
}

pub fn mark_replayed(file: &Path) -> Result<()> {
    update_active_state(file, CaptureState::Replayed)
}

pub fn mark_committed(file: &Path) -> Result<()> {
    update_active_state(file, CaptureState::Committed)
}

pub fn mark_discarded(file: &Path) -> Result<()> {
    update_active_state(file, CaptureState::Discarded)
}

/// `#stale-capture-after-compaction-blocks-route`: discard the capture sidecars
/// whose response body was just archived out of the live document by `compact`.
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
    let canonical = file.canonicalize()?;
    let hash = crate::snapshot::doc_hash(&canonical)?;
    let project_root = crate::snapshot::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let dir = project_root.join(".agent-doc/captures").join(hash);
    if !dir.exists() {
        return Ok(0);
    }

    let mut discarded = 0usize;
    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("failed to read capture directory {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".partial.json") || !name.ends_with(".json") {
            continue;
        }
        let content = std::fs::read_to_string(entry.path())
            .with_context(|| format!("failed to read capture {}", entry.path().display()))?;
        let mut record: CaptureRecord = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse capture {}", entry.path().display()))?;
        if record.state == CaptureState::Discarded || record.response_body.trim().is_empty() {
            continue;
        }
        let response_archived =
            crate::repair::response_already_applied(archived_text, &record.response_body)
                || crate::repair::response_already_applied_after_prefix_strip(
                    archived_text,
                    &record.response_body,
                );
        if !response_archived {
            continue;
        }
        if record.discarded_at.is_none() {
            record.discarded_at = Some(now_secs());
        }
        record.state = CaptureState::Discarded;
        record.updated_at = now_secs();
        write_record(file, &record)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "capture_discarded_for_archived_response file={} capture_id={}",
                file.display(),
                record.capture_id
            ),
        );
        discarded += 1;
    }
    if discarded > 0 {
        eprintln!(
            "[compact] discarded {} stale capture sidecar(s) for archived response(s) in {}",
            discarded,
            file.display()
        );
    }
    Ok(discarded)
}

fn update_active_state(file: &Path, state: CaptureState) -> Result<()> {
    let Some(mut record) = load_active(file)? else {
        return Ok(());
    };
    let prior_state = record.state.clone();
    let now = now_secs();
    if capture_state_rank(state.clone()) < capture_state_rank(record.state.clone()) {
        if matches!(state, CaptureState::Replayed)
            && matches!(record.state, CaptureState::Committed)
            && record.replayed_at.is_none()
        {
            record.replayed_at = Some(now);
            record.updated_at = now;
            return write_record(file, &record);
        }
        return Ok(());
    }
    let mut changed = false;
    match state {
        CaptureState::Captured => {}
        CaptureState::WriteApplied => {
            if record.write_applied_at.is_none() {
                record.write_applied_at = Some(now);
                changed = true;
            }
        }
        CaptureState::Replayed => {
            if record.replayed_at.is_none() {
                record.replayed_at = Some(now);
                changed = true;
            }
        }
        CaptureState::Committed => {
            if record.committed_at.is_none() {
                record.committed_at = Some(now);
                changed = true;
            }
            let current_file = std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {} for capture commit", file.display()))?;
            let current_file_hash = replay_file_hash(&current_file);
            if record.file_hash.as_deref() != Some(current_file_hash.as_str()) {
                record.file_hash = Some(current_file_hash);
                changed = true;
            }
            let current_snapshot = crate::snapshot::load(file)?;
            let current_snapshot_hash = current_snapshot
                .as_deref()
                .map(crate::ops_log::content_hash);
            if record.snapshot_hash != current_snapshot_hash {
                record.snapshot_hash = current_snapshot_hash;
                changed = true;
            }
        }
        CaptureState::Discarded => {
            if record.discarded_at.is_none() {
                record.discarded_at = Some(now);
                changed = true;
            }
        }
    }
    if record.state != state {
        record.state = state;
        changed = true;
    }
    if !changed {
        return Ok(());
    }
    record.updated_at = now;
    if matches!(record.state, CaptureState::Committed)
        && record.replayed_at.is_some()
        && !matches!(prior_state, CaptureState::Committed)
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "capture_committed_after_replay file={} capture_id={}",
                file.display(),
                record.capture_id
            ),
        );
    }
    write_record(file, &record)
}

fn capture_state_rank(state: CaptureState) -> u8 {
    match state {
        CaptureState::Captured => 0,
        CaptureState::WriteApplied => 1,
        CaptureState::Replayed => 2,
        CaptureState::Committed => 3,
        CaptureState::Discarded => 4,
    }
}

fn metadata_from_frontmatter(file_content: &str) -> CaptureMetadata {
    let Ok((fm, _)) = crate::frontmatter::parse(file_content) else {
        return CaptureMetadata::default();
    };
    let resolved = fm.resolve_mode();
    let harness = agent_doc_core::model_tier::detect_harness();
    let model_config = agent_doc_core::model_tier::ModelConfig::default();
    let resolved_model = fm
        .resolve_harness_model(&harness)
        .map(|s| agent_doc_core::model_tier::canonical_model_name(s, &harness, &model_config));
    CaptureMetadata {
        session_id: fm.session,
        agent: fm.agent,
        model: resolved_model,
        document_format: Some(resolved.format.to_string()),
        write_strategy: Some(resolved.write.to_string()),
    }
}

fn write_record(file: &Path, record: &CaptureRecord) -> Result<()> {
    let path = capture_path_for(file, &record.capture_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(record)?;
    atomic_write(&path, &json)
}

fn write_partial_record(file: &Path, record: &PartialCaptureRecord) -> Result<()> {
    let path = partial_capture_path_for(file, &record.cycle_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(record)?;
    atomic_write(&path, &json)
}

fn capture_path_for(file: &Path, capture_id: &str) -> Result<PathBuf> {
    let canonical = file.canonicalize()?;
    let hash = crate::snapshot::doc_hash(&canonical)?;
    let project_root = crate::snapshot::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root
        .join(".agent-doc/captures")
        .join(hash)
        .join(format!("{capture_id}.json")))
}

fn partial_capture_path_for(file: &Path, cycle_id: &str) -> Result<PathBuf> {
    let canonical = file.canonicalize()?;
    let hash = crate::snapshot::doc_hash(&canonical)?;
    let project_root = crate::snapshot::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root
        .join(".agent-doc/captures")
        .join(hash)
        .join(format!("{cycle_id}.partial.json")))
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;

    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    tmp.write_all(content.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    tmp.persist(path)
        .with_context(|| format!("failed to persist {}", path.display()))?;
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
        crate::snapshot::save(&doc, &std::fs::read_to_string(&doc).unwrap()).unwrap();

        let record = capture_response(&doc, "response body").unwrap();
        let active = load_active(&doc).unwrap().unwrap();
        let cycle = crate::cycle_state::load(&doc).unwrap().unwrap();

        assert_eq!(record, active);
        assert_eq!(record.state, CaptureState::Captured);
        assert_eq!(record.session_id.as_deref(), Some("sid"));
        assert_eq!(
            cycle.phase,
            crate::cycle_state::CyclePhase::ResponseCaptured
        );
        assert_eq!(
            cycle.capture_id.as_deref(),
            Some(record.capture_id.as_str())
        );
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
        crate::snapshot::save(&doc, &std::fs::read_to_string(&doc).unwrap()).unwrap();
        crate::cycle_state::start_preflight(
            &doc,
            crate::snapshot::load(&doc).unwrap().as_deref(),
            Some(&std::fs::read_to_string(&doc).unwrap()),
        )
        .unwrap();

        let mut writer = PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
        let checkpoint = writer
            .maybe_checkpoint("partial streamed response")
            .unwrap()
            .unwrap();
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        let loaded = latest_partial_checkpoint(&doc).unwrap().unwrap();

        assert_eq!(checkpoint, loaded);
        assert_eq!(loaded.response_body, "partial streamed response");
        assert_eq!(loaded.checkpoint_count, 1);
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
        assert!(state.capture_id.is_none());
    }

    #[test]
    fn partial_checkpoint_stops_after_cycle_commits() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        crate::snapshot::save(&doc, "body").unwrap();
        crate::cycle_state::start_preflight(&doc, Some("body"), Some("body")).unwrap();

        let mut writer = PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
        assert!(writer.maybe_checkpoint("first").unwrap().is_some());
        crate::cycle_state::mark_committed(&doc, "test", Some("body"), Some("body")).unwrap();

        assert!(writer.maybe_checkpoint("second").unwrap().is_none());
        let loaded = latest_partial_checkpoint(&doc).unwrap().unwrap();
        assert_eq!(loaded.response_body, "first");
    }

    #[test]
    fn partial_checkpoint_stops_after_cycle_is_abandoned() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        crate::snapshot::save(&doc, "body").unwrap();
        crate::cycle_state::start_preflight(&doc, Some("body"), Some("body")).unwrap();

        let mut writer = PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
        assert!(writer.maybe_checkpoint("first").unwrap().is_some());
        crate::cycle_state::mark_abandoned(&doc, "test", Some("body"), Some("body")).unwrap();

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
        crate::snapshot::save(&doc, "body").unwrap();
        crate::cycle_state::start_preflight(&doc, Some("body"), Some("body")).unwrap();

        // Capture A — a committed response that compaction is about to archive.
        let archived =
            capture_response(&doc, "### Re: old — opus-4-8\n\nArchived answer A.\n").unwrap();
        // Capture B — a distinct response NOT in the archive (e.g. still pending).
        let mut live = archived.clone();
        live.capture_id = "cycle-live-distinct".to_string();
        live.cycle_id = "cycle-live-distinct".to_string();
        live.response_body = "### Re: new — opus-4-8\n\nLive answer B.\n".to_string();
        live.state = CaptureState::Captured;
        live.discarded_at = None;
        write_record(&doc, &live).unwrap();

        let archived_text =
            "Preamble.\n### Re: old — opus-4-8\n\nArchived answer A.\nmore archive\n";
        let discarded = discard_captures_for_archived_responses(&doc, archived_text).unwrap();
        assert_eq!(
            discarded, 1,
            "only the archived capture should be discarded"
        );

        assert_eq!(
            load_by_id(&doc, &archived.capture_id)
                .unwrap()
                .unwrap()
                .state,
            CaptureState::Discarded,
        );
        assert_eq!(
            load_by_id(&doc, &live.capture_id).unwrap().unwrap().state,
            CaptureState::Captured,
            "a capture whose response was not archived must be left intact",
        );

        // Idempotent: a second pass discards nothing new.
        assert_eq!(
            discard_captures_for_archived_responses(&doc, archived_text).unwrap(),
            0,
        );
    }

    #[test]
    fn partial_checkpoint_skips_unchanged_response() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        crate::snapshot::save(&doc, "body").unwrap();

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
        crate::snapshot::save(&doc, "body").unwrap();
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
        crate::snapshot::save(&doc, "body").unwrap();
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
        crate::snapshot::save(&doc, original).unwrap();
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
            capture.snapshot_hash.as_deref(),
            "snapshot_hash should stay on the captured baseline until replay writes"
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
        crate::snapshot::save(&doc, original).unwrap();
        let response = "### Re: topic — gpt-5\n\nIntact response body.\n";
        let capture = capture_response(&doc, response).unwrap();

        // User committed a benign edit: added an unrelated backlog item but
        // left the response body untouched.
        let after_user_commit = "## Exchange\n\nold body\n### Re: topic — gpt-5\n\nIntact response body.\n\n## Backlog\n\n- new item added by user\n";
        std::fs::write(&doc, after_user_commit).unwrap();
        crate::snapshot::save(&doc, after_user_commit).unwrap();
        assert_ne!(
            crate::ops_log::content_hash(after_user_commit),
            capture.file_hash.clone().unwrap()
        );

        validate_replay(&doc, &capture).expect("benign drift must auto-refresh");

        let refreshed = load_active(&doc).unwrap().unwrap();
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(crate::ops_log::content_hash(after_user_commit).as_str()),
            "file_hash should be refreshed to current"
        );
        assert_eq!(
            refreshed.snapshot_hash.as_deref(),
            Some(crate::ops_log::content_hash(after_user_commit).as_str()),
            "snapshot_hash should be refreshed to current"
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
        crate::snapshot::save(&doc, original).unwrap();
        // Captured body simulates the JB cache-conflict spill: a stray "❯ "
        // accidentally prepended to one of the agent's response prose lines.
        let captured_response =
            "### Re: topic — gpt-5\n\nImplemented and verified.\n❯ Submodule pointer updated.\n";
        let capture = capture_response(&doc, captured_response).unwrap();

        // User ran sed (or equivalent) to strip the spurious ❯ markers.
        let cleaned_doc = "## Exchange\n\nuser prompt\n### Re: topic — gpt-5\n\nImplemented and verified.\nSubmodule pointer updated.\n";
        std::fs::write(&doc, cleaned_doc).unwrap();
        crate::snapshot::save(&doc, cleaned_doc).unwrap();

        validate_replay(&doc, &capture)
            .expect("user-normalized prefix strip must auto-refresh the baseline");

        let refreshed = load_active(&doc).unwrap().unwrap();
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(crate::ops_log::content_hash(cleaned_doc).as_str()),
            "file_hash should reflect the user-cleaned document"
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("capture_baseline_refreshed_for_benign_drift"),
            "prefix-strip refresh should use the same audit event:\n{log}"
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
        crate::snapshot::save(&doc, original).unwrap();
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
        crate::snapshot::save(&doc, "body").unwrap();
        capture_response(&doc, "response body").unwrap();

        mark_committed(&doc).unwrap();
        let active = load_active(&doc).unwrap().unwrap();
        assert_eq!(active.state, CaptureState::Committed);
        assert!(active.committed_at.is_some());
    }

    #[test]
    fn mark_write_applied_does_not_regress_committed_capture() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        crate::snapshot::save(&doc, "body").unwrap();
        capture_response(&doc, "response body").unwrap();

        mark_committed(&doc).unwrap();
        mark_write_applied(&doc).unwrap();

        let active = load_active(&doc).unwrap().unwrap();
        assert_eq!(active.state, CaptureState::Committed);
    }

    #[test]
    fn mark_replayed_backfills_committed_capture_provenance() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        crate::snapshot::save(&doc, "body").unwrap();
        capture_response(&doc, "response body").unwrap();

        mark_committed(&doc).unwrap();
        mark_replayed(&doc).unwrap();

        let active = load_active(&doc).unwrap().unwrap();
        assert_eq!(active.state, CaptureState::Committed);
        assert!(active.replayed_at.is_some());
        assert!(active.committed_at.is_some());
    }

    #[test]
    fn committed_capture_preserves_replay_provenance() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        crate::snapshot::save(&doc, "body").unwrap();
        capture_response(&doc, "response body").unwrap();

        mark_replayed(&doc).unwrap();
        mark_committed(&doc).unwrap();

        let active = load_active(&doc).unwrap().unwrap();
        assert_eq!(active.state, CaptureState::Committed);
        assert!(active.replayed_at.is_some());
        assert!(active.committed_at.is_some());
    }

    #[test]
    fn repeated_mark_committed_does_not_relog_replay_provenance() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        crate::snapshot::save(&doc, "body").unwrap();
        capture_response(&doc, "response body").unwrap();

        mark_replayed(&doc).unwrap();
        mark_committed(&doc).unwrap();
        let first = load_active(&doc).unwrap().unwrap();
        mark_committed(&doc).unwrap();
        let second = load_active(&doc).unwrap().unwrap();

        assert_eq!(second, first);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert_eq!(
            log.matches("capture_committed_after_replay").count(),
            1,
            "terminal replay provenance should be logged once:\n{log}"
        );
    }
}
