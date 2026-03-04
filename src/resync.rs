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

    // Deduplicate: if multiple sessions point to the same pane, keep the most recent
    let mut pane_to_sessions: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for (session_id, entry) in &registry {
        pane_to_sessions
            .entry(entry.pane.clone())
            .or_default()
            .push((session_id.clone(), entry.started.clone()));
    }
    let mut dedup_removed = 0usize;
    for (pane, mut sessions) in pane_to_sessions {
        if sessions.len() <= 1 {
            continue;
        }
        // Sort by started timestamp descending — keep the newest
        sessions.sort_by(|a, b| b.1.cmp(&a.1));
        let keeper = &sessions[0].0;
        for (session_id, _) in &sessions[1..] {
            let label = registry
                .get(session_id)
                .map(|e| {
                    if e.file.is_empty() {
                        session_id.as_str()
                    } else {
                        e.file.as_str()
                    }
                })
                .unwrap_or(session_id.as_str());
            eprintln!(
                "  dedup: removing {} (pane {} shared with {})",
                label, pane, keeper
            );
            registry.remove(session_id);
            dedup_removed += 1;
        }
    }
    if dedup_removed > 0 {
        eprintln!("Deduplicated {} session(s) sharing the same pane.", dedup_removed);
    }

    if removed > 0 || dedup_removed > 0 {
        sessions::save(&registry)?;
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
