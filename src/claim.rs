//! `agent-doc claim` — Claim a document for the current tmux pane.
//!
//! Usage: agent-doc claim <file.md>
//!
//! Reads the session UUID from frontmatter, detects the current tmux pane,
//! and registers the mapping in sessions.json. This allows the JetBrains
//! plugin (and `agent-doc route`) to send commands to the correct pane.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

use crate::{frontmatter, sessions};

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
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

    // Show a brief notification on the target pane
    let msg = format!("Claimed {} (pane {})", file_str, pane_id);
    let _ = std::process::Command::new("tmux")
        .args(["display-message", "-t", &pane_id, "-d", "3000", &msg])
        .status();

    // Append to claims log so the skill can display it on next invocation
    let log_line = format!("Claimed {} for pane {}\n", file_str, pane_id);
    let log_path = std::path::Path::new(".agent-doc/claims.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = write!(f, "{}", log_line);
    }

    eprintln!(
        "Claimed {} for pane {} (session {})",
        file.display(),
        pane_id,
        &session_id[..8]
    );

    Ok(())
}
