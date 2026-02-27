//! `agent-doc focus` — Focus the tmux pane for a session document.
//!
//! Usage: agent-doc focus <file.md>
//!
//! 1. Reads session UUID from file's frontmatter
//! 2. Looks up pane in sessions.json
//! 3. Runs `tmux select-pane -t <pane-id>` to focus it

use anyhow::{Context, Result};
use std::path::Path;

use crate::sessions::Tmux;
use crate::{frontmatter, sessions};

pub fn run(file: &Path) -> Result<()> {
    run_with_tmux(file, &Tmux::default_server())
}

pub fn run_with_tmux(file: &Path, tmux: &Tmux) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (_updated, session_id) = frontmatter::ensure_session(&content)?;

    let pane = sessions::lookup(&session_id)?;
    match pane {
        Some(pane_id) if tmux.pane_alive(&pane_id) => {
            tmux.select_pane(&pane_id)?;
            eprintln!("Focused pane {} ({})", pane_id, file.display());
            Ok(())
        }
        Some(pane_id) => {
            anyhow::bail!("pane {} is dead for {}", pane_id, file.display());
        }
        None => {
            anyhow::bail!(
                "no pane registered for {} (session {})",
                file.display(),
                &session_id[..std::cmp::min(8, session_id.len())]
            );
        }
    }
}
