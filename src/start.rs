//! `agent-doc start` — Start Claude in a tmux pane and register the session.
//!
//! Usage: agent-doc start <file.md>
//!
//! 1. Reads file, ensures session UUID exists (generates if missing)
//! 2. Registers session → current tmux pane in sessions.json
//! 3. Execs `claude` (replaces this process)

use anyhow::{Context, Result};
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

    // Must be inside tmux
    if !sessions::in_tmux() {
        anyhow::bail!("not running inside tmux — start a tmux session first");
    }

    let pane_id = sessions::current_pane()?;

    // Register session → pane
    sessions::register(&session_id, &pane_id)?;
    eprintln!(
        "Registered session {} → pane {}",
        &session_id[..8],
        pane_id
    );

    // Exec claude (replaces this process)
    eprintln!("Starting claude...");
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new("claude").exec();
    // exec() only returns on error
    Err(anyhow::anyhow!("failed to exec claude: {}", err))
}
