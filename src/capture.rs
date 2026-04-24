//! # Module: capture
//!
//! ## Spec
//! - Persists a durable response-capture ledger under
//!   `.agent-doc/captures/<doc-hash>/<cycle-id>.json`.
//! - Captures the final parsed response body plus cycle metadata before any
//!   document write or hook emission.
//! - `capture_response(file, response)` stores the response, marks the cycle as
//!   `response_captured`, and returns the saved record.
//! - `load_active(file)` resolves the active capture from cycle state.
//! - `validate_replay(file, capture)` fail-closes replay when the current file
//!   or snapshot hashes no longer match the captured baseline.
//! - `mark_write_applied`, `mark_replayed`, `mark_committed`, and
//!   `mark_discarded` advance the capture lifecycle in-place.
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
//! - `validate_replay_rejects_diverged_file_hash`
//! - `mark_committed_updates_capture_state`

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

#[derive(Debug, Default)]
struct CaptureMetadata {
    session_id: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    document_format: Option<String>,
    write_strategy: Option<String>,
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
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read capture {}", path.display()))?;
    let record: CaptureRecord = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse capture {}", path.display()))?;
    Ok(Some(record))
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
    let now = now_secs();
    match state {
        CaptureState::Captured => {}
        CaptureState::WriteApplied => {
            if record.write_applied_at.is_none() {
                record.write_applied_at = Some(now);
            }
        }
        CaptureState::Replayed => {
            if record.replayed_at.is_none() {
                record.replayed_at = Some(now);
            }
        }
        CaptureState::Committed => {
            if record.committed_at.is_none() {
                record.committed_at = Some(now);
            }
        }
        CaptureState::Discarded => {
            if record.discarded_at.is_none() {
                record.discarded_at = Some(now);
            }
        }
    }
    record.state = state;
    record.updated_at = now;
    if matches!(record.state, CaptureState::Committed) && record.replayed_at.is_some() {
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
    CaptureMetadata {
        session_id: fm.session,
        agent: fm.agent,
        model: fm.model,
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
}
