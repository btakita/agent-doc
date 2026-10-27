//! `#splicesessionack` (GH #273 remainder): the durable record of a retained
//! native save whose replica-rebuild budget is spent.
//!
//! The controller's retained-persistence Effect asks the editor to save one
//! immutable content (`content_hash`, `content_len`). When the editor refuses
//! that save after every delivered replica rebuild (`#rebuildconverge`), the
//! editor's VISIBLE buffer, not its replica, diverges from the retained write:
//! typically operator typing held as an unmerged local batch. The controller
//! logs `retained_persistence_replica_rebuild_exhausted` once and keeps its
//! capped save retry, but the log line is not something `session-check` can
//! read, so it kept reporting the generic "acknowledgement pending" wait for a
//! state that waiting cannot end.
//!
//! This module owns the `state.db` row that carries that fact across
//! processes: `document_runtime_state`, kind [`STATE_KIND`], keyed by the
//! document's state hash, with the refused content in the payload. The
//! controller writes it when the budget is exhausted and clears it when a save
//! of the document is applied or a different content starts a new refusal
//! streak. Readers additionally require that disk has not already reached the
//! recorded content ([`RetainedPersistenceRebuildExhausted::holds_against_disk`]),
//! so a convergence the controller has not observed yet can never be reported
//! as an operator-owned stall.

use agent_doc_sqlite::state_store::{self, DocumentRuntimeStateRecord};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `document_runtime_state.state_kind` of the record.
pub const STATE_KIND: &str = "retained_persistence_rebuild_exhausted";
/// The state token the controller log and every reader name.
pub const STATE: &str = "editor_holds_unmerged_visible_text";
/// The operator action the controller log and every reader name.
pub const OPERATOR_ACTION: &str = "reconcile_editor_buffer_with_retained_write";

/// A retained native save the editor keeps refusing after the replica-rebuild
/// budget for that exact content was spent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetainedPersistenceRebuildExhausted {
    /// SHA-256 of the retained content the editor refuses to save.
    pub content_hash: String,
    pub content_len: usize,
    /// Delivery epoch of the refusal that exhausted the budget.
    pub epoch: u64,
    pub controller_generation: u64,
    pub refusals: u32,
    pub rebuilds: u32,
    /// Terminal editor refusal of the in-place roll-forward, if one arrived.
    #[serde(default)]
    pub roll_forward_refusal: Option<String>,
    pub recorded_at_ms: u64,
}

impl RetainedPersistenceRebuildExhausted {
    /// Whether the record still describes the document: disk has not reached
    /// the refused content. A missing or unreadable disk keeps the record,
    /// because "I could not look" is not convergence.
    pub fn holds_against_disk(&self, disk: Option<&[u8]>) -> bool {
        !disk.is_some_and(|bytes| {
            bytes.len() == self.content_len
                && agent_doc_hash::bytes_hash(bytes).eq_ignore_ascii_case(&self.content_hash)
        })
    }

    /// Whether `authority` (the controller canonical text) is still the
    /// refused content. A canonical that moved on means a newer write owns the
    /// document and this record is about to be replaced or cleared.
    pub fn matches_authority(&self, authority: &str) -> bool {
        authority.len() == self.content_len
            && agent_doc_hash::content_hash(authority).eq_ignore_ascii_case(&self.content_hash)
    }

    /// The `key=value` facts every rendering of the state carries.
    pub fn facts(&self) -> String {
        let refusal = self
            .roll_forward_refusal
            .as_deref()
            .map(|reason| format!(" roll_forward_refusal={reason}"))
            .unwrap_or_default();
        format!(
            "state={STATE} content_hash={} content_len={} refusals={} rebuilds={}{} operator_action={OPERATOR_ACTION}",
            self.content_hash, self.content_len, self.refusals, self.rebuilds, refusal,
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
    exhausted: &RetainedPersistenceRebuildExhausted,
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
            payload_json: serde_json::to_string(exhausted)?,
            updated_at_ms: exhausted.recorded_at_ms,
        },
    )
}

/// Mark the current exhausted content as terminally refused by the editor.
pub fn mark_roll_forward_refusal(
    project_root: &Path,
    file: &Path,
    content_hash: &str,
    reason: &str,
) -> Result<bool> {
    let Some(mut exhausted) = load(project_root, file)? else {
        return Ok(false);
    };
    if !exhausted.content_hash.eq_ignore_ascii_case(content_hash) {
        return Ok(false);
    }
    exhausted.roll_forward_refusal = Some(reason.to_string());
    record(project_root, file, &exhausted)?;
    Ok(true)
}

/// `#clobberfollowups` (4): mark a TERMINAL roll-forward refusal durably,
/// whether or not the exhausted record for that content is already there.
///
/// [`mark_roll_forward_refusal`] marks only an existing record for the same
/// content, so a terminal refusal that won the race with the exhaustion record
/// (or followed a new refusal streak) logged `durable_state_marked=false` and
/// `session-check` never rendered its terminal remedy. `fallback` describes the
/// refused content from the caller's own observation and is recorded, with the
/// refusal, when no matching record exists. Returns whether a record now
/// carries the refusal.
pub fn mark_terminal_roll_forward_refusal(
    project_root: &Path,
    file: &Path,
    fallback: &RetainedPersistenceRebuildExhausted,
    reason: &str,
) -> Result<bool> {
    if mark_roll_forward_refusal(project_root, file, &fallback.content_hash, reason)? {
        return Ok(true);
    }
    let mut exhausted = fallback.clone();
    exhausted.roll_forward_refusal = Some(reason.to_string());
    record(project_root, file, &exhausted)?;
    Ok(true)
}

/// Load the record for `file`. A malformed payload reads as absent.
pub fn load(
    project_root: &Path,
    file: &Path,
) -> Result<Option<RetainedPersistenceRebuildExhausted>> {
    load_with_busy_timeout(project_root, file, None)
}

fn load_with_busy_timeout(
    project_root: &Path,
    file: &Path,
    busy_timeout: Option<std::time::Duration>,
) -> Result<Option<RetainedPersistenceRebuildExhausted>> {
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

/// Bound for the cross-process read: it runs on every retained-write
/// ownership observation, so a busy ledger reads as "not observed" rather than
/// stalling the caller (the same bound the delivery-rejection read uses).
const OBSERVE_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

/// Remove the record for `file` unless it describes `keep_content_hash`.
/// `None` removes any record. Returns whether a row was removed. Reads before
/// it deletes, so the common no-record case never takes the write lock.
pub fn clear_unless(
    project_root: &Path,
    file: &Path,
    keep_content_hash: Option<&str>,
) -> Result<bool> {
    let Some(existing) = load(project_root, file)? else {
        return Ok(false);
    };
    if keep_content_hash.is_some_and(|keep| existing.content_hash.eq_ignore_ascii_case(keep)) {
        return Ok(false);
    }
    let conn = state_store::open_state_db(project_root)?;
    state_store::clear_document_runtime_state_in_db(&conn, &document_key(file)?, STATE_KIND)
}

/// The cross-process durable read: the record for `file`, when one exists and
/// disk has not reached its content.
///
/// Asymmetric on purpose, like the delivery-rejection read: a process that may
/// not open `state.db` (the editor FFI), a missing project or database, or a
/// read failure is `None`. Not looking must never invent an operator-owned
/// stall; it leaves the caller's pre-existing (still fail-closed) reading.
pub fn observe(file: &Path) -> Option<RetainedPersistenceRebuildExhausted> {
    if state_store::state_db_connections_forbidden_for_process() {
        return None;
    }
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let exhausted = match load_with_busy_timeout(&project_root, file, Some(OBSERVE_BUSY_TIMEOUT)) {
        Ok(exhausted) => exhausted?,
        Err(error) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "retained_persistence_rebuild_exhausted_read_failed file={} error={error:#}",
                    file.display()
                ),
            );
            return None;
        }
    };
    let disk = std::fs::read(file).ok();
    exhausted
        .holds_against_disk(disk.as_deref())
        .then_some(exhausted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exhausted_for(content: &str) -> RetainedPersistenceRebuildExhausted {
        RetainedPersistenceRebuildExhausted {
            content_hash: agent_doc_hash::content_hash(content),
            content_len: content.len(),
            epoch: 130,
            controller_generation: 2,
            refusals: 9,
            rebuilds: 2,
            roll_forward_refusal: None,
            recorded_at_ms: 1,
        }
    }

    #[test]
    fn the_record_holds_until_disk_reaches_the_refused_content() {
        let exhausted = exhausted_for("retained closeout\n");
        assert!(exhausted.holds_against_disk(Some(b"operator buffer\n")));
        assert!(
            exhausted.holds_against_disk(None),
            "an unreadable disk is not convergence"
        );
        assert!(!exhausted.holds_against_disk(Some(b"retained closeout\n")));
        assert!(exhausted.matches_authority("retained closeout\n"));
        assert!(!exhausted.matches_authority("a newer canonical\n"));
        let facts = exhausted.facts();
        assert!(
            facts.contains("state=editor_holds_unmerged_visible_text"),
            "{facts}"
        );
        assert!(
            facts.contains("operator_action=reconcile_editor_buffer_with_retained_write"),
            "{facts}"
        );
        assert!(!facts.contains("roll_forward_refusal="), "{facts}");
    }

    #[test]
    fn the_record_round_trips_and_clears_only_for_other_content() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".agent-doc")).unwrap();
        let file = project.path().join("session.md");
        std::fs::write(&file, "operator buffer\n").unwrap();
        assert_eq!(load(project.path(), &file).unwrap(), None);
        assert!(!clear_unless(project.path(), &file, None).unwrap());

        let exhausted = exhausted_for("retained closeout\n");
        record(project.path(), &file, &exhausted).unwrap();
        assert_eq!(
            load(project.path(), &file).unwrap(),
            Some(exhausted.clone())
        );
        assert!(mark_roll_forward_refusal(
            project.path(),
            &file,
            &exhausted.content_hash,
            "visible_structure_invalid",
        )
        .unwrap());
        let refused = load(project.path(), &file).unwrap().unwrap();
        assert_eq!(
            refused.roll_forward_refusal.as_deref(),
            Some("visible_structure_invalid")
        );
        assert!(
            !clear_unless(project.path(), &file, Some(&exhausted.content_hash)).unwrap(),
            "the same refused content keeps its record"
        );
        assert!(clear_unless(project.path(), &file, Some("different-content")).unwrap());
        assert_eq!(load(project.path(), &file).unwrap(), None);
    }

    /// `#clobberfollowups` (4): a terminal roll-forward refusal is durable even
    /// when no exhausted record for that content exists yet, so `session-check`
    /// renders its terminal remedy instead of a pending wait.
    #[test]
    fn clobberfollowups_terminal_roll_forward_refusal_is_recorded_without_a_prior_record() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".agent-doc")).unwrap();
        let file = project.path().join("session.md");
        std::fs::write(&file, "operator buffer\n").unwrap();
        let refused = exhausted_for("retained closeout\n");
        assert!(
            !mark_roll_forward_refusal(project.path(), &file, &refused.content_hash, "x").unwrap(),
            "the plain mark needs an existing record"
        );
        assert!(
            mark_terminal_roll_forward_refusal(
                project.path(),
                &file,
                &refused,
                "editor_unreachable"
            )
            .unwrap()
        );
        let recorded = load(project.path(), &file).unwrap().unwrap();
        assert_eq!(recorded.content_hash, refused.content_hash);
        assert_eq!(
            recorded.roll_forward_refusal.as_deref(),
            Some("editor_unreachable")
        );
        assert!(
            recorded
                .facts()
                .contains("roll_forward_refusal=editor_unreachable")
        );

        // A record for the same content keeps its own streak counters.
        let mut existing = exhausted_for("retained closeout\n");
        existing.refusals = 42;
        record(project.path(), &file, &existing).unwrap();
        assert!(
            mark_terminal_roll_forward_refusal(
                project.path(),
                &file,
                &refused,
                "editor_endpoint_rejected"
            )
            .unwrap()
        );
        let marked = load(project.path(), &file).unwrap().unwrap();
        assert_eq!(marked.refusals, 42);
        assert_eq!(
            marked.roll_forward_refusal.as_deref(),
            Some("editor_endpoint_rejected")
        );
    }
}
