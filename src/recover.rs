//! `agent-doc recover` — Detect and apply orphaned pending responses.
//!
//! When context compaction interrupts the agent-doc workflow between
//! step 3 (respond) and step 4 (write), the response is saved to
//! `.agent-doc/pending/<hash>.md`. This module detects and applies it.

use anyhow::{Context, Result};
use std::path::Path;

use crate::{snapshot, write};

/// Check for a pending response and apply it if found.
///
/// Returns `true` if a pending response was recovered, `false` otherwise.
pub fn run(file: &Path) -> Result<bool> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let pending_path = snapshot::pending_path_for(file)?;
    if !pending_path.exists() {
        return Ok(false);
    }

    let response = std::fs::read_to_string(&pending_path)
        .with_context(|| format!("failed to read pending response {}", pending_path.display()))?;

    if response.trim().is_empty() {
        // Empty pending file — just clean up
        let _ = std::fs::remove_file(&pending_path);
        return Ok(false);
    }

    eprintln!(
        "[recover] Found orphaned response for {} ({} bytes). Applying...",
        file.display(),
        response.len()
    );

    // Check if response contains template patch blocks
    let is_template = response.contains("<!-- patch:");
    if is_template {
        write::apply_template_from_string(file, &response)?;
    } else {
        write::apply_append_from_string(file, &response)?;
    }

    // Remove the pending file after successful write
    std::fs::remove_file(&pending_path)
        .with_context(|| format!("failed to remove pending file {}", pending_path.display()))?;

    eprintln!("[recover] Response recovered and written to {}", file.display());
    Ok(true)
}

/// Save a response to the pending store before attempting write-back.
/// This makes the response durable across context compaction.
pub fn save_pending(file: &Path, response: &str) -> Result<()> {
    let pending_path = snapshot::pending_path_for(file)?;
    if let Some(parent) = pending_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pending_path, response)
        .with_context(|| format!("failed to save pending response {}", pending_path.display()))?;
    Ok(())
}

/// Remove the pending file after a successful write-back.
pub fn clear_pending(file: &Path) -> Result<()> {
    let pending_path = snapshot::pending_path_for(file)?;
    if pending_path.exists() {
        std::fs::remove_file(&pending_path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        dir
    }

    #[test]
    fn no_pending_returns_false() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "# Doc\n\n## User\n\nHello\n").unwrap();
        assert!(!run(&doc).unwrap());
    }

    #[test]
    fn save_and_clear_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();

        save_pending(&doc, "response text").unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(pending.exists());

        clear_pending(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn recover_append_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();

        // Save a pending response
        save_pending(&doc, "This is the recovered response.").unwrap();

        // Recover it
        let recovered = run(&doc).unwrap();
        assert!(recovered);

        // Verify the response was written
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("This is the recovered response."));
        assert!(result.contains("## Assistant"));

        // Pending file should be cleaned up
        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn empty_pending_cleaned_up() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();

        save_pending(&doc, "").unwrap();
        let recovered = run(&doc).unwrap();
        assert!(!recovered);

        let pending = snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }
}
