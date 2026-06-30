//! # Module: session_cmd
//!
//! ## Spec
//! - `agent-doc session` — show the currently configured tmux session (or "auto-detect")
//! - `agent-doc session set <name>` — pin to a specific session: update config.toml,
//!   migrate the agent-doc + stash windows via `tmux move-window`, then close the
//!   superseded old session when it is left a pure agent-doc orphan.
//! - `agent-doc session clear` — return to auto-detect mode (remove tmux_session from config).
//!
//! ## Agentic Contracts
//! - `show()` reads config.toml and prints the configured session (or "auto-detect").
//! - `set()` updates config.toml, moves the agent-doc window from the old session to the
//!   new session, and updates registry window references. If the old session has no
//!   agent-doc window, the command still updates config for future routing. Once the new
//!   session is canonical, the old session is closed via
//!   `resync::close_superseded_session` and its dead panes are pruned from the model —
//!   but only when the old session holds nothing but agent-doc-managed windows with no
//!   live agent; sessions with unmanaged user windows or a running agent are preserved.
//! - `clear()` removes tmux_session from config.toml, returning to auto-detect mode.
//! - Session names are not validated against live tmux sessions — tmux will error if the
//!   target session doesn't exist, and we propagate that error.
//!
//! ## Evals
//! - show_prints_configured_session: config has tmux_session="5" → prints "5"
//! - show_prints_auto_detect_when_unconfigured: no config → prints "(auto-detect)"
//! - set_updates_config: set "3" → config.toml contains tmux_session="3"
//! - clear_removes_config: clear after set "3" → config.toml has no tmux_session

use anyhow::{Context, Result};

use agent_doc_orchestration::resync;
use agent_doc_project_config_io as project_config_io;
use tmux_router::Tmux;

/// Show the currently configured tmux session.
pub fn show() -> Result<()> {
    let session = project_config_io::project_tmux_session();
    match session {
        Some(s) => println!("{}", s),
        None => println!("(auto-detect)"),
    }
    Ok(())
}

/// Clear the configured tmux session, returning to auto-detect mode.
pub fn clear() -> Result<()> {
    project_config_io::clear_project_tmux_session()?;
    Ok(())
}

/// Set the tmux session and migrate all registered panes.
pub fn set(name: &str) -> Result<()> {
    let tmux = Tmux::default_server();
    let old_session = project_config_io::project_tmux_session();

    // Update config first
    project_config_io::update_project_tmux_session(name)?;

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
                    stderr.trim(),
                    name
                );
            }

            // Also try to move stash window if it exists
            let stash_source = format!("{}:stash", old);
            let stash_output = tmux
                .cmd()
                .args([
                    "move-window",
                    "-s",
                    &stash_source,
                    "-t",
                    &format!("{}:", name),
                ])
                .output();
            if let Ok(o) = stash_output
                && o.status.success()
            {
                eprintln!("[session] moved stash window to session '{}'", name);
            }

            // The new session is now canonical; close the superseded old session
            // if it is a pure agent-doc orphan, and prune its now-dead panes from
            // the model (registry). Sessions still holding unmanaged user windows
            // or a live agent are preserved.
            match resync::close_superseded_session(&tmux, old) {
                Ok(true) => match resync::prune() {
                    Ok(n) if n > 0 => eprintln!(
                        "[session] pruned {} dead registry entr{} from the model",
                        n,
                        if n == 1 { "y" } else { "ies" }
                    ),
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("[session] registry prune after close failed: {}", e)
                    }
                },
                Ok(false) => {}
                Err(e) => eprintln!("[session] superseded-session cleanup error: {}", e),
            }
        } else {
            eprintln!("[session] already configured to '{}'", name);
        }
    } else {
        eprintln!(
            "[session] configured tmux_session to '{}' (was unset)",
            name
        );
    }

    Ok(())
}
