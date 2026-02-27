//! Resync — validate sessions.json against live tmux panes.
//!
//! Removes stale entries whose tmux panes no longer exist,
//! and reports the current state of the registry.

use anyhow::Result;

use crate::sessions::{self, Tmux};

pub fn run() -> Result<()> {
    let tmux = Tmux::default_server();
    let mut registry = sessions::load()?;
    let before = registry.len();

    // Partition into alive and dead entries.
    let mut dead: Vec<(String, String, String)> = Vec::new();
    registry.retain(|session_id, entry| {
        let alive = tmux.pane_alive(&entry.pane);
        if !alive {
            dead.push((
                session_id.clone(),
                entry.pane.clone(),
                entry.file.clone(),
            ));
        }
        alive
    });

    let removed = before - registry.len();

    if removed > 0 {
        sessions::save(&registry)?;
        eprintln!("Removed {} stale session(s):", removed);
        for (session_id, pane, file) in &dead {
            let label = if file.is_empty() {
                session_id.as_str()
            } else {
                file.as_str()
            };
            eprintln!("  {} (pane {} dead)", label, pane);
        }
    } else {
        eprintln!("All {} session(s) have live panes.", registry.len());
    }

    // Show current state.
    if !registry.is_empty() {
        eprintln!("\nActive sessions:");
        for (session_id, entry) in &registry {
            let label = if entry.file.is_empty() {
                session_id.as_str()
            } else {
                entry.file.as_str()
            };
            eprintln!("  {} → pane {}", label, entry.pane);
        }
    }

    Ok(())
}
