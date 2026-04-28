//! # Module: startup_miss
//!
//! ## Spec
//! - Persists per-document startup-miss markers under `.agent-doc/state/startup-miss/<doc-hash>.json`.
//! - A startup-miss is recorded when a fresh pane or routed trigger is accepted but no document
//!   cycle starts within the acknowledgment timeout.
//! - `record()` writes a new startup-miss marker with pane provenance and timestamp.
//! - `load()` returns the current persisted marker when present.
//! - `clear()` removes the marker (called on successful cycle acknowledgment).
//! - `is_startup_miss_pane()` checks whether a given pane matches a persisted startup-miss.
//!
//! ## Agentic Contracts
//! - State is per-document, never global across the repo.
//! - Writes are deterministic JSON file replacements.
//! - Missing project root or state file returns `Ok(None)`.
//!
//! ## Evals
//! - record_persists_startup_miss
//! - load_returns_none_when_missing
//! - clear_removes_marker
//! - is_startup_miss_pane_matches

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StartupMiss {
    pub file: String,
    pub pane_id: String,
    pub session_id: String,
    pub harness: String,
    pub timestamp: u64,
    pub origin: StartupMissOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_baseline_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StartupMissOrigin {
    FreshStart,
    RoutedTrigger,
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
        root.join(".agent-doc/state/startup-miss")
            .join(format!("{hash}.json")),
    ))
}

pub fn record(
    file: &Path,
    pane_id: &str,
    session_id: &str,
    harness: &str,
    origin: StartupMissOrigin,
    cycle_baseline_id: Option<&str>,
) -> Result<()> {
    let Some(path) = state_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let marker = StartupMiss {
        file: canonical.display().to_string(),
        pane_id: pane_id.to_string(),
        session_id: session_id.to_string(),
        harness: harness.to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        origin,
        cycle_baseline_id: cycle_baseline_id.map(|s| s.to_string()),
    };
    let json = serde_json::to_string_pretty(&marker)?;
    std::fs::write(&path, json)?;
    Ok(())
}

pub fn load(file: &Path) -> Result<Option<StartupMiss>> {
    let Some(path) = state_path(file)? else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let marker: StartupMiss = serde_json::from_str(&content)?;
    Ok(Some(marker))
}

pub fn clear(file: &Path) -> Result<()> {
    let Some(path) = state_path(file)? else {
        return Ok(());
    };
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn is_startup_miss_pane(file: &Path, pane_id: &str) -> bool {
    load(file)
        .ok()
        .flatten()
        .is_some_and(|m| m.pane_id == pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_project(tmp: &std::path::Path) -> PathBuf {
        let agent_doc_dir = tmp.join(".agent-doc/state/startup-miss");
        fs::create_dir_all(&agent_doc_dir).unwrap();
        let doc = tmp.join("test.md");
        fs::write(&doc, "# test\n").unwrap();
        doc
    }

    #[test]
    fn record_persists_startup_miss() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        record(
            &doc,
            "%42",
            "session-123",
            "claude",
            StartupMissOrigin::FreshStart,
            Some("cycle-abc"),
        )
        .unwrap();
        let loaded = load(&doc).unwrap().expect("should have marker");
        assert_eq!(loaded.pane_id, "%42");
        assert_eq!(loaded.session_id, "session-123");
        assert_eq!(loaded.harness, "claude");
        assert_eq!(loaded.origin, StartupMissOrigin::FreshStart);
        assert_eq!(loaded.cycle_baseline_id.as_deref(), Some("cycle-abc"));
    }

    #[test]
    fn load_returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        assert!(load(&doc).unwrap().is_none());
    }

    #[test]
    fn clear_removes_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        record(
            &doc,
            "%42",
            "session-123",
            "claude",
            StartupMissOrigin::RoutedTrigger,
            None,
        )
        .unwrap();
        assert!(load(&doc).unwrap().is_some());
        clear(&doc).unwrap();
        assert!(load(&doc).unwrap().is_none());
    }

    #[test]
    fn is_startup_miss_pane_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        record(
            &doc,
            "%42",
            "session-123",
            "claude",
            StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();
        assert!(is_startup_miss_pane(&doc, "%42"));
        assert!(!is_startup_miss_pane(&doc, "%99"));
    }
}
