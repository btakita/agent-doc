//! Resync — validate sessions.json against live tmux panes.
//!
//! Delegates to `tmux_router::prune()` for the core prune logic.
//! The `run()` function adds verbose output for the standalone `agent-doc resync` command.

use anyhow::Result;

use crate::sessions::{self, Tmux};

/// Quietly prune dead panes and deduplicate entries.
/// Called automatically before route, sync, and claim operations.
/// Returns the number of registry entries removed.
pub fn prune() -> Result<usize> {
    let tmux = Tmux::default_server();
    let registry_path = sessions::registry_path();
    let removed = tmux_router::prune(&registry_path, &tmux)?;
    if removed > 0 {
        eprintln!("resync: pruned {} stale session(s)", removed);
    }
    // Log orphaned windows for debugging (don't kill them)
    log_orphaned_windows(&tmux);
    Ok(removed)
}

/// Log tmux windows named "claude" or "stash" whose panes are all unregistered.
/// This helps diagnose why windows become orphaned without killing them.
fn log_orphaned_windows(tmux: &Tmux) {
    let registry = sessions::load().unwrap_or_default();
    let registered_panes: std::collections::HashSet<&str> = registry
        .values()
        .map(|e| e.pane.as_str())
        .collect();

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
        _ => return,
    };

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let (window_id, window_name, session_name) = (parts[0], parts[1], parts[2]);

        if window_name != "claude" && window_name != "stash" {
            continue;
        }

        let panes = tmux.list_window_panes(window_id).unwrap_or_default();
        if panes.is_empty() {
            continue;
        }

        let all_orphaned = panes.iter().all(|p| !registered_panes.contains(p.as_str()));
        if all_orphaned {
            eprintln!(
                "resync: orphaned {} window {} in session '{}' ({} unregistered panes: {})",
                window_name,
                window_id,
                session_name,
                panes.len(),
                panes.join(", ")
            );
        }
    }
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

