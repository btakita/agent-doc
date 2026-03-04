//! Resync — validate sessions.json against live tmux panes.
//!
//! Delegates to `tmux_router::prune()` for the core prune logic.
//! The `run()` function adds verbose output for the standalone `agent-doc resync` command.

use anyhow::Result;

use crate::sessions::{self, Tmux};

/// Quietly prune dead panes and deduplicate entries in sessions.json.
/// Called automatically before route, sync, and claim operations.
/// Returns the number of entries removed.
pub fn prune() -> Result<usize> {
    let tmux = Tmux::default_server();
    let registry_path = sessions::registry_path();
    let removed = tmux_router::prune(&registry_path, &tmux)?;
    if removed > 0 {
        eprintln!("resync: pruned {} stale session(s)", removed);
    }
    Ok(removed)
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
