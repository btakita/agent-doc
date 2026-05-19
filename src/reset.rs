//! # Module: reset
//!
//! ## Spec
//! - Resets a session document to a clean state by clearing the agent conversation resume pointer and deleting or rebuilding associated state files.
//! - `run(file)` performs three operations in sequence:
//!   1. Reads YAML frontmatter, sets `resume` to `None` (clears the conversation ID), rewrites the frontmatter while preserving all other fields and the document body.
//!   2. Deletes the snapshot file via `snapshot::delete`, or with `--from-current` saves the current markdown as the snapshot.
//!   3. Deletes the CRDT state file via `snapshot::delete_crdt`, or with `--from-current` rebuilds it from the current markdown.
//! - The `session` frontmatter field (routing key) is intentionally preserved; only `resume` (conversation continuity pointer) is cleared.
//! - After reset, the next `agent-doc submit` or `agent-doc stream` starts a fresh agent conversation.
//!
//! ## Agentic Contracts
//! - `run(file, from_current)` — returns `Err` if the file is missing or any I/O operation fails; returns `Ok(())` on success with a confirmation message on stderr.
//! - Callers may rely on snapshot and CRDT state being absent after a default reset.
//! - Callers may rely on snapshot and CRDT state matching the visible markdown after `--from-current`.
//! - Session identity (`session` field) is unaffected; document routing continues to work after reset.
//!
//! ## Evals
//! - file_not_found: missing path → Err containing "file not found"
//! - clears_resume: document with `resume: abc` → after reset, frontmatter has no `resume` field
//! - preserves_session: document with `session: xyz` → after reset, `session` field unchanged
//! - snapshot_deleted: snapshot exists before reset → absent after successful run
//! - crdt_deleted: CRDT state exists before reset → absent after successful run
//! - from_current_rebuilds_snapshot_and_crdt: `--from-current` saves current markdown to both state sidecars

use anyhow::Result;
use std::path::Path;

use crate::{frontmatter, snapshot};

pub fn run(file: &Path, from_current: bool) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Clear agent conversation ID (resume) — keep session (routing key)
    let content = std::fs::read_to_string(file)?;
    let (mut fm, body) = frontmatter::parse(&content)?;
    fm.resume = None;
    let updated = frontmatter::write(&fm, body)?;
    std::fs::write(file, updated)?;
    let updated_content = std::fs::read_to_string(file)?;

    if from_current {
        snapshot::save(file, &updated_content)?;
        let crdt = crate::crdt::CrdtDoc::from_text(&updated_content).encode_state();
        snapshot::save_crdt(file, &crdt)?;
        eprintln!(
            "Reset session for {} and rebuilt snapshot/CRDT from current file",
            file.display()
        );
    } else {
        // Delete snapshot
        snapshot::delete(file)?;

        // Delete CRDT state (stream mode)
        snapshot::delete_crdt(file)?;

        eprintln!("Reset session for {}", file.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn from_current_rebuilds_snapshot_and_crdt() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/crdt")).unwrap();
        let doc = dir.path().join("session.md");
        let current = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nresume: old\n---\n\nBody\n";
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, "stale snapshot").unwrap();
        snapshot::save_crdt(&doc, b"stale crdt").unwrap();

        run(&doc, true).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(!updated.contains("resume: old"));
        assert_eq!(snapshot::load(&doc).unwrap().unwrap(), updated);
        let crdt_state = snapshot::load_crdt(&doc).unwrap().unwrap();
        let crdt_text = crate::crdt::CrdtDoc::decode_state(&crdt_state)
            .unwrap()
            .to_text();
        assert_eq!(crdt_text, updated);
    }
}
