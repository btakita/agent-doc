//! Launch an external terminal with tmux attached to the correct session.
//!
//! This is a **fallback** for users who trigger agent-doc commands from an editor
//! plugin but don't yet have a terminal with tmux open. The command is designed
//! to never create duplicate terminals:
//!
//! 1. Check if frontmatter session exists and is attached → no-op
//! 2. Check if ANY tmux session has live agent-doc panes for this project → attach that
//! 3. Only launch a new terminal if no active session is found
//!
//! Terminal emulator is configured via `[terminal]` in `~/.config/agent-doc/config.toml`.
//! Fallback: `$TERMINAL` env var.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::config;
use crate::sessions::Tmux;

/// Run the terminal command: ensure tmux session exists, launch terminal if needed.
pub fn run(file: &Path, session_name: Option<&str>) -> Result<()> {
    let tmux = Tmux::default();
    let cfg = config::load()?;

    // Step 1: Resolve the target session
    let target = resolve_target_session(&tmux, file, session_name)?;

    match target {
        SessionTarget::Attached(name) => {
            eprintln!("[terminal] session '{}' already has an attached client — skipping", name);
            Ok(())
        }
        SessionTarget::Detached(name) => {
            eprintln!("[terminal] session '{}' exists but is detached — opening terminal to attach", name);
            launch_terminal(&cfg, &name)
        }
        SessionTarget::Create(name) => {
            eprintln!("[terminal] no active session found — creating '{}'", name);
            launch_terminal(&cfg, &name)
        }
    }
}

// ---------------------------------------------------------------------------
// Session resolution
// ---------------------------------------------------------------------------

/// The resolved target session state.
enum SessionTarget {
    /// Session exists and has an attached terminal client — do nothing.
    Attached(String),
    /// Session exists but no terminal is attached — open terminal to attach.
    Detached(String),
    /// No suitable session found — create a new one.
    Create(String),
}

/// Resolve which tmux session to target. Resolution order:
///
/// 1. Explicit `--session` flag
/// 2. `tmux_session` from document frontmatter (validated against live tmux)
/// 3. Scan sessions.json for ANY live session with agent-doc panes for this project
/// 4. Fall back to default session name "0"
///
/// For each candidate, checks if the session exists and is attached.
fn resolve_target_session(
    tmux: &Tmux,
    file: &Path,
    explicit: Option<&str>,
) -> Result<SessionTarget> {
    if !tmux.running() {
        // tmux not running at all — create with the best session name we have
        let name = resolve_session_name(file, explicit)?;
        return Ok(SessionTarget::Create(name));
    }

    // Try explicit --session flag first
    if let Some(name) = explicit {
        return Ok(classify_session(tmux, name));
    }

    // Try frontmatter tmux_session
    if let Some(fm_session) = read_frontmatter_session(file)? {
        if tmux.session_exists(&fm_session) {
            return Ok(classify_session(tmux, &fm_session));
        }
        // Frontmatter session is stale — fall through to registry scan
        eprintln!(
            "[terminal] frontmatter session '{}' does not exist — scanning registry",
            fm_session
        );
    }

    // Scan sessions.json for any live session hosting this project's panes
    if let Some(active_session) = find_active_project_session(tmux)? {
        return Ok(classify_session(tmux, &active_session));
    }

    // No active session found — create with frontmatter name or default
    let name = resolve_session_name(file, None)?;
    Ok(SessionTarget::Create(name))
}

/// Classify an existing tmux session as Attached or Detached.
fn classify_session(tmux: &Tmux, name: &str) -> SessionTarget {
    if is_session_attached(tmux, name) {
        SessionTarget::Attached(name.to_string())
    } else {
        SessionTarget::Detached(name.to_string())
    }
}

/// Read `tmux_session` from document frontmatter, if the file exists.
fn read_frontmatter_session(file: &Path) -> Result<Option<String>> {
    if !file.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    Ok(fm.tmux_session)
}

/// Scan sessions.json for any live pane, and return the tmux session that pane belongs to.
///
/// This handles the "stale frontmatter" case: if the frontmatter says session "foo"
/// but "foo" doesn't exist, we check if there's any OTHER session with live agent-doc
/// panes for this project.
fn find_active_project_session(tmux: &Tmux) -> Result<Option<String>> {
    let registry = crate::sessions::load()?;
    for (_session_id, entry) in &registry {
        if tmux.pane_alive(&entry.pane) {
            // This pane is alive — find which tmux session it belongs to
            if let Some(session_name) = pane_session_name(tmux, &entry.pane) {
                return Ok(Some(session_name));
            }
        }
    }
    Ok(None)
}

/// Get the tmux session name that a pane belongs to.
fn pane_session_name(tmux: &Tmux, pane_id: &str) -> Option<String> {
    let output = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-p",
            "#{session_name}",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Terminal launch
// ---------------------------------------------------------------------------

/// Build the tmux command and launch the configured terminal emulator.
fn launch_terminal(cfg: &config::Config, session_name: &str) -> Result<()> {
    let tmux_command = format!("tmux new-session -A -s {}", shell_escape(session_name));
    let terminal_cmd = resolve_terminal_command(cfg, &tmux_command)?;

    eprintln!("[terminal] launching: {}", terminal_cmd);
    spawn_terminal(&terminal_cmd)
}

/// Resolve the tmux session name from (in order):
/// 1. Explicit --session flag
/// 2. `tmux_session` in document frontmatter
/// 3. Default "0"
fn resolve_session_name(file: &Path, explicit: Option<&str>) -> Result<String> {
    if let Some(name) = explicit {
        return Ok(name.to_string());
    }
    if let Some(name) = read_frontmatter_session(file)? {
        return Ok(name);
    }
    Ok("0".to_string())
}

// ---------------------------------------------------------------------------
// Tmux queries
// ---------------------------------------------------------------------------

/// Check if a tmux session has at least one attached client.
fn is_session_attached(tmux: &Tmux, session: &str) -> bool {
    let output = tmux
        .cmd()
        .args(["list-clients", "-t", session, "-F", "#{client_name}"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            !stdout.trim().is_empty()
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Terminal emulator resolution and launch
// ---------------------------------------------------------------------------

/// Resolve the terminal launch command. Priority:
/// 1. `[terminal] command` in config.toml
/// 2. `$TERMINAL` env var (used as: `$TERMINAL -e {tmux_command}`)
/// 3. Error with instructions
fn resolve_terminal_command(cfg: &config::Config, tmux_command: &str) -> Result<String> {
    if let Some(ref terminal) = cfg.terminal {
        if let Some(ref cmd_template) = terminal.command {
            let resolved = cmd_template.replace("{tmux_command}", tmux_command);
            return Ok(resolved);
        }
    }

    if let Ok(terminal) = std::env::var("TERMINAL") {
        if !terminal.is_empty() {
            return Ok(format!("{} -e {}", terminal, tmux_command));
        }
    }

    anyhow::bail!(
        "No terminal configured.\n\
         \n\
         Add to ~/.config/agent-doc/config.toml:\n\
         \n\
         [terminal]\n\
         command = \"<your-terminal> -- {{tmux_command}}\"\n\
         \n\
         Examples:\n\
         command = \"wezterm start -- {{tmux_command}}\"\n\
         command = \"kitty -- {{tmux_command}}\"\n\
         command = \"alacritty -e {{tmux_command}}\"\n\
         command = \"gnome-terminal -- {{tmux_command}}\"\n\
         \n\
         Or set the $TERMINAL environment variable."
    )
}

/// Spawn the terminal process (detached from current process).
fn spawn_terminal(command: &str) -> Result<()> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        anyhow::bail!("terminal command is empty");
    }

    Command::new(parts[0])
        .args(&parts[1..])
        .spawn()
        .with_context(|| format!("failed to launch terminal: {}", parts[0]))?;

    Ok(())
}

/// Minimal shell escaping for session names.
fn shell_escape(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_escape_simple() {
        assert_eq!(shell_escape("my-session"), "my-session");
        assert_eq!(shell_escape("0"), "0");
    }

    #[test]
    fn shell_escape_special() {
        assert_eq!(shell_escape("my session"), "'my session'");
    }

    #[test]
    fn resolve_terminal_from_config() {
        let mut cfg = config::Config::default();
        cfg.terminal = Some(config::TerminalConfig {
            command: Some("wezterm start -- {tmux_command}".to_string()),
        });
        let result = resolve_terminal_command(&cfg, "tmux new-session -A -s 0").unwrap();
        assert_eq!(result, "wezterm start -- tmux new-session -A -s 0");
    }

    #[test]
    fn resolve_terminal_no_config_no_env() {
        let cfg = config::Config::default();
        // Clear TERMINAL env var for this test
        unsafe { std::env::remove_var("TERMINAL"); }
        let result = resolve_terminal_command(&cfg, "tmux new-session -A -s 0");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No terminal configured"), "got: {}", err);
    }

    #[test]
    fn classify_session_target() {
        // SessionTarget enum is correct at compile time — this tests the structure
        let target = SessionTarget::Create("test".to_string());
        match target {
            SessionTarget::Attached(n) => panic!("expected Create, got Attached({})", n),
            SessionTarget::Detached(n) => panic!("expected Create, got Detached({})", n),
            SessionTarget::Create(n) => assert_eq!(n, "test"),
        }
    }
}
