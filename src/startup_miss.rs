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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLogStatus {
    pub latest_start_pane: Option<String>,
    pub latest_start_timestamp: Option<u64>,
    pub last_event: Option<String>,
    pub saw_process_exit_after_latest_start: bool,
    pub saw_session_end_after_latest_start: bool,
}

impl SessionLogStatus {
    pub fn latest_session_open(&self) -> bool {
        self.latest_start_timestamp.is_some()
            && !self.saw_process_exit_after_latest_start
            && !self.saw_session_end_after_latest_start
    }

    pub fn latest_session_closed(&self) -> bool {
        self.latest_start_timestamp.is_some()
            && (self.saw_process_exit_after_latest_start || self.saw_session_end_after_latest_start)
    }
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

fn log_path(file: &Path, session_id: &str) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    Ok(Some(
        root.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
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

#[allow(dead_code)]
pub fn is_startup_miss_pane(file: &Path, pane_id: &str) -> bool {
    load(file)
        .ok()
        .flatten()
        .is_some_and(|m| m.pane_id == pane_id)
}

pub fn session_log_status(file: &Path, session_id: &str) -> Result<Option<SessionLogStatus>> {
    let Some(path) = log_path(file, session_id)? else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)?;
    let mut saw_start = false;
    let mut latest_start_pane = None;
    let mut latest_start_timestamp = None;
    let mut last_event = None;
    let mut saw_process_exit_after_latest_start = false;
    let mut saw_session_end_after_latest_start = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let event = line
            .split_once("] ")
            .map(|(_, event)| event)
            .unwrap_or(line)
            .trim();
        let timestamp = line
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
            .and_then(|(ts, _)| ts.parse::<u64>().ok());

        if event.starts_with("session_start ") {
            saw_start = true;
            latest_start_timestamp = timestamp;
            latest_start_pane = event
                .split_whitespace()
                .find_map(|part| part.strip_prefix("pane=").map(ToOwned::to_owned));
            last_event = Some(event.to_string());
            saw_process_exit_after_latest_start = false;
            saw_session_end_after_latest_start = false;
            continue;
        }

        if !saw_start {
            continue;
        }

        last_event = Some(event.to_string());
        if event.contains("_exit code=") {
            saw_process_exit_after_latest_start = true;
        }
        if event == "session_end" {
            saw_session_end_after_latest_start = true;
        }
    }

    if !saw_start {
        return Ok(None);
    }

    Ok(Some(SessionLogStatus {
        latest_start_pane,
        latest_start_timestamp,
        last_event,
        saw_process_exit_after_latest_start,
        saw_session_end_after_latest_start,
    }))
}

pub fn session_log_diagnostic(file: &Path, session_id: &str) -> Result<Option<String>> {
    let Some(status) = session_log_status(file, session_id)? else {
        return Ok(None);
    };
    let latest_start = status
        .latest_start_pane
        .as_deref()
        .map(|pane| format!("latest session_start pane={pane}"))
        .unwrap_or_else(|| "latest session_start pane=<unknown>".to_string());
    let detail = if status.latest_session_open() {
        format!("{latest_start}; session log still has no later child exit or session_end")
    } else if status.latest_session_closed() {
        format!("{latest_start}; session log recorded a later child exit/session_end")
    } else {
        latest_start
    };
    Ok(Some(detail))
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

    #[test]
    fn session_log_status_reports_open_latest_session() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::write(
            logs_dir.join("session-123.log"),
            "[1] session_start file=test.md pane=%41 session=session-123\n[2] ipc_started project_root=/tmp/project\n[3] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let status = session_log_status(&doc, "session-123")
            .unwrap()
            .expect("session log status");
        assert_eq!(status.latest_start_pane.as_deref(), Some("%41"));
        assert!(status.latest_session_open());
        assert!(!status.latest_session_closed());
        assert_eq!(
            session_log_diagnostic(&doc, "session-123").unwrap(),
            Some(
                "latest session_start pane=%41; session log still has no later child exit or session_end"
                    .to_string()
            )
        );
    }

    #[test]
    fn session_log_status_reports_closed_latest_session() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::write(
            logs_dir.join("session-456.log"),
            "[1] session_start file=test.md pane=%52 session=session-456\n[2] codex_exit code=0 restart_count=0\n[3] session_end\n",
        )
        .unwrap();

        let status = session_log_status(&doc, "session-456")
            .unwrap()
            .expect("session log status");
        assert_eq!(status.latest_start_pane.as_deref(), Some("%52"));
        assert!(!status.latest_session_open());
        assert!(status.latest_session_closed());
    }
}
