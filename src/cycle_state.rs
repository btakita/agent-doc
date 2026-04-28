//! # Module: cycle_state
//!
//! ## Spec
//! - Persists per-document cycle state under `.agent-doc/state/cycles/<doc-hash>.json`.
//! - Tracks the exact phase of the current or most recent response cycle:
//!   `preflight_started` → `response_captured` → `write_applied` → `committed`.
//! - Stores cycle-scoped snapshot/file content hashes so callers can reason
//!   about exact cycle state instead of inferring from file-size drift or only
//!   the last `ops.log` line.
//! - `start_preflight()` opens a new cycle for a document and overwrites any
//!   prior committed state for that document.
//! - `mark_response_captured()` advances the open cycle to `response_captured`
//!   once the final parsed response has been durably stored.
//! - `mark_write_applied()` advances the open cycle to `write_applied` (or
//!   creates a synthetic cycle if a write lands without a prior preflight).
//! - `mark_committed()` advances the cycle to `committed` (or creates a
//!   synthetic committed cycle if commit happens without a prior state file).
//! - `load()` returns the current persisted state when present.
//!
//! ## Agentic Contracts
//! - State is per-document, never global across the repo.
//! - Writes are deterministic JSON file replacements.
//! - Missing project root or state file returns `Ok(None)`.
//! - `is_open()` is true for any phase except `Committed`.
//!
//! ## Evals
//! - `start_preflight_persists_open_cycle`
//! - `mark_response_captured_sets_capture_metadata`
//! - `mark_write_applied_advances_existing_cycle`
//! - `mark_committed_closes_cycle`
//! - `mark_write_applied_creates_synthetic_cycle_when_missing`

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CyclePhase {
    PreflightStarted,
    ResponseCaptured,
    WriteApplied,
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CycleState {
    pub cycle_id: String,
    pub file: String,
    pub phase: CyclePhase,
    pub last_event: String,
    pub started_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_file_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_sha256: Option<String>,
    #[serde(default)]
    pub had_pending_mutations: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_done_ids: Vec<String>,
}

impl CycleState {
    pub fn is_open(&self) -> bool {
        self.phase != CyclePhase::Committed
    }
}

pub fn load(file: &Path) -> Result<Option<CycleState>> {
    let Some(path) = state_path(file)? else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let state: CycleState = serde_json::from_str(&content)?;
    Ok(Some(state))
}

pub fn start_preflight(
    file: &Path,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let now = now_secs();
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let state = CycleState {
        cycle_id: format!("cycle-{}", now_millis()),
        file: canonical.display().to_string(),
        phase: CyclePhase::PreflightStarted,
        last_event: "preflight_started".to_string(),
        started_at: now,
        updated_at: now,
        snapshot_hash: snapshot_content.map(crate::ops_log::content_hash),
        file_hash: file_content.map(crate::ops_log::content_hash),
        normalized_snapshot_hash: snapshot_content.map(normalized_content_hash),
        normalized_file_hash: file_content.map(normalized_content_hash),
        capture_id: None,
        response_sha256: None,
        had_pending_mutations: false,
        pending_done_ids: Vec::new(),
    };
    save(file, &state)?;
    Ok(state)
}

pub fn mark_write_applied(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let mut state =
        load(file)?.unwrap_or_else(|| synthetic_state(file, CyclePhase::PreflightStarted));
    state.phase = CyclePhase::WriteApplied;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    state.snapshot_hash = snapshot_content.map(crate::ops_log::content_hash);
    state.file_hash = file_content.map(crate::ops_log::content_hash);
    state.normalized_snapshot_hash = snapshot_content.map(normalized_content_hash);
    state.normalized_file_hash = file_content.map(normalized_content_hash);
    save(file, &state)?;
    Ok(state)
}

pub fn mark_response_captured(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
    response_sha256: &str,
    cycle_id_hint: Option<&str>,
) -> Result<CycleState> {
    let mut state = load(file)?.unwrap_or_else(|| {
        synthetic_state_with_id(file, CyclePhase::PreflightStarted, cycle_id_hint)
    });
    state.phase = CyclePhase::ResponseCaptured;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    state.snapshot_hash = snapshot_content.map(crate::ops_log::content_hash);
    state.file_hash = file_content.map(crate::ops_log::content_hash);
    state.normalized_snapshot_hash = snapshot_content.map(normalized_content_hash);
    state.normalized_file_hash = file_content.map(normalized_content_hash);
    state.capture_id = Some(state.cycle_id.clone());
    state.response_sha256 = Some(response_sha256.to_string());
    save(file, &state)?;
    Ok(state)
}

pub fn mark_pending_mutations(file: &Path) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.had_pending_mutations {
        state.had_pending_mutations = true;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_pending_done_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };

    let mut changed = false;
    for id in ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if !state
            .pending_done_ids
            .iter()
            .any(|existing| existing == &id)
        {
            state.pending_done_ids.push(id);
            changed = true;
        }
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn mark_committed(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let mut state = load(file)?.unwrap_or_else(|| synthetic_state(file, CyclePhase::WriteApplied));
    state.phase = CyclePhase::Committed;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    if let Some(snapshot) = snapshot_content {
        state.snapshot_hash = Some(crate::ops_log::content_hash(snapshot));
        state.normalized_snapshot_hash = Some(normalized_content_hash(snapshot));
    }
    if let Some(content) = file_content {
        state.file_hash = Some(crate::ops_log::content_hash(content));
        state.normalized_file_hash = Some(normalized_content_hash(content));
    }
    save(file, &state)?;
    Ok(state)
}

fn save(file: &Path, state: &CycleState) -> Result<()> {
    let Some(path) = state_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn state_path(file: &Path) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(&canonical)?;
    Ok(Some(
        root.join(".agent-doc/state/cycles")
            .join(format!("{hash}.json")),
    ))
}

fn synthetic_state(file: &Path, phase: CyclePhase) -> CycleState {
    synthetic_state_with_id(file, phase, None)
}

fn synthetic_state_with_id(
    file: &Path,
    phase: CyclePhase,
    cycle_id_hint: Option<&str>,
) -> CycleState {
    let now = now_secs();
    CycleState {
        cycle_id: cycle_id_hint
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("synthetic-{}", now_millis())),
        file: file.display().to_string(),
        phase,
        last_event: "synthetic_state".to_string(),
        started_at: now,
        updated_at: now,
        snapshot_hash: None,
        file_hash: None,
        normalized_snapshot_hash: None,
        normalized_file_hash: None,
        capture_id: None,
        response_sha256: None,
        had_pending_mutations: false,
        pending_done_ids: Vec::new(),
    }
}

fn normalized_content_hash(content: &str) -> String {
    crate::ops_log::content_hash(&crate::git::normalize_transient_agent_doc_markers(content))
}

fn normalize_pending_id(id: &str) -> String {
    id.trim().trim_start_matches('#').to_ascii_lowercase()
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
    use std::fs;

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    #[test]
    fn start_preflight_persists_open_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let state = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert_eq!(state.phase, CyclePhase::PreflightStarted);
        assert_eq!(
            load(&doc).unwrap().unwrap().phase,
            CyclePhase::PreflightStarted
        );
        assert!(load(&doc).unwrap().unwrap().is_open());
    }

    #[test]
    fn mark_write_applied_advances_existing_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = mark_write_applied(&doc, "write_template", Some("new"), Some("new")).unwrap();
        assert_eq!(state.phase, CyclePhase::WriteApplied);
        assert_eq!(state.last_event, "write_template");
        assert!(state.snapshot_hash.is_some());
    }

    #[test]
    fn mark_response_captured_sets_capture_metadata() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some("body"),
            "abc",
            None,
        )
        .unwrap();
        assert_eq!(state.phase, CyclePhase::ResponseCaptured);
        assert_eq!(state.capture_id.as_deref(), Some(state.cycle_id.as_str()));
        assert_eq!(state.response_sha256.as_deref(), Some("abc"));
    }

    #[test]
    fn mark_pending_mutations_sets_flag() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = mark_pending_mutations(&doc).unwrap().unwrap();
        assert!(state.had_pending_mutations);
        assert!(load(&doc).unwrap().unwrap().had_pending_mutations);
    }

    #[test]
    fn record_pending_done_ids_persists_normalized_ids() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_pending_done_ids(
            &doc,
            &["#AbC1".to_string(), "abc1".to_string(), "z9".to_string()],
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            state.pending_done_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
        assert_eq!(
            load(&doc).unwrap().unwrap().pending_done_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
    }

    #[test]
    fn mark_committed_closes_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_write_applied(&doc, "write_template", Some("new"), Some("new")).unwrap();

        let state = mark_committed(&doc, "commit", Some("new"), Some("new")).unwrap();
        assert_eq!(state.phase, CyclePhase::Committed);
        assert!(!state.is_open());
    }

    #[test]
    fn mark_write_applied_creates_synthetic_cycle_when_missing() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let state = mark_write_applied(&doc, "recover_apply", Some("body"), Some("body")).unwrap();
        assert_eq!(state.phase, CyclePhase::WriteApplied);
        assert!(state.cycle_id.starts_with("synthetic-"));
    }
}
