//! `#clobberfollowups` (4): the durable record of a retained write the
//! controller refused to project because rolling it forward would delete
//! operator text.
//!
//! `#retainedprojectionclobber` made the controller hold such a write and log
//! `retained_transition_refused_operator_text_loss` once per (intent, visible
//! cut). A log line is not something `session-check` can read, so it kept
//! reporting the generic "retained delivery remains unsettled ... reconciliation
//! remains scheduled" wait for a state no scheduled reconciliation can end: the
//! agent's write and the operator's edit changed the same line, and only a new
//! editor cut can resolve it.
//!
//! This module owns the `state.db` row that carries the refusal across
//! processes: `document_runtime_state`, kind [`STATE_KIND`], keyed by the
//! document's state hash. The controller writes it when it reports the refusal
//! and clears it when it projects a retained target for the document. Readers
//! additionally require that the visible cut the refusal was derived over is
//! still the canonical ([`RetainedOperatorTextConflictRecord::matches_authority`]),
//! because the controller re-derives the write over every newer cut.

use agent_doc_sqlite::state_store::{self, DocumentRuntimeStateRecord};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `document_runtime_state.state_kind` of the record.
pub const STATE_KIND: &str = "retained_transition_operator_text_conflict";
/// The state token the controller log and every reader name.
pub const STATE: &str = "retained_transition_refused_operator_text_loss";

/// A retained write held because its rebase over the visible editor cut would
/// delete operator-authored lines.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetainedOperatorTextConflictRecord {
    pub intent_id: String,
    /// SHA-256 of the retained target the controller held.
    pub target_hash: String,
    /// SHA-256 of the visible cut the refused rebase was derived over.
    pub visible_hash: String,
    pub delivery_version: u64,
    /// How many operator-added lines the rebase would have deleted.
    pub lost_operator_lines: usize,
    pub recorded_at_ms: u64,
}

impl RetainedOperatorTextConflictRecord {
    /// Whether `authority` (the live canonical) is still the cut the refusal
    /// was derived over. A newer cut is re-derived by the controller and
    /// either projects (clearing this record) or records a new refusal.
    pub fn matches_authority(&self, authority: &str) -> bool {
        agent_doc_hash::content_hash(authority).eq_ignore_ascii_case(&self.visible_hash)
    }

    /// The `key=value` facts every rendering of the state carries.
    pub fn facts(&self) -> String {
        format!(
            "state={STATE} intent_id={} target_hash={} visible_hash={} lost_operator_lines={} applied=false",
            self.intent_id, self.target_hash, self.visible_hash, self.lost_operator_lines,
        )
    }
}

fn document_key(file: &Path) -> Result<String> {
    agent_doc_fs::document_state_hash(file)
}

/// Upsert the record for `file` in `project_root`'s `state.db`.
pub fn record(
    project_root: &Path,
    file: &Path,
    conflict: &RetainedOperatorTextConflictRecord,
) -> Result<()> {
    let conn = state_store::open_state_db(project_root)?;
    state_store::upsert_document_runtime_state_in_db(
        &conn,
        &DocumentRuntimeStateRecord {
            document_hash: document_key(file)?,
            state_kind: STATE_KIND.to_string(),
            canonical_path: file
                .canonicalize()
                .unwrap_or_else(|_| file.to_path_buf())
                .to_string_lossy()
                .into_owned(),
            payload_json: serde_json::to_string(conflict)?,
            updated_at_ms: conflict.recorded_at_ms,
        },
    )
}

/// Load the record for `file`. A malformed payload reads as absent.
pub fn load(
    project_root: &Path,
    file: &Path,
) -> Result<Option<RetainedOperatorTextConflictRecord>> {
    load_with_busy_timeout(project_root, file, None)
}

fn load_with_busy_timeout(
    project_root: &Path,
    file: &Path,
    busy_timeout: Option<std::time::Duration>,
) -> Result<Option<RetainedOperatorTextConflictRecord>> {
    if !state_store::state_db_path(project_root).exists() {
        return Ok(None);
    }
    let conn = match busy_timeout {
        Some(timeout) => state_store::open_state_db_with_timeout(project_root, timeout)?,
        None => state_store::open_state_db(project_root)?,
    };
    Ok(
        state_store::load_document_runtime_state_from_db(&conn, &document_key(file)?, STATE_KIND)?
            .and_then(|row| serde_json::from_str(&row.payload_json).ok()),
    )
}

/// Remove the record for `file`. Returns whether a row was removed. Reads
/// before it deletes, so the common no-record case never takes the write lock.
pub fn clear(project_root: &Path, file: &Path) -> Result<bool> {
    if load(project_root, file)?.is_none() {
        return Ok(false);
    }
    let conn = state_store::open_state_db(project_root)?;
    state_store::clear_document_runtime_state_in_db(&conn, &document_key(file)?, STATE_KIND)
}

/// Same bound as the exhausted replica-rebuild read: this runs on
/// `session-check`'s retained-write gate, so a busy ledger reads as "not
/// observed" rather than stalling it.
const OBSERVE_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

/// The cross-process durable read. A process that may not open `state.db`, a
/// missing project or database, or a read failure is `None`: not looking never
/// invents an operator-owned state.
pub fn observe(file: &Path) -> Option<RetainedOperatorTextConflictRecord> {
    if state_store::state_db_connections_forbidden_for_process() {
        return None;
    }
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    match load_with_busy_timeout(&project_root, file, Some(OBSERVE_BUSY_TIMEOUT)) {
        Ok(conflict) => conflict,
        Err(error) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "retained_operator_text_conflict_read_failed file={} error={error:#}",
                    file.display()
                ),
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict_over(visible: &str) -> RetainedOperatorTextConflictRecord {
        RetainedOperatorTextConflictRecord {
            intent_id: "intent-1".to_string(),
            target_hash: agent_doc_hash::content_hash("agent target\n"),
            visible_hash: agent_doc_hash::content_hash(visible),
            delivery_version: 5,
            lost_operator_lines: 1,
            recorded_at_ms: 1,
        }
    }

    #[test]
    fn the_record_describes_only_the_cut_it_was_derived_over() {
        let conflict = conflict_over("operator cut\n");
        assert!(conflict.matches_authority("operator cut\n"));
        assert!(!conflict.matches_authority("a newer operator cut\n"));
        let facts = conflict.facts();
        assert!(
            facts.contains("state=retained_transition_refused_operator_text_loss"),
            "{facts}"
        );
        assert!(facts.contains("lost_operator_lines=1"), "{facts}");
        assert!(facts.contains("applied=false"), "{facts}");
    }

    #[test]
    fn the_record_round_trips_and_clears() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".agent-doc")).unwrap();
        let file = project.path().join("session.md");
        std::fs::write(&file, "operator cut\n").unwrap();
        assert_eq!(load(project.path(), &file).unwrap(), None);
        assert!(!clear(project.path(), &file).unwrap());

        let conflict = conflict_over("operator cut\n");
        record(project.path(), &file, &conflict).unwrap();
        assert_eq!(load(project.path(), &file).unwrap(), Some(conflict));
        assert!(clear(project.path(), &file).unwrap());
        assert_eq!(load(project.path(), &file).unwrap(), None);
    }
}
