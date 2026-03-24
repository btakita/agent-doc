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
    run_with_options(col_args, window, focus, true, &Tmux::default_server())
}

/// Run sync without auto-starting sessions. Used when called from route
/// (route already handled the target file — auto-start would create duplicates).
pub fn run_layout_only(col_args: &[String], window: Option<&str>, focus: Option<&str>) -> Result<()> {
    run_with_options(col_args, window, focus, false, &Tmux::default_server())
}

pub fn run_with_tmux(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    run_with_options(col_args, window, focus, true, tmux)
}

/// Normalize the tmux layout by consolidating stash windows and ensuring
/// the agent-doc window exists.
///
/// Phase 1: Stash consolidation — merge all `stash-*` and extra `stash` windows
/// into a single primary stash window.
///
/// Phase 2: Ensure the target window exists — if missing, break a registered
/// alive pane out of the stash to recreate it.
pub fn repair_layout(tmux: &Tmux, session_name: &str, target_window_name: &str) -> Result<()> {
    // List all windows in the session: window_id, window_name, pane count
    let output = tmux.raw_cmd(&[
        "list-windows",
        "-t",
        &format!("{}:", session_name),
        "-F",
        "#{window_id} #{window_name} #{window_panes}",
    ]);
    let window_list = match output {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[repair] failed to list windows for session {}: {}", session_name, e);
            return Ok(());
        }
    };

    // Parse windows into (id, name, pane_count)
    struct WinInfo {
        id: String,
        name: String,
        _pane_count: usize,
    }
    let windows: Vec<WinInfo> = window_list
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, ' ');
            let id = parts.next()?.to_string();
            let name = parts.next()?.to_string();
            let pane_count: usize = parts.next()?.parse().ok()?;
            Some(WinInfo { id, name, _pane_count: pane_count })
        })
        .collect();

    // ── Fast path: if layout is already correct, skip repair ──
    let has_target = windows.iter().any(|w| w.name == target_window_name);
    let stash_count = windows.iter().filter(|w| w.name == "stash" || w.name.starts_with("stash-")).count();
    // Check if Phase 1+2 can be skipped (target exists, single stash)
    let skip_phase_1_2 = has_target && stash_count <= 1;
    if skip_phase_1_2 {
        // Target exists and stash is consolidated. Skip Phases 1+2,
        // but still run Phase 3 (index normalization) below.
    } else {
    eprintln!("[repair] layout needs repair: target={} stash_count={}", has_target, stash_count);

    // ── Phase 1: Stash consolidation ──

    // Find the primary stash window (first one named exactly "stash")
    let primary_stash = windows.iter().find(|w| w.name == "stash");

    // Collect secondary stash windows: named "stash-*" OR extra "stash" windows
    // (after the first)
    let mut secondary_stash_ids: Vec<String> = Vec::new();
    let mut seen_primary = false;
    for w in &windows {
        if w.name == "stash" {
            if seen_primary {
                secondary_stash_ids.push(w.id.clone());
            }
            seen_primary = true;
        } else if w.name.starts_with("stash-") {
            secondary_stash_ids.push(w.id.clone());
        }
    }

    if !secondary_stash_ids.is_empty() {
        // Ensure we have a primary stash to consolidate into
        let primary_id = if let Some(p) = primary_stash {
            p.id.clone()
        } else {
            // No primary stash — create one
            match tmux.ensure_stash_window(session_name) {
                Ok(id) => {
                    eprintln!("[repair] created primary stash window {}", id);
                    id
                }
                Err(e) => {
                    eprintln!("[repair] failed to create stash window: {}", e);
                    return Ok(());
                }
            }
        };

        for sec_id in &secondary_stash_ids {
            eprintln!("[repair] consolidating stash window {} into {}", sec_id, primary_id);

            // List panes in the secondary window
            let panes = tmux.list_window_panes(sec_id).unwrap_or_default();
            for pane in &panes {
                // Resize stash to 1000 rows before each join to prevent "too small"
                let _ = tmux.raw_cmd(&[
                    "resize-window", "-t", &primary_id, "-y", "1000",
                ]);

                // Find the largest pane in primary stash as join target
                let target = tmux.largest_pane_in_window(&primary_id)
                    .unwrap_or_else(|| {
                        // Fallback: first pane in primary
                        tmux.list_window_panes(&primary_id)
                            .unwrap_or_default()
                            .into_iter()
                            .next()
                            .unwrap_or_default()
                    });
                if target.is_empty() {
                    eprintln!("[repair] no target pane in primary stash, skipping {}", pane);
                    continue;
                }

                match tmux.join_pane(pane, &target, "-dv") {
                    Ok(()) => {
                        eprintln!("[repair] joined pane {} → stash {}", pane, primary_id);
                    }
                    Err(e) => {
                        eprintln!("[repair] join-pane {} → {} failed: {}, leaving in place", pane, target, e);
                    }
                }
            }

            // After moving all panes, the empty window should auto-delete.
            // If it still exists (e.g. join failed for some panes), kill it only
            // if it has no panes left.
            let remaining = tmux.list_window_panes(sec_id).unwrap_or_default();
            if remaining.is_empty() {
                // Window should have auto-deleted, but try to kill just in case
                let _ = tmux.raw_cmd(&["kill-window", "-t", sec_id]);
                eprintln!("[repair] killed empty stash window {}", sec_id);
            }
        }
    }

    // ── Phase 2: Ensure agent-doc window exists ──

    let target_exists = windows.iter().any(|w| w.name == target_window_name);
    if !target_exists {
        eprintln!(
            "[repair] target window '{}' not found, attempting to rescue a pane from stash",
            target_window_name
        );

        // Load the registry and find any alive registered pane
        if let Ok(registry) = sessions::load() {
            let mut rescued = false;
            for entry in registry.values() {
                if tmux.pane_alive(&entry.pane) {
                    eprintln!("[repair] rescuing pane {} from stash", entry.pane);
                    match tmux.break_pane(&entry.pane) {
                        Ok(()) => {
                            if let Ok(new_win) = tmux.pane_window(&entry.pane) {
                                let _ = tmux.raw_cmd(&[
                                    "rename-window", "-t", &new_win, target_window_name,
                                ]);
                                eprintln!(
                                    "[repair] recreated window {} as '{}'",
                                    new_win, target_window_name
                                );
                            }
                            rescued = true;
                            break;
                        }
                        Err(e) => {
                            eprintln!("[repair] break-pane {} failed: {}", entry.pane, e);
                        }
                    }
                }
            }
            if !rescued {
                eprintln!("[repair] no alive registered panes found, sync will auto-start later");
            }
        }
    }
    } // end skip_phase_1_2 else

    // ── Phase 3: Normalize window indices (always runs) ──
    // agent-doc should be at index 0, stash at index 1+
    // Re-list windows after repairs
    let output = tmux.raw_cmd(&[
        "list-windows", "-t", &format!("{}:", session_name),
        "-F", "#{window_index} #{window_name}",
    ]);
    if let Ok(ref listing) = output {
        for line in listing.lines() {
            let mut parts = line.splitn(2, ' ');
            if let (Some(idx), Some(name)) = (parts.next(), parts.next())
                && name == target_window_name && idx != "0"
            {
                eprintln!("[repair] moving {}:{} to index 0", idx, name);
                let _ = tmux.raw_cmd(&[
                    "move-window",
                    "-s", &format!("{}:{}", session_name, idx),
                    "-t", &format!("{}:0", session_name),
                ]);
                break;
            }
        }
    }

    Ok(())
}

fn run_with_options(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    auto_start: bool,
    tmux: &Tmux,
) -> Result<()> {
    // Repair layout before anything else: consolidate stash windows and ensure
    // the agent-doc window exists.
    // Resolve session name from --window arg, or fall back to current session.
    let mut effective_window = window.map(|s| s.to_string());
    if let Some(ref w) = effective_window {
        let session_name = tmux
            .cmd()
            .args(["display-message", "-t", w, "-p", "#{session_name}"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            // If window doesn't exist, try to get session from the window ID prefix (e.g. "@0" → session "0")
            .or_else(|| {
                // Fall back to current session
                tmux.cmd()
                    .args(["display-message", "-p", "#{session_name}"])
                    .output().ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            })
            .unwrap_or_default();
        if !session_name.is_empty() {
            let _ = repair_layout(tmux, &session_name, "agent-doc");
            // After repair, the window ID may have changed. Re-resolve by name.
            let resolved = tmux.raw_cmd(&[
                "list-windows", "-t", &format!("{}:", session_name),
                "-F", "#{window_id} #{window_name}",
            ]);
            if let Ok(ref output) = resolved {
                for line in output.lines() {
                    let mut parts = line.splitn(2, ' ');
                    if let (Some(wid), Some(wname)) = (parts.next(), parts.next())
                        && wname == "agent-doc"
                    {
                        if wid != w.as_str() {
                            eprintln!("[sync] window ID changed after repair: {} → {}", w, wid);
                            effective_window = Some(wid.to_string());
                        }
                        break;
                    }
                }
            }
        }
    }
    let window = effective_window.as_deref();

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

    // Self-healing: if the target window doesn't exist (was deleted when all panes
    // were stashed), recreate it by breaking a registered pane out of the stash.
    if let Some(w) = window {
        let window_exists = tmux.list_window_panes(w).map(|p| !p.is_empty()).unwrap_or(false);
        if !window_exists {
            eprintln!("[sync] target window {} does not exist, attempting to recreate from stash", w);
            // Find any registered pane that's alive (even in stash)
            let all_files: Vec<PathBuf> = col_args
                .iter()
                .flat_map(|arg| arg.split(','))
                .map(|s| PathBuf::from(s.trim()))
                .collect();
            for file_path in &all_files {
                if let Ok(content) = std::fs::read_to_string(file_path)
                    && let Ok((fm, _)) = frontmatter::parse(&content)
                    && let Some(ref sid) = fm.session
                    && let Ok(Some(pane)) = sessions::lookup(sid)
                    && tmux.pane_alive(&pane)
                {
                    eprintln!("[sync] rescuing pane {} for {} from stash", pane, file_path.display());
                    // break-pane creates a new window with this pane
                    if tmux.break_pane(&pane).is_ok() {
                        // Rename the new window to "agent-doc"
                        if let Ok(new_win) = tmux.pane_window(&pane) {
                            let _ = tmux.raw_cmd(&["rename-window", "-t", &new_win, "agent-doc"]);
                            eprintln!("[sync] recreated window {} as agent-doc", new_win);
                        }
                        break;
                    }
                }
            }
        }
    }

    // Pre-sync: auto-start Claude sessions for files that have session UUIDs
    // but no alive panes. This ensures sync has panes to arrange.
    // Skipped when auto_start=false (e.g., when called from route which already handled the file).
    if auto_start {
        // Parse file paths from col_args (each arg is "file1.md,file2.md")
        let all_files: Vec<PathBuf> = col_args
            .iter()
            .flat_map(|arg| arg.split(','))
            .map(|s| PathBuf::from(s.trim()))
            .collect();

        // Determine the target session for auto-start. Prefer:
        // 1. Session of the --window argument
        // 2. tmux_session from any file in the batch
        // This avoids falling back to current_tmux_session() which may return
        // whichever session the user is viewing (not the intended one).
        let context_session: Option<String> = window
            .and_then(|w| {
                // Get session name from window ID
                let output = tmux
                    .cmd()
                    .args(["display-message", "-t", w, "-p", "#{session_name}"])
                    .output()
                    .ok()?;
                if output.status.success() {
                    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !name.is_empty() { Some(name) } else { None }
                } else {
                    None
                }
            })
            .or_else(|| {
                // Fall back to tmux_session from any file in the batch
                all_files.iter().find_map(|f| {
                    let content = std::fs::read_to_string(f).ok()?;
                    let (fm, _) = frontmatter::parse(&content).ok()?;
                    fm.tmux_session
                })
            });
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
                .map(|pane| {
                    if !tmux.pane_alive(pane) {
                        return false;
                    }
                    // A pane in a stash window is alive but not usable — treat as dead
                    // so auto-start creates a fresh pane in the correct window.
                    if let Ok(win_id) = tmux.pane_window(pane) {
                        let win_name = tmux
                            .cmd()
                            .args(["display-message", "-t", &win_id, "-p", "#{window_name}"])
                            .output()
                            .ok()
                            .filter(|o| o.status.success())
                            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                            .unwrap_or_default();
                        if win_name == "stash" || win_name.starts_with("stash-") {
                            eprintln!(
                                "[sync] pane {} for {} is in stash window '{}', treating as dead",
                                pane, file_path.display(), win_name
                            );
                            return false;
                        }
                    }
                    true
                })
                .unwrap_or(false);

            // Strip deprecated tmux_session from frontmatter.
            // Session is now determined at runtime — the field is no longer needed.
            if fm.tmux_session.is_some() {
                // Find and remove the tmux_session line (including trailing newline)
                let stripped = content
                    .lines()
                    .filter(|line| !line.starts_with("tmux_session:"))
                    .collect::<Vec<&str>>()
                    .join("\n");
                // Preserve trailing newline if original had one
                let stripped = if content.ends_with('\n') && !stripped.ends_with('\n') {
                    format!("{}\n", stripped)
                } else {
                    stripped
                };
                if stripped != content {
                    eprintln!(
                        "[sync] stripping deprecated tmux_session from {}",
                        file_path.display()
                    );
                    if let Err(e) = std::fs::write(file_path, &stripped) {
                        eprintln!("[sync] warning: failed to strip tmux_session: {}", e);
                    }
                }
            }

            if has_alive_pane {
                // Pane is alive, but check if the registered file still exists.
                // After a rename, the pane shows an error for the old path.
                // Detect this: registered file differs from current AND doesn't exist.
                if let Some(ref pane) = registered_pane {
                    if let Ok(Some(entry)) = sessions::lookup_entry(&session_id) {
                        let registered_file = Path::new(&entry.file);
                        let current_file = file_path.to_string_lossy();
                        if entry.file != *current_file && !registered_file.exists() {
                            eprintln!(
                                "[sync] registered file {} no longer exists (renamed to {}), killing stale pane {}",
                                entry.file, file_path.display(), pane
                            );
                            let _ = tmux.kill_pane(pane);
                            // Update registry with new file path, fall through to auto-start
                            if let Err(e) = sessions::register(&session_id, pane, &current_file) {
                                eprintln!("[sync] warning: re-register failed: {}", e);
                            }
                            // Fall through to auto-start below
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
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
            if let Err(e) = route::auto_start_no_wait(tmux, file_path, &session_id, &file_str, context_session.as_deref()) {
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

    // Post-sync: validate session state (report only, no kill).
    // Disabled --fix because auto_start with context_session intentionally places
    // cross-session panes — resync --fix would kill them (lesson: context_session override).
    if let Err(e) = resync::run(false) {
        eprintln!("[sync] warning: post-sync resync failed: {}", e);
    }

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
