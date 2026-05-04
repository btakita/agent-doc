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
use std::io::Write;
use std::path::{Path, PathBuf};

const RECENT_SESSION_LOSS_WINDOW_SECS: u64 = 600;
const RECENT_SESSION_LOSS_THRESHOLD: usize = 2;

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
    pub latest_run_timestamp: Option<u64>,
    pub latest_run_event: Option<String>,
    pub saw_committed_cycle_after_latest_run: bool,
    pub last_event: Option<String>,
    pub saw_process_exit_after_latest_start: bool,
    pub saw_session_end_after_latest_start: bool,
    pub saw_process_exit_after_latest_run: bool,
    pub saw_session_end_after_latest_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupMissSupersession {
    pub registered_pane: String,
    pub latest_start_pane: String,
    pub latest_open_timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentSessionLossWindow {
    pub count: usize,
    pub first_timestamp: u64,
    pub last_timestamp: u64,
    pub latest_reason: Option<String>,
}

impl SessionLogStatus {
    fn latest_anchor_timestamp(&self) -> Option<u64> {
        self.latest_run_timestamp.or(self.latest_start_timestamp)
    }

    fn latest_anchor_closed(&self) -> bool {
        if self.latest_run_timestamp.is_some() {
            self.saw_process_exit_after_latest_run || self.saw_session_end_after_latest_run
        } else {
            self.saw_process_exit_after_latest_start || self.saw_session_end_after_latest_start
        }
    }

    pub fn latest_session_open(&self) -> bool {
        self.latest_anchor_timestamp().is_some() && !self.latest_anchor_closed()
    }

    pub fn latest_session_closed(&self) -> bool {
        self.latest_anchor_timestamp().is_some() && self.latest_anchor_closed()
    }
}

fn is_harness_run_start_event(event: &str) -> bool {
    matches!(
        event.split_whitespace().next(),
        Some(token) if token.ends_with("_start") || token.ends_with("_restart")
    )
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn is_session_loss_event(event: &str) -> bool {
    event.starts_with("supervisor_exit code=missing_pane ")
}

fn event_reason(event: &str) -> Option<String> {
    event
        .split_whitespace()
        .find_map(|part| part.strip_prefix("reason=").map(ToOwned::to_owned))
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

fn project_root(file: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(file)
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    crate::snapshot::find_project_root(&canonical)
}

pub fn append_session_log_event(file: &Path, session_id: &str, event: &str) -> Result<bool> {
    let Some(path) = log_path(file, session_id)? else {
        return Ok(false);
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    writeln!(log, "[{timestamp}] {event}")?;
    Ok(true)
}

pub fn record(
    file: &Path,
    pane_id: &str,
    session_id: &str,
    harness: &str,
    origin: StartupMissOrigin,
    cycle_baseline_id: Option<&str>,
) -> Result<StartupMiss> {
    let Some(path) = state_path(file)? else {
        return Ok(StartupMiss {
            file: file.display().to_string(),
            pane_id: pane_id.to_string(),
            session_id: session_id.to_string(),
            harness: harness.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            origin,
            cycle_baseline_id: cycle_baseline_id.map(|s| s.to_string()),
        });
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
    Ok(marker)
}

pub fn load(file: &Path) -> Result<Option<StartupMiss>> {
    let Some(path) = state_path(file)? else {
        return Ok(None);
    };
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(None);
    };
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

pub fn format_timestamp(epoch_secs: u64) -> String {
    let epoch: libc::time_t = match epoch_secs.try_into() {
        Ok(value) => value,
        Err(_) => return epoch_secs.to_string(),
    };
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    let mut buf = [0u8; 21];
    let format = b"%Y-%m-%dT%H:%M:%SZ\0";

    // Format persisted startup-miss timestamps without depending on shell `date`
    // flags, which differ across Linux/macOS.
    unsafe {
        if libc::gmtime_r(&epoch, tm.as_mut_ptr()).is_null() {
            return epoch_secs.to_string();
        }
        let written = libc::strftime(
            buf.as_mut_ptr().cast(),
            buf.len(),
            format.as_ptr().cast(),
            tm.as_ptr(),
        );
        if written == 0 {
            return epoch_secs.to_string();
        }
        String::from_utf8_lossy(&buf[..written]).into_owned()
    }
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
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(None);
    };
    let mut saw_start = false;
    let mut latest_start_pane = None;
    let mut latest_start_timestamp = None;
    let mut latest_run_timestamp = None;
    let mut latest_run_event = None;
    let mut saw_committed_cycle_after_latest_run = false;
    let mut last_event = None;
    let mut saw_process_exit_after_latest_start = false;
    let mut saw_session_end_after_latest_start = false;
    let mut saw_process_exit_after_latest_run = false;
    let mut saw_session_end_after_latest_run = false;

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
            latest_run_timestamp = None;
            latest_run_event = None;
            saw_committed_cycle_after_latest_run = false;
            last_event = Some(event.to_string());
            saw_process_exit_after_latest_start = false;
            saw_session_end_after_latest_start = false;
            saw_process_exit_after_latest_run = false;
            saw_session_end_after_latest_run = false;
            continue;
        }

        if !saw_start {
            continue;
        }

        if is_harness_run_start_event(event) {
            latest_run_timestamp = timestamp.or(latest_start_timestamp);
            latest_run_event = Some(event.to_string());
            saw_committed_cycle_after_latest_run = false;
            last_event = Some(event.to_string());
            saw_process_exit_after_latest_run = false;
            saw_session_end_after_latest_run = false;
            continue;
        }

        last_event = Some(event.to_string());
        if event.starts_with("document_cycle phase=committed ") && latest_run_timestamp.is_some() {
            saw_committed_cycle_after_latest_run = true;
        }
        if event.contains("_exit code=") {
            saw_process_exit_after_latest_start = true;
            if latest_run_timestamp.is_some() {
                saw_process_exit_after_latest_run = true;
            }
        }
        if event
            .split_whitespace()
            .next()
            .is_some_and(|token| token == "session_end")
        {
            saw_session_end_after_latest_start = true;
            if latest_run_timestamp.is_some() {
                saw_session_end_after_latest_run = true;
            }
        }
    }

    if !saw_start {
        return Ok(None);
    }

    Ok(Some(SessionLogStatus {
        latest_start_pane,
        latest_start_timestamp,
        latest_run_timestamp,
        latest_run_event,
        saw_committed_cycle_after_latest_run,
        last_event,
        saw_process_exit_after_latest_start,
        saw_session_end_after_latest_start,
        saw_process_exit_after_latest_run,
        saw_session_end_after_latest_run,
    }))
}

pub fn session_log_diagnostic(file: &Path, session_id: &str) -> Result<Option<String>> {
    let Some(status) = session_log_status(file, session_id)? else {
        return Ok(None);
    };
    let latest_start = status
        .latest_run_event
        .as_deref()
        .map(|event| {
            format!(
                "latest harness run `{event}` on pane={}",
                status.latest_start_pane.as_deref().unwrap_or("<unknown>")
            )
        })
        .unwrap_or_else(|| {
            status
                .latest_start_pane
                .as_deref()
                .map(|pane| format!("latest session_start pane={pane}"))
                .unwrap_or_else(|| "latest session_start pane=<unknown>".to_string())
        });
    let detail = if status.latest_session_open() {
        format!("{latest_start}; session log still has no later child exit or session_end")
    } else if status.latest_session_closed() {
        format!("{latest_start}; session log recorded a later child exit/session_end")
    } else {
        latest_start
    };
    Ok(Some(detail))
}

pub fn superseded_by_newer_registered_start(
    file: &Path,
    miss: &StartupMiss,
) -> Result<Option<StartupMissSupersession>> {
    let Some(root) = project_root(file) else {
        return Ok(None);
    };
    let Some(registered_pane) = crate::sessions::load_in(&root)?
        .values()
        .find(|entry| entry.session_id == miss.session_id)
        .map(|entry| entry.pane.clone())
    else {
        return Ok(None);
    };
    if registered_pane == miss.pane_id {
        return Ok(None);
    }

    let Some(status) = session_log_status(file, &miss.session_id)? else {
        return Ok(None);
    };
    let Some(latest_open_timestamp) = latest_open_run_timestamp(&status) else {
        return Ok(None);
    };
    let Some(latest_start_pane) = status.latest_start_pane.clone() else {
        return Ok(None);
    };
    if latest_start_pane != registered_pane || latest_open_timestamp <= miss.timestamp {
        return Ok(None);
    }

    Ok(Some(StartupMissSupersession {
        registered_pane,
        latest_start_pane,
        latest_open_timestamp,
    }))
}

pub fn take_superseded_startup_miss(
    file: &Path,
) -> Result<Option<(StartupMiss, StartupMissSupersession)>> {
    let Some(miss) = load(file)? else {
        return Ok(None);
    };
    let Some(supersession) = superseded_by_newer_registered_start(file, &miss)? else {
        return Ok(None);
    };
    clear(file)?;
    Ok(Some((miss, supersession)))
}

pub fn latest_open_run_timestamp(status: &SessionLogStatus) -> Option<u64> {
    if status.latest_session_open() {
        status.latest_anchor_timestamp()
    } else {
        None
    }
}

pub fn latest_log_anchor(status: &SessionLogStatus) -> String {
    status
        .latest_run_event
        .as_deref()
        .map(|event| {
            format!(
                "latest_run={} pane={}",
                event,
                status.latest_start_pane.as_deref().unwrap_or("?")
            )
        })
        .unwrap_or_else(|| {
            format!(
                "latest session_start pane={}",
                status.latest_start_pane.as_deref().unwrap_or("?")
            )
        })
}

pub fn latest_log_outcome(status: &SessionLogStatus) -> &'static str {
    if status.latest_session_open() {
        "open"
    } else if status.latest_session_closed() {
        "closed"
    } else {
        "unknown"
    }
}

pub fn latest_log_last_event(status: &SessionLogStatus) -> &str {
    status.last_event.as_deref().unwrap_or("?")
}

pub fn recent_session_loss_window(
    file: &Path,
    session_id: &str,
) -> Result<Option<RecentSessionLossWindow>> {
    recent_session_loss_window_at(file, session_id, current_epoch_secs())
}

fn recent_session_loss_window_at(
    file: &Path,
    session_id: &str,
    now_epoch_secs: u64,
) -> Result<Option<RecentSessionLossWindow>> {
    let Some(path) = log_path(file, session_id)? else {
        return Ok(None);
    };
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(None);
    };
    let cutoff = now_epoch_secs.saturating_sub(RECENT_SESSION_LOSS_WINDOW_SECS);
    let mut count = 0usize;
    let mut first_timestamp = None;
    let mut last_timestamp = None;
    let mut latest_reason = None;

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
        let Some(timestamp) = line
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
            .and_then(|(ts, _)| ts.parse::<u64>().ok())
        else {
            continue;
        };

        if timestamp < cutoff || timestamp > now_epoch_secs || !is_session_loss_event(event) {
            continue;
        }

        count += 1;
        first_timestamp.get_or_insert(timestamp);
        last_timestamp = Some(timestamp);
        latest_reason = event_reason(event);
    }

    if count < RECENT_SESSION_LOSS_THRESHOLD {
        return Ok(None);
    }

    Ok(Some(RecentSessionLossWindow {
        count,
        first_timestamp: first_timestamp.unwrap_or(now_epoch_secs),
        last_timestamp: last_timestamp.unwrap_or(now_epoch_secs),
        latest_reason,
    }))
}

pub fn record_session_loss(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    reason: &str,
    last_known_window: Option<&str>,
) -> Result<bool> {
    let Some(status) = session_log_status(file, session_id)? else {
        return Ok(false);
    };
    if status.latest_session_closed() {
        return Ok(false);
    }

    let mut exit_event =
        format!("supervisor_exit code=missing_pane pane={pane_id} reason={reason}");
    if let Some(window_id) = last_known_window.filter(|window_id| !window_id.is_empty()) {
        exit_event.push_str(&format!(" last_known_window={window_id}"));
    }
    append_session_log_event(file, session_id, &exit_event)?;
    append_session_log_event(file, session_id, "session_end origin=sync_missing_pane")?;
    Ok(true)
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
    fn format_timestamp_renders_utc_iso8601() {
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00Z");
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
        assert_eq!(status.latest_run_timestamp, Some(3));
        assert_eq!(
            status.latest_run_event.as_deref(),
            Some("codex_start mode=fresh restart_count=0")
        );
        assert!(!status.saw_committed_cycle_after_latest_run);
        assert!(status.latest_session_open());
        assert!(!status.latest_session_closed());
        assert_eq!(
            session_log_diagnostic(&doc, "session-123").unwrap(),
            Some(
                "latest harness run `codex_start mode=fresh restart_count=0` on pane=%41; session log still has no later child exit or session_end"
                    .to_string()
            )
        );
    }

    #[test]
    fn session_log_status_reopens_after_child_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::write(
            logs_dir.join("session-789.log"),
            "[1] session_start file=test.md pane=%52 session=session-789\n[2] codex_start mode=fresh restart_count=0\n[3] codex_exit code=0 restart_count=0\n[4] codex_restart mode=continue restart_count=1\n",
        )
        .unwrap();

        let status = session_log_status(&doc, "session-789")
            .unwrap()
            .expect("session log status");
        assert_eq!(status.latest_start_pane.as_deref(), Some("%52"));
        assert_eq!(status.latest_start_timestamp, Some(1));
        assert_eq!(status.latest_run_timestamp, Some(4));
        assert_eq!(
            status.latest_run_event.as_deref(),
            Some("codex_restart mode=continue restart_count=1")
        );
        assert!(!status.saw_committed_cycle_after_latest_run);
        assert!(status.latest_session_open());
        assert!(!status.latest_session_closed());
        assert_eq!(latest_open_run_timestamp(&status), Some(4));
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
        assert_eq!(status.latest_run_timestamp, None);
        assert!(!status.saw_committed_cycle_after_latest_run);
        assert!(!status.latest_session_open());
        assert!(status.latest_session_closed());
    }

    #[test]
    fn session_log_status_tracks_committed_cycle_after_latest_run() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::write(
            logs_dir.join("session-commit.log"),
            concat!(
                "[1] session_start file=test.md pane=%52 session=session-commit\n",
                "[2] codex_start mode=fresh restart_count=0\n",
                "[3] document_cycle phase=response_captured cycle=cycle-1 event=response_captured capture_id=cycle-1\n",
                "[4] document_cycle phase=committed cycle=cycle-1 event=commit_success capture_id=cycle-1\n",
                "[5] codex_exit code=0 restart_count=0\n",
            ),
        )
        .unwrap();

        let status = session_log_status(&doc, "session-commit")
            .unwrap()
            .expect("session log status");
        assert!(status.saw_committed_cycle_after_latest_run);
        assert!(status.latest_session_closed());
        assert_eq!(
            status.last_event.as_deref(),
            Some("codex_exit code=0 restart_count=0")
        );
    }

    #[test]
    fn session_log_status_treats_session_end_with_origin_metadata_as_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::write(
            logs_dir.join("session-rebind.log"),
            concat!(
                "[1] session_start file=test.md pane=%52 session=session-rebind\n",
                "[2] codex_start mode=fresh restart_count=0\n",
                "[3] session_superseded old_pane=%52 new_pane=%84 old_window=@1 new_window=@2\n",
                "[4] session_end origin=registry_rebind pane=%52 next_pane=%84\n",
            ),
        )
        .unwrap();

        let status = session_log_status(&doc, "session-rebind")
            .unwrap()
            .expect("session log status");
        assert!(!status.latest_session_open());
        assert!(status.latest_session_closed());
        assert_eq!(
            status.last_event.as_deref(),
            Some("session_end origin=registry_rebind pane=%52 next_pane=%84")
        );
    }

    #[test]
    fn record_session_loss_closes_open_latest_session() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_project(tmp.path());
        let log_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        let log_path = log_dir.join("session-loss.log");
        std::fs::write(
            &log_path,
            "[1] session_start file=test.md pane=%61 session=session-loss\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let recorded = record_session_loss(
            &doc,
            "session-loss",
            "%61",
            "registered_pane_missing",
            Some("@9"),
        )
        .unwrap();
        assert!(recorded, "open sessions should record a loss event");

        let status = session_log_status(&doc, "session-loss")
            .unwrap()
            .expect("status should remain readable");
        assert!(status.latest_session_closed());

        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(log.contains("supervisor_exit code=missing_pane"));
        assert!(log.contains("reason=registered_pane_missing"));
        assert!(log.contains("last_known_window=@9"));
    }

    #[test]
    fn superseded_by_newer_registered_start_detects_stale_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let mut registry = crate::sessions::SessionRegistry::new();
        registry.insert(
            "session-123".to_string(),
            crate::sessions::SessionEntry {
                pane: "%408".to_string(),
                pid: 1,
                cwd: tmp.path().display().to_string(),
                started: "2026-04-29T00:00:00Z".to_string(),
                session_id: "session-123".to_string(),
                file: doc.display().to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        crate::sessions::save_in(tmp.path(), &registry).unwrap();
        fs::write(
            tmp.path().join(".agent-doc/logs/session-123.log"),
            concat!(
                "[1] session_start file=test.md pane=%401 session=session-123\n",
                "[2] codex_start mode=fresh restart_count=0\n",
                "[10] session_start file=test.md pane=%408 session=session-123\n",
                "[11] codex_start mode=fresh restart_count=0\n",
            ),
        )
        .unwrap();
        let miss = StartupMiss {
            file: doc.display().to_string(),
            pane_id: "%401".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 5,
            origin: StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: None,
        };

        let supersession = superseded_by_newer_registered_start(&doc, &miss)
            .unwrap()
            .expect("stale marker should be superseded");
        assert_eq!(supersession.registered_pane, "%408");
        assert_eq!(supersession.latest_start_pane, "%408");
        assert_eq!(supersession.latest_open_timestamp, 11);
    }

    #[test]
    fn take_superseded_startup_miss_clears_marker_and_returns_supersession() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let mut registry = crate::sessions::SessionRegistry::new();
        registry.insert(
            "session-123".to_string(),
            crate::sessions::SessionEntry {
                pane: "%408".to_string(),
                pid: 1,
                cwd: tmp.path().display().to_string(),
                started: "2026-04-29T00:00:00Z".to_string(),
                session_id: "session-123".to_string(),
                file: doc.display().to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        crate::sessions::save_in(tmp.path(), &registry).unwrap();
        fs::write(
            tmp.path().join(".agent-doc/logs/session-123.log"),
            concat!(
                "[1] session_start file=test.md pane=%401 session=session-123\n",
                "[2] codex_start mode=fresh restart_count=0\n",
                "[10] session_start file=test.md pane=%408 session=session-123\n",
                "[11] codex_start mode=fresh restart_count=0\n",
            ),
        )
        .unwrap();
        let miss = StartupMiss {
            file: doc.display().to_string(),
            pane_id: "%401".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 5,
            origin: StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: None,
        };
        let state_path = state_path(&doc)
            .unwrap()
            .expect("startup-miss state path should exist");
        fs::write(&state_path, serde_json::to_string_pretty(&miss).unwrap()).unwrap();

        let Some((cleared_miss, supersession)) = take_superseded_startup_miss(&doc).unwrap() else {
            panic!("expected stale startup miss to clear");
        };
        assert_eq!(cleared_miss.pane_id, "%401");
        assert_eq!(supersession.registered_pane, "%408");
        assert!(load(&doc).unwrap().is_none(), "marker should be cleared");
    }

    #[test]
    fn recent_session_loss_window_requires_multiple_recent_losses() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::write(
            logs_dir.join("session-loss-window.log"),
            concat!(
                "[100] supervisor_exit code=missing_pane pane=%41 reason=registered_pane_missing\n",
                "[200] supervisor_exit code=missing_pane pane=%42 reason=registered_pane_dead\n",
                "[250] pane_death_detected pane=%42 status=9 cycle_phase=preflight_started\n",
                "[900] supervisor_exit code=missing_pane pane=%43 reason=registered_pane_missing\n",
            ),
        )
        .unwrap();

        let recent = recent_session_loss_window_at(&doc, "session-loss-window", 260)
            .unwrap()
            .expect("two recent session losses should trip the guard");
        assert_eq!(recent.count, 2);
        assert_eq!(recent.first_timestamp, 100);
        assert_eq!(recent.last_timestamp, 200);
        assert_eq!(
            recent.latest_reason.as_deref(),
            Some("registered_pane_dead")
        );

        assert!(
            recent_session_loss_window_at(&doc, "session-loss-window", 1000)
                .unwrap()
                .is_none(),
            "old session-loss events outside the guard window should not trip it"
        );
    }
}
