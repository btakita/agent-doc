//! `agent-doc claim` — Claim a document for the current tmux pane.
//!
//! Usage: agent-doc claim <file.md>
//!
//! Reads the session UUID from frontmatter, detects the current tmux pane,
//! and registers the mapping in sessions.json. This allows the JetBrains
//! plugin (and `agent-doc route`) to send commands to the correct pane.
//!
//! ## Window resolution spec
//!
//! When `--window` is provided, the claim command resolves the effective window:
//!
//! 1. **Alive window** → use it directly (no change from original behavior)
//! 2. **Dead window** → scan sessions.json for entries with matching project cwd,
//!    check each entry's window for liveness, use the first alive match.
//!    If no alive windows found → fall through to no-window behavior.
//! 3. **No window** → existing behavior (position detection without window scoping)
//!
//! This matches the fallback pattern in `sync.rs` (line 310-322) where a dead
//! `--window` falls back to auto-detected best window.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

use crate::{frontmatter, sessions};

pub fn run(file: &Path, position: Option<&str>, pane: Option<&str>, window: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Validate --window if provided: if dead, fall back to a live project window
    let effective_window: Option<String> = if let Some(win) = window {
        let alive = is_window_alive(win);
        if alive {
            Some(win.to_string())
        } else {
            eprintln!("warning: window {} is dead, searching for alive window", win);
            find_alive_project_window()
        }
    } else {
        None
    };

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
        if let Some(ref win) = effective_window {
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

/// Check if a tmux window is alive by listing its panes.
fn is_window_alive(window: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["list-panes", "-t", window, "-F", "#{pane_id}"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Search sessions.json for a live window belonging to the current project.
///
/// Iterates all entries in the session registry. For each entry whose `cwd`
/// matches the current working directory and has a non-empty `window` field,
/// checks if the window is alive. Returns the first alive match.
fn find_alive_project_window() -> Option<String> {
    let registry = sessions::load().ok()?;
    let cwd = std::env::current_dir().ok()?.to_string_lossy().to_string();
    find_alive_window_in_registry(&registry, &cwd, is_window_alive)
}

/// Pure logic for finding an alive window in a registry.
/// Separated from I/O for testability.
fn find_alive_window_in_registry(
    registry: &sessions::SessionRegistry,
    cwd: &str,
    check_alive: impl Fn(&str) -> bool,
) -> Option<String> {
    for entry in registry.values() {
        if entry.cwd != cwd || entry.window.is_empty() {
            continue;
        }
        if check_alive(&entry.window) {
            eprintln!("found alive window {} from registry", entry.window);
            return Some(entry.window.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{SessionEntry, SessionRegistry};

    fn make_entry(cwd: &str, window: &str) -> SessionEntry {
        SessionEntry {
            pane: "%0".to_string(),
            pid: 1,
            cwd: cwd.to_string(),
            started: "2026-01-01".to_string(),
            file: "test.md".to_string(),
            window: window.to_string(),
        }
    }

    #[test]
    fn find_alive_window_returns_first_alive_match() {
        let mut registry = SessionRegistry::new();
        registry.insert("s1".into(), make_entry("/project", "@1"));
        registry.insert("s2".into(), make_entry("/project", "@2"));
        registry.insert("s3".into(), make_entry("/project", "@3"));

        // @1 dead, @2 alive, @3 alive → returns @2 or @3 (HashMap order)
        // Use deterministic check: only @3 is alive
        let result = find_alive_window_in_registry(&registry, "/project", |w| w == "@3");
        assert_eq!(result, Some("@3".to_string()));
    }

    #[test]
    fn find_alive_window_skips_wrong_cwd() {
        let mut registry = SessionRegistry::new();
        registry.insert("s1".into(), make_entry("/other-project", "@5"));
        registry.insert("s2".into(), make_entry("/project", "@6"));

        let result = find_alive_window_in_registry(&registry, "/project", |w| w == "@5" || w == "@6");
        assert_eq!(result, Some("@6".to_string()));
    }

    #[test]
    fn find_alive_window_skips_empty_window() {
        let mut registry = SessionRegistry::new();
        registry.insert("s1".into(), make_entry("/project", "")); // legacy entry
        registry.insert("s2".into(), make_entry("/project", "@7"));

        let result = find_alive_window_in_registry(&registry, "/project", |_| true);
        assert_eq!(result, Some("@7".to_string()));
    }

    #[test]
    fn find_alive_window_returns_none_when_all_dead() {
        let mut registry = SessionRegistry::new();
        registry.insert("s1".into(), make_entry("/project", "@1"));
        registry.insert("s2".into(), make_entry("/project", "@2"));

        let result = find_alive_window_in_registry(&registry, "/project", |_| false);
        assert_eq!(result, None);
    }

    #[test]
    fn find_alive_window_returns_none_for_empty_registry() {
        let registry = SessionRegistry::new();
        let result = find_alive_window_in_registry(&registry, "/project", |_| true);
        assert_eq!(result, None);
    }

    #[test]
    fn find_alive_window_returns_none_when_no_cwd_match() {
        let mut registry = SessionRegistry::new();
        registry.insert("s1".into(), make_entry("/other", "@1"));

        let result = find_alive_window_in_registry(&registry, "/project", |_| true);
        assert_eq!(result, None);
    }
}
