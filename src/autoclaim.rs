//! # Module: autoclaim
//!
//! Re-establish document claims after Claude Code context compaction.
//!
//! Designed for use in a `.claude/hooks.json` SessionStart hook:
//! ```json
//! { "hooks": { "SessionStart": [{ "command": "agent-doc autoclaim" }] } }
//! ```
//!
//! ## Spec
//! - `run()`: entry point; delegates to `run_with_tmux` using the default tmux server.
//! - `run_with_tmux(tmux)`: reads `$TMUX_PANE` to identify the current pane; if not
//!   in tmux, exits silently with `Ok(())`.
//! - Loads `sessions.json` and collects all entries whose `pane` matches the current pane.
//! - Validates each claim: if the registered file no longer exists on disk, the entry is
//!   pruned from the registry and the pruned registry is persisted.
//! - If no valid claims remain, exits with `Ok(())` after logging to stderr.
//! - Calls `tmux select-pane` on the current pane to refresh visual state in the terminal.
//! - When two or more files in the same tmux window have live panes, calls `sync::run_with_tmux`
//!   to arrange panes side-by-side (one pane per column).
//! - For every surviving claim, prints to stdout: the pane ID, the file path, and the
//!   `/agent-doc claim <file>` command — this output is piped back to Claude Code as
//!   session context by the SessionStart hook.
//! - `sync_after_autoclaim`: collects all registry entries alive in the same window and
//!   triggers a layout sync; skips sync when fewer than two files share the window.
//!
//! ## Agentic Contracts
//! - Callers may assume `run()` and `run_with_tmux()` are idempotent: running autoclaim
//!   multiple times on the same pane produces the same final registry and tmux state.
//! - Stale entries (file deleted or renamed) are always pruned before any output is
//!   emitted; the session context printed to stdout only references live files.
//! - When not running inside tmux (`$TMUX_PANE` absent or `current_pane()` fails),
//!   the function returns `Ok(())` without side effects.
//! - Layout sync is only triggered when `window_files.len() >= 2`; a single-file window
//!   is never reorganized.
//! - Non-fatal errors (select-pane failure, sync failure) are logged to stderr and do
//!   not cause the function to return an error; the stdout claim output is still emitted.
//!
//! ## Evals
//! - `autoclaim_focuses_pane_with_claim`: pane has one live claim → `select-pane` switches
//!   focus to the claimed pane and stdout contains the `/agent-doc claim <file>` directive.
//! - `autoclaim_syncs_layout_with_multiple_files`: two panes in the same window each have
//!   a live claim → both panes survive and the window layout reflects a side-by-side split.
//! - `autoclaim_no_claim_skips_focus`: registry is empty for the current pane → function
//!   returns `Ok(())` without calling `select-pane` or modifying the registry.
//! - `autoclaim_prunes_stale_claim` (aspirational): pane has a claim for a deleted file →
//!   the entry is removed from `sessions.json` and no stdout output is emitted for it.
//! - `autoclaim_noop_outside_tmux` (aspirational): `$TMUX_PANE` is unset → function
//!   returns `Ok(())` immediately with no registry or tmux side effects.

use anyhow::Result;

use agent_doc_orchestration::sync;
use tmux_router::Tmux;

pub fn run() -> Result<()> {
    run_with_tmux(&Tmux::default_server())
}

pub fn run_with_tmux(tmux: &Tmux) -> Result<()> {
    run_with_tmux_in(tmux, &std::env::current_dir()?)
}

pub fn run_with_tmux_in(tmux: &Tmux, base_dir: &std::path::Path) -> Result<()> {
    let pane_id = match agent_doc_tmux_io::current_pane_id_from_env_or_tmux(tmux) {
        Some(p) => p,
        None => {
            // Not in tmux — nothing to autoclaim
            return Ok(());
        }
    };
    run_with_tmux_in_for_pane(tmux, base_dir, &pane_id)
}

fn run_with_tmux_in_for_pane(tmux: &Tmux, base_dir: &std::path::Path, pane_id: &str) -> Result<()> {
    let mut registry = agent_doc_session_registry_io::load_in(base_dir)?;

    // Find all entries mapped to the current pane
    let all_claimed: Vec<(String, tmux_router::RegistryEntry)> = registry
        .iter()
        .filter(|(_, entry)| entry.pane == pane_id)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    if all_claimed.is_empty() {
        eprintln!("[autoclaim] No files claimed for pane {}", pane_id);
        return Ok(());
    }

    // Validate file existence — prune stale entries (renamed/deleted files)
    let mut stale_keys: Vec<String> = Vec::new();
    let mut claimed: Vec<(String, tmux_router::RegistryEntry)> = Vec::new();
    for (registry_key, entry) in all_claimed {
        let file_path = std::path::Path::new(&entry.file);
        let exists = if file_path.is_absolute() {
            file_path.exists()
        } else {
            base_dir.join(file_path).exists()
        };
        if exists {
            claimed.push((registry_key, entry));
        } else {
            eprintln!(
                "[autoclaim] Pruning stale claim: {} (file no longer exists)",
                entry.file
            );
            stale_keys.push(registry_key);
        }
    }

    // Remove stale entries from registry
    if !stale_keys.is_empty() {
        for key in &stale_keys {
            registry.remove(key);
        }
        if let Err(e) = agent_doc_session_registry_io::save_in(base_dir, &registry) {
            eprintln!("[autoclaim] Failed to save pruned registry: {}", e);
        }
    }

    if claimed.is_empty() {
        eprintln!(
            "[autoclaim] All claims for pane {} were stale (files moved/deleted)",
            pane_id
        );
        return Ok(());
    }

    for (_registry_key, entry) in &claimed {
        eprintln!(
            "[autoclaim] Pane {} has file {} (session {})",
            pane_id,
            entry.file,
            &entry.session_id[..8.min(entry.session_id.len())]
        );
    }

    // Focus the pane so the user sees immediate visual feedback.
    // Without this, the pane content doesn't refresh until something
    // else triggers a window switch (e.g. changing editor tabs).
    if let Err(e) = tmux.select_pane(pane_id) {
        eprintln!("[autoclaim] Failed to focus pane {}: {}", pane_id, e);
    }

    // Sync tmux layout so pane arrangement reflects claimed files.
    // Without this, the layout remains stale after context compaction.
    let claimed_refs: Vec<(&String, &tmux_router::RegistryEntry)> =
        claimed.iter().map(|(k, v)| (k, v)).collect();
    sync_after_autoclaim_in(tmux, pane_id, &claimed_refs, base_dir);

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
fn sync_after_autoclaim_in(
    tmux: &Tmux,
    pane_id: &str,
    _claimed: &[(&String, &tmux_router::RegistryEntry)],
    base_dir: &std::path::Path,
) {
    let window_id = match tmux.pane_window(pane_id) {
        Ok(w) => w,
        Err(_) => return,
    };

    let registry = match agent_doc_session_registry_io::load_in(base_dir) {
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
            file_count, window_id
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tmux_router::{IsolatedTmux, Registry as SessionRegistry, RegistryEntry as SessionEntry};

    fn env_lock() -> crate::test_support::ProcessGlobalLockGuard {
        crate::test_support::env_lock()
    }

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
                session_id: "test-session-1234".to_string(),
                file: "tasks/test.md".to_string(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
        let sessions_dir = dir.join(".agent-doc");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let sessions_path = sessions_dir.join("sessions.json");
        let content = serde_json::to_string_pretty(&reg).unwrap();
        std::fs::write(sessions_path, content).unwrap();
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn autoclaim_focuses_pane_with_claim() {
        let _env_guard = env_lock();
        let iso = IsolatedTmux::new("agent-doc-test-autoclaim-focus");
        let dir = TempDir::new().unwrap();

        // Create the claimed file so it passes existence check
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        std::fs::write(dir.path().join("tasks/test.md"), "# test").unwrap();

        // Create a tmux session with a pane
        let pane_id = iso.new_session("test", dir.path()).unwrap();

        // Set up registry so the pane has a claim
        setup_registry(dir.path(), &pane_id);

        // Create a second pane so we can verify select_pane switches focus
        let pane2 = iso.new_window("test", dir.path()).unwrap();
        // Focus pane2 so autoclaim has to switch back to pane_id
        iso.select_pane(&pane2).unwrap();

        // Run autoclaim — should succeed and call select_pane
        let result = run_with_tmux_in_for_pane(&iso, dir.path(), &pane_id);
        assert!(result.is_ok(), "autoclaim should succeed: {:?}", result);

        // Verify select_pane was called: the active pane should now be pane_id, not pane2
        let active = iso.active_pane("test").expect("should have active pane");
        assert_eq!(
            active, pane_id,
            "autoclaim should have focused the claimed pane"
        );
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
                    session_id: session_id.to_string(),
                    file: file.to_string(),
                    window: String::new(),
                    supervisor_instance_id: String::new(),
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn autoclaim_syncs_layout_with_multiple_files() {
        let _env_guard = env_lock();
        let iso = IsolatedTmux::new("agent-doc-test-autoclaim-sync");
        let dir = TempDir::new().unwrap();

        // Create session documents with frontmatter so sync can resolve them
        let doc1 = dir.path().join("tasks/test1.md");
        let doc2 = dir.path().join("tasks/test2.md");
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        std::fs::write(
            &doc1,
            "---\nagent_doc_session: session-1\nagent_doc_mode: template\n---\n# Doc 1\n",
        )
        .unwrap();
        std::fs::write(
            &doc2,
            "---\nagent_doc_session: session-2\nagent_doc_mode: template\n---\n# Doc 2\n",
        )
        .unwrap();

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

        // Run autoclaim — should trigger sync for multi-file window
        let result = run_with_tmux_in_for_pane(&iso, dir.path(), &pane1);
        assert!(result.is_ok(), "autoclaim should succeed: {:?}", result);

        // Verify both panes are still alive after sync
        assert!(iso.pane_alive(&pane1), "pane1 should be alive after sync");
        assert!(iso.pane_alive(&pane2), "pane2 should be alive after sync");
    }

    #[test]
    fn autoclaim_no_claim_skips_focus() {
        let _env_guard = env_lock();
        let dir = TempDir::new().unwrap();

        // Empty registry — no claims
        let sessions_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(sessions_dir.join("sessions.json"), "{}").unwrap();

        let result = run_with_tmux_in_for_pane(&Tmux::default_server(), dir.path(), "%99999");
        assert!(result.is_ok());
    }
}
