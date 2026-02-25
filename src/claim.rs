//! `agent-doc claim` — Claim a document for the current tmux pane.
//!
//! Usage: agent-doc claim <file.md>
//!
//! Reads the session UUID from frontmatter, detects the current tmux pane,
//! and registers the mapping in sessions.json. This allows the JetBrains
//! plugin (and `agent-doc route`) to send commands to the correct pane.

use anyhow::{Context, Result};
use std::path::Path;

use crate::{frontmatter, sessions};

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Must be inside tmux
    if !sessions::in_tmux() {
        anyhow::bail!("not running inside tmux — TMUX_PANE is not set");
    }

    // Ensure session UUID exists in frontmatter
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (updated_content, session_id) = frontmatter::ensure_session(&content)?;
    if updated_content != content {
        std::fs::write(file, &updated_content)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("Generated session UUID: {}", session_id);
    }

    let pane_id = sessions::current_pane()?;

    // Register session → pane
    let file_str = file.to_string_lossy();
    sessions::register(&session_id, &pane_id, &file_str)?;
    eprintln!(
        "Claimed {} for pane {} (session {})",
        file.display(),
        pane_id,
        &session_id[..8]
    );

    Ok(())
}
