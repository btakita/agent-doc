//! # Module: terminal
//!
//! ## Spec
//! - `run(file, session_name)` ensures a tmux session exists and opens a terminal
//!   emulator attached to it. Designed as a fallback for editor plugin users who
//!   have no terminal open yet.
//! - Session resolution order:
//!   1. Explicit `--session` flag, if provided.
//!   2. Scan `sessions.json` for any live pane belonging to this project; reuse its session.
//!   3. Default session name `"0"`.
//! - `tmux_session` frontmatter field is deprecated and never consulted during resolution.
//! - If the resolved session already has an attached client → no-op (no duplicate terminals).
//! - If the session exists but is detached → open a terminal to re-attach.
//! - If no session found → create a new one via `tmux new-session -A -s <name>`.
//! - Terminal command resolution order:
//!   1. `[terminal] command` in `~/.config/agent-doc/config.toml` (supports `{tmux_command}` placeholder).
//!   2. `$TERMINAL` env var (appended with `-e {tmux_command}`).
//!   3. Error with setup instructions.
//! - Session names containing characters outside `[a-zA-Z0-9_-]` are single-quote-escaped
//!   before being passed to the shell.
//!
//! ## Agentic Contracts
//! - `run()` is the sole public entry point.
//! - Never creates duplicate terminals; attaches to existing sessions when available.
//! - Terminal process is spawned detached (fire-and-forget); `run()` returns immediately.
//! - Resolution is idempotent: calling `run()` twice for an already-attached session is a no-op.
//!
//! ## Evals
//! - shell_escape_simple: alphanumeric names → returned unchanged
//! - shell_escape_special: name with space → single-quoted output
//! - resolve_terminal_from_config: config has `command` template → `{tmux_command}` replaced correctly
//! - resolve_terminal_no_config_no_env: no config, no `$TERMINAL` → error containing "No terminal configured"
//! - classify_session_target: `SessionTarget::Create` variant matches correctly
//! - resolve_session_name_defaults_to_zero: file with `tmux_session: custom` frontmatter, no flag → returns "0"
//! - resolve_session_name_uses_explicit_flag: explicit flag "correct" → returns "correct" ignoring frontmatter

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
            eprintln!(
                "[terminal] session '{}' already has an attached client — skipping",
                name
            );
            Ok(())
        }
        SessionTarget::Detached(name) => {
            eprintln!(
                "[terminal] session '{}' exists but is detached — opening terminal to attach",
                name
            );
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
/// 2. Scan sessions.json for ANY live session with agent-doc panes for this project
/// 3. Fall back to default session name "0"
///
/// Note: `tmux_session` frontmatter is deprecated and no longer consulted.
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

    // Scan sessions.json for any live session hosting this project's panes
    if let Some(active_session) = find_active_project_session(tmux)? {
        eprintln!(
            "[terminal] targeting session '{}' (from registry scan)",
            active_session
        );
        return Ok(classify_session(tmux, &active_session));
    }

    // No active session found — create with default name
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

/// Scan sessions.json for any live pane, and return the tmux session that pane belongs to.
///
/// This handles the "stale frontmatter" case: if the frontmatter says session "foo"
/// but "foo" doesn't exist, we check if there's any OTHER session with live agent-doc
/// panes for this project.
fn find_active_project_session(tmux: &Tmux) -> Result<Option<String>> {
    let registry = crate::sessions::load()?;
    for entry in registry.values() {
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
    tmux.pane_session(pane_id)
        .ok()
        .filter(|name| !name.is_empty())
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
/// 2. Default "0"
///
/// Note: `tmux_session` frontmatter is deprecated and no longer consulted.
fn resolve_session_name(_file: &Path, explicit: Option<&str>) -> Result<String> {
    if let Some(name) = explicit {
        return Ok(name.to_string());
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
    if let Some(ref terminal) = cfg.terminal
        && let Some(ref cmd_template) = terminal.command
    {
        let resolved = cmd_template.replace("{tmux_command}", tmux_command);
        return Ok(resolved);
    }

    if let Ok(terminal) = std::env::var("TERMINAL")
        && !terminal.is_empty()
    {
        return Ok(format!("{} -e {}", terminal, tmux_command));
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

    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
        _lock: crate::test_support::ProcessGlobalLockGuard,
    }

    impl EnvGuard {
        fn unset(key: &'static str) -> Self {
            let lock = crate::test_support::env_lock();
            let prior = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
            Self {
                key,
                prior,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

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
        let cfg = config::Config {
            terminal: Some(config::TerminalConfig {
                command: Some("wezterm start -- {tmux_command}".to_string()),
            }),
            ..Default::default()
        };
        let result = resolve_terminal_command(&cfg, "tmux new-session -A -s 0").unwrap();
        assert_eq!(result, "wezterm start -- tmux new-session -A -s 0");
    }

    #[test]
    fn resolve_terminal_no_config_no_env() {
        let cfg = config::Config::default();
        let _terminal = EnvGuard::unset("TERMINAL");
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

    /// Regression: resolve_session_name must default to "0", never read frontmatter.
    #[test]
    fn resolve_session_name_defaults_to_zero() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("test.md");
        // File with tmux_session set to a non-default value
        std::fs::write(&doc, "---\ntmux_session: custom-session\n---\n").unwrap();

        // With no explicit session, should return "0" regardless of frontmatter
        let name = resolve_session_name(&doc, None).unwrap();
        assert_eq!(
            name, "0",
            "should default to '0', not read frontmatter tmux_session"
        );
    }

    /// Regression: resolve_session_name with explicit flag should use it directly.
    #[test]
    fn resolve_session_name_uses_explicit_flag() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("test.md");
        std::fs::write(&doc, "---\ntmux_session: wrong-session\n---\n").unwrap();

        let name = resolve_session_name(&doc, Some("correct")).unwrap();
        assert_eq!(name, "correct", "explicit flag should take precedence");
    }
}
