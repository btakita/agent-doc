//! Queue journal sidecar path helpers.

use std::path::{Path, PathBuf};

/// Directory (relative to the project root) holding per-document queue journals.
pub const QUEUE_JOURNAL_DIR: &str = ".agent-doc/queue-journal";

/// Resolve the queue journal sidecar path for `file`.
///
/// Returns `None` when no `.agent-doc` project root can be resolved or the
/// document state hash cannot be computed.
pub fn queue_journal_path(file: &Path) -> Option<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = agent_doc_fs::find_project_root(&canonical)?;
    let hash = agent_doc_fs::document_state_hash(&canonical).ok()?;
    Some(root.join(QUEUE_JOURNAL_DIR).join(format!("{hash}.jsonl")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_journal_path_uses_project_root_and_document_state_hash() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert_eq!(
            queue_journal_path(&doc),
            Some(
                dir.path()
                    .join(".agent-doc/queue-journal")
                    .join(format!("{hash}.jsonl"))
            )
        );
    }

    #[test]
    fn queue_journal_path_returns_none_without_project_root() {
        let doc = Path::new("/__agent_doc_queue_io_no_project__/session.md");
        assert_eq!(queue_journal_path(&doc), None);
    }
}
