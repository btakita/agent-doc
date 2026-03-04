//! Session registry — maps session UUIDs to tmux pane IDs.
//!
//! Registry lives at `.agent-doc/sessions.json` relative to the project root.
//!
//! The `Tmux` struct and `IsolatedTmux` test helper are re-exported from the
//! `tmux-router` crate. Agent-doc-specific functions (capture_pane, send_key,
//! registry load/save with hardcoded path) remain here.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

// Re-export Tmux types from tmux-router.
pub use tmux_router::Tmux;
#[cfg(test)]
pub use tmux_router::IsolatedTmux;
pub use tmux_router::{RegistryEntry as SessionEntry, Registry as SessionRegistry};

const SESSIONS_FILE: &str = ".agent-doc/sessions.json";

/// Return the path to the sessions registry file.
pub fn registry_path() -> PathBuf {
    PathBuf::from(SESSIONS_FILE)
}

// ---------------------------------------------------------------------------
// Agent-doc-specific tmux operations (not in tmux-router)
// ---------------------------------------------------------------------------

/// Capture the visible content of a tmux pane.
pub fn capture_pane(tmux: &Tmux, pane_id: &str) -> Result<String> {
    let output = tmux
        .cmd()
        .args(["capture-pane", "-t", pane_id, "-p"])
        .output()
        .context("failed to capture tmux pane")?;
    if !output.status.success() {
        anyhow::bail!("tmux capture-pane failed for {}", pane_id);
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Send a single key (not literal text) to a tmux pane.
///
/// Unlike `Tmux::send_keys` (which sends literal text + Enter), this sends
/// a single key name like "Up", "Down", "Enter" — used for TUI navigation.
pub fn send_key(tmux: &Tmux, pane_id: &str, key: &str) -> Result<()> {
    let status = tmux
        .cmd()
        .args(["send-keys", "-t", pane_id, key])
        .status()
        .context("failed to send key to tmux pane")?;
    if !status.success() {
        anyhow::bail!("tmux send-keys failed for pane {}", pane_id);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Free functions — registry operations and env-based checks
// ---------------------------------------------------------------------------

/// Load the session registry from disk. Returns empty map if file doesn't exist.
pub fn load() -> Result<SessionRegistry> {
    let path = PathBuf::from(SESSIONS_FILE);
    if !path.exists() {
        return Ok(SessionRegistry::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", SESSIONS_FILE))?;
    let registry: SessionRegistry = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", SESSIONS_FILE))?;
    Ok(registry)
}

/// Save the session registry to disk.
pub fn save(registry: &SessionRegistry) -> Result<()> {
    let path = PathBuf::from(SESSIONS_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(registry)?;
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", SESSIONS_FILE))?;
    Ok(())
}

/// Register a session → pane mapping.
/// Get the PID of the foreground process in a tmux pane.
pub fn pane_pid(pane_id: &str) -> Result<u32> {
    let output = Command::new("tmux")
        .args(["display-message", "-t", pane_id, "-p", "#{pane_pid}"])
        .output()
        .context("failed to query tmux pane PID")?;
    if !output.status.success() {
        anyhow::bail!("tmux display-message failed for pane {}", pane_id);
    }
    let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    pid_str
        .parse::<u32>()
        .with_context(|| format!("invalid PID '{}' for pane {}", pid_str, pane_id))
}

pub fn register(session_id: &str, pane_id: &str, file: &str) -> Result<()> {
    register_with_pid(session_id, pane_id, file, std::process::id())
}

pub fn register_with_pid(session_id: &str, pane_id: &str, file: &str, pid: u32) -> Result<()> {
    // Query the window ID for this pane
    let window = pane_window(pane_id).unwrap_or_default();
    register_full(session_id, pane_id, file, pid, &window)
}

pub fn register_full(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
) -> Result<()> {
    let mut registry = load()?;
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let started = chrono_now();

    // Enforce single session per pane: remove stale entries pointing to same pane
    let stale_keys: Vec<String> = registry
        .iter()
        .filter(|(k, e)| e.pane == pane_id && k.as_str() != session_id)
        .map(|(k, _)| k.clone())
        .collect();
    for key in &stale_keys {
        eprintln!(
            "[registry] removing stale session {} (was pane {})",
            key, pane_id
        );
        registry.remove(key);
    }

    registry.insert(
        session_id.to_string(),
        SessionEntry {
            pane: pane_id.to_string(),
            pid,
            cwd,
            started,
            file: file.to_string(),
            window: window.to_string(),
        },
    );
    save(&registry)
}

/// Query the tmux window ID for a pane.
pub fn pane_window(pane_id: &str) -> Result<String> {
    let output = Command::new("tmux")
        .args(["display-message", "-t", pane_id, "-p", "#{window_id}"])
        .output()
        .context("failed to query tmux window ID")?;
    if !output.status.success() {
        anyhow::bail!("tmux display-message failed for pane {}", pane_id);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Look up the pane ID for a session.
pub fn lookup(session_id: &str) -> Result<Option<String>> {
    let registry = load()?;
    Ok(registry.get(session_id).map(|e| e.pane.clone()))
}

/// Get the pane ID of the current pane.
/// Tries TMUX_PANE env var first, then falls back to querying tmux
/// for the active pane (works from outside tmux, e.g. IDE processes).
pub fn current_pane() -> Result<String> {
    if let Ok(pane) = std::env::var("TMUX_PANE") {
        return Ok(pane);
    }
    // Fallback: query tmux for the active pane
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#{pane_id}"])
        .output()
        .context("failed to query tmux for active pane — is tmux running?")?;
    if !output.status.success() {
        anyhow::bail!("tmux display-message failed — not inside tmux and no tmux server found");
    }
    let pane = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if pane.is_empty() {
        anyhow::bail!("tmux returned empty pane ID");
    }
    Ok(pane)
}

/// Resolve a pane by positional hint (left, right, top, bottom).
/// Queries `tmux list-panes` for the current window and selects the pane
/// at the requested position based on coordinates.
pub fn pane_by_position(position: &str) -> Result<String> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-F",
            "#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height}",
        ])
        .output()
        .context("failed to query tmux panes")?;
    if !output.status.success() {
        anyhow::bail!("tmux list-panes failed");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut panes: Vec<(String, u32, u32, u32, u32)> = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 5 {
            let id = parts[0].to_string();
            let left: u32 = parts[1].parse().unwrap_or(0);
            let top: u32 = parts[2].parse().unwrap_or(0);
            let width: u32 = parts[3].parse().unwrap_or(0);
            let height: u32 = parts[4].parse().unwrap_or(0);
            panes.push((id, left, top, width, height));
        }
    }
    if panes.is_empty() {
        anyhow::bail!("no panes found in current tmux window");
    }
    if panes.len() == 1 {
        return Ok(panes[0].0.clone());
    }
    let selected = match position {
        "left" => panes.iter().min_by_key(|p| p.1),
        "right" => panes.iter().max_by_key(|p| p.1 + p.3),
        "top" => panes.iter().min_by_key(|p| p.2),
        "bottom" => panes.iter().max_by_key(|p| p.2 + p.4),
        _ => anyhow::bail!(
            "invalid position '{}' — use left, right, top, or bottom",
            position
        ),
    };
    match selected {
        Some(pane) => Ok(pane.0.clone()),
        None => anyhow::bail!("could not resolve pane for position '{}'", position),
    }
}

/// Resolve a pane by positional hint within a specific tmux window.
/// Like `pane_by_position` but scoped to the given window ID (e.g. `@1`).
pub fn pane_by_position_in_window(position: &str, window: &str) -> Result<String> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-t",
            window,
            "-F",
            "#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height}",
        ])
        .output()
        .context("failed to query tmux panes")?;
    if !output.status.success() {
        anyhow::bail!("tmux list-panes failed for window {}", window);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut panes: Vec<(String, u32, u32, u32, u32)> = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 5 {
            let id = parts[0].to_string();
            let left: u32 = parts[1].parse().unwrap_or(0);
            let top: u32 = parts[2].parse().unwrap_or(0);
            let width: u32 = parts[3].parse().unwrap_or(0);
            let height: u32 = parts[4].parse().unwrap_or(0);
            panes.push((id, left, top, width, height));
        }
    }
    if panes.is_empty() {
        anyhow::bail!("no panes found in tmux window {}", window);
    }
    if panes.len() == 1 {
        return Ok(panes[0].0.clone());
    }
    let selected = match position {
        "left" => panes.iter().min_by_key(|p| p.1),
        "right" => panes.iter().max_by_key(|p| p.1 + p.3),
        "top" => panes.iter().min_by_key(|p| p.2),
        "bottom" => panes.iter().max_by_key(|p| p.2 + p.4),
        _ => anyhow::bail!(
            "invalid position '{}' — use left, right, top, or bottom",
            position
        ),
    };
    match selected {
        Some(pane) => Ok(pane.0.clone()),
        None => anyhow::bail!("could not resolve pane for position '{}'", position),
    }
}

/// Check if we're inside tmux.
pub fn in_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// Simple UTC timestamp without pulling in chrono.
fn chrono_now() -> String {
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn registry_roundtrip() {
        let dir = TempDir::new().unwrap();
        let _guard = std::env::set_current_dir(dir.path());

        let mut reg = SessionRegistry::new();
        reg.insert(
            "test-session".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 12345,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                file: "test.md".to_string(),
                window: String::new(),
            },
        );
        save(&reg).unwrap();
        let loaded = load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["test-session"].pane, "%42");
    }

    #[test]
    fn load_empty_returns_empty_map() {
        let dir = TempDir::new().unwrap();
        let _guard = std::env::set_current_dir(dir.path());
        let reg = load().unwrap();
        assert!(reg.is_empty());
    }

    #[test]
    fn pane_alive_returns_false_for_nonexistent() {
        assert!(!Tmux::default_server().pane_alive("%99999"));
    }

    #[test]
    fn registry_multiple_sessions_isolated() {
        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-a".to_string(),
            SessionEntry {
                pane: "%10".to_string(),
                pid: 1000,
                cwd: "/tmp/a".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                file: String::new(),
                window: String::new(),
            },
        );
        reg.insert(
            "session-b".to_string(),
            SessionEntry {
                pane: "%20".to_string(),
                pid: 2000,
                cwd: "/tmp/b".to_string(),
                started: "2026-01-01T00:01:00Z".to_string(),
                file: String::new(),
                window: String::new(),
            },
        );

        let json = serde_json::to_string_pretty(&reg).unwrap();
        let loaded: SessionRegistry = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded["session-a"].pane, "%10");
        assert_eq!(loaded["session-b"].pane, "%20");
        assert_ne!(loaded["session-a"].pane, loaded["session-b"].pane);
        assert_ne!(loaded["session-a"].pid, loaded["session-b"].pid);
    }

    #[test]
    fn registry_overwrite_existing_session() {
        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-x".to_string(),
            SessionEntry {
                pane: "%old".to_string(),
                pid: 100,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                file: String::new(),
                window: String::new(),
            },
        );
        reg.insert(
            "session-x".to_string(),
            SessionEntry {
                pane: "%new".to_string(),
                pid: 200,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:05:00Z".to_string(),
                file: String::new(),
                window: String::new(),
            },
        );

        assert_eq!(reg.len(), 1);
        assert_eq!(reg["session-x"].pane, "%new");
        assert_eq!(reg["session-x"].pid, 200);
    }

    #[test]
    fn prune_removes_dead_panes_from_map() {
        let tmux = Tmux::default_server();
        let mut reg = SessionRegistry::new();
        reg.insert(
            "dead-session-1".to_string(),
            SessionEntry {
                pane: "%99998".to_string(),
                pid: 1,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                file: String::new(),
                window: String::new(),
            },
        );
        reg.insert(
            "dead-session-2".to_string(),
            SessionEntry {
                pane: "%99997".to_string(),
                pid: 2,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                file: String::new(),
                window: String::new(),
            },
        );

        let before = reg.len();
        reg.retain(|_, entry| tmux.pane_alive(&entry.pane));
        let removed = before - reg.len();

        assert_eq!(removed, 2);
        assert!(reg.is_empty());
    }

    // -----------------------------------------------------------------------
    // Tmux isolation tests — use `-L` to create independent tmux servers
    // -----------------------------------------------------------------------

    #[test]
    fn tmux_isolated_server_not_running_initially() {
        let t = IsolatedTmux::new("agent-doc-test-not-running");
        assert!(!t.running());
    }

    #[test]
    fn tmux_create_session_and_verify() {
        let t = IsolatedTmux::new("agent-doc-test-create-session");
        let tmp = TempDir::new().unwrap();

        let pane_id = t.new_session("test-session", tmp.path()).unwrap();
        assert!(!pane_id.is_empty(), "pane_id should not be empty");
        assert!(pane_id.starts_with('%'), "pane_id should start with %");

        assert!(t.running());
        assert!(t.session_exists("test-session"));
        assert!(t.pane_alive(&pane_id));
    }

    #[test]
    fn tmux_session_exists_returns_false_for_missing() {
        let t = IsolatedTmux::new("agent-doc-test-session-missing");
        let tmp = TempDir::new().unwrap();

        t.new_session("existing", tmp.path()).unwrap();

        assert!(t.session_exists("existing"));
        assert!(!t.session_exists("nonexistent"));
    }

    #[test]
    fn tmux_new_window_creates_second_pane() {
        let t = IsolatedTmux::new("agent-doc-test-new-window");
        let tmp = TempDir::new().unwrap();

        let pane1 = t.new_session("test", tmp.path()).unwrap();
        let pane2 = t.new_window("test", tmp.path()).unwrap();

        assert_ne!(pane1, pane2, "two windows should have different pane IDs");
        assert!(t.pane_alive(&pane1));
        assert!(t.pane_alive(&pane2));
    }

    #[test]
    fn tmux_send_keys_to_pane() {
        let t = IsolatedTmux::new("agent-doc-test-send-keys");
        let tmp = TempDir::new().unwrap();

        let pane_id = t.new_session("test", tmp.path()).unwrap();
        t.send_keys(&pane_id, "echo hello").unwrap();
    }

    #[test]
    fn tmux_pane_alive_returns_false_after_kill() {
        let t = IsolatedTmux::new("agent-doc-test-pane-kill");
        let tmp = TempDir::new().unwrap();

        let pane_id = t.new_session("test", tmp.path()).unwrap();
        assert!(t.pane_alive(&pane_id));

        // Create a second window so we can kill the first without killing the session
        let _pane2 = t.new_window("test", tmp.path()).unwrap();

        let _ = t.cmd().args(["kill-pane", "-t", &pane_id]).status();

        assert!(!t.pane_alive(&pane_id));
    }

    #[test]
    fn tmux_auto_start_cascade_no_server() {
        let t = IsolatedTmux::new("agent-doc-test-autostart-no-server");
        let tmp = TempDir::new().unwrap();

        assert!(!t.running());
        let pane_id = t.auto_start("claude", tmp.path()).unwrap();
        assert!(!pane_id.is_empty());
        assert!(t.running());
        assert!(t.session_exists("claude"));
        assert!(t.pane_alive(&pane_id));
    }

    #[test]
    fn tmux_auto_start_cascade_no_session() {
        let t = IsolatedTmux::new("agent-doc-test-autostart-no-session");
        let tmp = TempDir::new().unwrap();

        t.new_session("other", tmp.path()).unwrap();
        assert!(t.running());
        assert!(!t.session_exists("claude"));

        let pane_id = t.auto_start("claude", tmp.path()).unwrap();
        assert!(t.session_exists("claude"));
        assert!(t.pane_alive(&pane_id));
    }

    #[test]
    fn tmux_auto_start_cascade_session_exists() {
        let t = IsolatedTmux::new("agent-doc-test-autostart-exists");
        let tmp = TempDir::new().unwrap();

        let pane1 = t.new_session("claude", tmp.path()).unwrap();

        let pane2 = t.auto_start("claude", tmp.path()).unwrap();
        assert_ne!(pane1, pane2, "should be a different pane (new window)");
        assert!(t.pane_alive(&pane1));
        assert!(t.pane_alive(&pane2));
    }

    #[test]
    #[ignore] // uses set_current_dir which is not thread-safe with other tests
    fn register_full_deduplicates_pane() {
        // When a new session claims a pane, old sessions pointing to the same pane are removed
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        std::fs::create_dir_all(".agent-doc").unwrap();

        // Seed registry with two sessions pointing to the same pane
        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-a".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 100,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                file: "old-file.md".to_string(),
                window: "@1".to_string(),
            },
        );
        reg.insert(
            "session-b".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 100,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:01:00Z".to_string(),
                file: "another-old.md".to_string(),
                window: "@1".to_string(),
            },
        );
        save(&reg).unwrap();

        // Now register session-c with the same pane %42
        register_full("session-c", "%42", "new-file.md", 200, "@1").unwrap();

        let loaded = load().unwrap();
        // Only session-c should remain for pane %42
        assert!(loaded.contains_key("session-c"), "new session should exist");
        assert!(!loaded.contains_key("session-a"), "old session-a should be removed");
        assert!(!loaded.contains_key("session-b"), "old session-b should be removed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["session-c"].file, "new-file.md");
    }
}
