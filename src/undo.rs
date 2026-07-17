//! # Module: undo
//!
//! ## Spec
//! - `run(file)` reverts the document to the undo checkpoint saved by the
//!   last `agent-doc write` (or `agent-doc run`) call.
//! - Bails with an error if the file does not exist.
//! - When a undo checkpoint is present:
//!   1. Atomically writes the snapshot content back to the document.
//!   2. Updates the main snapshot to match the restored state.
//!   3. Deletes the undo checkpoint (consumed; subsequent undo is a no-op).
//! - When no undo checkpoint exists, logs a message and returns `Ok(())` — undo
//!   is idempotent and safe to call when there is nothing to undo.
//!
//! ## Agentic Contracts
//! - `run()` is the sole public entry point.
//! - The undo checkpoint is single-use: calling `run()` twice reverts to
//!   the same state as calling it once (no stacking of undos).
//! - All writes go through `agent_doc_document_realtime_io::atomic_write_through_authority` to guarantee crash-safety.
//!
//! ## Evals
//! - undo_restores_pre_response: undo checkpoint present → document reverted, checkpoint cleared
//! - undo_noop_when_no_snapshot: no snapshot present → `Ok(())`, document unchanged
//! - undo_file_not_found: non-existent file path → `Err` returned

use anyhow::Result;
use std::path::Path;

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let pre_response = agent_doc_snapshot_io::load_undo_content(file)?;
    match pre_response {
        Some(content) => {
            // Restore the pre-response content
            agent_doc_document_realtime_io::atomic_write_through_authority(file, &content)?;

            // Update the main snapshot to match the restored state
            agent_doc_snapshot_io::checkpoint_document_baseline(
                file,
                &content,
                agent_doc_ops_log_io::log_op,
            )?;

            // Delete the undo checkpoint (consumed)
            agent_doc_snapshot_io::clear_undo_content(file)?;

            eprintln!(
                "[undo] Restored {} to state before response capture",
                file.display()
            );
            Ok(())
        }
        None => {
            eprintln!("[undo] No undo checkpoint found for {}", file.display());
            eprintln!(
                "[undo] Nothing to undo — no agent response has been written since the last undo."
            );
            Ok(())
        }
    }
}
