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
//! - Hash validation uses exact content SHA-256 for both the document and the
//!   snapshot baseline.
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

    pub(crate) fn with_interval(file: &Path, interval: Duration) -> Self {
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
        file_hash: Some(crate::ops_log::content_hash(&file_content)),
        response_sha256: response_sha256.clone(),
        response_body: response.to_string(),
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
        file_hash: Some(crate::ops_log::content_hash(&file_content)),
        response_sha256,
        response_body: response.to_string(),
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

pub fn validate_replay(file: &Path, capture: &CaptureRecord) -> Result<()> {
    let current_file = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for capture replay", file.display()))?;
    let current_file_hash = crate::ops_log::content_hash(&current_file);
    if capture.file_hash.as_deref() != Some(current_file_hash.as_str()) {
        anyhow::bail!(
            "captured response baseline no longer matches current document for {}",
            file.display()
        );
    }

    let current_snapshot_hash = crate::snapshot::load(file)?
        .as_deref()
        .map(crate::ops_log::content_hash);
    if capture.snapshot_hash != current_snapshot_hash {
        anyhow::bail!(
            "captured response snapshot no longer matches current baseline for {}",
            file.display()
        );
    }

    Ok(())
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

fn update_active_state(file: &Path, state: CaptureState) -> Result<()> {
    let Some(mut record) = load_active(file)? else {
        return Ok(());
    };
    if capture_state_rank(state.clone()) < capture_state_rank(record.state.clone()) {
        return Ok(());
    }
    let prior_state = record.state.clone();
    let now = now_secs();
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
    let harness = agent_doc::model_tier::detect_harness();
    let model_config = agent_doc::model_tier::ModelConfig::default();
    let resolved_model = fm
        .resolve_harness_model(&harness)
        .map(|s| agent_doc::model_tier::canonical_model_name(s, &harness, &model_config));
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
    fn validate_replay_rejects_diverged_file_hash() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();
        crate::snapshot::save(&doc, "body").unwrap();
        let capture = capture_response(&doc, "response body").unwrap();

        std::fs::write(&doc, "body changed").unwrap();
        let err = validate_replay(&doc, &capture).unwrap_err();
        assert!(err.to_string().contains("baseline no longer matches"));
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
