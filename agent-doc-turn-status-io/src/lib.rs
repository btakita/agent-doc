//! `agent-doc turn-status active|idle` — surface a turn-in-progress status on the
//! agent's own tmux pane (`#claude-busy-status-during-active-turn`, continuous
//! monitor).
//!
//! Designed to be driven by harness turn-lifecycle hooks: a `UserPromptSubmit`
//! hook calls `active` when a turn starts and a `Stop` hook calls `idle` when it
//! ends. Because the hook runs INSIDE the agent's own pane, it sets that pane's
//! border title via `$TMUX_PANE` — no supervisor poll thread and no document/pane
//! resolution. Crucially the harness fires `Stop` only after the WHOLE turn
//! completes, including any Bash it auto-backgrounded and re-invoked on, so the
//! status covers the backgrounded window that pane busy-cue detection (which only
//! reads visible pane content) fundamentally cannot see.
//!
//! Visibility note: the status rides the pane border title, shown when tmux
//! `pane-border-status` is enabled. The command is best-effort — it never fails
//! the turn: outside tmux, or on any tmux error, it succeeds quietly.

use agent_doc_sqlite::state_store::{
    Connection, CoordinationLeaseRecord, clear_coordination_lease_if_holder_in_db,
    clear_coordination_lease_in_db, clear_coordination_leases_heartbeat_at_or_before_in_db,
    load_coordination_lease_from_db, load_coordination_leases_for_scope_kind_from_db,
    open_state_db, upsert_coordination_lease_in_db,
};
use agent_doc_turn::turn_status::{
    TurnActiveMarker, pane_title_for_status, turn_active_expiry_cutoff,
    turn_active_marker_is_fresh, turn_active_marker_matches_pane,
};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

const TURN_ACTIVE_SCOPE: &str = "turn_active";
const SUPERVISOR_STALE_SCOPE: &str = "supervisor_stale";
// Compatibility key written by agent-doc versions before turn-active leases
// became pane-scoped.
const PROJECT_SCOPE_ID: &str = "project";

/// Record the current turn owner in the project state database.
pub fn write_turn_active_marker(base: &Path, pane: &str) -> Result<()> {
    write_turn_active_marker_at(base, pane, now_secs())
}

fn write_turn_active_marker_at(base: &Path, pane: &str, written_at: u64) -> Result<()> {
    let conn = open_state_db(base)?;
    upsert_coordination_lease_in_db(
        &conn,
        &CoordinationLeaseRecord {
            scope_kind: TURN_ACTIVE_SCOPE.to_string(),
            scope_id: pane.to_string(),
            holder: pane.to_string(),
            holder_pid: Some(std::process::id()),
            heartbeat_secs: written_at,
        },
    )
}

/// Clear one pane's turn owner (turn idle / superseded). Absent is OK.
pub fn clear_turn_active_marker(base: &Path, pane: &str) -> Result<()> {
    let conn = open_state_db(base)?;
    clear_coordination_lease_in_db(&conn, TURN_ACTIVE_SCOPE, pane)?;
    // Retire only a matching legacy singleton. A Stop hook from another pane
    // must not erase the active owner written by an older installed binary.
    clear_coordination_lease_if_holder_in_db(&conn, TURN_ACTIVE_SCOPE, PROJECT_SCOPE_ID, pane)?;
    Ok(())
}

fn marker_from_lease(lease: CoordinationLeaseRecord, now: u64) -> Option<TurnActiveMarker> {
    let marker = TurnActiveMarker {
        pane: lease.holder,
        written_at: lease.heartbeat_secs,
    };
    turn_active_marker_is_fresh(&marker, now).then_some(marker)
}

fn sweep_expired_turn_active_leases_in_db(conn: &Connection, now: u64) -> Result<usize> {
    let Some(cutoff) = turn_active_expiry_cutoff(now) else {
        return Ok(0);
    };
    clear_coordination_leases_heartbeat_at_or_before_in_db(conn, TURN_ACTIVE_SCOPE, cutoff)
}

/// Delete every turn-active lease past `TURN_ACTIVE_TTL_SECS` (GH #135).
///
/// The sweep is keyed to heartbeat age only, never to the pane that wrote the
/// row: a pane that died before its idle hook can never clear its own lease, so
/// a per-pane clear cannot reclaim it. One bounded `DELETE`; the `WHERE` clause
/// re-checks the heartbeat, so a lease refreshed by a live turn between a read
/// and the sweep is never deleted. Returns the number of rows removed.
pub fn sweep_expired_turn_active_markers(base: &Path) -> Result<usize> {
    sweep_expired_turn_active_markers_at(base, now_secs())
}

pub fn sweep_expired_turn_active_markers_at(base: &Path, now: u64) -> Result<usize> {
    let conn = open_state_db(base)?;
    sweep_expired_turn_active_leases_in_db(&conn, now)
}

/// Count the turn-active leases a sweep at `now` would delete (`gc --dry-run`).
pub fn count_expired_turn_active_markers_at(base: &Path, now: u64) -> Result<usize> {
    let conn = open_state_db(base)?;
    Ok(
        load_coordination_leases_for_scope_kind_from_db(&conn, TURN_ACTIVE_SCOPE)?
            .into_iter()
            .filter(|lease| marker_from_lease(lease.clone(), now).is_none())
            .count(),
    )
}

/// A read that observed an expired lease reclaims it instead of only skipping
/// it, so expiry deletes the row rather than guaranteeing it survives
/// (GH #135). Best-effort: a failed reclaim never changes the read result.
fn reclaim_expired_turn_active_leases_on_read(conn: &Connection, now: u64) {
    let _ = sweep_expired_turn_active_leases_in_db(conn, now);
}

/// Read the turn-active marker if it exists and is not expired. An expired
/// marker is treated as absent so a missed `idle` hook self-heals instead of
/// wedging the session busy, and its row is reclaimed.
pub fn read_turn_active_marker_at(base: &Path, now: u64) -> Option<TurnActiveMarker> {
    let conn = open_state_db(base).ok()?;
    let leases = load_coordination_leases_for_scope_kind_from_db(&conn, TURN_ACTIVE_SCOPE).ok()?;
    let lease_count = leases.len();
    let fresh: Vec<TurnActiveMarker> = leases
        .into_iter()
        .filter_map(|lease| marker_from_lease(lease, now))
        .collect();
    if fresh.len() < lease_count {
        reclaim_expired_turn_active_leases_on_read(&conn, now);
    }
    fresh.into_iter().max_by_key(|marker| marker.written_at)
}

fn read_turn_active_marker_for_pane_at(
    base: &Path,
    pane: &str,
    now: u64,
) -> Option<TurnActiveMarker> {
    let conn = open_state_db(base).ok()?;
    let pane_lease = load_coordination_lease_from_db(&conn, TURN_ACTIVE_SCOPE, pane)
        .ok()
        .flatten();
    let lease = match pane_lease {
        Some(lease) => lease,
        None => {
            let legacy =
                load_coordination_lease_from_db(&conn, TURN_ACTIVE_SCOPE, PROJECT_SCOPE_ID)
                    .ok()??;
            (legacy.holder == pane).then_some(legacy)?
        }
    };
    let marker = marker_from_lease(lease, now);
    if marker.is_none() {
        reclaim_expired_turn_active_leases_on_read(&conn, now);
    }
    marker
}

/// Read the non-expired turn-active marker under `base`, if present.
pub fn read_turn_active_marker(base: &Path) -> Option<TurnActiveMarker> {
    read_turn_active_marker_at(base, now_secs())
}

/// Read the non-expired turn-active marker for the project containing `file`.
pub fn read_turn_active_marker_for_file(file: &Path) -> Option<TurnActiveMarker> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    read_turn_active_marker(&root)
}

/// True when a non-expired turn-active marker is present under `base`.
pub fn turn_active(base: &Path) -> bool {
    read_turn_active_marker(base).is_some()
}

/// True when the non-expired marker belongs to `pane`.
pub fn turn_active_for_pane(base: &Path, pane: &str) -> bool {
    read_turn_active_marker_for_pane_at(base, pane, now_secs())
        .is_some_and(|marker| turn_active_marker_matches_pane(&marker, pane))
}

/// True when the project containing `file` has a fresh marker for `pane`.
pub fn turn_active_for_pane_for_file(file: &Path, pane: &str) -> bool {
    agent_doc_project_root_io::project_root_containing(file)
        .is_some_and(|root| turn_active_for_pane(&root, pane))
}

/// Return the fresh turn-active marker only when it belongs to this document's
/// durable owner pane.
///
/// Project-wide turn markers are pane-scoped, so reading the newest marker by
/// itself can accidentally borrow liveness from a sibling document. Resolve the
/// document's registry owner first and require the marker for that exact pane.
/// Missing/unreadable registry or lease state remains `None`, preserving the
/// fail-closed behavior of callers that use this only as positive liveness
/// evidence.
pub fn active_turn_owner_for_file(file: &Path) -> Option<TurnActiveMarker> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    let owner = agent_doc_session_registry_io::lookup_file_entry_in(&root, file)
        .ok()
        .flatten()?;
    read_turn_active_marker_for_pane_at(&root, &owner.pane, now_secs())
        .filter(|marker| turn_active_marker_matches_pane(marker, &owner.pane))
}

/// Publish the stale-supervisor flag in the project state database.
/// Best-effort cross-process channel for the supervisor → turn-status hook.
pub fn set_supervisor_stale_marker(base: &Path, pane: &str, stale: bool) -> Result<()> {
    let conn = open_state_db(base)?;
    if stale {
        upsert_coordination_lease_in_db(
            &conn,
            &CoordinationLeaseRecord {
                scope_kind: SUPERVISOR_STALE_SCOPE.to_string(),
                scope_id: pane.to_string(),
                holder: "stale".to_string(),
                holder_pid: Some(std::process::id()),
                heartbeat_secs: now_secs(),
            },
        )
    } else {
        clear_coordination_lease_in_db(&conn, SUPERVISOR_STALE_SCOPE, pane).map(|_| ())
    }
}

/// True when the stale-supervisor marker is present under `base` (`#suptmuxstale`).
/// Read-only display probe — absent / unreadable reads as fresh (not stale).
pub fn supervisor_stale(base: &Path, pane: &str) -> bool {
    let Ok(conn) = open_state_db(base) else {
        return false;
    };
    load_coordination_lease_from_db(&conn, SUPERVISOR_STALE_SCOPE, pane)
        .ok()
        .flatten()
        .is_some()
}

/// Effect sink for the process-owned freshness projection. This marker is display
/// output only, never authority for recycle or turn admission.
pub fn project_supervisor_freshness(base: &Path, pane: &str, stale: bool) -> Result<()> {
    set_supervisor_stale_marker(base, pane, stale)?;
    let tmux = agent_doc_tmux_io::configured_tmux();
    let output = tmux
        .cmd()
        .args(["display-message", "-p", "-t", pane, "#{pane_title}"])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "failed to read pane {pane} title: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let title = String::from_utf8_lossy(&output.stdout);
    let title = title.trim_end_matches(['\r', '\n']);
    let document_name = document_name_for_pane(base, pane);
    let updated = agent_doc_turn::turn_status::pane_title_with_freshness(
        title,
        document_name.as_deref(),
        stale,
    );
    if updated != title {
        let output = tmux
            .cmd()
            .args(["select-pane", "-t", pane, "-T", &updated])
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "failed to update pane {pane} title: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn set_pane_title(pane: &str, title: &str) {
    let tmux = agent_doc_tmux_io::configured_tmux();
    if let Err(e) = tmux
        .cmd()
        .args(["select-pane", "-t", pane, "-T", title])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        // Best-effort UX surface — must never fail the turn.
        eprintln!("[turn-status] warning: failed to set pane {pane} title: {e}");
    }
}

/// Resolve the registered document basename for a pane. The registry is display
/// evidence only: failure leaves the legacy generic title in place and never
/// affects turn or supervisor authority.
fn document_name_for_pane(base: &Path, pane: &str) -> Option<String> {
    agent_doc_session_registry_io::load_in(base)
        .ok()?
        .values()
        .find(|entry| entry.pane == pane)
        .and_then(|entry| Path::new(&entry.file).file_name())
        .map(|name| name.to_string_lossy().into_owned())
}

/// Set a specific tmux pane's border title from turn/stale-supervisor state.
/// Best-effort: tmux failures are logged and ignored.
pub fn set_pane_title_for_status(base: &Path, pane: &str, active: bool) {
    let document_name = document_name_for_pane(base, pane);
    let title = pane_title_for_status(
        document_name.as_deref(),
        active,
        supervisor_stale(base, pane),
    );
    set_pane_title(pane, &title);
}

/// Clear a specific pane's turn-active projection. This is for supervisors that
/// have stronger idle evidence than a missed harness `Stop` hook.
pub fn clear_turn_status_for_pane(base: &Path, pane: &str) -> Result<()> {
    set_pane_title_for_status(base, pane, false);
    clear_turn_active_marker(base, pane)
}

/// Set the current tmux pane's border title to reflect the turn state. No-op
/// (Ok) when not running inside a tmux pane so the hook never breaks the turn.
pub fn run(active: bool) -> anyhow::Result<()> {
    let Ok(pane) = std::env::var("TMUX_PANE") else {
        return Ok(());
    };
    let pane = pane.trim().to_string();
    if pane.is_empty() {
        return Ok(());
    }
    // `#suptmuxstale` — decorate the pane title with a stale-supervisor warning when
    // the route-owned supervisor has published its `binary_stale` probe on disk.
    // Read-only display; absent/unreadable marker reads as fresh.
    let base = resolve_marker_base();
    let stale = base
        .as_ref()
        .map(|base| supervisor_stale(base, &pane))
        .unwrap_or(false);
    let document_name = base
        .as_ref()
        .and_then(|base| document_name_for_pane(base, &pane));
    let title = pane_title_for_status(document_name.as_deref(), active, stale);
    set_pane_title(&pane, &title);

    // Also maintain the readable turn-state marker so route/supervisor can tell
    // the agent is mid-turn (the bridge to a future hard busy-lease). The hook
    // runs in the agent's CWD = project root; if there is no `.agent-doc`
    // ancestor, skip the marker (the pane title still updated). Best-effort —
    // never fail the turn.
    if let Some(base) = base {
        let result = if active {
            write_turn_active_marker(&base, &pane)
        } else {
            clear_turn_active_marker(&base, &pane)
        };
        if let Err(e) = result {
            eprintln!("[turn-status] warning: failed to update turn-active marker: {e:#}");
        }
    }
    Ok(())
}

/// Resolve the project root for the turn-active marker from the current working
/// directory (the harness hook runs there). `None` when there is no
/// `.agent-doc` ancestor.
fn resolve_marker_base() -> Option<PathBuf> {
    agent_doc_project_root_io::project_root_from_cwd().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_turn::turn_status::TURN_ACTIVE_TTL_SECS;

    #[test]
    fn turn_active_marker_write_read_clear_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        assert!(
            read_turn_active_marker_at(base, 1000).is_none(),
            "no marker before active"
        );

        write_turn_active_marker_at(base, "%7", 1000).unwrap();
        let marker = read_turn_active_marker_at(base, 1000).expect("present after write");
        assert_eq!(marker.pane, "%7");
        assert_eq!(marker.written_at, 1000);

        clear_turn_active_marker(base, "%7").unwrap();
        assert!(
            read_turn_active_marker_at(base, 1000).is_none(),
            "absent after clear"
        );
        // Clearing an absent marker is a no-op, not an error.
        clear_turn_active_marker(base, "%7").unwrap();
    }

    #[test]
    fn supervisor_stale_marker_write_read_clear_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        assert!(!supervisor_stale(base, "%7"), "fresh before any marker");

        set_supervisor_stale_marker(base, "%7", true).unwrap();
        assert!(supervisor_stale(base, "%7"), "stale after marker write");
        assert!(!supervisor_stale(base, "%8"), "another pane is independent");
        set_supervisor_stale_marker(base, "%8", false).unwrap();
        assert!(
            supervisor_stale(base, "%7"),
            "fresh sibling cannot clear stale owner"
        );

        set_supervisor_stale_marker(base, "%7", false).unwrap();
        assert!(!supervisor_stale(base, "%7"), "fresh after marker clear");
        // Clearing an absent marker is a no-op, not an error.
        set_supervisor_stale_marker(base, "%7", false).unwrap();
    }

    #[test]
    fn turn_active_for_pane_matches_only_marker_pane() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        write_turn_active_marker_at(base, "%7", now_secs()).unwrap();

        assert!(turn_active_for_pane(base, "%7"));
        assert!(!turn_active_for_pane(base, "%8"));
    }

    #[test]
    fn active_turn_owner_for_file_requires_exact_registered_pane() {
        let dir = agent_doc_base();
        let base = dir.path();
        let file = base.join("doc.md");
        std::fs::write(&file, "body\n").unwrap();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            file.display().to_string(),
            tmux_router::RegistryEntry {
                pane: "%152".to_string(),
                pid: std::process::id(),
                cwd: base.display().to_string(),
                started: "2026-10-08T22:06:04Z".to_string(),
                session_id: "session-live".to_string(),
                file: file.display().to_string(),
                window: "@2".to_string(),
                supervisor_instance_id: "supervisor-live".to_string(),
            },
        );
        agent_doc_session_registry_io::save_in(base, &registry).unwrap();

        write_turn_active_marker(base, "%999").unwrap();
        assert!(
            active_turn_owner_for_file(&file).is_none(),
            "a sibling pane's fresh marker is not document liveness"
        );

        write_turn_active_marker(base, "%152").unwrap();
        let marker = active_turn_owner_for_file(&file).expect("exact owner is active");
        assert_eq!(marker.pane, "%152");

        write_turn_active_marker_at(
            base,
            "%152",
            now_secs().saturating_sub(TURN_ACTIVE_TTL_SECS),
        )
        .unwrap();
        assert!(
            active_turn_owner_for_file(&file).is_none(),
            "an expired exact-owner lease is not liveness proof"
        );
    }

    #[test]
    fn concurrent_pane_markers_survive_sibling_idle() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();

        write_turn_active_marker_at(base, "%7", now_secs()).unwrap();
        write_turn_active_marker_at(base, "%8", now_secs()).unwrap();
        assert!(turn_active_for_pane(base, "%7"));
        assert!(turn_active_for_pane(base, "%8"));

        clear_turn_active_marker(base, "%8").unwrap();
        assert!(
            turn_active_for_pane(base, "%7"),
            "sibling Stop must preserve the active pane"
        );
        assert!(!turn_active_for_pane(base, "%8"));
    }

    #[test]
    fn pane_scoped_reads_and_clears_support_legacy_project_marker() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let conn = open_state_db(base).unwrap();
        upsert_coordination_lease_in_db(
            &conn,
            &CoordinationLeaseRecord {
                scope_kind: TURN_ACTIVE_SCOPE.to_string(),
                scope_id: PROJECT_SCOPE_ID.to_string(),
                holder: "%7".to_string(),
                holder_pid: None,
                heartbeat_secs: now_secs(),
            },
        )
        .unwrap();

        assert!(turn_active_for_pane(base, "%7"));
        clear_turn_active_marker(base, "%8").unwrap();
        assert!(
            turn_active_for_pane(base, "%7"),
            "foreign idle must not clear a legacy owner"
        );
        clear_turn_active_marker(base, "%7").unwrap();
        assert!(!turn_active_for_pane(base, "%7"));
    }

    fn turn_active_rows(base: &Path) -> Vec<String> {
        let conn = open_state_db(base).unwrap();
        let mut ids: Vec<String> =
            load_coordination_leases_for_scope_kind_from_db(&conn, TURN_ACTIVE_SCOPE)
                .unwrap()
                .into_iter()
                .map(|lease| lease.scope_id)
                .collect();
        ids.sort();
        ids
    }

    fn agent_doc_base() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        dir
    }

    #[test]
    fn registered_document_name_is_resolved_by_pane() {
        let dir = agent_doc_base();
        let base = dir.path();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "sample-session.md".to_string(),
            tmux_router::RegistryEntry {
                pane: "%71".to_string(),
                pid: std::process::id(),
                cwd: base.display().to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-71".to_string(),
                file: "tasks/sample-session.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: "supervisor-71".to_string(),
            },
        );
        agent_doc_session_registry_io::save_in(base, &registry).unwrap();

        assert_eq!(
            document_name_for_pane(base, "%71").as_deref(),
            Some("sample-session.md")
        );
        assert_eq!(document_name_for_pane(base, "%72"), None);
    }

    /// GH #135: a pane that died before its idle hook leaves a row no per-pane
    /// clear can ever reach. The age sweep removes it without knowing the pane.
    #[test]
    fn sweep_deletes_expired_orphan_row_for_nonexistent_pane() {
        let dir = agent_doc_base();
        let base = dir.path();
        let now = 1_000_000;
        write_turn_active_marker_at(base, "%53", now - 292_943).unwrap();
        write_turn_active_marker_at(base, "%436", now - 76_837).unwrap();
        write_turn_active_marker_at(base, "%434", now - 144).unwrap();

        let deleted = sweep_expired_turn_active_markers_at(base, now).unwrap();

        assert_eq!(deleted, 2, "both expired rows are reclaimed");
        assert_eq!(turn_active_rows(base), vec!["%434".to_string()]);
    }

    /// The live turn's lease is never swept, including one exactly one second
    /// inside the TTL, and the sweep is a no-op when nothing is expired.
    #[test]
    fn sweep_keeps_fresh_rows() {
        let dir = agent_doc_base();
        let base = dir.path();
        let now = 1_000_000;
        write_turn_active_marker_at(base, "%7", now).unwrap();
        write_turn_active_marker_at(base, "%8", now - (TURN_ACTIVE_TTL_SECS - 1)).unwrap();
        write_turn_active_marker_at(base, "%9", now - TURN_ACTIVE_TTL_SECS).unwrap();

        assert_eq!(count_expired_turn_active_markers_at(base, now).unwrap(), 1);
        assert_eq!(sweep_expired_turn_active_markers_at(base, now).unwrap(), 1);
        assert_eq!(
            turn_active_rows(base),
            vec!["%7".to_string(), "%8".to_string()]
        );
        assert_eq!(sweep_expired_turn_active_markers_at(base, now).unwrap(), 0);

        // Inside the first TTL window nothing can be expired.
        let early = agent_doc_base();
        write_turn_active_marker_at(early.path(), "%1", 0).unwrap();
        assert_eq!(
            sweep_expired_turn_active_markers_at(early.path(), TURN_ACTIVE_TTL_SECS - 1).unwrap(),
            0
        );
        assert_eq!(turn_active_rows(early.path()), vec!["%1".to_string()]);
    }

    /// `clear_matching_turn_status_projection` gates its clear on
    /// `turn_active_for_pane_for_file`, which reads an expired lease as absent
    /// and so never reached the clear. That read now reclaims the row itself.
    #[test]
    fn expired_row_is_reclaimed_by_the_read_that_reports_it_absent() {
        let dir = agent_doc_base();
        let base = dir.path();
        let file = base.join("session.md");
        std::fs::write(&file, "# session\n").unwrap();
        let stale = now_secs() - TURN_ACTIVE_TTL_SECS - 10;
        write_turn_active_marker_at(base, "%65", stale).unwrap();
        write_turn_active_marker_at(base, "%66", now_secs()).unwrap();

        assert!(
            !turn_active_for_pane_for_file(&file, "%65"),
            "an expired lease still reads as absent"
        );
        assert_eq!(
            turn_active_rows(base),
            vec!["%66".to_string()],
            "the read reclaimed the expired row and kept the fresh one"
        );

        // The project-wide read reclaims too.
        write_turn_active_marker_at(base, "%55", stale).unwrap();
        let marker = read_turn_active_marker(base).expect("fresh marker survives");
        assert_eq!(marker.pane, "%66");
        assert_eq!(turn_active_rows(base), vec!["%66".to_string()]);
    }
}
