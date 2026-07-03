//! Pending response sidecar I/O.

use anyhow::{Context, Result};
use std::path::Path;

/// Save a response to the pending store before attempting write-back.
/// This makes the response durable across context compaction.
pub fn save_pending(file: &Path, response: &str) -> Result<()> {
    let response = agent_doc_template_io::canonicalize_response_for_capture(file, response)?;
    agent_doc_capture_io::capture_response(file, &response)?;
    let pending_path = agent_doc_fs::pending_response_path_for(file)?;
    if let Some(parent) = pending_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pending_path, &response)
        .with_context(|| format!("failed to save pending response {}", pending_path.display()))?;
    Ok(())
}

/// Remove the pending file after a successful write-back.
pub fn clear_pending(file: &Path) -> Result<()> {
    let pending_path = agent_doc_fs::pending_response_path_for(file)?;
    if pending_path.exists() {
        std::fs::remove_file(&pending_path)?;
    }
    // Also clean up the pre-response snapshot (saved before write for undo support).
    // Without this, pre-response files accumulate indefinitely after successful writes.
    if let Err(e) = agent_doc_snapshot_io::delete_pre_response(file) {
        eprintln!("[repair] warning: failed to delete pre-response: {}", e);
    }
    if let Err(e) = agent_doc_capture_io::mark_write_applied(file) {
        eprintln!("[repair] warning: failed to update capture state: {}", e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_clears_pending_response() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        save_pending(&doc, "response text").unwrap();
        let pending = agent_doc_fs::pending_response_path_for(&doc).unwrap();
        assert!(pending.exists());
        assert_eq!(std::fs::read_to_string(&pending).unwrap(), "response text");

        clear_pending(&doc).unwrap();
        assert!(!pending.exists());
    }
}
