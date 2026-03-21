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
use crate::{frontmatter, resync, route, sessions};

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

    // Pre-sync: auto-start Claude sessions for files that have session UUIDs
    // but no alive panes. This ensures sync has panes to arrange.
    // We parse file paths from col_args directly (before tmux_router::sync calls resolve_file).
    {
        // Parse file paths from col_args (each arg is "file1.md,file2.md")
        let all_files: Vec<PathBuf> = col_args
            .iter()
            .flat_map(|arg| arg.split(','))
            .map(|s| PathBuf::from(s.trim()))
            .collect();
        for file_path in &all_files {
            if !file_path.exists() {
                continue;
            }
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let (fm, _) = match frontmatter::parse(&content) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let session_id = match fm.session {
                Some(ref id) => id.clone(),
                None => continue,
            };

            let registered_pane = sessions::lookup(&session_id)
                .ok()
                .flatten();
            let has_alive_pane = registered_pane
                .as_ref()
                .map(|pane| tmux.pane_alive(pane))
                .unwrap_or(false);

            if has_alive_pane {
                // Pane is alive — no auto-start needed.
                // Re-register with correct window if the sync will move it.
                continue;
            }

            // No alive pane in registry. Before auto-starting, check if any
            // alive pane in the target session is already running agent-doc
            // for this file (registry may have been pruned or stale).
            // This prevents creating duplicate panes.
            let file_str = file_path.to_string_lossy().to_string();
            if let Some(existing) = find_alive_pane_for_file(tmux, &file_str) {
                eprintln!(
                    "[sync] found alive pane {} for {} (re-registering)",
                    existing, file_path.display()
                );
                if let Err(e) = sessions::register(&session_id, &existing, &file_str) {
                    eprintln!(
                        "[sync] warning: re-register failed for {}: {}",
                        file_path.display(), e
                    );
                }
                continue;
            }

            eprintln!(
                "[sync] auto-starting session for {} (no alive pane)",
                file_path.display()
            );
            if let Err(e) = route::auto_start(tmux, file_path, &session_id, &file_str) {
                eprintln!(
                    "[sync] warning: auto-start failed for {}: {}",
                    file_path.display(),
                    e
                );
            }
        }
    }

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

/// Find an alive tmux pane that is running `agent-doc start <file>`.
///
/// Scans all tmux panes for one whose command line matches the file path.
/// This catches panes that were pruned from the registry but are still alive.
///
/// Uses `ps -p <pid> -o command=` for cross-platform compatibility (Linux + macOS).
fn find_alive_pane_for_file(tmux: &Tmux, file_path: &str) -> Option<String> {
    let output = tmux.cmd()
        .args(["list-panes", "-a", "-F", "#{pane_id} #{pane_pid}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() != 2 {
            continue;
        }
        let pane_id = parts[0];
        let pid_str = parts[1];

        // Check the pane's process and its children for agent-doc + file_path
        if pid_has_agent_doc_for_file(pid_str, file_path) {
            eprintln!(
                "[sync] found alive agent-doc pane {} (pid {}) for {}",
                pane_id, pid_str, file_path
            );
            return Some(pane_id.to_string());
        }

        // Check child processes (pane PID is usually a shell)
        if let Ok(children) = std::process::Command::new("pgrep")
            .args(["-P", pid_str])
            .output()
        {
            for child_pid in String::from_utf8_lossy(&children.stdout).lines() {
                let child_pid = child_pid.trim();
                if !child_pid.is_empty() && pid_has_agent_doc_for_file(child_pid, file_path) {
                    eprintln!(
                        "[sync] found alive agent-doc child (pid {}) in pane {} for {}",
                        child_pid, pane_id, file_path
                    );
                    return Some(pane_id.to_string());
                }
            }
        }
    }
    None
}

/// Check if a process (by PID) is running agent-doc for a specific file.
///
/// Uses `ps -p <pid> -o command=` which works on both Linux and macOS.
fn pid_has_agent_doc_for_file(pid: &str, file_path: &str) -> bool {
    let output = match std::process::Command::new("ps")
        .args(["-p", pid, "-o", "command="])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let cmdline = String::from_utf8_lossy(&output.stdout);
    let has_agent = cmdline.contains("agent-doc") || cmdline.contains("claude");
    let has_file = cmdline.contains(file_path);
    has_agent && has_file
}
