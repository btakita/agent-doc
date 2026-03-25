//! # Module: start
//!
//! ## Spec
//! - `run(file)`: validates the file exists, then ensures a session UUID is
//!   present in the YAML frontmatter (generates and writes one if absent).
//! - Resolves `claude_args` from three sources in priority order: frontmatter
//!   `claude_args` field > global config (`~/.config/agent-doc/config.toml`) >
//!   `AGENT_DOC_CLAUDE_ARGS` environment variable.
//! - Requires an active tmux session; bails immediately if not inside tmux.
//! - Registers the session UUID → current tmux pane ID in `sessions.json` so
//!   other subcommands (`route`, `focus`, etc.) can locate the pane.
//! - Runs `claude` as a blocking child process inside a persistent restart loop
//!   so the tmux pane never dies on its own.
//! - On non-zero exit (context exhaustion, crash, etc.): auto-restarts after a
//!   2-second delay using `--continue` to resume the previous conversation.
//! - On clean exit (code 0): prints a prompt to stderr and reads stdin; pressing
//!   Enter restarts fresh (no `--continue`), typing `q` + Enter exits.
//! - Prints the truncated session UUID and pane ID to stderr on registration.
//!
//! ## Agentic Contracts
//! - The file path must exist before `run` is called; callers must not rely on
//!   `run` to create the document.
//! - After `run` returns `Ok(())`, the session has ended cleanly (user chose
//!   to quit); the sessions.json entry is not automatically removed.
//! - Session UUID in frontmatter is idempotent: calling `run` on a file that
//!   already has a UUID does not regenerate or overwrite it.
//! - `claude_args` are prepended to every `claude` invocation inside the loop,
//!   including restarts; they are resolved once at startup and held for the
//!   lifetime of the loop.
//! - The module only writes to the document file (UUID injection) and to
//!   `sessions.json`; it does not touch snapshots, git, or claims.
//! - Must be called from within an active tmux session; violating this contract
//!   returns an immediate `Err`.
//!
//! ## Evals
//! - `start_missing_file`: call `run` with a non-existent path → returns `Err`
//!   containing "file not found".
//! - `start_outside_tmux`: call `run` with a valid file while `TMUX` env var is
//!   unset → returns `Err` containing "not running inside tmux".
//! - `start_generates_uuid`: call `run` on a file with no frontmatter UUID →
//!   UUID is injected into the file and a "Generated session UUID" line appears
//!   on stderr before `claude` is launched.
//! - `start_preserves_existing_uuid`: call `run` on a file that already has a
//!   `session:` key → file content is unchanged (no re-write), no "Generated"
//!   message on stderr.
//! - `start_registers_session`: after setup, `sessions.json` maps the session
//!   UUID to the current tmux pane ID.
//! - `start_claude_args_precedence`: frontmatter `claude_args` overrides config
//!   which overrides `AGENT_DOC_CLAUDE_ARGS`; each layer verified independently.

use anyhow::{Context, Result};
use std::path::Path;

use crate::{config, frontmatter, sessions};

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

    // Resolve claude_args: frontmatter > config > env var
    let (fm, _body) = frontmatter::parse(&updated_content)?;
    let resolved_claude_args = fm
        .claude_args
        .or_else(|| config::load().ok().and_then(|c| c.claude_args))
        .or_else(|| std::env::var("AGENT_DOC_CLAUDE_ARGS").ok());

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
        // Add resolved claude_args before other flags
        if let Some(ref args) = resolved_claude_args {
            for arg in args.split_whitespace() {
                cmd.arg(arg);
            }
        }
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
