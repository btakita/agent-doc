//! # Module: session_cmd
//!
//! ## Spec
//! - `agent-doc session` — show the currently configured tmux session
//! - `agent-doc session set <name>` — update config.toml and migrate all registered
//!   panes to the new session by moving the agent-doc window via `tmux move-window`.
//!
//! ## Agentic Contracts
//! - `show()` reads config.toml and prints the configured session (or "(none)").
//! - `set()` updates config.toml, moves the agent-doc window from the old session to the
//!   new session, and updates registry window references. If the old session has no
//!   agent-doc window, the command still updates config for future routing.
//! - Session names are not validated against live tmux sessions — tmux will error if the
//!   target session doesn't exist, and we propagate that error.
//!
//! ## Evals
//! - show_prints_configured_session: config has tmux_session="5" → prints "5"
//! - show_prints_none_when_unconfigured: no config → prints "(none)"
//! - set_updates_config: set "3" → config.toml contains tmux_session="3"

use anyhow::{Context, Result};

use crate::{config, sessions::Tmux};

/// Show the currently configured tmux session.
pub fn show() -> Result<()> {
    let session = config::project_tmux_session();
    match session {
        Some(s) => println!("{}", s),
        None => println!("(none)"),
    }
    Ok(())
}

/// Set the tmux session and migrate all registered panes.
pub fn set(name: &str) -> Result<()> {
    let tmux = Tmux::default_server();
    let old_session = config::project_tmux_session();

    // Update config first
    config::update_project_tmux_session(name)?;

    // Try to move the agent-doc window from old session to new session
    if let Some(ref old) = old_session {
        if old != name {
            let source = format!("{}:agent-doc", old);
            let output = tmux
                .cmd()
                .args(["move-window", "-s", &source, "-t", &format!("{}:", name)])
                .output()
                .context("failed to run tmux move-window")?;

            if output.status.success() {
                eprintln!(
                    "[session] moved agent-doc window from session '{}' to '{}'",
                    old, name
                );
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!(
                    "[session] could not move agent-doc window: {} (config updated, panes will route to '{}' on next claim/route)",
                    stderr.trim(), name
                );
            }

            // Also try to move stash window if it exists
            let stash_source = format!("{}:stash", old);
            let stash_output = tmux
                .cmd()
                .args(["move-window", "-s", &stash_source, "-t", &format!("{}:", name)])
                .output();
            if let Ok(o) = stash_output
                && o.status.success()
            {
                eprintln!("[session] moved stash window to session '{}'", name);
            }
        } else {
            eprintln!("[session] already configured to '{}'", name);
        }
    } else {
        eprintln!("[session] configured tmux_session to '{}' (was unset)", name);
    }

    Ok(())
}
