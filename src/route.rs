//! `agent-doc route` — Route /agent-doc commands to the correct tmux pane.
//!
//! Usage: agent-doc route <file.md>
//!
//! 1. Reads session UUID from file's frontmatter
//! 2. Looks up pane in sessions.json
//! 3. If pane alive: sends `/agent-doc <path>` via tmux send-keys
//! 4. If dead/missing: auto-starts a new Claude session

use anyhow::{Context, Result};
use std::path::Path;

use crate::sessions::Tmux;
use crate::{frontmatter, sessions};

const TMUX_SESSION_NAME: &str = "claude";

pub fn run(file: &Path) -> Result<()> {
    run_with_tmux(file, &Tmux::default_server())
}

pub fn run_with_tmux(file: &Path, tmux: &Tmux) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Ensure session UUID exists in frontmatter (generate if missing)
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (updated_content, session_id) = frontmatter::ensure_session(&content)?;
    if updated_content != content {
        std::fs::write(file, &updated_content)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("Generated session UUID: {}", session_id);
    }

    // Compute the file path to send (relative to cwd)
    let file_path = file.to_string_lossy();

    // Look up pane for this session
    let pane = sessions::lookup(&session_id)?;

    if let Some(ref pane_id) = pane {
        if tmux.pane_alive(pane_id) {
            // Pane is alive — send the command
            let command = format!("/agent-doc {}", file_path);
            tmux.send_keys(pane_id, &command)?;
            eprintln!("Sent /agent-doc {} → pane {}", file_path, pane_id);
            return Ok(());
        }
        eprintln!("Pane {} is dead, auto-starting...", pane_id);
    } else {
        eprintln!(
            "No pane registered for session {}, auto-starting...",
            &session_id[..std::cmp::min(8, session_id.len())]
        );
    }

    // Auto-start cascade (can be disabled for testing)
    if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
        anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
    }
    auto_start(tmux, file, &session_id, &file_path)?;
    Ok(())
}

/// Auto-start a new Claude session in tmux.
///
/// Cascade:
/// 1. tmux not running → create "claude" session
/// 2. "claude" session missing → create it
/// 3. "claude" session exists → create new window
/// 4. Send `agent-doc start <file>` in new pane
fn auto_start(tmux: &Tmux, file: &Path, session_id: &str, file_path: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;

    // Resolve the agent-doc binary path (same binary that's currently running)
    let agent_doc_bin = std::env::current_exe()
        .unwrap_or_else(|_| "agent-doc".into())
        .to_string_lossy()
        .to_string();

    let new_pane = tmux.auto_start(TMUX_SESSION_NAME, &cwd)?;

    // Register immediately so subsequent route calls find this pane
    sessions::register(session_id, &new_pane)?;

    // Start agent-doc start in the new pane
    let start_cmd = format!("{} start {}", agent_doc_bin, file_path);
    tmux.send_keys(&new_pane, &start_cmd)?;

    eprintln!(
        "Started Claude for {} in pane {} (session {})",
        file_path,
        new_pane,
        &session_id[..std::cmp::min(8, session_id.len())]
    );
    eprintln!(
        "Wait for Claude to start, then run `agent-doc route {}` again to send the command.",
        file_path
    );

    let _ = file; // suppress unused warning
    Ok(())
}
