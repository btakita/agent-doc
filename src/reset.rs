//! # Module: reset
//!
//! ## Spec
//! - Resets a session document to a clean state by clearing the agent conversation resume pointer and deleting associated state files.
//! - `run(file)` performs three operations in sequence:
//!   1. Reads YAML frontmatter, sets `resume` to `None` (clears the conversation ID), rewrites the frontmatter while preserving all other fields and the document body.
//!   2. Deletes the snapshot file via `snapshot::delete`.
//!   3. Deletes the CRDT state file via `snapshot::delete_crdt`.
//! - The `session` frontmatter field (routing key) is intentionally preserved; only `resume` (conversation continuity pointer) is cleared.
//! - After reset, the next `agent-doc submit` or `agent-doc stream` starts a fresh agent conversation.
//!
//! ## Agentic Contracts
//! - `run(file)` — returns `Err` if the file is missing or any I/O operation fails; returns `Ok(())` on success with a confirmation message on stderr.
//! - Callers may rely on snapshot and CRDT state being absent after a successful reset.
//! - Session identity (`session` field) is unaffected; document routing continues to work after reset.
//!
//! ## Evals
//! - file_not_found: missing path → Err containing "file not found"
//! - clears_resume: document with `resume: abc` → after reset, frontmatter has no `resume` field
//! - preserves_session: document with `session: xyz` → after reset, `session` field unchanged
//! - snapshot_deleted: snapshot exists before reset → absent after successful run
//! - crdt_deleted: CRDT state exists before reset → absent after successful run

use anyhow::Result;
use std::path::Path;

use crate::{frontmatter, snapshot};

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Clear agent conversation ID (resume) — keep session (routing key)
    let content = std::fs::read_to_string(file)?;
    let (mut fm, body) = frontmatter::parse(&content)?;
    fm.resume = None;
    let updated = frontmatter::write(&fm, body)?;
    std::fs::write(file, updated)?;

    // Delete snapshot
    snapshot::delete(file)?;

    // Delete CRDT state (stream mode)
    snapshot::delete_crdt(file)?;

    eprintln!("Reset session for {}", file.display());
    Ok(())
}
