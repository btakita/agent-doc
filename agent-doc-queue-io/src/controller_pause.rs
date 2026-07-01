//! Controller queue pause state I/O.

use std::path::Path;

/// Whether the document's effective controller queue-control state is `paused`
/// (`#qpausego`).
///
/// An accepted `agent-doc admin queue pause <FILE>` records a durable
/// `queue_controls` row that the controller *dispatch* RPC already honors
/// (`load_effective_queue_control_from_db` -> `failed_stage=queue_paused`). But
/// the supervisor idle-watch injects `agent-doc <FILE>` triggers straight into
/// the pane - bypassing that RPC - and this continuation signal was computed from
/// the document alone, so a `go`-mode auto-queue kept re-dispatching after an
/// accepted pause. Resolving the controller pause here lets both the idle-watch
/// drain decision and `preflight` defer to an accepted pause even for `go`-mode
/// queues. `resume`/`drain` are not `paused`, so they do not block here (the
/// controller owns draining).
///
/// Best-effort and read-only: returns `false` (not paused) when the project root
/// or controller state DB cannot be resolved/opened, so a missing control plane
/// never wedges an otherwise-active queue. A non-absent open/query error is
/// logged to stderr (never silently swallowed) and treated as not-paused so a
/// transient DB hiccup cannot strand the drain.
pub fn document_queue_controller_paused(file: &Path) -> bool {
    document_queue_controller_pause_reason(file).is_some()
}

/// The effective controller pause reason for `file` when (and only when) the
/// queue-control state is `paused` (`#qpausego` / `#qpausemix`).
///
/// Returns `Some(reason)` when an accepted `admin queue pause` is the effective
/// control state - `reason` is the operator/controller-recorded pause reason, or
/// an empty string when the pause carried none. Returns `None` when the queue is
/// not controller-paused (or the control plane / state DB cannot be resolved).
///
/// Surfacing the reason is what resolves the operator-perceived "mixed signal"
/// (`queue_paused: true` alongside `queue_continuation_required: true`): the
/// reason and the pause-aware
/// [`agent_doc_queue::queue_continuation::continuation_guidance`] preamble let
/// the agent see *why* the queue was paused and that the pause only suppresses
/// the unattended supervisor idle-watch, instead of guessing whether the pause is
/// operator intent or transient drain-coordination state. Same best-effort,
/// read-only error handling as [`document_queue_controller_paused`].
pub fn document_queue_controller_pause_reason(file: &Path) -> Option<String> {
    let root = agent_doc_fs::find_project_root(file)?;
    let db_path = agent_doc_sqlite::state_store::state_db_path(&root);
    if !db_path.exists() {
        // No control plane has ever run for this project: nothing can be paused.
        return None;
    }
    let canonical = file.canonicalize().ok()?;
    let document_id = canonical.to_string_lossy().to_string();
    let conn = match agent_doc_sqlite::state_store::open_state_db(&root) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!(
                "[agent-doc] queue_continuation: failed to open controller state DB at {} ({err:#}) — treating queue as not controller-paused",
                root.display()
            );
            return None;
        }
    };
    match agent_doc_sqlite::state_store::load_effective_queue_control_from_db(
        &conn,
        &document_id,
        &root.to_string_lossy(),
    ) {
        Ok(control) => control.and_then(|control| {
            (control.state == "paused").then(|| control.reason.unwrap_or_default())
        }),
        Err(err) => {
            eprintln!(
                "[agent-doc] queue_continuation: failed to load controller queue control for {} ({err:#}) — treating queue as not controller-paused",
                file.display()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_doc(dir: &Path) -> std::path::PathBuf {
        let doc = dir.join("session.md");
        std::fs::write(
            &doc,
            "---\nqueue_active: true\n---\n\n# Session\n\n--- queue auto go\n\n- do [#a]\n",
        )
        .unwrap();
        doc
    }

    /// `#qpausego`: set the document-scope controller queue-control state for a doc.
    fn set_document_queue_control(root: &Path, doc: &Path, state: &str) {
        let conn = agent_doc_sqlite::state_store::open_state_db(root).unwrap();
        let scope_id = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_sqlite::state_store::upsert_queue_control_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueControlInsert {
                scope_kind: "document",
                scope_id: &scope_id,
                state,
                reason: Some("test pause"),
                operation_receipt_id: None,
            },
        )
        .unwrap();
    }

    /// `#qpausego`: with no controller state DB at all, a doc is never reported
    /// as controller-paused (a missing control plane must not wedge a queue).
    #[test]
    fn document_queue_controller_paused_false_without_state_db() {
        let dir = tempfile::tempdir().unwrap();
        // `.agent-doc` must exist for project-root resolution, but no state DB.
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = write_doc(dir.path());
        assert!(
            !document_queue_controller_paused(&doc),
            "no state DB means nothing can be controller-paused"
        );
    }

    /// `#qpausego`: an accepted `admin queue pause` (document-scope `paused`
    /// control row) is reported as paused; `resume` clears it.
    #[test]
    fn document_queue_controller_paused_reflects_paused_then_resumed() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path());

        set_document_queue_control(dir.path(), &doc, "paused");
        assert!(
            document_queue_controller_paused(&doc),
            "an accepted pause must report the queue as controller-paused"
        );
        assert_eq!(
            document_queue_controller_pause_reason(&doc).as_deref(),
            Some("test pause")
        );

        set_document_queue_control(dir.path(), &doc, "resumed");
        assert!(
            !document_queue_controller_paused(&doc),
            "resume must clear the controller pause"
        );
        assert_eq!(document_queue_controller_pause_reason(&doc), None);
    }
}
