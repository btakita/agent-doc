//! Per-document operator answer-ledger index (`#answerbyrequestid`).
//!
//! The session document and its done archives are the authority for operator
//! answers; this index is a rebuildable cache in the controller `state.db`.
//! It keeps what Markdown alone cannot: when each answer and question key was
//! first observed, the stable decision id every key belongs to (one key maps
//! to exactly one decision, enforced by the primary key), answers whose source
//! rows were later pruned, and typed decision-graph edges.

use anyhow::Result;
use rusqlite::{Connection, params};

/// One `key -> decision` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerKeyRow {
    pub key: String,
    pub decision_id: String,
    pub first_seen_at: u64,
}

/// One remembered answer revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerRevisionRow {
    pub decision_id: String,
    pub answer: String,
    pub location: String,
    pub relation: String,
    pub first_seen_at: u64,
    pub change_flagged: bool,
}

/// One typed decision-graph edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerEdgeRow {
    pub from: String,
    pub kind: String,
    pub to: String,
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS answer_ledger_keys (
            document_id TEXT NOT NULL,
            key TEXT NOT NULL,
            decision_id TEXT NOT NULL,
            first_seen_at INTEGER NOT NULL,
            PRIMARY KEY (document_id, key)
        );
        CREATE TABLE IF NOT EXISTS answer_ledger_revisions (
            document_id TEXT NOT NULL,
            decision_id TEXT NOT NULL,
            answer TEXT NOT NULL,
            location TEXT NOT NULL,
            relation TEXT NOT NULL,
            first_seen_at INTEGER NOT NULL,
            change_flagged INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (document_id, decision_id, answer)
        );
        CREATE TABLE IF NOT EXISTS answer_ledger_edges (
            document_id TEXT NOT NULL,
            from_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            to_id TEXT NOT NULL,
            recorded_at INTEGER NOT NULL,
            PRIMARY KEY (document_id, from_id, kind, to_id)
        );
        CREATE TABLE IF NOT EXISTS answer_ledger_history_seeds (
            document_id TEXT NOT NULL PRIMARY KEY,
            seeded_at INTEGER NOT NULL,
            versions_scanned INTEGER NOT NULL,
            answers_recovered INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}

fn sql_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn sql_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Load every key and revision row for one document.
pub fn load(
    conn: &Connection,
    document_id: &str,
) -> Result<(Vec<AnswerKeyRow>, Vec<AnswerRevisionRow>)> {
    ensure_schema(conn)?;
    let mut keys = Vec::new();
    let mut statement = conn.prepare(
        "SELECT key, decision_id, first_seen_at FROM answer_ledger_keys WHERE document_id = ?1 ORDER BY key",
    )?;
    let rows = statement.query_map(params![document_id], |row| {
        Ok(AnswerKeyRow {
            key: row.get(0)?,
            decision_id: row.get(1)?,
            first_seen_at: sql_u64(row.get(2)?),
        })
    })?;
    for row in rows {
        keys.push(row?);
    }
    let mut revisions = Vec::new();
    let mut statement = conn.prepare(
        "SELECT decision_id, answer, location, relation, first_seen_at, change_flagged
         FROM answer_ledger_revisions WHERE document_id = ?1 ORDER BY first_seen_at, rowid",
    )?;
    let rows = statement.query_map(params![document_id], |row| {
        Ok(AnswerRevisionRow {
            decision_id: row.get(0)?,
            answer: row.get(1)?,
            location: row.get(2)?,
            relation: row.get(3)?,
            first_seen_at: sql_u64(row.get(4)?),
            change_flagged: row.get::<_, i64>(5)? != 0,
        })
    })?;
    for row in rows {
        revisions.push(row?);
    }
    Ok((keys, revisions))
}

/// Upsert the rows one ledger build implies. A key keeps its first-seen time
/// and is repointed when decisions merge; a revision keeps its first-seen time
/// and only ever gains its change flag. Nothing is deleted: the index
/// remembers answers the document no longer shows.
pub fn store(
    conn: &mut Connection,
    document_id: &str,
    keys: &[AnswerKeyRow],
    revisions: &[AnswerRevisionRow],
    edges: &[AnswerEdgeRow],
    now: u64,
) -> Result<()> {
    ensure_schema(conn)?;
    let tx = crate::immediate_transaction(conn)?;
    for key in keys {
        tx.execute(
            "INSERT INTO answer_ledger_keys (document_id, key, decision_id, first_seen_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(document_id, key) DO UPDATE SET decision_id = excluded.decision_id",
            params![
                document_id,
                key.key,
                key.decision_id,
                sql_i64(key.first_seen_at)
            ],
        )?;
    }
    for revision in revisions {
        tx.execute(
            "INSERT INTO answer_ledger_revisions
                (document_id, decision_id, answer, location, relation, first_seen_at, change_flagged)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(document_id, decision_id, answer) DO UPDATE SET
                relation = excluded.relation,
                change_flagged = MAX(change_flagged, excluded.change_flagged)",
            params![
                document_id,
                revision.decision_id,
                revision.answer,
                revision.location,
                revision.relation,
                sql_i64(revision.first_seen_at),
                i64::from(revision.change_flagged),
            ],
        )?;
    }
    for edge in edges {
        tx.execute(
            "INSERT OR IGNORE INTO answer_ledger_edges (document_id, from_id, kind, to_id, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![document_id, edge.from, edge.kind, edge.to, sql_i64(now)],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Whether the index for `document_id` was already rebuilt from the
/// document's git history (`#decisionidstamp`).
pub fn history_seeded(conn: &Connection, document_id: &str) -> Result<bool> {
    ensure_schema(conn)?;
    Ok(conn
        .query_row(
            "SELECT 1 FROM answer_ledger_history_seeds WHERE document_id = ?1",
            params![document_id],
            |_| Ok(()),
        )
        .map(|()| true)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(false),
            other => Err(other),
        })?)
}

/// Store the rows a git-history rebuild produced and record that the rebuild
/// ran, in one transaction (`#decisionidstamp`). History only ever moves a
/// first-seen time earlier; it never repoints a key the index already holds,
/// and never clears a change flag.
#[allow(clippy::too_many_arguments)]
pub fn store_history_seed(
    conn: &mut Connection,
    document_id: &str,
    keys: &[AnswerKeyRow],
    revisions: &[AnswerRevisionRow],
    edges: &[AnswerEdgeRow],
    versions_scanned: usize,
    answers_recovered: usize,
    now: u64,
) -> Result<()> {
    ensure_schema(conn)?;
    let tx = crate::immediate_transaction(conn)?;
    for key in keys {
        tx.execute(
            "INSERT INTO answer_ledger_keys (document_id, key, decision_id, first_seen_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(document_id, key) DO UPDATE SET
                first_seen_at = MIN(first_seen_at, excluded.first_seen_at)",
            params![
                document_id,
                key.key,
                key.decision_id,
                sql_i64(key.first_seen_at)
            ],
        )?;
    }
    for revision in revisions {
        tx.execute(
            "INSERT INTO answer_ledger_revisions
                (document_id, decision_id, answer, location, relation, first_seen_at, change_flagged)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(document_id, decision_id, answer) DO UPDATE SET
                first_seen_at = MIN(first_seen_at, excluded.first_seen_at),
                change_flagged = MAX(change_flagged, excluded.change_flagged)",
            params![
                document_id,
                revision.decision_id,
                revision.answer,
                revision.location,
                revision.relation,
                sql_i64(revision.first_seen_at),
                i64::from(revision.change_flagged),
            ],
        )?;
    }
    for edge in edges {
        tx.execute(
            "INSERT OR IGNORE INTO answer_ledger_edges (document_id, from_id, kind, to_id, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![document_id, edge.from, edge.kind, edge.to, sql_i64(now)],
        )?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO answer_ledger_history_seeds
            (document_id, seeded_at, versions_scanned, answers_recovered)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            document_id,
            sql_i64(now),
            i64::try_from(versions_scanned).unwrap_or(i64::MAX),
            i64::try_from(answers_recovered).unwrap_or(i64::MAX),
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// [`history_seeded`] in the project's `state.db`; `None` where the state
/// database may not be opened (no rebuild runs there).
pub fn history_seeded_for_project(
    project_root: &std::path::Path,
    document_id: &str,
) -> Option<Result<bool>> {
    if crate::state_store::state_db_connections_forbidden_for_process() {
        return None;
    }
    let conn = crate::state_store::open_state_db(project_root).ok()?;
    Some(history_seeded(&conn, document_id))
}

/// [`store_history_seed`] into the project's `state.db`.
#[allow(clippy::too_many_arguments)]
pub fn store_history_seed_for_project(
    project_root: &std::path::Path,
    document_id: &str,
    keys: &[AnswerKeyRow],
    revisions: &[AnswerRevisionRow],
    edges: &[AnswerEdgeRow],
    versions_scanned: usize,
    answers_recovered: usize,
    now: u64,
) -> Result<()> {
    if crate::state_store::state_db_connections_forbidden_for_process() {
        return Ok(());
    }
    let mut conn = crate::state_store::open_state_db(project_root)?;
    store_history_seed(
        &mut conn,
        document_id,
        keys,
        revisions,
        edges,
        versions_scanned,
        answers_recovered,
        now,
    )
}

/// [`load`] from the project's `state.db`. `None` when this process may not
/// open the state database or the database cannot be opened.
pub fn load_for_project(
    project_root: &std::path::Path,
    document_id: &str,
) -> Option<Result<(Vec<AnswerKeyRow>, Vec<AnswerRevisionRow>)>> {
    if crate::state_store::state_db_connections_forbidden_for_process() {
        return None;
    }
    let conn = crate::state_store::open_state_db(project_root).ok()?;
    Some(load(&conn, document_id))
}

/// [`store`] into the project's `state.db`; a no-op where the state database
/// may not be opened.
pub fn store_for_project(
    project_root: &std::path::Path,
    document_id: &str,
    keys: &[AnswerKeyRow],
    revisions: &[AnswerRevisionRow],
    edges: &[AnswerEdgeRow],
    now: u64,
) -> Result<()> {
    if crate::state_store::state_db_connections_forbidden_for_process() {
        return Ok(());
    }
    let mut conn = crate::state_store::open_state_db(project_root)?;
    store(&mut conn, document_id, keys, revisions, edges, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_round_trip_and_keep_first_seen_times() {
        let mut conn = Connection::open_in_memory().unwrap();
        let key = AnswerKeyRow {
            key: "id:y2fn".into(),
            decision_id: "dec-a".into(),
            first_seen_at: 10,
        };
        let revision = AnswerRevisionRow {
            decision_id: "dec-a".into(),
            answer: "JWS makes the cap moot.".into(),
            location: "under [#y2fn]".into(),
            relation: "initial".into(),
            first_seen_at: 10,
            change_flagged: false,
        };
        let edge = AnswerEdgeRow {
            from: "dec-a@1".into(),
            kind: "answers".into(),
            to: "dec-a".into(),
        };
        store(
            &mut conn,
            "doc",
            std::slice::from_ref(&key),
            std::slice::from_ref(&revision),
            std::slice::from_ref(&edge),
            10,
        )
        .unwrap();
        // A later build repoints the key (decision merge) and flags the change,
        // but first-seen times never move.
        let moved = AnswerKeyRow {
            decision_id: "dec-b".into(),
            first_seen_at: 99,
            ..key
        };
        let flagged = AnswerRevisionRow {
            change_flagged: true,
            first_seen_at: 99,
            ..revision
        };
        store(&mut conn, "doc", &[moved], &[flagged], &[edge], 99).unwrap();
        let (keys, revisions) = load(&conn, "doc").unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].decision_id, "dec-b");
        assert_eq!(keys[0].first_seen_at, 10);
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].first_seen_at, 10);
        assert!(revisions[0].change_flagged);
        assert!(load(&conn, "other").unwrap().0.is_empty());
    }

    /// `#decisionidstamp`: a history rebuild only moves first-seen times
    /// earlier, keeps the decision a key already maps to, and runs once.
    #[test]
    fn a_history_seed_moves_first_seen_back_and_is_recorded_once() {
        let mut conn = Connection::open_in_memory().unwrap();
        let key = AnswerKeyRow {
            key: "id:tdport".into(),
            decision_id: "dec-live".into(),
            first_seen_at: 500,
        };
        let revision = AnswerRevisionRow {
            decision_id: "dec-live".into(),
            answer: "Use port 3000.".into(),
            location: "queue row".into(),
            relation: "initial".into(),
            first_seen_at: 500,
            change_flagged: false,
        };
        store(
            &mut conn,
            "doc",
            std::slice::from_ref(&key),
            std::slice::from_ref(&revision),
            &[],
            500,
        )
        .unwrap();
        assert!(!history_seeded(&conn, "doc").unwrap());
        let older_key = AnswerKeyRow {
            decision_id: "dec-history".into(),
            first_seen_at: 100,
            ..key.clone()
        };
        let older_revision = AnswerRevisionRow {
            first_seen_at: 100,
            ..revision.clone()
        };
        let pruned = AnswerRevisionRow {
            answer: "Use port 4000.".into(),
            location: "git history 1111111, queue row".into(),
            first_seen_at: 50,
            change_flagged: true,
            ..revision
        };
        store_history_seed(
            &mut conn,
            "doc",
            &[older_key],
            &[older_revision, pruned],
            &[],
            2,
            1,
            900,
        )
        .unwrap();
        assert!(history_seeded(&conn, "doc").unwrap());
        assert!(!history_seeded(&conn, "other").unwrap());
        let (keys, revisions) = load(&conn, "doc").unwrap();
        assert_eq!(
            keys[0].decision_id, "dec-live",
            "history never repoints a key"
        );
        assert_eq!(keys[0].first_seen_at, 100);
        assert_eq!(revisions.len(), 2);
        assert_eq!(revisions[0].answer, "Use port 4000.");
        assert_eq!(revisions[1].first_seen_at, 100);
    }
}
