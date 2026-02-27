//! Session registry — maps session UUIDs to tmux pane IDs.
//!
//! Registry lives at `.agent-doc/sessions.json` relative to the project root.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const SESSIONS_FILE: &str = ".agent-doc/sessions.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub pane: String,
    pub pid: u32,
    pub cwd: String,
    pub started: String,
    /// Relative path to the session document (empty for legacy entries).
    #[serde(default)]
    pub file: String,
}

pub type SessionRegistry = HashMap<String, SessionEntry>;

/// Tmux server handle — supports isolated `-L` servers for testing.
#[derive(Debug, Clone, Default)]
pub struct Tmux {
    /// If set, uses `-L <socket> -f /dev/null` for an isolated tmux server.
    server_socket: Option<String>,
}

impl Tmux {
    /// Create a Tmux handle that targets the default server (user's tmux).
    pub fn default_server() -> Self {
        Tmux::default()
    }

    /// Build a tmux command with the appropriate `-L` and `-f` flags.
    fn cmd(&self) -> Command {
        let mut cmd = Command::new("tmux");
        if let Some(ref socket) = self.server_socket {
            cmd.args(["-L", socket, "-f", "/dev/null"]);
        }
        cmd
    }

    /// Check if a tmux pane is alive.
    pub fn pane_alive(&self, pane_id: &str) -> bool {
        let output = self
            .cmd()
            .args(["list-panes", "-a", "-F", "#{pane_id}"])
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                stdout.lines().any(|line| line.trim() == pane_id)
            }
            Err(_) => false,
        }
    }

    /// Check if a tmux server is running (has any sessions).
    pub fn running(&self) -> bool {
        self.cmd()
            .args(["has-session"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Check if a named tmux session exists.
    pub fn session_exists(&self, name: &str) -> bool {
        self.cmd()
            .args(["has-session", "-t", name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Create a new tmux session and return the pane ID of the first pane.
    pub fn new_session(&self, name: &str, cwd: &Path) -> Result<String> {
        let output = self
            .cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                name,
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .output()
            .context("failed to create tmux session")?;
        if !output.status.success() {
            anyhow::bail!(
                "tmux new-session failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Create a new window in an existing tmux session and return the pane ID.
    pub fn new_window(&self, session: &str, cwd: &Path) -> Result<String> {
        let output = self
            .cmd()
            .args([
                "new-window",
                "-a",
                "-t",
                session,
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .output()
            .context("failed to create tmux window")?;
        if !output.status.success() {
            anyhow::bail!(
                "tmux new-window failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Send keys to a tmux pane.
    ///
    /// Uses `-l` for literal text (no special key interpretation), then sends
    /// Enter separately. A small delay between text and Enter ensures the TUI
    /// (e.g., Claude Code) processes the input before the submit.
    pub fn send_keys(&self, pane_id: &str, text: &str) -> Result<()> {
        // Send text literally (no tmux key interpretation)
        let status = self
            .cmd()
            .args(["send-keys", "-t", pane_id, "-l", text])
            .status()
            .context("failed to run tmux send-keys (text)")?;
        if !status.success() {
            anyhow::bail!("tmux send-keys failed (text)");
        }

        // Brief pause for TUI to process input
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Send Enter separately
        let status = self
            .cmd()
            .args(["send-keys", "-t", pane_id, "Enter"])
            .status()
            .context("failed to run tmux send-keys (enter)")?;
        if !status.success() {
            anyhow::bail!("tmux send-keys failed (enter)");
        }
        Ok(())
    }

    /// Capture the visible content of a tmux pane.
    pub fn capture_pane(&self, pane_id: &str) -> Result<String> {
        let output = self
            .cmd()
            .args(["capture-pane", "-t", pane_id, "-p"])
            .output()
            .context("failed to run tmux capture-pane")?;
        if !output.status.success() {
            anyhow::bail!(
                "tmux capture-pane failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Send a single key (not literal text) to a tmux pane.
    pub fn send_key(&self, pane_id: &str, key: &str) -> Result<()> {
        let status = self
            .cmd()
            .args(["send-keys", "-t", pane_id, key])
            .status()
            .context("failed to run tmux send-keys")?;
        if !status.success() {
            anyhow::bail!("tmux send-keys failed for key: {}", key);
        }
        Ok(())
    }

    /// Select (focus) a tmux pane.
    pub fn select_pane(&self, pane_id: &str) -> Result<()> {
        // Switch to the window containing the pane first (select-pane alone
        // doesn't change the active window).
        let status = self
            .cmd()
            .args(["select-window", "-t", pane_id])
            .status()
            .context("failed to run tmux select-window")?;
        if !status.success() {
            anyhow::bail!("tmux select-window failed for {}", pane_id);
        }
        let status = self
            .cmd()
            .args(["select-pane", "-t", pane_id])
            .status()
            .context("failed to run tmux select-pane")?;
        if !status.success() {
            anyhow::bail!("tmux select-pane failed for {}", pane_id);
        }
        Ok(())
    }

    /// Get the window ID that contains a pane.
    pub fn pane_window(&self, pane_id: &str) -> Result<String> {
        let output = self
            .cmd()
            .args([
                "display-message",
                "-t",
                pane_id,
                "-p",
                "#{window_id}",
            ])
            .output()
            .context("failed to run tmux display-message")?;
        if !output.status.success() {
            anyhow::bail!(
                "tmux display-message failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Move a pane into another pane's window with the given split direction.
    ///
    /// `split_flag` is `-h` for horizontal (side-by-side) or `-v` for vertical (stacked).
    pub fn join_pane(&self, src_pane: &str, dst_pane: &str, split_flag: &str) -> Result<()> {
        let status = self
            .cmd()
            .args(["join-pane", "-s", src_pane, "-t", dst_pane, split_flag])
            .status()
            .context("failed to run tmux join-pane")?;
        if !status.success() {
            anyhow::bail!("tmux join-pane failed: {} → {}", src_pane, dst_pane);
        }
        Ok(())
    }

    /// List all pane IDs in a given window.
    pub fn list_window_panes(&self, window_id: &str) -> Result<Vec<String>> {
        let output = self
            .cmd()
            .args([
                "list-panes",
                "-t",
                window_id,
                "-F",
                "#{pane_id}",
            ])
            .output()
            .context("failed to run tmux list-panes")?;
        if !output.status.success() {
            anyhow::bail!("tmux list-panes failed for window {}", window_id);
        }
        let panes = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(panes)
    }

    /// Break a pane out of its window into a new window.
    /// Used by `layout` to disassemble a mirror window before rebuilding.
    pub fn break_pane(&self, pane_id: &str) -> Result<()> {
        let status = self
            .cmd()
            .args(["break-pane", "-s", pane_id, "-d"])
            .status()
            .context("failed to run tmux break-pane")?;
        if !status.success() {
            anyhow::bail!("tmux break-pane failed for {}", pane_id);
        }
        Ok(())
    }

    /// Auto-start cascade: create session/window as needed, return pane ID.
    ///
    /// 1. Server not running → create session
    /// 2. Session missing → create session
    /// 3. Session exists → create new window
    pub fn auto_start(&self, session_name: &str, cwd: &Path) -> Result<String> {
        if !self.running() || !self.session_exists(session_name) {
            self.new_session(session_name, cwd)
        } else {
            self.new_window(session_name, cwd)
        }
    }
}

/// Test-only methods on Tmux.
#[cfg(test)]
impl Tmux {
    /// Create a Tmux handle that targets an isolated server via `-L`.
    /// The server is completely independent from the user's tmux.
    pub fn isolated(socket_name: &str) -> Self {
        Tmux {
            server_socket: Some(socket_name.to_string()),
        }
    }

    /// Kill the tmux server (only useful for isolated test servers).
    pub fn kill_server(&self) -> Result<()> {
        self.cmd()
            .args(["kill-server"])
            .status()
            .context("failed to kill tmux server")?;
        Ok(())
    }
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
pub fn register(session_id: &str, pane_id: &str, file: &str) -> Result<()> {
    let mut registry = load()?;
    let pid = std::process::id();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let started = chrono_now();

    registry.insert(
        session_id.to_string(),
        SessionEntry {
            pane: pane_id.to_string(),
            pid,
            cwd,
            started,
            file: file.to_string(),
        },
    );
    save(&registry)
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

    /// RAII guard that kills the isolated tmux server on drop.
    struct IsolatedTmux {
        tmux: Tmux,
    }

    impl IsolatedTmux {
        fn new(name: &str) -> Self {
            IsolatedTmux {
                tmux: Tmux::isolated(name),
            }
        }
    }

    impl Drop for IsolatedTmux {
        fn drop(&mut self) {
            let _ = self.tmux.kill_server();
        }
    }

    impl std::ops::Deref for IsolatedTmux {
        type Target = Tmux;
        fn deref(&self) -> &Tmux {
            &self.tmux
        }
    }

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
}
