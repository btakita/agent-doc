//! `#gh133sigterm` (GH #133 follow-up) — durable intentional-exit marker I/O and
//! the supervisor's `SIGTERM` handler.
//!
//! Policy lives in [`agent_doc_supervisor::intentional_exit`]. This module owns
//! the `state.db` row (`document_runtime_state`, kind
//! [`INTENTIONAL_EXIT_STATE_KIND`], keyed by the hash of the actor's canonical
//! document id) and the signal plumbing.
//!
//! The handler records the marker and then re-delivers `SIGTERM` under the
//! default disposition, so the process still dies exactly as before (status
//! 143, child PTY torn down by the kernel). The record has a bounded budget
//! ([`HANDLER_WRITE_BUDGET`]) that fits inside the 750 ms SIGTERM→SIGKILL
//! grace of `force_kill_verified_supervisor_pid`; a write that cannot finish in
//! time is abandoned and the exit proceeds unmarked (read as a crash — the
//! historical behaviour, never a stuck process).

use std::path::{Path, PathBuf};
use std::time::Duration;

use agent_doc_sqlite::state_store::{self, Connection, DocumentRuntimeStateRecord};
use agent_doc_supervisor::intentional_exit::{
    INTENTIONAL_EXIT_STATE_KIND, IntentionalExitMarker, parse_intentional_exit_marker,
};
use anyhow::Result;

/// Total time the `SIGTERM` handler may spend recording the marker before it
/// lets the process exit regardless.
pub const HANDLER_WRITE_BUDGET: Duration = Duration::from_millis(500);
/// SQLite busy timeout for the handler's own connection.
const HANDLER_BUSY_TIMEOUT: Duration = Duration::from_millis(300);

/// Row key for a document's marker: the same path-string hash every other
/// per-document state row uses, applied to the actor's canonical document id.
pub fn marker_key(document_id: &str) -> String {
    agent_doc_fs::document_state_hash_from_str(document_id)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Upsert the marker for `document_id` on an open connection.
pub fn record_intentional_exit_in_db(
    conn: &Connection,
    document_id: &str,
    marker: &IntentionalExitMarker,
) -> Result<()> {
    state_store::upsert_document_runtime_state_in_db(
        conn,
        &DocumentRuntimeStateRecord {
            document_hash: marker_key(document_id),
            state_kind: INTENTIONAL_EXIT_STATE_KIND.to_string(),
            canonical_path: document_id.to_string(),
            payload_json: serde_json::to_string(marker)?,
            updated_at_ms: marker.recorded_at_ms,
        },
    )
}

/// Load the marker for `document_id`, if any. A malformed payload is absent.
pub fn load_intentional_exit_from_db(
    conn: &Connection,
    document_id: &str,
) -> Result<Option<IntentionalExitMarker>> {
    Ok(state_store::load_document_runtime_state_from_db(
        conn,
        &marker_key(document_id),
        INTENTIONAL_EXIT_STATE_KIND,
    )?
    .and_then(|row| parse_intentional_exit_marker(&row.payload_json)))
}

/// Remove the marker for `document_id`. Returns whether a row was removed.
pub fn clear_intentional_exit_in_db(conn: &Connection, document_id: &str) -> Result<bool> {
    state_store::clear_document_runtime_state_in_db(
        conn,
        &marker_key(document_id),
        INTENTIONAL_EXIT_STATE_KIND,
    )
}

/// Remove the marker for `document_id` in the project's `state.db`.
///
/// Called by the controller-initiated replacement path after it has stopped
/// the old supervisor (its SIGTERM is controller intent to *continue* the
/// document, not operator intent to stop it), and by a supervisor once it has
/// registered (a newly started generation supersedes any earlier exit).
pub fn clear_intentional_exit(project_root: &Path, document_id: &str) -> Result<bool> {
    let conn = state_store::open_state_db(project_root)?;
    clear_intentional_exit_in_db(&conn, document_id)
}

/// `#runfrontendcrashed`: after a controller replacement has stopped the old
/// supervisor, replace its exit marker with a time-bounded cold-start marker so
/// the crash watchdog does not issue a second replacement that kills the
/// booting successor. Returns the marker written, or `None` when neither the
/// handler's marker nor the kill outcome identified the stopped pid (the row is
/// then cleared, preserving the historical crash-recovery behaviour).
pub fn mark_replacement_cold_start_pending(
    project_root: &Path,
    document_id: &str,
    killed_pid: Option<u32>,
    generation: u64,
    pane_id: &str,
    session_id: &str,
) -> Result<Option<IntentionalExitMarker>> {
    let conn = state_store::open_state_db(project_root)?;
    let prior = load_intentional_exit_from_db(&conn, document_id)?;
    let Some(marker) = agent_doc_supervisor::intentional_exit::replacement_cold_start_marker(
        prior.as_ref(),
        killed_pid,
        generation,
        pane_id,
        session_id,
        now_ms(),
    ) else {
        clear_intentional_exit_in_db(&conn, document_id)?;
        return Ok(None);
    };
    record_intentional_exit_in_db(&conn, document_id, &marker)?;
    Ok(Some(marker))
}

/// Who and what is exiting — captured when the supervisor registers, so the
/// handler never has to consult shared runtime state from signal context.
#[derive(Debug, Clone)]
pub struct IntentionalExitIdentity {
    pub project_root: PathBuf,
    pub document_id: String,
    pub supervisor_pid: u32,
    pub generation: u64,
    pub pane_id: String,
    pub session_id: String,
}

impl IntentionalExitIdentity {
    fn marker(&self, signal: &str) -> IntentionalExitMarker {
        IntentionalExitMarker {
            supervisor_pid: self.supervisor_pid,
            generation: self.generation,
            pane_id: self.pane_id.clone(),
            session_id: self.session_id.clone(),
            signal: signal.to_string(),
            recorded_at_ms: now_ms(),
        }
    }
}

/// Record the marker for `identity` directly (bounded busy timeout).
pub fn record_intentional_exit(identity: &IntentionalExitIdentity, signal: &str) -> Result<()> {
    let conn =
        state_store::open_state_db_with_timeout(&identity.project_root, HANDLER_BUSY_TIMEOUT)?;
    record_intentional_exit_in_db(&conn, &identity.document_id, &identity.marker(signal))
}

/// Outcome of the handler's bounded record attempt, reported to the caller's
/// logger before the process exits.
#[derive(Debug)]
pub enum IntentionalExitRecordOutcome {
    Recorded,
    Failed(String),
    TimedOut,
}

/// Install the supervisor `SIGTERM` handler.
///
/// On `SIGTERM` a dedicated thread records the marker within `write_budget`
/// (production: [`HANDLER_WRITE_BUDGET`]), reports the outcome through `on_outcome`, and then
/// re-raises `SIGTERM` with the default disposition so the process terminates
/// exactly as it did before the handler existed.
#[cfg(unix)]
pub fn install_sigterm_intentional_exit_handler<F>(
    identity: IntentionalExitIdentity,
    write_budget: Duration,
    on_outcome: F,
) -> Result<()>
where
    F: Fn(&IntentionalExitIdentity, &IntentionalExitRecordOutcome) + Send + 'static,
{
    use signal_hook::consts::SIGTERM;
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGTERM])?;
    std::thread::Builder::new()
        .name("agent-doc-supervisor-sigterm".to_string())
        .spawn(move || {
            if signals.forever().next().is_none() {
                return;
            }
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let writer_identity = identity.clone();
            let _ = std::thread::Builder::new()
                .name("agent-doc-supervisor-sigterm-record".to_string())
                .spawn(move || {
                    let outcome = match record_intentional_exit(&writer_identity, "SIGTERM") {
                        Ok(()) => IntentionalExitRecordOutcome::Recorded,
                        Err(err) => IntentionalExitRecordOutcome::Failed(format!("{err:#}")),
                    };
                    let _ = sender.send(outcome);
                });
            let outcome = receiver
                .recv_timeout(write_budget)
                .unwrap_or(IntentionalExitRecordOutcome::TimedOut);
            on_outcome(&identity, &outcome);
            // Resets SIGTERM to SIG_DFL and re-delivers it: same exit as before.
            let _ = signal_hook::low_level::emulate_default_handler(SIGTERM);
            // `emulate_default_handler` does not return for a terminating signal;
            // this is a defensive fallback that preserves the 128+15 status.
            std::process::exit(143);
        })?;
    Ok(())
}

#[cfg(not(unix))]
pub fn install_sigterm_intentional_exit_handler<F>(
    _identity: IntentionalExitIdentity,
    _write_budget: Duration,
    _on_outcome: F,
) -> Result<()>
where
    F: Fn(&IntentionalExitIdentity, &IntentionalExitRecordOutcome) + Send + 'static,
{
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_supervisor::intentional_exit::{
        IntentionalExitDecision, watchdog_intentional_exit_decision,
    };

    const HELPER_ROOT_ENV: &str = "AGENT_DOC_TEST_SIGTERM_HELPER_ROOT";
    const DOCUMENT_ID: &str = "/fixture/doc.md";

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        dir
    }

    fn identity(root: &Path, pid: u32) -> IntentionalExitIdentity {
        IntentionalExitIdentity {
            project_root: root.to_path_buf(),
            document_id: DOCUMENT_ID.to_string(),
            supervisor_pid: pid,
            generation: 4,
            pane_id: "%9".to_string(),
            session_id: "session".to_string(),
        }
    }

    #[test]
    fn record_load_clear_round_trip() {
        let dir = project();
        record_intentional_exit(&identity(dir.path(), 77), "SIGTERM").unwrap();
        let conn = state_store::open_state_db(dir.path()).unwrap();
        let marker = load_intentional_exit_from_db(&conn, DOCUMENT_ID)
            .unwrap()
            .expect("marker recorded");
        assert_eq!(marker.supervisor_pid, 77);
        assert_eq!(marker.generation, 4);
        assert_eq!(marker.signal, "SIGTERM");
        assert!(
            load_intentional_exit_from_db(&conn, "/fixture/other.md")
                .unwrap()
                .is_none()
        );
        assert!(clear_intentional_exit(dir.path(), DOCUMENT_ID).unwrap());
        assert!(
            load_intentional_exit_from_db(&conn, DOCUMENT_ID)
                .unwrap()
                .is_none()
        );
    }

    /// Controller replacement: the replacement path clears the marker its own
    /// SIGTERM produced, so neither the cold start nor a later crash of the
    /// replaced document is mistaken for operator intent.
    #[test]
    fn controller_replacement_clears_marker_so_watchdog_still_restarts() {
        let dir = project();
        record_intentional_exit(&identity(dir.path(), 77), "SIGTERM").unwrap();
        let conn = state_store::open_state_db(dir.path()).unwrap();
        let before = load_intentional_exit_from_db(&conn, DOCUMENT_ID).unwrap();
        assert_eq!(
            watchdog_intentional_exit_decision(before.as_ref(), 77, 4, now_ms()),
            IntentionalExitDecision::Intentional
        );
        clear_intentional_exit(dir.path(), DOCUMENT_ID).unwrap();
        let after = load_intentional_exit_from_db(&conn, DOCUMENT_ID).unwrap();
        assert!(
            watchdog_intentional_exit_decision(after.as_ref(), 77, 4, now_ms()).allows_restart()
        );
    }

    /// Child-process body for [`sigterm_records_intentional_exit_marker_and_still_exits`].
    /// Inert unless the parent test sets [`HELPER_ROOT_ENV`].
    #[test]
    #[ignore = "helper process for the SIGTERM handler test"]
    fn sigterm_helper_process() {
        let Some(root) = std::env::var_os(HELPER_ROOT_ENV) else {
            return;
        };
        let root = PathBuf::from(root);
        let outcome_path = root.join("outcome.txt");
        install_sigterm_intentional_exit_handler(
            identity(&root, std::process::id()),
            // The fixture asserts the record itself, not the production budget;
            // a saturated test host must not turn it into a timeout.
            Duration::from_secs(15),
            move |_, outcome| {
                let _ = std::fs::write(&outcome_path, format!("{outcome:?}"));
            },
        )
        .unwrap();
        std::fs::write(root.join("ready"), std::process::id().to_string()).unwrap();
        std::thread::sleep(Duration::from_secs(30));
        panic!("helper was never terminated");
    }

    /// Deliberate SIGTERM: a real process with the handler installed records a
    /// marker naming its own pid, and still dies by SIGTERM (status 143).
    #[cfg(unix)]
    #[test]
    fn sigterm_records_intentional_exit_marker_and_still_exits() {
        use std::os::unix::process::ExitStatusExt;

        let dir = project();
        // Production supervisors write into an existing state.db.
        drop(state_store::open_state_db(dir.path()).unwrap());
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "intentional_exit::tests::sigterm_helper_process",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(HELPER_ROOT_ENV, dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let ready = dir.path().join("ready");
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !ready.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "helper never became ready"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // Signal only the fixture child this test spawned.
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
        let status = child.wait().unwrap();
        assert_eq!(
            status.signal(),
            Some(libc::SIGTERM),
            "exit stays a SIGTERM death"
        );

        let conn = state_store::open_state_db(dir.path()).unwrap();
        let marker = load_intentional_exit_from_db(&conn, DOCUMENT_ID)
            .unwrap()
            .expect("SIGTERM handler recorded the marker");
        assert_eq!(marker.supervisor_pid, child.id());
        assert_eq!(
            watchdog_intentional_exit_decision(Some(&marker), child.id(), 4, now_ms()),
            IntentionalExitDecision::Intentional,
            "watchdog must not respawn a deliberately terminated supervisor"
        );
        let outcome = std::fs::read_to_string(dir.path().join("outcome.txt")).unwrap();
        assert_eq!(outcome, "Recorded");
    }
}
