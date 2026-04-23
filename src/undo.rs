//! # Module: undo
//!
//! ## Spec
//! - `run(file)` reverts the document to the pre-response snapshot saved by the
//!   last `agent-doc write` (or `agent-doc run`) call.
//! - Bails with an error if the file does not exist.
//! - When a pre-response snapshot is present:
//!   1. Atomically writes the snapshot content back to the document.
//!   2. Updates the main snapshot to match the restored state.
//!   3. Deletes the pre-response snapshot (consumed; subsequent undo is a no-op).
//! - When no pre-response snapshot exists, logs a message and returns `Ok(())` — undo
//!   is idempotent and safe to call when there is nothing to undo.
//!
//! ## Agentic Contracts
//! - `run()` is the sole public entry point.
//! - The pre-response snapshot is single-use: calling `run()` twice reverts to
//!   the same state as calling it once (no stacking of undos).
//! - All writes go through `write::atomic_write_pub` to guarantee crash-safety.
//!
//! ## Evals
//! - undo_restores_pre_response: pre-response snapshot present → document reverted, snapshot deleted
//! - undo_noop_when_no_snapshot: no snapshot present → `Ok(())`, document unchanged
//! - undo_file_not_found: non-existent file path → `Err` returned

use anyhow::Result;
use std::path::Path;

use crate::{snapshot, write};

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let pre_response = snapshot::load_pre_response(file)?;
    match pre_response {
        Some(content) => {
            // Restore the pre-response content
            write::atomic_write_pub(file, &content)?;

            // Update the main snapshot to match the restored state
            snapshot::save(file, &content)?;

            // Delete the pre-response snapshot (consumed)
            snapshot::delete_pre_response(file)?;

            eprintln!("[undo] Restored {} to pre-response state", file.display());
            Ok(())
        }
        None => {
            eprintln!(
                "[undo] No pre-response snapshot found for {}",
                file.display()
            );
            eprintln!(
                "[undo] Nothing to undo — no agent response has been written since the last undo."
            );
            Ok(())
        }
    }
}
