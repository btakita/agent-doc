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

use crate::sessions::{self, Tmux};

pub fn run() -> Result<()> {
    run_with_tmux(&Tmux::default_server())
}

pub fn run_with_tmux(tmux: &Tmux) -> Result<()> {
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

    // Focus the pane so the user sees immediate visual feedback.
    // Without this, the pane content doesn't refresh until something
    // else triggers a window switch (e.g. changing editor tabs).
    if let Err(e) = tmux.select_pane(&pane_id) {
        eprintln!("[autoclaim] Failed to focus pane {}: {}", pane_id, e);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{IsolatedTmux, SessionEntry, SessionRegistry};
    use tempfile::TempDir;

    /// Helper: set up a temp dir with a sessions.json containing a claim for the given pane.
    fn setup_registry(dir: &std::path::Path, pane_id: &str) {
        let mut reg = SessionRegistry::new();
        reg.insert(
            "test-session-1234".to_string(),
            SessionEntry {
                pane: pane_id.to_string(),
                pid: std::process::id(),
                cwd: dir.to_string_lossy().to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                file: "tasks/test.md".to_string(),
                window: String::new(),
            },
        );
        let sessions_dir = dir.join(".agent-doc");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let sessions_path = sessions_dir.join("sessions.json");
        let content = serde_json::to_string_pretty(&reg).unwrap();
        std::fs::write(sessions_path, content).unwrap();
    }

    #[test]
    #[ignore] // uses set_current_dir which is not thread-safe with other tests
    fn autoclaim_focuses_pane_with_claim() {
        let iso = IsolatedTmux::new("agent-doc-test-autoclaim-focus");
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // Create a tmux session with a pane
        let pane_id = iso.new_session("test", dir.path()).unwrap();

        // Set up registry so the pane has a claim
        setup_registry(dir.path(), &pane_id);

        // Set TMUX_PANE so current_pane() returns our test pane
        unsafe { std::env::set_var("TMUX_PANE", &pane_id) };

        // Create a second pane so we can verify select_pane switches focus
        let pane2 = iso.new_window("test", dir.path()).unwrap();
        // Focus pane2 so autoclaim has to switch back to pane_id
        iso.select_pane(&pane2).unwrap();

        // Run autoclaim — should succeed and call select_pane
        let result = run_with_tmux(&iso);
        assert!(result.is_ok(), "autoclaim should succeed: {:?}", result);

        // Verify select_pane was called: the active pane should now be pane_id, not pane2
        let active = iso.active_pane("test").expect("should have active pane");
        assert_eq!(active, pane_id, "autoclaim should have focused the claimed pane");

        unsafe { std::env::remove_var("TMUX_PANE") };
    }

    #[test]
    #[ignore] // uses set_current_dir which is not thread-safe with other tests
    fn autoclaim_no_claim_skips_focus() {
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // Empty registry — no claims
        let sessions_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(sessions_dir.join("sessions.json"), "{}").unwrap();

        // Set a fake pane ID
        unsafe { std::env::set_var("TMUX_PANE", "%99999") };

        let result = run_with_tmux(&Tmux::default_server());
        assert!(result.is_ok());

        unsafe { std::env::remove_var("TMUX_PANE") };
    }
}
