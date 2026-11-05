//! First-principles derivations stored in the decision-graph tables
//! (`#firstprinciplesstore`).
//!
//! A derivation is one inference step from premises (principles, facts,
//! decisions) to an answer for one decision. It lives in the same per-document
//! `answer_ledger_nodes` / `answer_ledger_edges` tables as the rest of the
//! decision graph (`#decisiongraph`); there is no parallel store:
//!
//! - a `derivation` node: `label` is the stated inference, `status` its review
//!   status (`agent_derived_unreviewed`, `operator_confirmed`, `rejected`),
//!   `status_at` / `status_note` when and why the operator reviewed it;
//! - premise nodes `principle:<id>` and `fact:<id>` (label: the statement),
//!   and decision premises as the decision's own node;
//! - `derived_from` edges from the derivation to each premise;
//! - one `concludes` edge from the derivation to the decision, whose `clause`
//!   is the concluded answer.
//!
//! Edges carry source `proposed` until the operator confirms the step, then
//! `operator`. Migration: `answer_ledger_nodes` gains `status`, `status_at`
//! and `status_note` in place ([`ensure_derivation_schema`]).

use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

use crate::answer_ledger_index::ensure_schema;

pub const NODE_KIND_DERIVATION: &str = "derivation";
pub const EDGE_DERIVED_FROM: &str = "derived_from";
pub const EDGE_CONCLUDES: &str = "concludes";
pub const STATUS_UNREVIEWED: &str = "agent_derived_unreviewed";
pub const STATUS_CONFIRMED: &str = "operator_confirmed";
pub const STATUS_REJECTED: &str = "rejected";

/// One premise of a stored derivation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivationPremiseRow {
    /// Graph node id: `principle:<id>`, `fact:<id>`, or a decision id.
    pub node_id: String,
    /// `principle`, `fact` or `decision`.
    pub kind: String,
    /// Principle or fact statement, or the decision's question.
    pub label: String,
}

/// One stored derivation step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivationRow {
    pub id: String,
    pub inference: String,
    pub status: String,
    pub premises: Vec<DerivationPremiseRow>,
    pub decision_id: String,
    /// The decision's question (used only when the decision node is new).
    pub decision_label: String,
    pub answer: String,
    pub first_seen_at: Option<u64>,
    pub status_at: Option<u64>,
    pub status_note: Option<String>,
}

impl DerivationRow {
    /// The same inference step: premises, inference, decision and answer.
    pub fn same_step(&self, other: &Self) -> bool {
        fn premises(row: &DerivationRow) -> Vec<&str> {
            let mut ids: Vec<&str> = row.premises.iter().map(|p| p.node_id.as_str()).collect();
            ids.sort_unstable();
            ids
        }
        self.inference == other.inference
            && self.decision_id == other.decision_id
            && self.answer == other.answer
            && premises(self) == premises(other)
    }
}

/// Whether [`store_derivation`] created the step or found it on record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreOutcome {
    Created,
    /// Already stored; the stored row (with its review status) is returned.
    Existing(Box<DerivationRow>),
}

/// The `#firstprinciplesstore` migration: review-status columns on
/// `answer_ledger_nodes`, added in place to older databases.
pub fn ensure_derivation_schema(conn: &Connection) -> Result<()> {
    ensure_schema(conn)?;
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(answer_ledger_nodes)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<_>>()?;
    for (column, ddl) in [
        (
            "status",
            "ALTER TABLE answer_ledger_nodes ADD COLUMN status TEXT",
        ),
        (
            "status_at",
            "ALTER TABLE answer_ledger_nodes ADD COLUMN status_at INTEGER",
        ),
        (
            "status_note",
            "ALTER TABLE answer_ledger_nodes ADD COLUMN status_note TEXT",
        ),
    ] {
        if !columns.iter().any(|existing| existing == column) {
            conn.execute_batch(ddl)?;
        }
    }
    Ok(())
}

fn sql_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn sql_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn edge_source(status: &str) -> &'static str {
    if status == STATUS_CONFIRMED {
        "operator"
    } else {
        "proposed"
    }
}

fn load_one(conn: &Connection, document_id: &str, id: &str) -> Result<Option<DerivationRow>> {
    let head = conn
        .query_row(
            "SELECT kind, label, status, first_seen_at, status_at, status_note
             FROM answer_ledger_nodes WHERE document_id = ?1 AND node_id = ?2",
            params![document_id, id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((kind, inference, status, first_seen_at, status_at, status_note)) = head else {
        return Ok(None);
    };
    if kind != NODE_KIND_DERIVATION {
        bail!("{id} is a {kind} node, not a derivation");
    }
    let mut premises = Vec::new();
    let mut conclusion: Option<(String, String, String)> = None;
    let mut statement = conn.prepare(
        "SELECT e.kind, e.to_id, e.clause, COALESCE(n.kind, ''), COALESCE(n.label, '')
         FROM answer_ledger_edges e
         LEFT JOIN answer_ledger_nodes n ON n.document_id = e.document_id AND n.node_id = e.to_id
         WHERE e.document_id = ?1 AND e.from_id = ?2 AND e.kind IN ('derived_from', 'concludes')
         ORDER BY e.rowid",
    )?;
    let rows = statement.query_map(params![document_id, id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (edge_kind, to, clause, node_kind, label) = row?;
        if edge_kind == EDGE_CONCLUDES {
            conclusion = Some((to, label, clause.unwrap_or_default()));
        } else {
            premises.push(DerivationPremiseRow {
                node_id: to,
                kind: node_kind,
                label,
            });
        }
    }
    let (decision_id, decision_label, answer) = conclusion.unwrap_or_default();
    Ok(Some(DerivationRow {
        id: id.to_string(),
        inference,
        status: status.unwrap_or_else(|| STATUS_UNREVIEWED.to_string()),
        premises,
        decision_id,
        decision_label,
        answer,
        first_seen_at: first_seen_at.map(sql_u64),
        status_at: status_at.map(sql_u64),
        status_note,
    }))
}

/// One stored derivation, or `None`.
pub fn load_derivation(
    conn: &Connection,
    document_id: &str,
    id: &str,
) -> Result<Option<DerivationRow>> {
    ensure_derivation_schema(conn)?;
    load_one(conn, document_id, id)
}

/// Every stored derivation for one document, in id order.
pub fn load_derivations(conn: &Connection, document_id: &str) -> Result<Vec<DerivationRow>> {
    ensure_derivation_schema(conn)?;
    let ids: Vec<String> = conn
        .prepare(
            "SELECT node_id FROM answer_ledger_nodes
             WHERE document_id = ?1 AND kind = 'derivation' ORDER BY node_id",
        )?
        .query_map(params![document_id], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::new();
    for id in ids {
        if let Some(row) = load_one(conn, document_id, &id)? {
            out.push(row);
        }
    }
    Ok(out)
}

/// Store one derivation step: its node, its premise and decision nodes (an
/// existing node keeps its kind; a principle or fact label follows the latest
/// statement), `derived_from` edges and the `concludes` edge. A step already
/// on record with the same content is left as it is, review status included;
/// a different step under the same id is refused.
pub fn store_derivation(
    conn: &mut Connection,
    document_id: &str,
    row: &DerivationRow,
    now: u64,
) -> Result<StoreOutcome> {
    if row.premises.is_empty() {
        bail!("derivation {} has no premises", row.id);
    }
    if row.inference.trim().is_empty() {
        bail!("derivation {} states no inference step", row.id);
    }
    ensure_derivation_schema(conn)?;
    let tx = crate::immediate_transaction(conn)?;
    if let Some(existing) = load_one(&tx, document_id, &row.id)? {
        if !existing.same_step(row) {
            bail!(
                "derivation {} already holds a different step; give the new one another id",
                row.id
            );
        }
        tx.commit()?;
        return Ok(StoreOutcome::Existing(Box::new(existing)));
    }
    tx.execute(
        "INSERT INTO answer_ledger_nodes
            (document_id, node_id, kind, label, open, location, first_seen_at, recorded_at,
             status, status_at, status_note)
         VALUES (?1, ?2, 'derivation', ?3, NULL, NULL, ?4, ?4, ?5, NULL, NULL)",
        params![document_id, row.id, row.inference, sql_i64(now), row.status],
    )?;
    let premise_nodes = row
        .premises
        .iter()
        .map(|premise| (&premise.node_id, &premise.kind, &premise.label))
        .chain(std::iter::once((
            &row.decision_id,
            &"decision".to_string(),
            &row.decision_label,
        )))
        .map(|(id, kind, label)| (id.clone(), kind.clone(), label.clone()))
        .collect::<Vec<_>>();
    for (id, kind, label) in &premise_nodes {
        tx.execute(
            "INSERT INTO answer_ledger_nodes
                (document_id, node_id, kind, label, open, location, first_seen_at, recorded_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?5)
             ON CONFLICT(document_id, node_id) DO UPDATE SET
                label = CASE
                    WHEN excluded.label = '' OR kind <> excluded.kind
                         OR kind NOT IN ('principle', 'fact') THEN label
                    ELSE excluded.label END",
            params![document_id, id, kind, label, sql_i64(now)],
        )?;
    }
    let source = edge_source(&row.status);
    for premise in &row.premises {
        tx.execute(
            "INSERT INTO answer_ledger_edges
                (document_id, from_id, kind, to_id, recorded_at, source, clause)
             VALUES (?1, ?2, 'derived_from', ?3, ?4, ?5, NULL)
             ON CONFLICT(document_id, from_id, kind, to_id) DO UPDATE SET source = excluded.source",
            params![document_id, row.id, premise.node_id, sql_i64(now), source],
        )?;
    }
    tx.execute(
        "INSERT INTO answer_ledger_edges
            (document_id, from_id, kind, to_id, recorded_at, source, clause)
         VALUES (?1, ?2, 'concludes', ?3, ?4, ?5, ?6)
         ON CONFLICT(document_id, from_id, kind, to_id) DO UPDATE SET
            source = excluded.source, clause = excluded.clause",
        params![
            document_id,
            row.id,
            row.decision_id,
            sql_i64(now),
            source,
            row.answer
        ],
    )?;
    tx.commit()?;
    Ok(StoreOutcome::Created)
}

/// Record the operator's review of one stored step. Returns `Ok(false)` when
/// the status and note already match (the edges' source is resynchronised
/// either way) and an error when no such derivation is stored.
pub fn set_derivation_status(
    conn: &mut Connection,
    document_id: &str,
    id: &str,
    status: &str,
    note: Option<&str>,
    now: u64,
) -> Result<bool> {
    if ![STATUS_UNREVIEWED, STATUS_CONFIRMED, STATUS_REJECTED].contains(&status) {
        bail!("unknown derivation status {status}");
    }
    ensure_derivation_schema(conn)?;
    let tx = crate::immediate_transaction(conn)?;
    let Some(existing) = load_one(&tx, document_id, id)? else {
        bail!("no derivation {id} is stored for this document");
    };
    let changed = existing.status != status || existing.status_note.as_deref() != note;
    if changed {
        tx.execute(
            "UPDATE answer_ledger_nodes SET status = ?3, status_at = ?4, status_note = ?5
             WHERE document_id = ?1 AND node_id = ?2",
            params![document_id, id, status, sql_i64(now), note],
        )?;
    }
    tx.execute(
        "UPDATE answer_ledger_edges SET source = ?3
         WHERE document_id = ?1 AND from_id = ?2 AND kind IN ('derived_from', 'concludes')",
        params![document_id, id, edge_source(status)],
    )?;
    tx.commit()?;
    Ok(changed)
}

/// [`load_derivations`] from the project's `state.db`; `None` where the
/// state database may not or cannot be opened.
pub fn load_derivations_for_project(
    project_root: &std::path::Path,
    document_id: &str,
) -> Option<Result<Vec<DerivationRow>>> {
    if crate::state_store::state_db_connections_forbidden_for_process() {
        return None;
    }
    let conn = crate::state_store::open_state_db(project_root).ok()?;
    Some(load_derivations(&conn, document_id))
}

/// [`store_derivation`] into the project's `state.db`.
pub fn store_derivation_for_project(
    project_root: &std::path::Path,
    document_id: &str,
    row: &DerivationRow,
    now: u64,
) -> Result<StoreOutcome> {
    if crate::state_store::state_db_connections_forbidden_for_process() {
        bail!("this process may not open the controller state.db");
    }
    let mut conn = crate::state_store::open_state_db(project_root)?;
    store_derivation(&mut conn, document_id, row, now)
}

/// [`set_derivation_status`] in the project's `state.db`; `Ok(false)` where
/// the state database may not be opened.
pub fn set_derivation_status_for_project(
    project_root: &std::path::Path,
    document_id: &str,
    id: &str,
    status: &str,
    note: Option<&str>,
    now: u64,
) -> Result<bool> {
    if crate::state_store::state_db_connections_forbidden_for_process() {
        return Ok(false);
    }
    let mut conn = crate::state_store::open_state_db(project_root)?;
    set_derivation_status(&mut conn, document_id, id, status, note, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step() -> DerivationRow {
        DerivationRow {
            id: "drv-1a2b3c4d".into(),
            inference: "JWS credentials carry their own expiry, so no cap is needed.".into(),
            status: STATUS_UNREVIEWED.into(),
            premises: vec![
                DerivationPremiseRow {
                    node_id: "principle:simple".into(),
                    kind: "principle".into(),
                    label: "Prefer the simplest mechanism.".into(),
                },
                DerivationPremiseRow {
                    node_id: "fact:jws".into(),
                    kind: "fact".into(),
                    label: "Credentials are JWS.".into(),
                },
            ],
            decision_id: "dec-cap".into(),
            decision_label: "Cap the token lifetime?".into(),
            answer: "No cap.".into(),
            first_seen_at: None,
            status_at: None,
            status_note: None,
        }
    }

    #[test]
    fn a_derivation_round_trips_through_the_graph_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(
            store_derivation(&mut conn, "doc", &step(), 10).unwrap(),
            StoreOutcome::Created
        );
        let loaded = load_derivations(&conn, "doc").unwrap();
        assert_eq!(loaded.len(), 1);
        let row = &loaded[0];
        assert!(row.same_step(&step()));
        assert_eq!(row.status, STATUS_UNREVIEWED);
        assert_eq!(row.first_seen_at, Some(10));
        assert_eq!(row.premises[0].kind, "principle");
        assert_eq!(row.premises[1].label, "Credentials are JWS.");

        // The rows are ordinary decision-graph rows.
        let (nodes, edges) = crate::answer_ledger_index::load_graph(&conn, "doc").unwrap();
        let kinds: Vec<(&str, &str)> = nodes
            .iter()
            .map(|node| (node.id.as_str(), node.kind.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("dec-cap", "decision"),
                ("drv-1a2b3c4d", "derivation"),
                ("fact:jws", "fact"),
                ("principle:simple", "principle"),
            ]
        );
        let edges: Vec<(&str, &str, &str, Option<&str>)> = edges
            .iter()
            .map(|e| {
                (
                    e.kind.as_str(),
                    e.to.as_str(),
                    e.source.as_str(),
                    e.clause.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            edges,
            vec![
                ("concludes", "dec-cap", "proposed", Some("No cap.")),
                ("derived_from", "fact:jws", "proposed", None),
                ("derived_from", "principle:simple", "proposed", None),
            ]
        );
    }

    #[test]
    fn re_storing_keeps_the_review_and_a_different_step_is_refused() {
        let mut conn = Connection::open_in_memory().unwrap();
        store_derivation(&mut conn, "doc", &step(), 10).unwrap();
        assert!(
            set_derivation_status(&mut conn, "doc", "drv-1a2b3c4d", STATUS_CONFIRMED, None, 20)
                .unwrap()
        );
        let StoreOutcome::Existing(existing) =
            store_derivation(&mut conn, "doc", &step(), 30).unwrap()
        else {
            panic!("expected the stored step");
        };
        assert_eq!(existing.status, STATUS_CONFIRMED);
        assert_eq!(existing.status_at, Some(20));
        let (_, edges) = crate::answer_ledger_index::load_graph(&conn, "doc").unwrap();
        assert!(edges.iter().all(|edge| edge.source == "operator"));

        let mut other = step();
        other.answer = "Cap at one hour.".into();
        let err = store_derivation(&mut conn, "doc", &other, 40).unwrap_err();
        assert!(err.to_string().contains("different step"), "{err}");
    }

    #[test]
    fn a_rejection_keeps_its_note_and_premises() {
        let mut conn = Connection::open_in_memory().unwrap();
        store_derivation(&mut conn, "doc", &step(), 10).unwrap();
        set_derivation_status(
            &mut conn,
            "doc",
            "drv-1a2b3c4d",
            STATUS_REJECTED,
            Some("tokens, not keys"),
            20,
        )
        .unwrap();
        assert!(
            !set_derivation_status(
                &mut conn,
                "doc",
                "drv-1a2b3c4d",
                STATUS_REJECTED,
                Some("tokens, not keys"),
                30,
            )
            .unwrap(),
            "the same review again changes nothing"
        );
        let row = load_derivation(&conn, "doc", "drv-1a2b3c4d")
            .unwrap()
            .unwrap();
        assert_eq!(row.status, STATUS_REJECTED);
        assert_eq!(row.status_note.as_deref(), Some("tokens, not keys"));
        assert_eq!(row.status_at, Some(20));
        assert_eq!(row.premises.len(), 2);
        assert!(
            set_derivation_status(&mut conn, "doc", "drv-missing", STATUS_REJECTED, None, 1)
                .is_err()
        );
    }

    #[test]
    fn a_premise_never_rewrites_an_existing_decision_node() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::answer_ledger_index::store_nodes(
            &mut conn,
            "doc",
            &[crate::answer_ledger_index::AnswerNodeRow {
                id: "dec-cap".into(),
                kind: "decision".into(),
                label: "Should the token lifetime be capped?".into(),
                open: Some(false),
                location: None,
                first_seen_at: Some(1),
            }],
            1,
        )
        .unwrap();
        store_derivation(&mut conn, "doc", &step(), 10).unwrap();
        let (nodes, _) = crate::answer_ledger_index::load_graph(&conn, "doc").unwrap();
        let decision = nodes.iter().find(|node| node.id == "dec-cap").unwrap();
        assert_eq!(decision.label, "Should the token lifetime be capped?");
        assert_eq!(decision.open, Some(false));
    }

    #[test]
    fn the_migration_adds_status_columns_to_an_older_node_table() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE answer_ledger_nodes (
                document_id TEXT NOT NULL, node_id TEXT NOT NULL, kind TEXT NOT NULL,
                label TEXT NOT NULL, open INTEGER, location TEXT, first_seen_at INTEGER,
                recorded_at INTEGER NOT NULL, PRIMARY KEY (document_id, node_id));
             INSERT INTO answer_ledger_nodes VALUES ('doc', 'dec-a', 'decision', 'q', 1, NULL, 1, 1);",
        )
        .unwrap();
        ensure_derivation_schema(&conn).unwrap();
        ensure_derivation_schema(&conn).unwrap();
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(answer_ledger_nodes)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        for column in ["status", "status_at", "status_note"] {
            assert!(columns.iter().any(|c| c == column), "{column}");
        }
        assert!(load_derivations(&conn, "doc").unwrap().is_empty());
    }
}
