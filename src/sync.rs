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

    // Post-sync: ensure each synced file's registry entry has the file path set.
    // This makes `autoclaim` work for files arranged by sync (not just individually claimed).
    update_registry_file_paths(&session_files.borrow());

    Ok(())
}

/// Update registry entries to include file paths for synced documents.
/// Reads the registry once, patches entries that have empty `file` fields,
/// then writes back if any changes were made.
fn update_registry_file_paths(session_files: &[(String, PathBuf)]) {
    if session_files.is_empty() {
        return;
    }
    let registry_path = sessions::registry_path();
    let Ok(_lock) = sessions::RegistryLock::acquire(&registry_path) else {
        return;
    };
    let Ok(mut registry) = sessions::load() else {
        return;
    };

    let mut changed = false;
    for (session_id, file_path) in session_files {
        if let Some(entry) = registry.get_mut(session_id) {
            let file_str = file_path.to_string_lossy().to_string();
            if entry.file != file_str {
                eprintln!(
                    "[sync] updating file path for session {} → {}",
                    &session_id[..8.min(session_id.len())],
                    file_path.display()
                );
                entry.file = file_str;
                changed = true;
            }
        }
    }

    if changed {
        let _ = sessions::save(&registry);
    }
}
