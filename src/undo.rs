//! `agent-doc undo` — Revert the last agent response.
//!
//! Restores the document from the pre-response snapshot saved during the last
//! `agent-doc write`. This effectively undoes the agent's last response while
//! preserving any user edits made after the response was written.

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
            eprintln!("[undo] No pre-response snapshot found for {}", file.display());
            eprintln!("[undo] Nothing to undo — no agent response has been written since the last undo.");
            Ok(())
        }
    }
}
