//! Resync — validate sessions.json against live tmux panes.
//!
//! Delegates to `tmux_router::prune()` for the core prune logic.
//! The `run()` function adds verbose output for the standalone `agent-doc resync` command.
//! After pruning the registry, also cleans up orphaned tmux windows (claude/stash).

use anyhow::Result;
use std::collections::HashSet;

use crate::sessions::{self, Tmux};

/// Quietly prune dead panes, deduplicate entries, and clean orphaned windows.
/// Called automatically before route, sync, and claim operations.
/// Returns the number of registry entries removed.
pub fn prune() -> Result<usize> {
    let tmux = Tmux::default_server();
    let registry_path = sessions::registry_path();
    let removed = tmux_router::prune(&registry_path, &tmux)?;
    if removed > 0 {
        eprintln!("resync: pruned {} stale session(s)", removed);
    }
    // Also clean orphaned windows (best-effort, don't fail on errors)
    let windows_killed = prune_orphaned_windows(&tmux);
    if windows_killed > 0 {
        eprintln!("resync: killed {} orphaned window(s)", windows_killed);
    }
    Ok(removed)
}

/// Kill tmux windows named "claude" or "stash" whose panes are all unregistered.
/// Returns the number of windows killed.
fn prune_orphaned_windows(tmux: &Tmux) -> usize {
    prune_orphaned_windows_inner(tmux, &sessions::load().unwrap_or_default())
}

/// Testable inner function.
fn prune_orphaned_windows_inner(
    tmux: &Tmux,
    registry: &sessions::SessionRegistry,
) -> usize {
    // Collect all registered pane IDs
    let registered_panes: HashSet<&str> = registry
        .values()
        .map(|e| e.pane.as_str())
        .collect();

    // List all windows
    let output = tmux
        .cmd()
        .args([
            "list-windows",
            "-a",
            "-F",
            "#{window_id}\t#{window_name}\t#{session_name}",
        ])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return 0,
    };

    let mut killed = 0;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let (window_id, window_name, _session_name) = (parts[0], parts[1], parts[2]);

        // Only clean "claude" and "stash" windows
        if window_name != "claude" && window_name != "stash" {
            continue;
        }

        // Get panes in this window
        let panes = tmux.list_window_panes(window_id).unwrap_or_default();
        if panes.is_empty() {
            continue;
        }

        // Check if ALL panes in this window are unregistered
        let all_orphaned = panes.iter().all(|p| !registered_panes.contains(p.as_str()));
        if !all_orphaned {
            continue;
        }

        // Kill the orphaned window
        eprintln!(
            "resync: killing orphaned window {} ({}, {} panes)",
            window_id, window_name, panes.len()
        );
        if let Err(e) = tmux.raw_cmd(&["kill-window", "-t", window_id]) {
            eprintln!("warning: failed to kill window {}: {}", window_id, e);
        } else {
            killed += 1;
        }
    }

    killed
}

/// Verbose resync for the standalone `agent-doc resync` command.
pub fn run() -> Result<()> {
    let tmux = Tmux::default_server();
    let registry_path = sessions::registry_path();

    // Show what's being removed (verbose)
    let registry_before = sessions::load()?;
    let before = registry_before.len();

    let removed = tmux_router::prune(&registry_path, &tmux)?;

    if removed > 0 {
        // Show which entries were removed by diffing before/after
        let registry_after = sessions::load()?;
        eprintln!("Removed {} stale session(s):", removed);
        for (key, entry) in &registry_before {
            if !registry_after.contains_key(key) {
                let label = if entry.file.is_empty() {
                    key.as_str()
                } else {
                    entry.file.as_str()
                };
                eprintln!("  {} (pane {} removed)", label, entry.pane);
            }
        }
    } else {
        eprintln!("All {} session(s) have live panes.", before);
    }

    // Clean orphaned windows
    let windows_killed = prune_orphaned_windows(&tmux);
    if windows_killed > 0 {
        eprintln!("Killed {} orphaned window(s)", windows_killed);
    }

    // Show current state
    let registry = sessions::load()?;
    if !registry.is_empty() {
        eprintln!("\nActive sessions:");
        for (key, entry) in &registry {
            let label = if entry.file.is_empty() {
                key.as_str()
            } else {
                entry.file.as_str()
            };
            eprintln!("  {} → pane {}", label, entry.pane);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::SessionEntry;
    use tmux_router::IsolatedTmux;

    fn make_entry(pane: &str) -> SessionEntry {
        SessionEntry {
            pane: pane.to_string(),
            pid: 1,
            cwd: "/tmp".to_string(),
            started: "2026-01-01".to_string(),
            file: "test.md".to_string(),
            window: String::new(),
        }
    }

    #[test]
    fn prune_kills_orphaned_claude_window() {
        let iso = IsolatedTmux::new("test-prune-orphan");
        let tmux: &Tmux = &iso;

        // Create a session with a "claude" window
        let pane1 = tmux.new_session("test", std::path::Path::new("/tmp")).unwrap();
        let pane2_output = tmux.cmd()
            .args(["new-window", "-t", "test", "-n", "claude", "-P", "-F", "#{pane_id}"])
            .output().unwrap();
        let _pane2 = String::from_utf8_lossy(&pane2_output.stdout).trim().to_string();

        // Registry has pane1 but NOT pane2
        let mut registry = sessions::SessionRegistry::new();
        registry.insert("s1".into(), make_entry(&pane1));

        // The claude window (pane2) should be killed since its pane is unregistered
        let killed = prune_orphaned_windows_inner(tmux, &registry);
        assert_eq!(killed, 1, "should kill the orphaned claude window");
    }

    #[test]
    fn prune_preserves_window_with_registered_pane() {
        let iso = IsolatedTmux::new("test-prune-preserve");
        let tmux: &Tmux = &iso;

        // Create session with a "claude" window
        let _pane1 = tmux.new_session("test", std::path::Path::new("/tmp")).unwrap();
        let pane2_output = tmux.cmd()
            .args(["new-window", "-t", "test", "-n", "claude", "-P", "-F", "#{pane_id}"])
            .output().unwrap();
        let pane2 = String::from_utf8_lossy(&pane2_output.stdout).trim().to_string();

        // Registry has pane2 (the claude window's pane)
        let mut registry = sessions::SessionRegistry::new();
        registry.insert("s1".into(), make_entry(&pane2));

        // Should NOT kill the window because its pane is registered
        let killed = prune_orphaned_windows_inner(tmux, &registry);
        assert_eq!(killed, 0, "should NOT kill window with registered pane");
    }

    #[test]
    fn prune_ignores_non_claude_windows() {
        let iso = IsolatedTmux::new("test-prune-ignore");
        let tmux: &Tmux = &iso;

        // Create session with a "zsh" window (user shell)
        let _pane1 = tmux.new_session("test", std::path::Path::new("/tmp")).unwrap();
        let _pane2_output = tmux.cmd()
            .args(["new-window", "-t", "test", "-n", "zsh", "-P", "-F", "#{pane_id}"])
            .output().unwrap();

        // Empty registry — no panes registered
        let registry = sessions::SessionRegistry::new();

        // Should NOT kill zsh windows even though their panes are unregistered
        let killed = prune_orphaned_windows_inner(tmux, &registry);
        assert_eq!(killed, 0, "should NOT kill non-claude/non-stash windows");
    }

    #[test]
    fn prune_kills_orphaned_stash_window() {
        let iso = IsolatedTmux::new("test-prune-stash");
        let tmux: &Tmux = &iso;

        let pane1 = tmux.new_session("test", std::path::Path::new("/tmp")).unwrap();
        let _stash_output = tmux.cmd()
            .args(["new-window", "-t", "test", "-n", "stash", "-P", "-F", "#{pane_id}"])
            .output().unwrap();

        // Registry has pane1 but not the stash pane
        let mut registry = sessions::SessionRegistry::new();
        registry.insert("s1".into(), make_entry(&pane1));

        let killed = prune_orphaned_windows_inner(tmux, &registry);
        assert_eq!(killed, 1, "should kill orphaned stash window");
    }
}
