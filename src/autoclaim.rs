//! `agent-doc autoclaim` — Re-establish claims after context compaction.
//!
//! Designed for use in a `.claude/hooks.json` SessionStart hook:
//! ```json
//! { "hooks": { "SessionStart": [{ "command": "agent-doc autoclaim" }] } }
//! ```
//!
//! 1. Looks up the current tmux pane in the session registry
//! 2. Focuses the pane to refresh visual state
//! 3. Syncs tmux layout to match claimed files in the window
//! 4. Outputs claim information so the new Claude session knows which files are
//!    bound to this pane.

use anyhow::Result;

use crate::sessions::{self, Tmux};
use crate::sync;

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

    // Sync tmux layout so pane arrangement reflects claimed files.
    // Without this, the layout remains stale after context compaction.
    sync_after_autoclaim(tmux, &pane_id, &claimed);

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

/// Sync tmux layout after autoclaim, similar to `route::sync_after_claim`.
///
/// Collects all files with panes in the same window and triggers a layout sync.
fn sync_after_autoclaim(
    tmux: &Tmux,
    pane_id: &str,
    _claimed: &[(&String, &sessions::SessionEntry)],
) {
    let window_id = match tmux.pane_window(pane_id) {
        Ok(w) => w,
        Err(_) => return,
    };

    let registry = match sessions::load() {
        Ok(r) => r,
        Err(_) => return,
    };

    let window_files: Vec<String> = registry
        .values()
        .filter(|entry| {
            !entry.pane.is_empty()
                && tmux.pane_alive(&entry.pane)
                && tmux.pane_window(&entry.pane).ok().as_deref() == Some(&window_id)
                && !entry.file.is_empty()
        })
        .map(|entry| entry.file.clone())
        .collect();

    if window_files.len() < 2 {
        return; // Single file — no layout sync needed
    }

    let file_count = window_files.len();
    // Each file as its own column (side-by-side / horizontal layout).
    // Previously this joined all files into a single column, which caused
    // tmux panes to stack vertically (top/bottom) instead of side-by-side.
    let col_args: Vec<String> = window_files;
    if let Err(e) = sync::run_with_tmux(&col_args, Some(&window_id), None, tmux) {
        eprintln!("[autoclaim] warning: post-claim sync failed: {}", e);
    } else {
        eprintln!(
            "[autoclaim] Auto-synced {} files in window {}",
            file_count,
            window_id
        );
    }
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

    /// Helper: set up a multi-file registry for sync tests.
    fn setup_multi_file_registry(
        dir: &std::path::Path,
        entries: &[(&str, &str, &str)], // (session_id, pane_id, file)
    ) {
        let mut reg = SessionRegistry::new();
        for (session_id, pane_id, file) in entries {
            reg.insert(
                session_id.to_string(),
                SessionEntry {
                    pane: pane_id.to_string(),
                    pid: std::process::id(),
                    cwd: dir.to_string_lossy().to_string(),
                    started: "2026-01-01T00:00:00Z".to_string(),
                    file: file.to_string(),
                    window: String::new(),
                },
            );
        }
        let sessions_dir = dir.join(".agent-doc");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let sessions_path = sessions_dir.join("sessions.json");
        let content = serde_json::to_string_pretty(&reg).unwrap();
        std::fs::write(sessions_path, content).unwrap();
    }

    #[test]
    #[ignore] // uses set_current_dir which is not thread-safe with other tests
    fn autoclaim_syncs_layout_with_multiple_files() {
        let iso = IsolatedTmux::new("agent-doc-test-autoclaim-sync");
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // Create session documents with frontmatter so sync can resolve them
        let doc1 = dir.path().join("tasks/test1.md");
        let doc2 = dir.path().join("tasks/test2.md");
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        std::fs::write(&doc1, "---\nagent_doc_session: session-1\nagent_doc_mode: template\n---\n# Doc 1\n").unwrap();
        std::fs::write(&doc2, "---\nagent_doc_session: session-2\nagent_doc_mode: template\n---\n# Doc 2\n").unwrap();

        // Create tmux session with two panes in the same window
        let pane1 = iso.new_session("test", dir.path()).unwrap();
        let pane2 = iso.new_window("test", dir.path()).unwrap();
        iso.join_pane(&pane2, &pane1, "-dh").unwrap();

        // Register both files in the same window
        setup_multi_file_registry(
            dir.path(),
            &[
                ("session-1", &pane1, "tasks/test1.md"),
                ("session-2", &pane2, "tasks/test2.md"),
            ],
        );

        // Set TMUX_PANE to pane1
        unsafe { std::env::set_var("TMUX_PANE", &pane1) };

        // Run autoclaim — should trigger sync for multi-file window
        let result = run_with_tmux(&iso);
        assert!(result.is_ok(), "autoclaim should succeed: {:?}", result);

        // Verify both panes are still alive after sync
        assert!(iso.pane_alive(&pane1), "pane1 should be alive after sync");
        assert!(iso.pane_alive(&pane2), "pane2 should be alive after sync");

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
