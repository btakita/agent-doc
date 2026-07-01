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

use agent_doc_supervisor::startup_miss::{
    RecentSessionLossWindow, SessionLogStatus, StartupMiss, StartupMissOrigin,
    StartupMissSupersession,
};
use agent_doc_supervisor_io::startup_miss::{
    startup_miss_project_root, startup_miss_state_path, supervisor_session_log_path,
};
use anyhow::Result;
use std::io::Write;
use std::path::Path;

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn append_session_log_event(file: &Path, session_id: &str, event: &str) -> Result<bool> {
    let Some(path) = supervisor_session_log_path(file, session_id)? else {
        return Ok(false);
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let timestamp = agent_doc_log_time::format_log_timestamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );
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
    let Some(path) = startup_miss_state_path(file)? else {
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
    let Some(path) = startup_miss_state_path(file)? else {
        return Ok(None);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(None);
    };
    let marker: StartupMiss = serde_json::from_str(&content)?;
    Ok(Some(marker))
}

pub fn clear(file: &Path) -> Result<()> {
    let Some(path) = startup_miss_state_path(file)? else {
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
    let Some(path) = supervisor_session_log_path(file, session_id)? else {
        return Ok(None);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(None);
    };
    Ok(agent_doc_supervisor::startup_miss::session_log_status_from_content(&content))
}

pub fn session_log_has_event_after_latest_start(
    file: &Path,
    session_id: &str,
    event_prefix: &str,
) -> Result<bool> {
    session_log_has_event_after_latest_start_matching(file, session_id, event_prefix, |_| true)
}

pub fn session_log_has_event_after_latest_start_containing(
    file: &Path,
    session_id: &str,
    event_prefix: &str,
    required_fragment: &str,
) -> Result<bool> {
    session_log_has_event_after_latest_start_matching(file, session_id, event_prefix, |event| {
        event.contains(required_fragment)
    })
}

fn session_log_has_event_after_latest_start_matching(
    file: &Path,
    session_id: &str,
    event_prefix: &str,
    matches_event: impl Fn(&str) -> bool,
) -> Result<bool> {
    let Some(path) = supervisor_session_log_path(file, session_id)? else {
        return Ok(false);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(false);
    };
    Ok(
        agent_doc_supervisor::startup_miss::session_log_has_event_after_latest_start(
            &content,
            event_prefix,
            matches_event,
        ),
    )
}

pub fn session_log_diagnostic(file: &Path, session_id: &str) -> Result<Option<String>> {
    let Some(status) = session_log_status(file, session_id)? else {
        return Ok(None);
    };
    Ok(Some(
        agent_doc_supervisor::startup_miss::session_log_diagnostic(&status),
    ))
}

pub fn superseded_by_newer_registered_start(
    file: &Path,
    miss: &StartupMiss,
) -> Result<Option<StartupMissSupersession>> {
    let Some(root) = startup_miss_project_root(file) else {
        return Ok(None);
    };
    let registry = crate::sessions::load_in(&root)?;
    let registry_key =
        tmux_router::registry::canonical_registry_key_in(&root, &file.display().to_string());
    let Some(registered_entry) = registry.get(&registry_key) else {
        return Ok(None);
    };
    let registered_pane = registered_entry.pane.clone();
    if registered_pane == miss.pane_id {
        return Ok(None);
    }

    let Some(status) = session_log_status(file, &registered_entry.session_id)? else {
        return Ok(None);
    };
    let Some(latest_start_timestamp) = status.latest_start_timestamp else {
        return Ok(None);
    };
    let Some(latest_start_pane) = status.latest_start_pane.clone() else {
        return Ok(None);
    };
    if latest_start_pane != registered_pane || latest_start_timestamp <= miss.timestamp {
        return Ok(None);
    }

    Ok(Some(StartupMissSupersession {
        registered_pane,
        latest_start_pane,
        latest_start_timestamp,
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
    let Some(path) = supervisor_session_log_path(file, session_id)? else {
        return Ok(None);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(None);
    };
    Ok(agent_doc_supervisor::startup_miss::recent_session_loss_window_at(&content, now_epoch_secs))
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
    use std::path::PathBuf;

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
        assert_eq!(
            agent_doc_supervisor::startup_miss::format_timestamp(0),
            "1970-01-01T00:00:00Z"
        );
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
            "[1] session_start file=test.md pane=%41 session=session-123 generation=1\n[2] ipc_started project_root=/tmp/project\n[3] codex_start mode=fresh restart_count=0\n",
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
    fn session_log_event_after_latest_start_resets_on_new_start() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::write(
            logs_dir.join("session-123.log"),
            "[1] session_start file=test.md pane=%41 session=session-123 generation=1\n\
             [2] codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=1\n\
             [3] session_start file=test.md pane=%42 session=session-123 generation=2\n",
        )
        .unwrap();

        assert!(
            !session_log_has_event_after_latest_start(
                &doc,
                "session-123",
                "codex_capability_proof status=proven"
            )
            .unwrap()
        );

        append_session_log_event(
            &doc,
            "session-123",
            "codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=1",
        )
        .unwrap();
        assert!(
            session_log_has_event_after_latest_start(
                &doc,
                "session-123",
                "codex_capability_proof status=proven"
            )
            .unwrap()
        );
    }

    #[test]
    fn session_log_event_after_latest_start_resets_on_agent_restart_performed() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::write(
            logs_dir.join("session-123.log"),
            "[1] session_start file=test.md pane=%41 session=session-123 generation=1\n\
             [2] codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=1\n\
             [3] agent_restart_performed old_harness=claude new_harness=codex action=spawn_fresh_harness\n",
        )
        .unwrap();

        assert!(
            !session_log_has_event_after_latest_start(
                &doc,
                "session-123",
                "codex_capability_proof status=proven"
            )
            .unwrap()
        );

        append_session_log_event(
            &doc,
            "session-123",
            "codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=1",
        )
        .unwrap();
        assert!(
            session_log_has_event_after_latest_start(
                &doc,
                "session-123",
                "codex_capability_proof status=proven"
            )
            .unwrap()
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
            "[1] session_start file=test.md pane=%52 session=session-789 generation=1\n[2] codex_start mode=fresh restart_count=0\n[3] codex_exit code=0 restart_count=0\n[4] codex_restart mode=continue restart_count=1\n",
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
        assert_eq!(
            agent_doc_supervisor::startup_miss::latest_open_run_timestamp(&status),
            Some(4)
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
            "[1] session_start file=test.md pane=%52 session=session-456 generation=1\n[2] codex_exit code=0 restart_count=0\n[3] session_end\n",
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
                "[1] ownership_transition caller=start reason=session_start prior_generation=0 new_generation=1 old_pane=none new_pane=%52 old_window=none new_window=@1\n",
                "[2] session_start file=test.md pane=%52 session=session-commit generation=1\n",
                "[3] codex_start mode=fresh restart_count=0\n",
                "[4] document_cycle phase=response_captured cycle=cycle-1 event=response_captured capture_id=cycle-1\n",
                "[5] document_cycle phase=committed cycle=cycle-1 event=commit_success capture_id=cycle-1\n",
                "[6] codex_exit code=0 restart_count=0\n",
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
                "[1] session_start file=test.md pane=%52 session=session-rebind generation=1\n",
                "[2] codex_start mode=fresh restart_count=0\n",
                "[3] ownership_transition caller=route reason=dispatch_bind prior_generation=1 new_generation=2 old_pane=%52 new_pane=%84 old_window=@1 new_window=@2\n",
                "[4] session_superseded old_pane=%52 new_pane=%84 old_window=@1 new_window=@2 prior_generation=1 new_generation=2\n",
                "[5] session_end origin=registry_rebind pane=%52 next_pane=%84 generation=1 next_generation=2\n",
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
            Some(
                "session_end origin=registry_rebind pane=%52 next_pane=%84 generation=1 next_generation=2"
            )
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
            "[1] session_start file=test.md pane=%61 session=session-loss generation=1\n[2] codex_start mode=fresh restart_count=0\n",
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
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-123".to_string(),
            tmux_router::RegistryEntry {
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
        assert_eq!(supersession.latest_start_timestamp, 10);
    }

    #[test]
    fn superseded_by_newer_registered_start_uses_current_file_owner_session() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            doc.display().to_string(),
            tmux_router::RegistryEntry {
                pane: "%408".to_string(),
                pid: 1,
                cwd: tmp.path().display().to_string(),
                started: "2026-04-29T00:00:00Z".to_string(),
                session_id: "session-456".to_string(),
                file: doc.display().to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        crate::sessions::save_in(tmp.path(), &registry).unwrap();
        fs::write(
            tmp.path().join(".agent-doc/logs/session-456.log"),
            concat!(
                "[10] session_start file=test.md pane=%408 session=session-456\n",
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
            .expect(
                "current file owner should supersede stale marker even after session id rollover",
            );
        assert_eq!(supersession.registered_pane, "%408");
        assert_eq!(supersession.latest_start_pane, "%408");
        assert_eq!(supersession.latest_start_timestamp, 10);
    }

    #[test]
    fn take_superseded_startup_miss_clears_marker_and_returns_supersession() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-123".to_string(),
            tmux_router::RegistryEntry {
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
        let state_path = startup_miss_state_path(&doc)
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
