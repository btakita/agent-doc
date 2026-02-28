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

pub fn run(file: &Path, position: Option<&str>, pane: Option<&str>, window: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Validate --window if provided: check that the window is alive
    if let Some(win) = window {
        let alive = std::process::Command::new("tmux")
            .args(["list-panes", "-t", win, "-F", "#{pane_id}"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !alive {
            anyhow::bail!(
                "tmux window {} is dead or not found — re-claim from the terminal first",
                win
            );
        }
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

    let pane_id = if let Some(p) = pane {
        p.to_string() // Plugin-provided, authoritative
    } else if let Some(pos) = position {
        if let Some(win) = window {
            // Scope position detection to the specified window
            sessions::pane_by_position_in_window(pos, win)?
        } else {
            sessions::pane_by_position(pos)?
        }
    } else {
        sessions::current_pane()?
    };

    // Register session → pane (use the pane's actual PID, not our short-lived CLI PID)
    let file_str = file.to_string_lossy();
    let pane_pid = sessions::pane_pid(&pane_id).unwrap_or(std::process::id());
    sessions::register_with_pid(&session_id, &pane_id, &file_str, pane_pid)?;

    // Focus the claimed pane (select its window first for cross-window support)
    let _ = std::process::Command::new("tmux")
        .args(["select-pane", "-t", &pane_id])
        .status();

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
