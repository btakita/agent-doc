//! Storage for the steering completion-gate decision log (`#steergatelog`).
//!
//! The table is declared with the canonical schema in
//! [`crate::state_store`]. This module is the write-only sink and the cold
//! reads around it: the live decision state (rows, outcome facts, labels)
//! is folded in a Lazily graph (`agent_doc_session_check_io::steering_gate_log`),
//! whose `Effect` calls [`merge_gate_row`]. The merge is monotone, so two
//! processes recording the same decision converge on the same row.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

/// Retained rows per project. Older decisions are dropped first.
pub const STEERING_GATE_LOG_MAX_ROWS: i64 = 10_000;

/// One row as written or read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGateRow {
    /// SQLite row id (`None` before the first write).
    pub id: Option<i64>,
    pub row_key: String,
    pub document: String,
    pub consumer: String,
    pub phase: String,
    pub decided_at_ms: u64,
    pub row_json: String,
    pub delivered_at_ms: Option<u64>,
    pub superseded_at_ms: Option<u64>,
    pub re_edited_at_ms: Option<u64>,
    pub label: Option<String>,
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn opt_i64(value: Option<u64>) -> Option<i64> {
    value.map(to_i64)
}

/// Insert a decision row or merge its outcome facts into the stored one.
///
/// The decision itself (`row_json`, `phase`, `decided_at_ms`) is written
/// once. Facts and the label are first-write-wins (`COALESCE`), which makes
/// the merge commutative across processes. Returns the row id.
pub fn merge_gate_row(conn: &Connection, row: &StoredGateRow) -> Result<i64> {
    conn.query_row(
        "INSERT INTO steering_gate_log \
           (row_key, document, consumer, phase, decided_at_ms, row_json, \
            delivered_at_ms, superseded_at_ms, re_edited_at_ms, label) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
         ON CONFLICT(row_key) DO UPDATE SET \
           delivered_at_ms = COALESCE(steering_gate_log.delivered_at_ms, excluded.delivered_at_ms), \
           superseded_at_ms = COALESCE(steering_gate_log.superseded_at_ms, excluded.superseded_at_ms), \
           re_edited_at_ms = COALESCE(steering_gate_log.re_edited_at_ms, excluded.re_edited_at_ms), \
           label = COALESCE(steering_gate_log.label, excluded.label) \
         RETURNING id",
        params![
            row.row_key,
            row.document,
            row.consumer,
            row.phase,
            to_i64(row.decided_at_ms),
            row.row_json,
            opt_i64(row.delivered_at_ms),
            opt_i64(row.superseded_at_ms),
            opt_i64(row.re_edited_at_ms),
            row.label,
        ],
        |r| r.get(0),
    )
    .context("merge steering gate log row")
}

/// Persist learned gate weights (`#steergateperceptron`) unless the stored
/// copy has already absorbed at least as many updates: a long-lived process
/// holding older weights can never roll back a newer model.
pub fn upsert_gate_model(
    conn: &Connection,
    state_key: &str,
    payload: &str,
    updates: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO project_runtime_state (state_key, payload, updated_at_ms) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT(state_key) DO UPDATE SET \
           payload = excluded.payload, updated_at_ms = excluded.updated_at_ms \
         WHERE COALESCE(json_extract(project_runtime_state.payload, '$.updates'), -1) \
               <= ?3",
        params![state_key, payload, to_i64(updates)],
    )
    .context("upsert steering gate model")?;
    Ok(())
}

/// Keep at most `max_rows` rows, dropping the oldest.
pub fn prune_gate_rows(conn: &Connection, max_rows: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM steering_gate_log WHERE id <= \
           (SELECT MAX(id) FROM steering_gate_log) - ?1",
        params![max_rows],
    )?)
}

fn read_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredGateRow> {
    let u = |v: Option<i64>| v.and_then(|v| u64::try_from(v).ok());
    Ok(StoredGateRow {
        id: Some(r.get(0)?),
        row_key: r.get(1)?,
        document: r.get(2)?,
        consumer: r.get(3)?,
        phase: r.get(4)?,
        decided_at_ms: u(Some(r.get(5)?)).unwrap_or(0),
        row_json: r.get(6)?,
        delivered_at_ms: u(r.get(7)?),
        superseded_at_ms: u(r.get(8)?),
        re_edited_at_ms: u(r.get(9)?),
        label: r.get(10)?,
    })
}

const COLUMNS: &str = "id, row_key, document, consumer, phase, decided_at_ms, row_json, \
                       delivered_at_ms, superseded_at_ms, re_edited_at_ms, label";

/// Rows for `document` decided at or after `since_ms`, oldest first (cold
/// hydration of the live decision graph).
pub fn load_gate_rows_since(
    conn: &Connection,
    document: &str,
    since_ms: u64,
) -> Result<Vec<StoredGateRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM steering_gate_log \
         WHERE document = ?1 AND decided_at_ms >= ?2 ORDER BY id"
    ))?;
    let rows = stmt
        .query_map(params![document, to_i64(since_ms)], read_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Rows for `document` that still have no label, the newest `limit` of them,
/// oldest first. Cold hydration loads them whatever their age: a delivery is
/// labelled by the NEXT observation of its document, and when no operator
/// edit arrives mid-turn that is the closeout write, long after the recent
/// horizon. Without them the row is never labelled and never trains the gate.
pub fn load_unlabelled_gate_rows(
    conn: &Connection,
    document: &str,
    limit: usize,
) -> Result<Vec<StoredGateRow>> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM (SELECT {COLUMNS} FROM steering_gate_log \
         WHERE document = ?1 AND label IS NULL ORDER BY id DESC LIMIT ?2) ORDER BY id"
    ))?;
    let rows = stmt
        .query_map(params![document, limit], read_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Rows for export, oldest first. `document` filters to one document;
/// `after_id` continues a previous page.
pub fn list_gate_rows(
    conn: &Connection,
    document: Option<&str>,
    after_id: i64,
    limit: usize,
) -> Result<Vec<StoredGateRow>> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM steering_gate_log \
         WHERE (?1 IS NULL OR document = ?1) AND id > ?2 ORDER BY id LIMIT ?3"
    ))?;
    let rows = stmt
        .query_map(params![document, after_id, limit], read_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The most recent row (optionally for one document).
pub fn latest_gate_row(conn: &Connection, document: Option<&str>) -> Result<Option<StoredGateRow>> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM steering_gate_log \
             WHERE (?1 IS NULL OR document = ?1) ORDER BY id DESC LIMIT 1"
        ),
        params![document],
        read_row,
    )
    .optional()
    .context("load latest steering gate log row")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, at: u64) -> StoredGateRow {
        StoredGateRow {
            id: None,
            row_key: key.to_string(),
            document: "plan.md".to_string(),
            consumer: "hook".to_string(),
            phase: "delivered".to_string(),
            decided_at_ms: at,
            row_json: "{}".to_string(),
            delivered_at_ms: Some(at),
            superseded_at_ms: None,
            re_edited_at_ms: None,
            label: None,
        }
    }

    #[test]
    fn gate_rows_merge_monotonically_and_prune_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::state_store::open_state_db(dir.path()).unwrap();
        let first = merge_gate_row(&conn, &row("a", 10)).unwrap();
        // A later process observed the re-edit and the label.
        let mut update = row("a", 99);
        update.row_json = "{\"rewritten\":true}".to_string();
        update.re_edited_at_ms = Some(20);
        update.label = Some("premature".to_string());
        assert_eq!(merge_gate_row(&conn, &update).unwrap(), first);
        // A stale writer cannot clear or replace a fact.
        let mut stale = row("a", 10);
        stale.re_edited_at_ms = Some(500);
        stale.label = Some("on_time".to_string());
        merge_gate_row(&conn, &stale).unwrap();
        let stored = latest_gate_row(&conn, Some("plan.md")).unwrap().unwrap();
        assert_eq!(stored.row_json, "{}", "the decision is written once");
        assert_eq!(stored.decided_at_ms, 10);
        assert_eq!(stored.re_edited_at_ms, Some(20));
        assert_eq!(stored.label.as_deref(), Some("premature"));

        for i in 0..5 {
            merge_gate_row(&conn, &row(&format!("k{i}"), 100 + i)).unwrap();
        }
        assert_eq!(prune_gate_rows(&conn, 3).unwrap(), 3);
        let kept = list_gate_rows(&conn, None, 0, 100).unwrap();
        assert_eq!(
            kept.iter().map(|r| r.row_key.as_str()).collect::<Vec<_>>(),
            vec!["k2", "k3", "k4"]
        );
        assert_eq!(
            load_gate_rows_since(&conn, "plan.md", 104).unwrap().len(),
            1
        );
        assert!(
            list_gate_rows(&conn, Some("other.md"), 0, 10)
                .unwrap()
                .is_empty()
        );
    }

    /// An old unlabelled row hydrates whatever its age; labelled rows do not.
    #[test]
    fn unlabelled_rows_load_regardless_of_age_newest_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::state_store::open_state_db(dir.path()).unwrap();
        merge_gate_row(&conn, &row("old", 10)).unwrap();
        let mut labelled = row("done", 20);
        labelled.label = Some("on_time".to_string());
        merge_gate_row(&conn, &labelled).unwrap();
        merge_gate_row(&conn, &row("newer", 30)).unwrap();
        let keys =
            |rows: Vec<StoredGateRow>| rows.into_iter().map(|r| r.row_key).collect::<Vec<_>>();
        assert!(
            load_gate_rows_since(&conn, "plan.md", 1_000)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            keys(load_unlabelled_gate_rows(&conn, "plan.md", 10).unwrap()),
            vec!["old", "newer"]
        );
        assert_eq!(
            keys(load_unlabelled_gate_rows(&conn, "plan.md", 1).unwrap()),
            vec!["newer"]
        );
    }
}
