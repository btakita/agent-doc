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
use crate::{frontmatter, resync, sessions};

const TMUX_SESSION_NAME: &str = "claude";

pub fn run(file: &Path, pane: Option<&str>) -> Result<()> {
    run_with_tmux(file, &Tmux::default_server(), pane)
}

pub fn run_with_tmux(file: &Path, tmux: &Tmux, pane: Option<&str>) -> Result<()> {
    let _ = resync::prune(); // Clean stale entries before lookup
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
    let registered = sessions::lookup(&session_id)?;

    if let Some(ref registered_pane) = registered {
        if tmux.pane_alive(registered_pane) {
            // Pane is alive — send the command
            let command = format!("/agent-doc {}", file_path);
            tmux.send_keys(registered_pane, &command)?;
            let _ = tmux.select_pane(registered_pane); // Focus pane for visual feedback
            eprintln!("Sent /agent-doc {} → pane {}", file_path, registered_pane);
            return Ok(());
        }
        // Pane is dead — try lazy claim
        let target_pane = pane
            .map(|p| p.to_string())
            .or_else(|| tmux.active_pane(TMUX_SESSION_NAME));
        if let Some(new_pane) = target_pane
            && tmux.pane_alive(&new_pane)
        {
            eprintln!(
                "Pane {} is dead, lazy-claiming to pane {}",
                registered_pane, new_pane
            );
            sessions::register(&session_id, &new_pane, &file_path)?;
            let command = format!("/agent-doc {}", file_path);
            tmux.send_keys(&new_pane, &command)?;
            let _ = tmux.select_pane(&new_pane); // Focus pane for visual feedback
            eprintln!("Sent /agent-doc {} → pane {}", file_path, new_pane);
            return Ok(());
        }
        eprintln!("Pane {} is dead, auto-starting...", registered_pane);
    } else {
        // No registered pane — try lazy claim
        let target_pane = pane
            .map(|p| p.to_string())
            .or_else(|| tmux.active_pane(TMUX_SESSION_NAME));
        if let Some(new_pane) = target_pane
            && tmux.pane_alive(&new_pane)
        {
            eprintln!(
                "No pane registered for session {}, lazy-claiming to pane {}",
                &session_id[..std::cmp::min(8, session_id.len())],
                new_pane
            );
            sessions::register(&session_id, &new_pane, &file_path)?;
            let command = format!("/agent-doc {}", file_path);
            tmux.send_keys(&new_pane, &command)?;
            let _ = tmux.select_pane(&new_pane); // Focus pane for visual feedback
            eprintln!("Sent /agent-doc {} → pane {}", file_path, new_pane);
            return Ok(());
        }
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

    // Join into the existing active window instead of leaving in a separate window.
    // auto_start creates a new window; we want the pane alongside existing panes.
    if let Some(active) = tmux.active_pane(TMUX_SESSION_NAME)
        && active != new_pane
    {
        let _ = tmux.join_pane(&new_pane, &active, "-dh");
    }

    // Register immediately so subsequent route calls find this pane
    sessions::register(session_id, &new_pane, file_path)?;

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
