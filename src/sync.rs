//! `agent-doc sync` — 2D layout sync: mirror a columnar editor layout in tmux.
//!
//! Usage: agent-doc sync --col plan.md,corky.md --col agent-doc.md [--window @1] [--focus plan.md]
//!
//! Each `--col` is a comma-separated list of files. Columns arrange left-to-right.
//! Within each column, files stack top-to-bottom.
//!
//! Delegates the actual layout algorithm to the `tmux-router` crate.
//! This module provides the agent-doc-specific frontmatter resolution layer.

use anyhow::Result;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::sessions::Tmux;
use crate::{frontmatter, resync, sessions};

use tmux_router::FileResolution;

pub fn run(col_args: &[String], window: Option<&str>, focus: Option<&str>) -> Result<()> {
    run_with_tmux(col_args, window, focus, &Tmux::default_server())
}

pub fn run_with_tmux(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    let _ = resync::prune(); // Clean stale entries before layout calculation
    let registry_path = sessions::registry_path();
    let files_needing_session = RefCell::new(Vec::new());
    // Track session_id → file path for post-sync claim updates
    let session_files: RefCell<Vec<(String, PathBuf)>> = RefCell::new(Vec::new());

    let resolve_file = |path: &Path| -> Option<FileResolution> {
        let content = std::fs::read_to_string(path).ok()?;
        let (fm, _) = frontmatter::parse(&content).ok()?;
        match fm.session {
            Some(key) => {
                if fm.tmux_session.is_none() {
                    files_needing_session.borrow_mut().push(path.to_path_buf());
                }
                session_files
                    .borrow_mut()
                    .push((key.clone(), path.to_path_buf()));
                Some(FileResolution::Registered {
                    key,
                    tmux_session: fm.tmux_session,
                })
            }
            None => Some(FileResolution::Unmanaged),
        }
    };

    let result =
        tmux_router::sync(col_args, window, focus, tmux, &registry_path, &resolve_file)?;

    // Write tmux_session back to files that need it
    if let Some(ref session_name) = result.target_session {
        for file in files_needing_session.borrow().iter() {
            if let Ok(content) = std::fs::read_to_string(file)
                && let Ok(updated) = frontmatter::set_tmux_session(&content, session_name)
                && updated != content
            {
                let _ = std::fs::write(file, &updated);
            }
        }
    }

    // Post-sync: register/update claims for all synced files using the
    // file→pane assignments from tmux-router. This ensures autoclaim works
    // for files arranged by sync, even if they were never individually claimed.
    register_synced_files(&session_files.borrow(), &result.file_panes);

    Ok(())
}

/// Register or update registry entries for synced files.
///
/// Uses the file→pane assignments from `SyncResult::file_panes` to create
/// registry entries for files that don't have one yet, and update file paths
/// for existing entries.
fn register_synced_files(
    session_files: &[(String, PathBuf)],
    file_panes: &[(PathBuf, String)],
) {
    if session_files.is_empty() || file_panes.is_empty() {
        return;
    }

    // Build file→pane lookup from sync result
    let pane_lookup: std::collections::HashMap<&Path, &str> = file_panes
        .iter()
        .map(|(p, id)| (p.as_path(), id.as_str()))
        .collect();

    let registry_path = sessions::registry_path();
    let Ok(_lock) = sessions::RegistryLock::acquire(&registry_path) else {
        return;
    };
    let Ok(mut registry) = sessions::load() else {
        return;
    };

    let mut changed = false;
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    for (session_id, file_path) in session_files {
        let file_str = file_path.to_string_lossy().to_string();

        if let Some(entry) = registry.get_mut(session_id) {
            // Existing entry — update file path if needed
            if entry.file != file_str {
                eprintln!(
                    "[sync] updating file path for session {} → {}",
                    &session_id[..8.min(session_id.len())],
                    file_path.display()
                );
                entry.file = file_str;
                changed = true;
            }
            // Also update pane if sync assigned a different one
            if let Some(&pane_id) = pane_lookup.get(file_path.as_path())
                && entry.pane != pane_id
            {
                eprintln!(
                    "[sync] updating pane for {} → {}",
                    file_path.display(),
                    pane_id
                );
                entry.pane = pane_id.to_string();
                changed = true;
            }
        } else if let Some(&pane_id) = pane_lookup.get(file_path.as_path()) {
            // New entry — file was synced but never claimed
            let pane_pid = sessions::pane_pid(pane_id).unwrap_or(std::process::id());
            let window = sessions::pane_window(pane_id).unwrap_or_default();
            eprintln!(
                "[sync] registering {} → pane {} (session {})",
                file_path.display(),
                pane_id,
                &session_id[..8.min(session_id.len())]
            );
            registry.insert(
                session_id.clone(),
                sessions::SessionEntry {
                    pane: pane_id.to_string(),
                    pid: pane_pid,
                    cwd: cwd.clone(),
                    started: String::new(),
                    file: file_str,
                    window,
                },
            );
            changed = true;
        }
    }

    if changed {
        let _ = sessions::save(&registry);
    }
}
