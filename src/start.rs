//! `agent-doc start` — Start Claude in a tmux pane and register the session.
//!
//! Usage: agent-doc start <file.md>
//!
//! 1. Reads file, ensures session UUID exists (generates if missing)
//! 2. Registers session → current tmux pane in sessions.json
//! 3. Runs `claude` as a child process in a restart loop
//!
//! When Claude exits, the pane stays alive:
//! - Context exhaustion or non-zero exit → auto-restart with `--continue`
//! - Clean exit (code 0) → prompt user to restart or quit

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

    // Register session → pane (with relative file path)
    let file_str = file.to_string_lossy();
    sessions::register(&session_id, &pane_id, &file_str)?;
    eprintln!(
        "Registered session {} → pane {}",
        &session_id[..8],
        pane_id
    );

    // Run claude in a restart loop — pane never dies
    let mut first_run = true;
    loop {
        let mut cmd = std::process::Command::new("claude");
        if !first_run {
            // After first run, continue the previous session
            cmd.arg("--continue");
            eprintln!("Restarting claude (--continue)...");
        } else {
            eprintln!("Starting claude...");
        }

        let status = cmd.status().context("failed to run claude")?;
        first_run = false;

        let code = status.code().unwrap_or(1);
        if code == 0 {
            // Clean exit — prompt user
            eprintln!("\nClaude exited cleanly.");
            eprintln!("Press Enter to restart, or 'q' to exit.");
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_err() {
                break;
            }
            if input.trim().eq_ignore_ascii_case("q") {
                break;
            }
            // User pressed Enter — restart fresh
            first_run = true;
        } else {
            // Non-zero exit (context exhaustion, crash, etc.) — auto-restart
            eprintln!(
                "\nClaude exited with code {}. Auto-restarting in 2s...",
                code
            );
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }

    eprintln!("Session ended for {}", file.display());
    Ok(())
}
