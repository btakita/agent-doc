//! `#controllerrecyclelost` (2): the durable record of an editor replica attach
//! that the editor paused on operator evidence.
//!
//! When the JetBrains plugin refuses to attach a document's replica on a
//! fail-closed projection hold (`ambiguous-retained-projection`,
//! `ambiguous-reregister-projection`, `retained-reseed-missing-settled-shadow`),
//! the native `attach_failure_diagnosis` answers
//! `retry_disposition=await_operator_evidence` and the plugin DISARMS every
//! time-driven retry. Nothing an agent or the controller does re-arms it: the
//! plugin waits for the operator to close, repair, and reopen the document. The
//! only trace of that state used to be one `editor_surface_event ...
//! status=attach_refused_<token>` ops-log line, which no retained-write reader
//! can consult, so every reader kept promising that the retained write "delivers
//! once the editor replica converges". Observed 2026-11-04 on
//! `src/haiven-dev/tasks/sdk/sdk-dev.md`: the retained write for
//! `cycle-1793794981490` waited 20 minutes behind that replica until an operator
//! click supplied the evidence.
//!
//! The record is written at the native boundary that already receives the hold
//! (`agent_doc_record_editor_surface_event`, called from the editor process), so
//! it needs no plugin generation. It is a small JSON file under
//! [`EDITOR_ATTACH_HOLDS_DIR`] keyed by the document state hash, because the
//! editor process may not open `state.db`.
//!
//! The record only states what the editor reported. Whether it still describes
//! the document is decided by the reader, against two observations the record
//! cannot make about itself: the recording editor process is still alive
//! ([`observe`]), and the document is still open without a live replica (the
//! caller's authority read, `agent_doc_document_realtime_io`). A closed tab —
//! the first step of the operator action — makes the hold moot, and the reader
//! clears it then.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Project-relative directory of the hold records.
pub const EDITOR_ATTACH_HOLDS_DIR: &str = ".agent-doc/state/editor-attach-holds";

/// One editor's paused replica attach for one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorAttachHold {
    /// The attach-failure status token the editor reported (the
    /// `attach_refused_<token>` suffix), e.g. `ambiguous-retained-projection`.
    pub status_token: String,
    /// The reporting editor family (`jetbrains`, `vscode`).
    pub editor_source: String,
    /// The editor process that paused its retry. The hold ends with it: a new
    /// editor process is new evidence.
    pub editor_pid: u32,
    /// The document's canonical path, for diagnostics.
    pub canonical_path: String,
    pub recorded_at_ms: u64,
}

fn hold_path(project_root: &Path, file: &Path) -> Result<PathBuf> {
    Ok(project_root
        .join(EDITOR_ATTACH_HOLDS_DIR)
        .join(format!("{}.json", agent_doc_fs::document_state_hash(file)?)))
}

/// Durably record `hold` for `file` in `project_root`.
pub fn record(project_root: &Path, file: &Path, hold: &EditorAttachHold) -> Result<()> {
    let path = hold_path(project_root, file)?;
    let payload = serde_json::to_vec(hold).context("failed to encode editor attach hold")?;
    agent_doc_fs::write_atomic(&path, &payload)
        .with_context(|| format!("failed to record editor attach hold {}", path.display()))
}

/// Remove the hold for `file`. Returns whether one was removed.
pub fn clear(project_root: &Path, file: &Path) -> Result<bool> {
    let path = hold_path(project_root, file)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err)
            .with_context(|| format!("failed to clear editor attach hold {}", path.display())),
    }
}

/// Load the hold for `file`. A missing record is `None`; an unreadable or
/// malformed one is an error the caller must surface.
pub fn load(project_root: &Path, file: &Path) -> Result<Option<EditorAttachHold>> {
    let path = hold_path(project_root, file)?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to read editor attach hold {}", path.display()));
        }
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .with_context(|| format!("malformed editor attach hold {}", path.display()))
}

/// The hold for `file` when its recording editor process is still alive.
///
/// Asymmetric on purpose, like the other retained-write durable reads: a
/// missing project or record, or a read failure, is `None` (logged), because
/// not looking must never invent an operator-owned stall. A record whose editor
/// process has exited no longer describes anything and is removed.
pub fn observe(file: &Path) -> Option<EditorAttachHold> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let hold = match load(&project_root, file) {
        Ok(hold) => hold?,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "editor_attach_hold_read_failed file={} error={err:#} (#controllerrecyclelost)",
                    file.display()
                ),
            );
            return None;
        }
    };
    if agent_doc_reliable_sync_io::process_pid_is_live(hold.editor_pid) {
        return Some(hold);
    }
    clear_with_reason(&project_root, file, &hold, "editor_process_exited");
    None
}

/// Clear `hold` for `file` and log why. A failed removal is logged, never
/// swallowed; the next reader re-validates the record anyway.
pub fn clear_with_reason(project_root: &Path, file: &Path, hold: &EditorAttachHold, reason: &str) {
    let outcome = match clear(project_root, file) {
        Ok(removed) => format!("removed={removed}"),
        Err(err) => format!("clear_failed={err:#}"),
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "editor_attach_hold_cleared file={} status_token={} editor_pid={} reason={reason} {outcome} (#controllerrecyclelost)",
            file.display(),
            hold.status_token,
            hold.editor_pid,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();
        (dir, doc)
    }

    fn hold(editor_pid: u32) -> EditorAttachHold {
        EditorAttachHold {
            status_token: "ambiguous-retained-projection".to_string(),
            editor_source: "jetbrains".to_string(),
            editor_pid,
            canonical_path: "plan.md".to_string(),
            recorded_at_ms: 1,
        }
    }

    #[test]
    fn a_recorded_hold_round_trips_and_clears() {
        let (dir, doc) = project();
        assert_eq!(load(dir.path(), &doc).unwrap(), None);
        record(dir.path(), &doc, &hold(std::process::id())).unwrap();
        assert_eq!(
            load(dir.path(), &doc).unwrap(),
            Some(hold(std::process::id()))
        );
        assert_eq!(observe(&doc), Some(hold(std::process::id())));
        assert!(clear(dir.path(), &doc).unwrap());
        assert!(!clear(dir.path(), &doc).unwrap());
        assert_eq!(observe(&doc), None);
    }

    /// The hold belongs to the editor process that paused its retry. Once that
    /// process is gone, the record describes nothing and is removed.
    #[test]
    fn a_hold_from_an_exited_editor_is_not_observed_and_is_removed() {
        let (dir, doc) = project();
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let exited = child.id();
        child.wait().unwrap();
        record(dir.path(), &doc, &hold(exited)).unwrap();
        assert_eq!(observe(&doc), None);
        assert_eq!(load(dir.path(), &doc).unwrap(), None);
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("editor_attach_hold_cleared") && ops.contains("editor_process_exited"),
            "{ops}"
        );
    }

    /// A malformed record is an error at `load` and is reported, never read as
    /// a hold.
    #[test]
    fn a_malformed_hold_is_reported_not_observed() {
        let (dir, doc) = project();
        let path = hold_path(dir.path(), &doc).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load(dir.path(), &doc).is_err());
        assert_eq!(observe(&doc), None);
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("editor_attach_hold_read_failed"), "{ops}");
    }
}
