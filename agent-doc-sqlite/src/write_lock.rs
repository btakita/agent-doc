//! Write-lock discipline shared by every SQLite writer in agent-doc
//! (`#sqlitebusyupgrade`).
//!
//! ## Why a write transaction must take the lock up front
//!
//! `Connection::transaction()` issues a DEFERRED `BEGIN`. The first `SELECT`
//! takes a read snapshot; the first `INSERT`/`UPDATE`/`DELETE` then tries to
//! *upgrade* that read transaction to a write transaction. When another
//! connection already holds the write lock, SQLite refuses the upgrade with
//! `SQLITE_BUSY` (error code 5) **immediately and without invoking the busy
//! handler**: waiting could deadlock, since the holder may itself need this
//! reader's snapshot to clear. `busy_timeout` therefore cannot help a deferred
//! read-then-write transaction (`sqlite3BtreeBeginTrans` only retries through
//! the busy handler while `inTransaction == TRANS_NONE`). Under WAL the same
//! upgrade can also fail with `SQLITE_BUSY_SNAPSHOT` (517) once a concurrent
//! writer has committed past the reader's snapshot.
//!
//! On 2026-11-04 this failed `agent-doc <FILE>` as a bare
//! `Error: database is locked` whenever the project controller happened to be
//! writing `state.db` at the moment the owner-pane wedge counter upgraded its
//! `SELECT` to an `UPSERT` — in the controller's handler and then again in the
//! CLI's direct fallback.
//!
//! [`immediate_transaction`] / [`immediate_transaction_unchecked`] issue
//! `BEGIN IMMEDIATE`, which acquires the write lock *before* the first read.
//! Acquiring it from no transaction is the one place SQLite does run the busy
//! handler, so a contended writer waits out `busy_timeout` and then succeeds.
//! Use them for every transaction that may write; a transaction that only reads
//! may stay deferred.
//!
//! [`open_state_db_file`] covers the other half: a raw `Connection::open` has
//! no busy handler at all, so even an autocommit write fails immediately under
//! contention. Co-tenant table owners that cannot run the state-store
//! initialization (the op log, Lazily's reliable-sync outbox) open through it.

use anyhow::{Context, Result};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::path::Path;
use std::time::Duration;

/// Busy timeout for every `state.db` opener (mirrors `state_store`'s
/// 30-second recovery budget).
pub const STATE_DB_BUSY_TIMEOUT: Duration = Duration::from_secs(30);

/// Begin a transaction that may write, reserving the write lock before the
/// first statement so contention waits through the busy handler.
pub fn immediate_transaction(conn: &mut Connection) -> rusqlite::Result<Transaction<'_>> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
}

/// [`immediate_transaction`] for callers that only hold `&Connection`.
///
/// Like `Connection::unchecked_transaction`, nesting is the caller's
/// responsibility: SQLite rejects a `BEGIN` inside an open transaction.
pub fn immediate_transaction_unchecked(conn: &Connection) -> rusqlite::Result<Transaction<'_>> {
    Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
}

/// Open a raw connection to an existing-or-new `state.db` file with the
/// standard busy timeout, without the state-store schema initialization.
///
/// For co-tenant tables whose owner manages its own schema (the op log, the
/// Lazily reliable-sync outbox). A bare `Connection::open` has no busy handler,
/// so every contended statement on it fails immediately with `SQLITE_BUSY`.
pub fn open_state_db_file(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let conn =
        Connection::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    conn.busy_timeout(STATE_DB_BUSY_TIMEOUT)?;
    Ok(conn)
}

/// Test support: run `work` on `worker` while a second connection holds the
/// database write lock, and prove the write path *waits* for that holder.
///
/// The holder takes `BEGIN IMMEDIATE`. `worker`'s busy handler is replaced with
/// one that reports the first wait. When the worker reports waiting, the holder
/// commits and the worker's result is returned. If `work` instead finishes
/// while the lock is still held, SQLite never consulted the busy handler — the
/// deferred read-then-write upgrade defect — and this returns an error carrying
/// the worker's outcome. No sleeps or load are involved: the verdict depends
/// only on whether SQLite invoked the busy handler. (The probe installs its own
/// handler, so it cannot see a missing `busy_timeout`; assert `PRAGMA
/// busy_timeout` for that.)
#[doc(hidden)]
pub fn run_while_write_lock_held<T: Send + 'static>(
    db_path: &Path,
    worker: Connection,
    work: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
) -> Result<T> {
    use std::sync::mpsc;

    enum Event {
        Waiting,
        Done,
    }

    thread_local! {
        static WAITING: std::cell::RefCell<Option<mpsc::Sender<Event>>> =
            const { std::cell::RefCell::new(None) };
    }

    fn report_wait_then_retry(count: i32) -> bool {
        if count == 0 {
            WAITING.with(|waiting| {
                if let Some(sender) = waiting.borrow().as_ref() {
                    let _ = sender.send(Event::Waiting);
                }
            });
        }
        std::thread::sleep(Duration::from_millis(1));
        // Bound the wait so a holder that never commits fails the test
        // instead of hanging it.
        count < 30_000
    }

    let holder = Connection::open(db_path)
        .with_context(|| format!("open lock holder on {}", db_path.display()))?;
    holder.execute_batch("BEGIN IMMEDIATE")?;
    worker.busy_handler(Some(report_wait_then_retry))?;

    let (event_tx, event_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let mut worker = worker;
        WAITING.with(|waiting| *waiting.borrow_mut() = Some(event_tx.clone()));
        let result = work(&mut worker);
        let _ = result_tx.send(result);
        let _ = event_tx.send(Event::Done);
    });

    let first = event_rx.recv_timeout(Duration::from_secs(60));
    match first {
        Ok(Event::Waiting) => {
            holder.execute_batch("COMMIT")?;
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("write-lock probe worker panicked"))?;
            result_rx
                .recv()
                .context("write-lock probe worker sent no result")?
        }
        Ok(Event::Done) => {
            holder.execute_batch("ROLLBACK")?;
            let _ = thread.join();
            let outcome = match result_rx.recv() {
                Ok(Ok(_)) => "Ok(..)".to_string(),
                Ok(Err(error)) => format!("Err({error:#})"),
                Err(_) => "<no result>".to_string(),
            };
            anyhow::bail!(
                "write path finished while another connection held the write lock without \
                 ever waiting on the busy handler (a deferred read-then-write upgrade): \
                 {outcome}"
            )
        }
        Err(_) => {
            holder.execute_batch("ROLLBACK")?;
            anyhow::bail!("write-lock probe worker neither waited nor finished within 60s")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded_db(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("probe.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE counter (id INTEGER PRIMARY KEY, value INTEGER NOT NULL);
             INSERT INTO counter (id, value) VALUES (1, 0);",
        )
        .unwrap();
        path
    }

    fn read_then_write(tx: &Transaction<'_>) -> Result<i64> {
        let value: i64 = tx.query_row("SELECT value FROM counter WHERE id = 1", [], |row| {
            row.get(0)
        })?;
        tx.execute("UPDATE counter SET value = ?1 WHERE id = 1", [value + 1])?;
        Ok(value + 1)
    }

    #[test]
    fn deferred_read_then_write_fails_without_waiting_for_the_holder() {
        // Pins the SQLite behavior this module exists for: the probe must see
        // the deferred upgrade fail immediately, or it could not detect it.
        let dir = tempfile::tempdir().unwrap();
        let path = seeded_db(dir.path());
        let worker = open_state_db_file(&path).unwrap();
        let error = run_while_write_lock_held(&path, worker, |conn| {
            // The defect under test: a deferred transaction.
            #[allow(clippy::disallowed_methods)]
            let tx = conn.transaction()?;
            let value = read_then_write(&tx)?;
            tx.commit()?;
            Ok(value)
        })
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("without ever waiting") && message.contains("database is locked"),
            "{message}"
        );
    }

    #[test]
    fn immediate_read_then_write_waits_for_the_holder_then_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let path = seeded_db(dir.path());
        let worker = open_state_db_file(&path).unwrap();
        let value = run_while_write_lock_held(&path, worker, |conn| {
            let tx = immediate_transaction(conn)?;
            let value = read_then_write(&tx)?;
            tx.commit()?;
            Ok(value)
        })
        .unwrap();
        assert_eq!(value, 1);
    }

    #[test]
    fn unchecked_immediate_read_then_write_waits_for_the_holder_then_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let path = seeded_db(dir.path());
        let worker = open_state_db_file(&path).unwrap();
        let value = run_while_write_lock_held(&path, worker, |conn| {
            let tx = immediate_transaction_unchecked(conn)?;
            let value = read_then_write(&tx)?;
            tx.commit()?;
            Ok(value)
        })
        .unwrap();
        assert_eq!(value, 1);
    }

    #[test]
    fn open_state_db_file_installs_the_state_db_busy_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_state_db_file(&dir.path().join("nested").join("state.db")).unwrap();
        let timeout_ms: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout_ms, STATE_DB_BUSY_TIMEOUT.as_millis() as i64);
    }
}
