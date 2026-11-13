//! One-transaction schema declaration for the co-tenant stores
//! (`#storeschemabatch`).
//!
//! The answer-ledger (with its derivation columns), premise and reliable-sync
//! stores each declare their tables with `CREATE TABLE IF NOT EXISTS` and a few
//! guarded `ALTER TABLE`s, on every load. `execute_batch` runs each statement in
//! autocommit, so on a fresh project every table, index and added column was its
//! own commit (~26 in all), and under WAL each commit is an `fsync` on the
//! admission path. `state.db`'s own shape got the same fix in
//! `#releasegateflake`.
//!
//! Wrapping the declaration in `BEGIN IMMEDIATE` alone would make the fresh
//! case one commit but would take the database write lock on *every* load,
//! including the converged case, where today the `IF NOT EXISTS` statements are
//! reads. So [`declare_store_schema`] first checks, with reads only, whether the
//! database already holds the declared shape, and only when it does not opens
//! one immediate transaction for the whole declaration.
//!
//! The shape it checks is not a second hand-written schema: it is read back
//! from running the store's own declaration against a scratch in-memory
//! database, once per process. The declaration stays the single source of the
//! shape, and the check cannot drift from it.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

/// A store's schema declaration. It must be idempotent (`IF NOT EXISTS`,
/// column-guarded `ALTER`s) and must not open its own transaction.
pub(crate) type Declare = fn(&Connection) -> Result<()>;

/// Every table (with its columns) and index a declaration produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclaredShape {
    pub(crate) tables: Vec<(String, BTreeSet<String>)>,
    pub(crate) indexes: Vec<String>,
}

impl DeclaredShape {
    /// The shape `declare` leaves on an empty database.
    pub(crate) fn of(declare: Declare) -> Result<Self> {
        let scratch =
            Connection::open_in_memory().context("open scratch schema-declaration database")?;
        declare(&scratch).context("declare store schema on scratch database")?;
        Self::read(&scratch)
    }

    /// Every user table and named index in `conn`'s main schema.
    pub(crate) fn read(conn: &Connection) -> Result<Self> {
        let mut statement = conn.prepare(
            "SELECT type, name FROM main.sqlite_master
             WHERE type IN ('table', 'index') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
             ORDER BY type, name",
        )?;
        let objects = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut shape = Self {
            tables: Vec::new(),
            indexes: Vec::new(),
        };
        for (kind, name) in objects {
            if kind == "table" {
                let columns = table_columns(conn, &name)?;
                shape.tables.push((name, columns));
            } else {
                shape.indexes.push(name);
            }
        }
        Ok(shape)
    }

    /// Whether `conn` already holds every table, column and index of this
    /// shape, so the declaration would change nothing. Reads only. Extra
    /// tables or columns (a newer binary's) do not matter.
    pub(crate) fn holds_in(&self, conn: &Connection) -> Result<bool> {
        for (table, columns) in &self.tables {
            let present = table_columns(conn, table)?;
            if !columns.is_subset(&present) {
                return Ok(false);
            }
        }
        let mut index_exists = conn.prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type = 'index' AND name = ?1)",
        )?;
        for index in &self.indexes {
            if !index_exists.query_row(params![index], |row| row.get::<_, bool>(0))? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// The column names of `table` in `conn`'s main schema; empty when the table
/// does not exist.
fn table_columns(conn: &Connection, table: &str) -> Result<BTreeSet<String>> {
    let mut statement = conn.prepare_cached("SELECT name FROM pragma_table_info(?1, 'main')")?;
    let columns = statement
        .query_map(params![table], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    Ok(columns)
}

/// Declare a store's schema on `conn` with at most one write transaction.
///
/// When `conn` already holds the shape `declare` produces, this only reads:
/// it takes no write lock and commits nothing. Otherwise the whole declaration
/// runs in one `BEGIN IMMEDIATE` transaction (one commit, one `fsync`), which
/// re-runs the idempotent declaration under the write lock, so two processes
/// declaring a fresh database at once converge without error. A caller already
/// inside a transaction gets the declaration as part of that transaction.
///
/// `shape` memoizes the declared shape for the process; if it cannot be
/// derived, every call falls back to declaring in a transaction.
pub(crate) fn declare_store_schema(
    conn: &Connection,
    shape: &'static OnceLock<Option<DeclaredShape>>,
    declare: Declare,
) -> Result<()> {
    let shape = shape.get_or_init(|| DeclaredShape::of(declare).ok());
    if let Some(shape) = shape
        && shape.holds_in(conn)?
    {
        return Ok(());
    }
    if !conn.is_autocommit() {
        return declare(conn);
    }
    let tx = crate::write_lock::immediate_transaction_unchecked(conn)
        .context("begin store schema declaration")?;
    declare(&tx)?;
    tx.commit().context("commit store schema declaration")
}

/// Test support: run `work` on `conn` and return its result with the number
/// of transactions SQLite committed on `conn` meanwhile. SQLite's commit hook
/// fires once per committed write transaction, so this counts `fsync`s, not
/// wall-clock time.
#[cfg(test)]
pub(crate) fn count_commits<T>(
    conn: &Connection,
    work: impl FnOnce(&Connection) -> Result<T>,
) -> Result<(T, usize)> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    unsafe extern "C" fn count_commit(counter: *mut std::ffi::c_void) -> std::ffi::c_int {
        // SAFETY: `counter` is the `AtomicUsize` below, which outlives the
        // hook: the hook is removed before this function returns.
        unsafe { (*(counter as *const AtomicUsize)).fetch_add(1, Ordering::SeqCst) };
        0
    }

    let commits = AtomicUsize::new(0);
    // SAFETY: the handle is live for this call, and the hook argument points
    // at `commits`, which outlives the hook (removed below on every path).
    unsafe {
        rusqlite::ffi::sqlite3_commit_hook(
            conn.handle(),
            Some(count_commit),
            &commits as *const AtomicUsize as *mut std::ffi::c_void,
        );
    }
    let result = work(conn);
    // SAFETY: as above; this uninstalls the hook.
    unsafe {
        rusqlite::ffi::sqlite3_commit_hook(conn.handle(), None, std::ptr::null_mut());
    }
    Ok((result?, commits.load(Ordering::SeqCst)))
}

/// Test support: run `work` on a fresh connection to `db_path` while another
/// connection holds that database's write lock, with no busy handler on the
/// worker. Any statement of `work` that needs the write lock fails at once
/// with `SQLITE_BUSY` instead of waiting, so an `Ok` result proves `work`
/// never asked for it. Also returns the commits `work` made (always 0 when it
/// succeeds, since it cannot have written).
#[cfg(test)]
pub(crate) fn run_without_write_lock<T>(
    db_path: &std::path::Path,
    work: impl FnOnce(&Connection) -> Result<T>,
) -> Result<(T, usize)> {
    let holder = Connection::open(db_path)?;
    holder.execute_batch("BEGIN IMMEDIATE")?;
    let worker = Connection::open(db_path)?;
    worker.busy_timeout(std::time::Duration::ZERO)?;
    let outcome = count_commits(&worker, work);
    holder.execute_batch("ROLLBACK")?;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declare_two_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS a (id INTEGER PRIMARY KEY, v TEXT);
             CREATE TABLE IF NOT EXISTS b (id INTEGER PRIMARY KEY);
             CREATE INDEX IF NOT EXISTS a_v ON a (v);",
        )?;
        let columns = table_columns(conn, "b")?;
        if !columns.contains("added") {
            conn.execute_batch("ALTER TABLE b ADD COLUMN added TEXT")?;
        }
        Ok(())
    }

    static SHAPE: OnceLock<Option<DeclaredShape>> = OnceLock::new();

    #[test]
    fn the_declared_shape_is_read_back_from_the_declaration() -> Result<()> {
        let shape = DeclaredShape::of(declare_two_tables)?;
        let columns = |names: &[&str]| names.iter().map(|name| name.to_string()).collect();
        assert_eq!(
            shape,
            DeclaredShape {
                tables: vec![
                    ("a".to_string(), columns(&["id", "v"])),
                    ("b".to_string(), columns(&["added", "id"])),
                ],
                indexes: vec!["a_v".to_string()],
            }
        );
        Ok(())
    }

    /// A fresh declaration is one commit; a converged one takes no write lock
    /// and commits nothing; a database missing one column converges in one.
    #[test]
    fn fresh_is_one_commit_converged_is_none_and_partial_is_one() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let path = dir.path().join("store.db");
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL")?;

        let ((), fresh) = count_commits(&conn, |conn| {
            declare_store_schema(conn, &SHAPE, declare_two_tables)
        })?;
        assert_eq!(fresh, 1, "a fresh declaration must commit once");

        let ((), converged) = run_without_write_lock(&path, |conn| {
            declare_store_schema(conn, &SHAPE, declare_two_tables)
        })?;
        assert_eq!(converged, 0);

        conn.execute_batch("DROP INDEX a_v")?;
        let ((), partial) = count_commits(&conn, |conn| {
            declare_store_schema(conn, &SHAPE, declare_two_tables)
        })?;
        assert_eq!(partial, 1);
        assert!(SHAPE.get().unwrap().as_ref().unwrap().holds_in(&conn)?);
        Ok(())
    }

    /// Load every co-tenant store this module batches, as the first cycle on
    /// a new project does: answer ledger, derivations, premises, reliable-sync.
    fn load_every_store(conn: &Connection) -> Result<()> {
        crate::answer_ledger_index::load(conn, "doc")?;
        crate::answer_ledger_derivations::load_derivations(conn, "doc")?;
        crate::premise_statement_index::load(conn, "doc")?;
        crate::reliable_sync_inbox::ensure_schema(conn)
    }

    /// `#storeschemabatch`: on a fresh `state.db` the answer-ledger (with its
    /// derivation columns), premise and reliable-sync stores used to commit
    /// every table, index and added column separately (~22 commits, each an
    /// `fsync` under WAL). Now each store declares in one transaction, and a
    /// load of the converged stores takes no write lock and commits nothing.
    #[test]
    fn fresh_stores_declare_in_one_commit_each_and_converged_loads_never_write() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let path = dir.path().join("state.db");
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL")?;

        let ((), fresh) = count_commits(&conn, load_every_store)?;
        assert_eq!(
            fresh, 3,
            "answer ledger + derivations, premises, and reliable-sync must each declare a fresh \
             schema in a single transaction"
        );
        assert_eq!(
            DeclaredShape::read(&conn)?.tables.len(),
            7 + 2 + 2,
            "the batched declarations must still create every table"
        );

        // Converged: every load succeeds while another connection holds the
        // write lock and the loader has no busy handler, so none of them asked
        // for it, and nothing commits.
        let ((), converged) = run_without_write_lock(&path, load_every_store)?;
        assert_eq!(converged, 0);
        Ok(())
    }

    /// An older answer-ledger database (pre-`#decisiongraph` edges, no
    /// derivation status columns) converges in ONE transaction, not one per
    /// added column.
    #[test]
    fn an_older_answer_ledger_converges_in_one_commit() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = Connection::open(dir.path().join("state.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE answer_ledger_edges (
                 document_id TEXT NOT NULL, from_id TEXT NOT NULL, kind TEXT NOT NULL,
                 to_id TEXT NOT NULL, recorded_at INTEGER NOT NULL,
                 PRIMARY KEY (document_id, from_id, kind, to_id));
             CREATE TABLE answer_ledger_nodes (
                 document_id TEXT NOT NULL, node_id TEXT NOT NULL, kind TEXT NOT NULL,
                 label TEXT NOT NULL, open INTEGER, location TEXT, first_seen_at INTEGER,
                 recorded_at INTEGER NOT NULL, PRIMARY KEY (document_id, node_id));",
        )?;
        let (_, commits) = count_commits(&conn, |conn| {
            crate::answer_ledger_derivations::load_derivations(conn, "doc")
        })?;
        assert_eq!(commits, 1);
        let shape = DeclaredShape::read(&conn)?;
        let columns = |table: &str| {
            shape
                .tables
                .iter()
                .find(|(name, _)| name == table)
                .map(|(_, columns)| columns.clone())
                .unwrap_or_default()
        };
        assert!(columns("answer_ledger_edges").contains("clause"));
        assert!(columns("answer_ledger_nodes").contains("status_note"));
        Ok(())
    }

    /// The probe must not make a write-needing declaration look converged
    /// under the lock: with the write lock held elsewhere, a fresh declaration
    /// fails fast instead of silently skipping.
    #[test]
    fn an_unconverged_declaration_needs_the_write_lock() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let path = dir.path().join("store.db");
        Connection::open(&path)?.execute_batch("PRAGMA journal_mode = WAL")?;
        let error = run_without_write_lock(&path, |conn| {
            declare_store_schema(conn, &SHAPE, declare_two_tables)
        })
        .expect_err("declaring a fresh schema must ask for the write lock");
        assert!(format!("{error:#}").contains("locked"), "{error:#}");
        Ok(())
    }
}
