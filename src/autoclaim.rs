//! `agent-doc autoclaim` — Re-establish claims after context compaction.
//!
//! Designed for use in a `.claude/hooks.json` SessionStart hook:
//! ```json
//! { "hooks": { "SessionStart": [{ "command": "agent-doc autoclaim" }] } }
//! ```
//!
//! Looks up the current tmux pane in the session registry and outputs
//! claim information so the new Claude session knows which files are
//! bound to this pane.

use anyhow::Result;

use crate::sessions;

pub fn run() -> Result<()> {
    let pane_id = match sessions::current_pane() {
        Ok(p) => p,
        Err(_) => {
            // Not in tmux — nothing to autoclaim
            return Ok(());
        }
    };

    let registry = sessions::load()?;

    // Find all entries mapped to the current pane
    let claimed: Vec<(&String, &sessions::SessionEntry)> = registry
        .iter()
        .filter(|(_, entry)| entry.pane == pane_id)
        .collect();

    if claimed.is_empty() {
        eprintln!("[autoclaim] No files claimed for pane {}", pane_id);
        return Ok(());
    }

    for (session_id, entry) in &claimed {
        eprintln!(
            "[autoclaim] Pane {} has file {} (session {})",
            pane_id,
            entry.file,
            &session_id[..8.min(session_id.len())]
        );
    }

    // Output claim commands for the new session context.
    // Claude Code's SessionStart hook pipes stdout back as context.
    for (_, entry) in &claimed {
        println!(
            "This pane ({}) has an active agent-doc claim on: {}",
            pane_id, entry.file
        );
        println!(
            "To re-establish the claim, run: /agent-doc claim {}",
            entry.file
        );
    }

    Ok(())
}
