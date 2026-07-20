//! Phase E rung 2 (`#adstatechart2`): advisory observability read of the
//! local-process `adstatechart` four-region configuration.
//!
//! This assembles [`ChartFacts`] / [`ObservedPhases`] from signals `session-check`
//! already has cheaply at hand and emits a single compact
//! `adstatechart_snapshot transport.x editor_sync.x closeout.x supervisor.x`
//! marker to `ops.log` alongside the existing markers. It is **read-only**: the
//! snapshot builds its own throwaway chart in
//! [`agent_doc_state_backbone::adstatechart::configuration_snapshot`] and never
//! touches the live write/commit path — no closeout behavior changes.
//!
//! Every fact source fails safe to its chart-initial default, so a missing live
//! buffer, unresolved project root, or absent cycle state degrades the advisory
//! line rather than erroring the caller. The `editor_synced` guard used for the
//! closeout region is the same `edit_epoch <= last_synced_epoch` the live commit
//! path enforces (`git.rs` `commit_blocked_live_buffer_ahead_of_disk`), so the
//! advisory never disagrees with the shipped A/C guard.

use std::path::Path;

use agent_doc_state_backbone::adstatechart::{
    ChartFacts, CloseoutPhase, ObservedPhases, configuration_snapshot,
};
use agent_doc_turn::CyclePhase;

/// Map the persisted cycle phase to the advisory closeout leaf. `SessionOk` is
/// intentionally never produced here: it is reached only after `session-check`
/// itself passes, which has not happened at the point this advisory is emitted.
fn closeout_phase_from_cycle(phase: CyclePhase) -> CloseoutPhase {
    match phase {
        CyclePhase::PreflightStarted | CyclePhase::Abandoned => CloseoutPhase::Idle,
        CyclePhase::ResponseCaptured | CyclePhase::WriteApplied => CloseoutPhase::Written,
        CyclePhase::Committed => CloseoutPhase::Committed,
    }
}

/// Derived Lazily reliable-sync state for `file`, represented as a minimal epoch
/// pair for the advisory chart.
fn editor_sync_epochs(file: &Path) -> (u64, u64) {
    let file_key = file.to_string_lossy();
    if agent_doc_reliable_sync_io::plane_document_in_flight_for_path(&file_key).unwrap_or(false) {
        (1, 0)
    } else {
        (0, 0)
    }
}

/// `true` when a live PID-scoped editor endpoint owns the document. A missing /
/// unresolvable endpoint leaves transport degraded while the same durable intent
/// waits for re-registration; it never selects a file transport.
fn ipc_listener_active(file: &Path) -> bool {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    let registration = agent_doc_reliable_sync_io::global_liveness_plane()
        .lock()
        .projection()
        .live_registrations(&document_hash)
        .into_iter()
        .max_by_key(|registration| registration.timestamp_ms);
    let Some(registration) = registration else {
        return false;
    };
    file.canonicalize()
        .ok()
        .map(|canonical| agent_doc_project_root_io::resolve_ipc_project_root(&canonical))
        .is_some_and(|root| agent_doc_ipc_io::is_listener_active_for_pid(&root, registration.pid))
}

/// `true` when the installed agent-doc artifacts predate the newest local source
/// edit — the same signal preflight's `stale_install` warning uses. Any missing
/// input fails safe to "not stale".
fn install_is_stale(doc_git_root: &Path) -> bool {
    let Some(repo) = agent_doc_fs::install_freshness::locate_agent_doc_source_repo(doc_git_root)
    else {
        return false;
    };
    let Some(source_ts) = agent_doc_fs::install_freshness::newest_crate_source_mtime_secs(&repo)
    else {
        return false;
    };
    let artifacts = agent_doc_fs::install_freshness::agent_doc_install_artifacts(&repo);
    !agent_doc_supervisor::config::classify_stale_install_artifacts(
        source_ts,
        &artifacts,
        agent_doc_supervisor::config::STALE_INSTALL_GRACE_SECS,
    )
    .is_empty()
}

/// Assemble the advisory facts + observed phases for `file` (whose git root is
/// `doc_git_root`). Read-only and fail-safe.
fn assemble(file: &Path, doc_git_root: &Path) -> (ChartFacts, ObservedPhases) {
    let (edit_epoch, last_synced_epoch) = editor_sync_epochs(file);
    // Represent a proven stale install as a build-id mismatch so
    // `supervisor_stale` (which requires two known, differing ids) reports it;
    // otherwise leave both ids unknown → not stale.
    let (running_build_id, installed_build_id) = if install_is_stale(doc_git_root) {
        (Some("running".to_string()), Some("installed".to_string()))
    } else {
        (None, None)
    };

    let facts = ChartFacts {
        edit_epoch,
        last_synced_epoch,
        // No cheap point-in-time send-failure signal at session-check; the
        // meaningful transport signal here is listener presence.
        ipc_send_failed: false,
        ipc_no_listener: !ipc_listener_active(file),
        running_build_id,
        installed_build_id,
    };

    let closeout = agent_doc_cycle_state_io::load_with_closeout_projection(file)
        .ok()
        .flatten()
        .map(|state| closeout_phase_from_cycle(state.phase))
        .unwrap_or_default();

    let observed = ObservedPhases {
        closeout,
        // No cheap in-turn supervisor-busy signal at session-check; staleness is
        // the load-bearing supervisor observation and is sourced above.
        supervisor_busy: false,
    };

    (facts, observed)
}

/// Best-effort git root for `file`, falling back to its parent (or `.`) so the
/// advisory never errors on a non-repo path.
fn doc_git_root_of(file: &Path) -> std::path::PathBuf {
    agent_doc_git_io::dirs::resolve_to_git_root(file)
        .map(|(root, _)| root)
        .unwrap_or_else(|_| {
            file.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| Path::new(".").to_path_buf())
        })
}

/// Compute the advisory named snapshot string for `file`.
pub fn advisory_snapshot(file: &Path) -> String {
    let doc_git_root = doc_git_root_of(file);
    let (facts, observed) = assemble(file, &doc_git_root);
    configuration_snapshot(&facts, &observed)
}

/// Emit the advisory snapshot to `ops.log` as a single read-only marker. Best
/// effort: never fails the caller, never changes closeout behavior.
pub fn log_advisory_snapshot(file: &Path) {
    let snapshot = advisory_snapshot(file);
    agent_doc_ops_log_io::log_op(
        file,
        &format!("adstatechart_snapshot advisory=1 {snapshot}"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closeout_phase_mapping_never_reports_session_ok() {
        assert_eq!(
            closeout_phase_from_cycle(CyclePhase::PreflightStarted),
            CloseoutPhase::Idle
        );
        assert_eq!(
            closeout_phase_from_cycle(CyclePhase::Abandoned),
            CloseoutPhase::Idle
        );
        assert_eq!(
            closeout_phase_from_cycle(CyclePhase::ResponseCaptured),
            CloseoutPhase::Written
        );
        assert_eq!(
            closeout_phase_from_cycle(CyclePhase::WriteApplied),
            CloseoutPhase::Written
        );
        assert_eq!(
            closeout_phase_from_cycle(CyclePhase::Committed),
            CloseoutPhase::Committed
        );
    }

    #[test]
    fn advisory_snapshot_is_well_formed_for_missing_state() {
        // A path with no live buffer / cycle state still yields a stable, fully
        // labeled four-region advisory line (all regions at their initial leaf,
        // no endpoint → degraded transport awaiting registration).
        let dir = std::env::temp_dir().join("adstatechart_snapshot_test_missing");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("doc.md");
        let snap = advisory_snapshot(&file);
        assert!(snap.contains("transport."), "got: {snap}");
        assert!(snap.contains("editor_sync.synced"), "got: {snap}");
        assert!(snap.contains("closeout.idle"), "got: {snap}");
        assert!(snap.contains("supervisor.sup_idle"), "got: {snap}");
    }

    #[test]
    fn advisory_snapshot_reports_terminal_ledger_projection() {
        let dir = std::env::temp_dir().join(format!(
            "adstatechart_snapshot_projection_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let file = dir.join("doc.md");
        let content = "body";
        std::fs::write(&file, content).unwrap();
        agent_doc_cycle_state_io::start_preflight(&file, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &file,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        let snap = advisory_snapshot(&file);
        assert!(snap.contains("closeout.committed"), "got: {snap}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
