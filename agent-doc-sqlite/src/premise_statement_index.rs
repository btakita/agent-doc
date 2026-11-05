//! Per-document premise statement digests (`#firstprinciplesrederive`).
//!
//! Each row is one principle or fact in scope for a document, with the digest
//! of its statement as of the last turn that started. Preflight compares the
//! current statements with these rows: a premise whose digest differs, or that
//! is gone, puts the conclusions derived from it into the cycle contract. The
//! Markdown files stay the authority for the statements; this table only
//! remembers what they said, so it is safe to drop (the next turn records a
//! fresh baseline and reports nothing).
//!
//! `premise_statement_baselines` records that a document's set was stored at
//! least once, so an empty set ("every premise was deleted") is told apart
//! from "never recorded".

use anyhow::Result;
use rusqlite::{Connection, params};

/// One stored premise statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PremiseStatementRow {
    /// `principle` or `fact`.
    pub kind: String,
    pub premise_id: String,
    pub digest: String,
    pub statement: String,
    pub source: String,
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS premise_statement_hashes (
            document_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            premise_id TEXT NOT NULL,
            statement_hash TEXT NOT NULL,
            statement TEXT NOT NULL,
            source TEXT NOT NULL,
            recorded_at INTEGER NOT NULL,
            PRIMARY KEY (document_id, kind, premise_id)
        );
        CREATE TABLE IF NOT EXISTS premise_statement_baselines (
            document_id TEXT NOT NULL PRIMARY KEY,
            recorded_at INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}

fn sql_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The stored statements for one document in `(kind, premise_id)` order, or
/// `None` when none were ever stored.
pub fn load(conn: &Connection, document_id: &str) -> Result<Option<Vec<PremiseStatementRow>>> {
    ensure_schema(conn)?;
    let recorded: Option<i64> = conn
        .query_row(
            "SELECT recorded_at FROM premise_statement_baselines WHERE document_id = ?1",
            params![document_id],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    if recorded.is_none() {
        return Ok(None);
    }
    let mut statement = conn.prepare(
        "SELECT kind, premise_id, statement_hash, statement, source
         FROM premise_statement_hashes WHERE document_id = ?1
         ORDER BY kind, premise_id",
    )?;
    let rows = statement
        .query_map(params![document_id], |row| {
            Ok(PremiseStatementRow {
                kind: row.get(0)?,
                premise_id: row.get(1)?,
                digest: row.get(2)?,
                statement: row.get(3)?,
                source: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(rows))
}

/// Make `rows` the stored set for one document, in one write transaction.
/// A row whose digest is unchanged keeps its `recorded_at`; rows not in
/// `rows` are deleted.
pub fn replace(
    conn: &mut Connection,
    document_id: &str,
    rows: &[PremiseStatementRow],
    now: u64,
) -> Result<()> {
    ensure_schema(conn)?;
    let tx = crate::immediate_transaction(conn)?;
    tx.execute(
        "CREATE TEMP TABLE IF NOT EXISTS premise_statement_keep (kind TEXT NOT NULL, premise_id TEXT NOT NULL)",
        [],
    )?;
    tx.execute("DELETE FROM premise_statement_keep", [])?;
    for row in rows {
        tx.execute(
            "INSERT INTO premise_statement_hashes
                (document_id, kind, premise_id, statement_hash, statement, source, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(document_id, kind, premise_id) DO UPDATE SET
                recorded_at = CASE
                    WHEN statement_hash = excluded.statement_hash THEN recorded_at
                    ELSE excluded.recorded_at END,
                statement_hash = excluded.statement_hash,
                statement = excluded.statement,
                source = excluded.source",
            params![
                document_id,
                row.kind,
                row.premise_id,
                row.digest,
                row.statement,
                row.source,
                sql_i64(now),
            ],
        )?;
        tx.execute(
            "INSERT INTO premise_statement_keep (kind, premise_id) VALUES (?1, ?2)",
            params![row.kind, row.premise_id],
        )?;
    }
    tx.execute(
        "DELETE FROM premise_statement_hashes
         WHERE document_id = ?1
           AND NOT EXISTS (
             SELECT 1 FROM premise_statement_keep keep
             WHERE keep.kind = premise_statement_hashes.kind
               AND keep.premise_id = premise_statement_hashes.premise_id)",
        params![document_id],
    )?;
    tx.execute("DELETE FROM premise_statement_keep", [])?;
    tx.execute(
        "INSERT INTO premise_statement_baselines (document_id, recorded_at) VALUES (?1, ?2)
         ON CONFLICT(document_id) DO UPDATE SET recorded_at = excluded.recorded_at",
        params![document_id, sql_i64(now)],
    )?;
    tx.commit()?;
    Ok(())
}

/// [`load`] from the project's `state.db`; `None` where the state database
/// may not or cannot be opened.
pub fn load_for_project(
    project_root: &std::path::Path,
    document_id: &str,
) -> Option<Result<Option<Vec<PremiseStatementRow>>>> {
    if crate::state_store::state_db_connections_forbidden_for_process() {
        return None;
    }
    let conn = crate::state_store::open_state_db(project_root).ok()?;
    Some(load(&conn, document_id))
}

/// [`replace`] into the project's `state.db`; a no-op where the state
/// database may not be opened.
pub fn replace_for_project(
    project_root: &std::path::Path,
    document_id: &str,
    rows: &[PremiseStatementRow],
    now: u64,
) -> Result<()> {
    if crate::state_store::state_db_connections_forbidden_for_process() {
        return Ok(());
    }
    let mut conn = crate::state_store::open_state_db(project_root)?;
    replace(&mut conn, document_id, rows, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, id: &str, digest: &str) -> PremiseStatementRow {
        PremiseStatementRow {
            kind: kind.into(),
            premise_id: id.into(),
            digest: digest.into(),
            statement: format!("{id} says {digest}"),
            source: "doc.md".into(),
        }
    }

    fn recorded_at(conn: &Connection, id: &str) -> i64 {
        conn.query_row(
            "SELECT recorded_at FROM premise_statement_hashes WHERE premise_id = ?1",
            params![id],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn a_document_never_recorded_loads_none_and_an_emptied_one_loads_empty() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(load(&conn, "doc").unwrap(), None);
        replace(&mut conn, "doc", &[row("principle", "p", "1")], 10).unwrap();
        replace(&mut conn, "doc", &[], 20).unwrap();
        assert_eq!(load(&conn, "doc").unwrap(), Some(Vec::new()));
        assert_eq!(load(&conn, "other").unwrap(), None);
    }

    #[test]
    fn replace_upserts_deletes_missing_rows_and_keeps_unchanged_times() {
        let mut conn = Connection::open_in_memory().unwrap();
        replace(
            &mut conn,
            "doc",
            &[
                row("principle", "keep", "1"),
                row("principle", "edit", "1"),
                row("fact", "gone", "1"),
            ],
            10,
        )
        .unwrap();
        replace(&mut conn, "other", &[row("fact", "gone", "9")], 10).unwrap();
        replace(
            &mut conn,
            "doc",
            &[row("principle", "keep", "1"), row("principle", "edit", "2")],
            20,
        )
        .unwrap();
        assert_eq!(
            load(&conn, "doc").unwrap(),
            Some(vec![
                row("principle", "edit", "2"),
                row("principle", "keep", "1")
            ])
        );
        assert_eq!(recorded_at(&conn, "keep"), 10);
        assert_eq!(recorded_at(&conn, "edit"), 20);
        assert_eq!(
            load(&conn, "other").unwrap(),
            Some(vec![row("fact", "gone", "9")]),
            "another document's rows are untouched"
        );
    }
}
