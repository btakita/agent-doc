//! # Module: op_log
//!
//! Durable SQLite op-log writer — phase 1 of the operation-scoped drift model
//! (`#op-scoped-drift-1`, `tasks/agent-doc/plan-operation-scoped-drift.md`).
//!
//! ## Spec
//! - Persists node-keyed document operations (`agent_doc_turn::op_log::DocumentOp`)
//!   tagged with actor + causal (Lamport / session-origin) clock to derived
//!   tables in the controller's sole `.agent-doc/state.db`.
//! - The durable store owns Lamport assignment: each appended op gets the next
//!   monotonic per-document tick, so the log is totally ordered per document.
//!   The caller's `clock.lamport` is ignored; `clock.origin_session` is honored.
//! - Append is idempotent against the most recent op for the same node: a
//!   repeated preflight pass over the same uncommitted diff does not duplicate
//!   rows or advance the clock.
//! - The DB is rebuildable derived state; like the archive index it is best
//!   effort and may be dropped without losing canonical document history.
//!
//! ## Agentic Contracts
//! - The DB lives under the project root that owns `.agent-doc`.
//! - `append_ops` opens (creating if needed) the DB, ensures the schema, and
//!   writes the batch in a single transaction.
//! - `read_ops` returns rows for a document in Lamport order for inspection and
//!   for the phase-2/3 readers.
//!
//! ## Evals
//! - append_assigns_monotonic_lamport: sequential appends get increasing ticks
//! - append_is_idempotent_for_repeated_op: re-appending the same op is a no-op
//! - lamport_is_per_document: independent documents keep independent clocks
//! - read_ops_returns_lamport_order: rows come back ordered by Lamport tick

use agent_doc_turn::op_log::{CausalClock, DocumentOp, OpActor};
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Path to the sole controller state database containing the op-log tables.
pub fn db_path(project_root: &Path) -> PathBuf {
    crate::state_store::state_db_path(project_root)
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS op_log (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             document_path TEXT NOT NULL,
             component TEXT NOT NULL,
             node_key TEXT NOT NULL,
             item_id TEXT NOT NULL DEFAULT '',
             op_kind TEXT NOT NULL,
             actor TEXT NOT NULL,
             lamport INTEGER NOT NULL,
             origin_session TEXT,
             before_preview TEXT,
             after_preview TEXT,
             recorded_at TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_op_log_document_lamport
             ON op_log(document_path, lamport);
         CREATE INDEX IF NOT EXISTS idx_op_log_node
             ON op_log(document_path, node_key);",
    )?;
    Ok(())
}

/// Highest Lamport tick recorded for a document, or 0 when the log is empty.
fn max_lamport(conn: &Connection, document_path: &str) -> Result<u64> {
    let value: Option<i64> = conn.query_row(
        "SELECT MAX(lamport) FROM op_log WHERE document_path = ?1",
        params![document_path],
        |row| row.get(0),
    )?;
    Ok(value.unwrap_or(0).max(0) as u64)
}

/// The latest persisted op for a node, used to suppress duplicate re-appends.
fn latest_op_for_node(
    conn: &Connection,
    document_path: &str,
    node_key: &str,
) -> Result<Option<DocumentOp>> {
    let mut stmt = conn.prepare(
        "SELECT document_path, component, node_key, item_id, op_kind, actor, lamport,
                origin_session, before_preview, after_preview, recorded_at
         FROM op_log
         WHERE document_path = ?1 AND node_key = ?2
         ORDER BY lamport DESC, id DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![document_path, node_key])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_op(row)?)),
        None => Ok(None),
    }
}

fn row_to_op(row: &rusqlite::Row<'_>) -> Result<DocumentOp> {
    let actor_str: String = row.get(5)?;
    let actor = OpActor::from_str_lenient(&actor_str).unwrap_or(OpActor::User);
    let lamport: i64 = row.get(6)?;
    Ok(DocumentOp {
        document_path: row.get(0)?,
        component: row.get(1)?,
        node_key: row.get(2)?,
        // The durable store does not persist the within-component node index;
        // it is only needed by the live preflight affectedness classifier, never
        // by a replayed op (`#loop-guard-exchange-node-granularity`).
        node_index: None,
        item_id: row.get(3)?,
        op_kind: row.get(4)?,
        actor,
        clock: CausalClock {
            lamport: lamport.max(0) as u64,
            origin_session: row.get(7)?,
        },
        before_preview: row.get(8)?,
        after_preview: row.get(9)?,
        recorded_at: row.get(10)?,
    })
}

/// Append a batch of ops, assigning a monotonic per-document Lamport tick to
/// each new op. Returns the number of ops actually written (deduped ops are
/// skipped). Best-effort: callers should log and continue on error rather than
/// failing the cycle.
pub fn append_ops(project_root: &Path, ops: &[DocumentOp]) -> Result<usize> {
    if ops.is_empty() {
        return Ok(0);
    }
    let path = db_path(project_root);
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut conn =
        Connection::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    ensure_schema(&conn)?;

    let tx = conn.transaction()?;
    // Lazily seed the next Lamport tick per document from the persisted max so a
    // batch spanning multiple documents keeps independent clocks.
    let mut next_lamport: HashMap<String, u64> = HashMap::new();
    let mut written = 0usize;
    for op in ops {
        if let Some(previous) = latest_op_for_node(&tx, &op.document_path, &op.node_key)?
            && previous.same_mutation(op)
        {
            // Repeated observation of the same node mutation — keep the log
            // idempotent and do not advance the clock.
            continue;
        }
        let base = match next_lamport.get(&op.document_path) {
            Some(value) => *value,
            None => max_lamport(&tx, &op.document_path)?,
        };
        let lamport = base + 1;
        next_lamport.insert(op.document_path.clone(), lamport);

        let recorded_at = op
            .recorded_at
            .clone()
            .unwrap_or_else(|| agent_doc_log_time::current_epoch_secs().to_string());
        tx.execute(
            "INSERT INTO op_log (
                 document_path, component, node_key, item_id, op_kind, actor,
                 lamport, origin_session, before_preview, after_preview, recorded_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                op.document_path,
                op.component,
                op.node_key,
                op.item_id,
                op.op_kind,
                op.actor.as_str(),
                lamport as i64,
                op.clock.origin_session,
                op.before_preview,
                op.after_preview,
                recorded_at,
            ],
        )?;
        written += 1;
    }
    tx.commit()?;
    Ok(written)
}

/// Build and append durable document ops from a semantic diff summary.
///
/// The sqlite op-log owns the timestamp used for durable persistence so callers
/// do not reimplement low-level clock formatting at orchestration boundaries.
pub fn append_semantic_diff_ops(
    project_root: &Path,
    document_path: &str,
    origin_session: Option<&str>,
    summary: &agent_doc_diff::semantic::SemanticDiffSummary,
) -> Result<usize> {
    if summary.node_events.is_empty() {
        return Ok(0);
    }
    let recorded_at = agent_doc_log_time::current_epoch_secs().to_string();
    let ops = agent_doc_turn::op_log::build_ops_from_semantic_diff(
        document_path,
        origin_session,
        &recorded_at,
        summary,
    );
    append_ops(project_root, &ops)
}

/// Read persisted ops for a document in Lamport order (most recent last),
/// capped at `limit` rows (0 = unlimited).
pub fn read_ops(project_root: &Path, document_path: &str, limit: usize) -> Result<Vec<DocumentOp>> {
    let path = db_path(project_root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn =
        Connection::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    ensure_schema(&conn)?;
    let sql = "SELECT document_path, component, node_key, item_id, op_kind, actor, lamport,
                      origin_session, before_preview, after_preview, recorded_at
               FROM op_log
               WHERE document_path = ?1
               ORDER BY lamport ASC, id ASC"
        .to_string();
    let sql = if limit > 0 {
        format!("{sql} LIMIT {limit}")
    } else {
        sql
    };
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![document_path])?;
    let mut ops = Vec::new();
    while let Some(row) = rows.next()? {
        ops.push(row_to_op(row)?);
    }
    Ok(ops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn project_with_agent_doc() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        dir
    }

    fn op(doc: &str, node_key: &str, op_kind: &str, after: &str) -> DocumentOp {
        DocumentOp {
            document_path: doc.to_string(),
            component: "queue".to_string(),
            node_key: node_key.to_string(),
            node_index: None,
            item_id: node_key.to_string(),
            op_kind: op_kind.to_string(),
            actor: OpActor::User,
            clock: CausalClock {
                lamport: 0,
                origin_session: Some("sess-1".to_string()),
            },
            before_preview: None,
            after_preview: Some(after.to_string()),
            recorded_at: Some("100".to_string()),
        }
    }

    #[test]
    fn append_assigns_monotonic_lamport() {
        let dir = project_with_agent_doc();
        let written = append_ops(
            dir.path(),
            &[
                op("plan.md", "queue:0:a:0", "insert", "- do [#a]"),
                op("plan.md", "queue:0:b:0", "insert", "- do [#b]"),
            ],
        )
        .unwrap();
        assert_eq!(written, 2);
        let ops = read_ops(dir.path(), "plan.md", 0).unwrap();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].clock.lamport, 1);
        assert_eq!(ops[1].clock.lamport, 2);
        assert_eq!(ops[0].clock.origin_session.as_deref(), Some("sess-1"));

        // A later append keeps climbing from the persisted max.
        let written = append_ops(
            dir.path(),
            &[op("plan.md", "queue:0:c:0", "insert", "- do [#c]")],
        )
        .unwrap();
        assert_eq!(written, 1);
        let ops = read_ops(dir.path(), "plan.md", 0).unwrap();
        assert_eq!(ops.len(), 3);
        assert_eq!(ops[2].clock.lamport, 3);
    }

    #[test]
    fn append_is_idempotent_for_repeated_op() {
        let dir = project_with_agent_doc();
        let same = op("plan.md", "queue:0:a:0", "insert", "- do [#a]");
        assert_eq!(
            append_ops(dir.path(), std::slice::from_ref(&same)).unwrap(),
            1
        );
        // Re-observe the identical node mutation: no new row, clock unchanged.
        assert_eq!(
            append_ops(dir.path(), std::slice::from_ref(&same)).unwrap(),
            0
        );
        let ops = read_ops(dir.path(), "plan.md", 0).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].clock.lamport, 1);

        // A genuinely different mutation on the same node is still recorded.
        let edited = op("plan.md", "queue:0:a:0", "replace", "- do [#a] edited");
        assert_eq!(append_ops(dir.path(), &[edited]).unwrap(), 1);
        let ops = read_ops(dir.path(), "plan.md", 0).unwrap();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[1].clock.lamport, 2);
    }

    #[test]
    fn lamport_is_per_document() {
        let dir = project_with_agent_doc();
        append_ops(
            dir.path(),
            &[
                op("a.md", "queue:0:x:0", "insert", "- do [#x]"),
                op("b.md", "queue:0:y:0", "insert", "- do [#y]"),
                op("a.md", "queue:0:z:0", "insert", "- do [#z]"),
            ],
        )
        .unwrap();
        let a = read_ops(dir.path(), "a.md", 0).unwrap();
        let b = read_ops(dir.path(), "b.md", 0).unwrap();
        assert_eq!(
            a.iter().map(|o| o.clock.lamport).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            b.iter().map(|o| o.clock.lamport).collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn read_ops_returns_empty_without_db() {
        let dir = project_with_agent_doc();
        assert!(read_ops(dir.path(), "missing.md", 0).unwrap().is_empty());
    }

    #[test]
    fn append_semantic_diff_ops_builds_and_persists_user_ops() {
        let dir = project_with_agent_doc();
        let summary = agent_doc_diff::semantic::semantic_diff_summary(
            "<!-- agent:queue -->\n<!-- /agent:queue -->\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
            &[],
        )
        .unwrap();

        let written =
            append_semantic_diff_ops(dir.path(), "plan.md", Some("sess-1"), &summary).unwrap();

        assert_eq!(written, 1);
        let ops = read_ops(dir.path(), "plan.md", 0).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].clock.origin_session.as_deref(), Some("sess-1"));
        assert!(ops[0].recorded_at.is_some());
    }
}
