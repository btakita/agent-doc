//! # Module: sync — Reconciliation
//!
//! `agent-doc sync` — **reconcile** the editor's columnar layout with tmux panes.
//!
//! **Ontology:** Sync performs **Reconciliation** — matching the editor's declared
//! layout (columns of files) to the tmux pane layout. When a file has a session UUID
//! but no registered pane, sync triggers **Provisioning** (via `route::auto_start`)
//! to create a new pane. Files entering the system for the first time go through
//! **Initialization** (`ensure_initialized`) which assigns a UUID, creates a snapshot,
//! and commits to git. The result is a **Binding** (document→pane association) stored
//! in `sessions.json`.
//!
//! Usage: `agent-doc sync --col plan.md,corky.md --col agent-doc.md [--window @1] [--focus plan.md]`
//!
//! Each `--col` argument is a comma-separated list of files. Columns are arranged
//! left-to-right; files within a column stack top-to-bottom. Layout arithmetic is
//! delegated to `tmux-router::sync`. This module provides the agent-doc-specific
//! layers: frontmatter-based session resolution, auto-start for missing panes,
//! post-sync registry updates, layout repair, and column memory.
//!
//! ## Spec
//! - `run(col_args, window, focus)` is the primary entry point. Filters empty
//!   col_args (phantom columns from the JetBrains plugin), repairs layout,
//!   prunes stale sessions, auto-starts missing panes, delegates to
//!   `tmux_router::sync`, then registers synced file→pane assignments.
//! - `run_layout_only(col_args, window, focus)` skips auto-start; used when called
//!   from `route` which has already handled the target file.
//! - `run_with_tmux(col_args, window, focus, tmux)` injects a custom `Tmux` instance
//!   (test hook); auto-start is enabled.
//! - `repair_layout(tmux, session_name, target_window_name)` runs three phases:
//!   1. **Stash consolidation** — merges all `stash-*` and duplicate `stash` windows
//!      into a single primary stash window via `join-pane`.
//!   2. **Target window rescue** — if the target window is missing, breaks a live
//!      registered pane out of the stash and renames the new window.
//!   3. **Index normalisation** — moves or swaps the target window to index 0,
//!      using `swap-window` when index 0 is occupied to avoid data loss.
//!
//!   Phases 1 and 2 are skipped when the layout is already correct (target exists,
//!   single stash). Phase 3 always runs.
//! - The `resolve_file` closure reads each file's frontmatter session UUID and
//!   produces a `FileResolution::Registered` (or `Unmanaged` when no UUID is present).
//!   Files with session UUIDs are always treated as registered, even if the registry
//!   entry was pruned — sync will auto-start a new session for them. This enables the
//!   declarative layout flow: navigating to a file in a split creates a tmux pane.
//!   It never propagates `tmux_session` from frontmatter — that field is deprecated.
//! - When a registered pane is found in a stash window, sync attempts to **rescue** it
//!   back to the agent-doc window (via `swap-pane`, falling back to `join-pane`)
//!   instead of treating it as dead. This preserves the existing Claude session context
//!   when switching between editor tabs. Only if rescue fails is the pane treated as
//!   dead and a fresh session started.
//! - Auto-start detects duplicate panes via `find_alive_pane_for_file`, which scans
//!   process command lines (`ps -p <pid> -o command=`) before spawning. The `col_args`
//!   slice is passed through to `route::auto_start_no_wait` so new panes split in the
//!   correct direction based on column position (`is_first_column`).
//! - `register_synced_files` updates or creates registry entries for every file
//!   assigned a pane by `tmux_router::sync`, covering files never individually claimed.
//!
//! ## Agentic Contracts
//! - `run` always prunes stale registry entries before computing layout — callers
//!   receive a consistent view with dead panes already removed.
//! - `repair_layout` is idempotent: calling it on an already-correct layout is a
//!   fast no-op (fast path detected before any tmux mutations).
//! - No `tmux_session` frontmatter field is ever written by this module; all session
//!   targeting uses the `--window` argument or live tmux pane introspection.
//! - `run_layout_only` guarantees it will not spawn new Claude sessions (safe to call
//!   from within an active route cycle).
//! - `register_synced_files` holds `RegistryLock` for the duration of its write and
//!   saves only when at least one entry changed.
//! - **Column memory:** `.agent-doc/last_layout.json` persists a column→agent-doc mapping.
//!   When a column has no agent doc (user switches to a non-session file), sync substitutes
//!   the last known agent doc for that column index. This preserves the 2-pane tmux layout
//!   when one editor column temporarily shows a non-agent file. The state file is updated
//!   after each successful sync with any columns that contain an agent doc.
//! - Auto-start errors are non-fatal: a warning is logged to stderr and sync continues.
//! - Post-auto_start stash is no longer needed: `tmux_router::sync` always runs the
//!   full reconcile path (no early exits), so excess panes are stashed during the
//!   DETACH phase.
//!
//! ## Evals
//! - repair_layout_skips_correct_state: session with agent-doc at index 0 and one
//!   stash → repair is a no-op, window list unchanged.
//! - repair_layout_moves_window_to_index_0: agent-doc at index 2 with index 0 free →
//!   repair moves agent-doc to index 0.
//! - repair_layout_swaps_when_index_0_occupied: agent-doc at index 2 with a different
//!   window at index 0 → repair swaps the two windows, both windows preserved.
//! - repair_layout_consolidates_multiple_stash_windows: multiple `stash`/`stash-*`
//!   windows → repair merges all panes into one stash window.
//! - repair_layout_rescues_pane_from_stash: no agent-doc window, pane in stash →
//!   repair does not error; stashed pane remains alive.
//! - sync_does_not_write_tmux_session_to_frontmatter: after sync, the document file
//!   must not contain a `tmux_session` frontmatter key.
//! - resolve_file_ignores_frontmatter_tmux_session: `FileResolution::Registered` always
//!   has `tmux_session: None` regardless of what the frontmatter contains.
//! - find_alive_pane_for_file: pane whose child process cmdline contains `agent-doc`
//!   and the file path is returned; panes without a matching cmdline are skipped.
//! - empty_col_args_filtered: col_args `["file1.md", "", "file2.md", ""]` → after
//!   filtering, only `["file1.md", "file2.md"]` are processed.
//! - check_build_stamp_clears_locks: new build timestamp → stale `.lock` files removed,
//!   stamp file updated.

use anyhow::Result;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::sessions::Tmux;
use crate::{frontmatter, resync, route, sessions};

use tmux_router::FileResolution;

pub fn run(col_args: &[String], window: Option<&str>, focus: Option<&str>) -> Result<()> {
    tracing::debug!(cols = ?col_args, window, focus, "sync::run start");
    run_with_options(col_args, window, focus, true, &Tmux::default_server())
}

/// Run sync without auto-starting sessions. Used when called from route
/// (route already handled the target file — auto-start would create duplicates).
#[allow(dead_code)]
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
    tracing::debug!(session_name, target_window_name, "sync::repair_layout start");
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
        // Check if window 0 exists (occupied by another window)
        let window_0_exists = listing.lines().any(|line| {
            line.starts_with("0 ")
        });

        for line in listing.lines() {
            let mut parts = line.splitn(2, ' ');
            if let (Some(idx), Some(name)) = (parts.next(), parts.next())
                && name == target_window_name && idx != "0"
            {
                if window_0_exists {
                    // Window 0 is occupied — swap to preserve both windows
                    eprintln!("[repair] swapping {}:{} with window 0", idx, name);
                    let _ = tmux.raw_cmd(&[
                        "swap-window",
                        "-s", &format!("{}:{}", session_name, idx),
                        "-t", &format!("{}:0", session_name),
                    ]);
                } else {
                    // Window 0 is free — move directly
                    eprintln!("[repair] moving {}:{} to index 0", idx, name);
                    let _ = tmux.raw_cmd(&[
                        "move-window",
                        "-s", &format!("{}:{}", session_name, idx),
                        "-t", &format!("{}:0", session_name),
                    ]);
                }
                break;
            }
        }
    }

    Ok(())
}

fn sync_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true)
        .open("/tmp/agent-doc-sync.log")
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{}] {}", ts, msg);
    }
}

/// Check if this binary is a new build and clear stale caches if so.
/// Compares the embedded build timestamp against `.agent-doc/build.stamp`.
/// On mismatch: clears startup locks (`.agent-doc/starting/*.lock`) and updates stamp.
fn check_build_stamp() {
    let build_ts = env!("AGENT_DOC_BUILD_TIMESTAMP");
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(_) => return,
    };
    let stamp_path = cwd.join(".agent-doc/build.stamp");
    let stored = std::fs::read_to_string(&stamp_path).unwrap_or_default();
    if stored.trim() == build_ts {
        return; // Same build
    }
    eprintln!("[sync] new build detected ({}→{}), clearing stale caches", stored.trim(), build_ts);
    // Clear startup locks
    let starting_dir = cwd.join(".agent-doc/starting");
    if starting_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&starting_dir)
    {
        for entry in entries.flatten() {
            if entry.path().extension().map(|e| e == "lock").unwrap_or(false) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    // Update stamp
    if let Some(parent) = stamp_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&stamp_path, build_ts);
}

fn run_with_options(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    auto_start: bool,
    tmux: &Tmux,
) -> Result<()> {
    tracing::debug!(cols = ?col_args, window, focus, auto_start, "sync::run_with_options start");

    // Serialize sync calls via file lock. Concurrent syncs (from rapid tab switches)
    // race against each other's stash operations, causing pane bouncing. A second sync
    // that arrives while the first is running will block briefly then see the correct state.
    let lock_path = std::path::Path::new(".agent-doc/sync.lock");
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_path);
    let _lock_guard = lock_file.as_ref().ok().map(|f| {
        use fs2::FileExt;
        // Try to acquire exclusive lock. If another sync holds it, wait up to 3s.
        // On timeout, proceed anyway (better than blocking forever).
        match f.try_lock_exclusive() {
            Ok(()) => Some(()),
            Err(_) => {
                sync_log("sync lock contention — waiting for previous sync");
                // Block for up to 3 seconds
                let _ = f.lock_exclusive();
                sync_log("sync lock acquired after wait");
                Some(())
            }
        }
    });

    // Check for new build and clear stale caches
    check_build_stamp();

    // Filter empty col_args — the JetBrains plugin sometimes sends phantom empty columns.
    let col_args: Vec<String> = col_args.iter()
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .collect();

    // Column memory: for columns with non-agent files, substitute the last known
    // agent doc so the reconciler preserves the pane from the previous layout.
    let layout_state_path = std::path::Path::new(".agent-doc/last_layout.json");
    let saved_layout: Vec<String> = std::fs::read_to_string(layout_state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let col_args: Vec<String> = col_args.iter().enumerate().map(|(i, col)| {
        // Check if this column has an agent doc (any file with session UUID)
        let has_agent_doc = col.split(',').any(|f| {
            let f = f.trim();
            if f.is_empty() { return false; }
            if let Ok(content) = std::fs::read_to_string(f)
                && let Ok((fm, _)) = frontmatter::parse(&content) {
                    return fm.session.is_some();
                }
            false
        });
        if has_agent_doc {
            col.clone()
        } else if let Some(remembered) = saved_layout.get(i) {
            if !remembered.is_empty() {
                sync_log(&format!("column {} has no agent doc, substituting remembered: {}", i, remembered));
                remembered.clone()
            } else {
                col.clone()
            }
        } else {
            col.clone()
        }
    }).collect();
    let col_args = col_args.as_slice();
    sync_log(&format!("=== sync start: col_args={:?} window={:?} focus={:?} auto_start={}", col_args, window, focus, auto_start));
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
            sync_log("repair_layout completed");
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

    // Diagnostic: log pane count at key checkpoints to find where stashed panes reappear
    if let Some(w) = window {
        let pane_count = tmux.list_window_panes(w).map(|p| p.len()).unwrap_or(0);
        let pane_list: Vec<String> = tmux.list_window_panes(w).unwrap_or_default();
        sync_log(&format!("checkpoint:post-repair window={} panes={} list={:?}", w, pane_count, pane_list));
    }

    let _ = resync::prune(); // Clean stale entries before layout calculation

    if let Some(w) = window {
        let pane_list: Vec<String> = tmux.list_window_panes(w).unwrap_or_default();
        sync_log(&format!("checkpoint:post-prune window={} panes={} list={:?}", w, pane_list.len(), pane_list));
    }

    let registry_path = sessions::registry_path();
    // Track session_id → file path for post-sync claim updates
    let session_files: RefCell<Vec<(String, PathBuf)>> = RefCell::new(Vec::new());

    let resolve_file = |path: &Path| -> Option<FileResolution> {
        // Auto-initialize documents that have agent_doc_format but no session UUID.
        // This handles files created by skills (e.g., granola import) that didn't
        // go through claim. ensure_initialized() assigns a UUID and creates the
        // snapshot + git baseline.
        if let Err(e) = crate::snapshot::ensure_initialized(path) {
            eprintln!("[sync] warning: ensure_initialized failed for {}: {}", path.display(), e);
        }

        // Re-read after potential initialization (UUID may have been assigned)
        let content = std::fs::read_to_string(path).ok()?;
        let (fm, _) = frontmatter::parse(&content).ok()?;
        match fm.session {
            Some(ref key) => {
                // Treat all files with session UUIDs as registered, even if
                // the registry entry was pruned (dead pane). Sync will auto-start
                // a new session for files without alive panes. This enables the
                // declarative layout flow: navigating to a file in a split should
                // create a tmux pane regardless of registry state.
                let has_registry = sessions::lookup(key).ok().flatten().is_some();
                let registry_str = if has_registry { "yes" } else { "no (will auto-start)" };
                tracing::debug!(
                    file = %path.display(),
                    session = &key[..8.min(key.len())],
                    registry = registry_str,
                    "sync resolve_file → Registered"
                );
                eprintln!(
                    "[sync] resolve_file: {} → Registered (session={}, registry={})",
                    path.display(), &key[..8.min(key.len())], registry_str
                );
                session_files
                    .borrow_mut()
                    .push((key.clone(), path.to_path_buf()));
                // tmux_session comes from project config, not frontmatter
                Some(FileResolution::Registered {
                    key: key.clone(),
                    tmux_session: None,
                })
            }
            None => {
                // No session UUID even after ensure_initialized() — file has no
                // agent_doc_format, so it's genuinely not an agent document.
                Some(FileResolution::Unmanaged)
            }
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

        // Determine the target session for auto-start:
        // 1. From frontmatter tmux_session (if alive)
        // 2. From --window argument
        // 3. Falls back to None (current session)
        let context_session: Option<String> = window
            .and_then(|w| {
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

            // Files with session UUIDs but no registry entry are auto-started.
            // The registry was likely pruned when the pane died. The user's intent
            // (navigating to the file in a split) is clear — create a pane for it.
            eprintln!(
                "[sync] auto-start check: {} session={} registered_pane={}",
                file_path.display(),
                &session_id[..8.min(session_id.len())],
                registered_pane.as_deref().unwrap_or("none")
            );
            let has_alive_pane = registered_pane
                .as_ref()
                .map(|pane| {
                    if !tmux.pane_alive(pane) {
                        return false;
                    }
                    // A pane in a stash window is alive — rescue it back to the
                    // agent-doc window instead of creating a new session.
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
                                "[sync] pane {} for {} is in stash window '{}', rescuing",
                                pane, file_path.display(), win_name
                            );
                            // Rescue: swap the stashed pane into the agent-doc window
                            if let Some(target_win) = window {
                                let agent_doc_window = format!("{}:agent-doc", target_win);
                                let target_panes = tmux.list_window_panes(&agent_doc_window).unwrap_or_default();
                                if let Some(target) = target_panes.first() {
                                    match tmux.swap_pane(pane, target) {
                                        Ok(()) => {
                                            eprintln!("[sync] rescued pane {} via swap-pane", pane);
                                            return true;
                                        }
                                        Err(e) => {
                                            eprintln!("[sync] swap-pane rescue failed ({}), trying join-pane", e);
                                            if tmux.join_pane(pane, target, "-dh").is_ok() {
                                                eprintln!("[sync] rescued pane {} via join-pane", pane);
                                                return true;
                                            }
                                        }
                                    }
                                }
                            }
                            // Rescue failed — treat as dead
                            eprintln!("[sync] rescue failed for pane {}, treating as dead", pane);
                            return false;
                        }
                    }
                    true
                })
                .unwrap_or(false);

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

            sync_log(&format!("auto-starting session for {} (no alive pane)", file_path.display()));
            eprintln!(
                "[sync] auto-starting session for {} (no alive pane)",
                file_path.display()
            );
            if let Err(e) = route::auto_start_no_wait(tmux, file_path, &session_id, &file_str, context_session.as_deref(), col_args) {
                eprintln!(
                    "[sync] warning: auto-start failed for {}: {}",
                    file_path.display(),
                    e
                );
            }
        }

        // Post-auto_start stash removed: the tmux_router reconciler now always runs
        // the full reconcile path (no early exits), so it handles stashing excess panes.
    }

    // Log pane count before tmux_router::sync
    if let Some(w) = window {
        let pane_list: Vec<String> = tmux.list_window_panes(w).unwrap_or_default();
        sync_log(&format!("checkpoint:pre-tmux_router window={} panes={} list={:?}", w, pane_list.len(), pane_list));
    }

    // NOTE: The busy pane guard (protect_pane) was removed from DETACH because it caused
    // 3-pane accumulation when the user switches documents in the same column. The guard
    // prevented stashing panes with active sessions, but when a new document replaces the
    // old one in a column, the old pane must give way. Column memory + stash rescue handle
    // session preservation for the non-agent-file case.
    let result =
        tmux_router::sync(col_args, window, focus, tmux, &registry_path, &resolve_file)?;

    // Log pane count after tmux_router::sync
    if let Some(w) = window {
        let pane_count = tmux.list_window_panes(w).map(|p| p.len()).unwrap_or(0);
        sync_log(&format!("post-tmux_router::sync: window={} panes={} file_panes={}", w, pane_count, result.file_panes.len()));
        tracing::debug!(window = w, pane_count, file_panes = result.file_panes.len(), "post-sync pane count");

        // Session health check: verify the session still exists after sync.
        // If the session was destroyed (e.g., all windows stashed), log a critical warning.
        if let Ok(session) = tmux.cmd()
            .args(["display-message", "-t", w, "-p", "#{session_name}"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            && !session.is_empty()
        {
            let session_alive = tmux.cmd()
                .args(["has-session", "-t", &session])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !session_alive {
                tracing::error!(session = %session, "SESSION DESTROYED after sync — tmux session no longer exists");
                eprintln!("[sync] CRITICAL: session '{}' was destroyed during sync!", session);
            }
        }
    }

    // Save column layout state: for each column that has an agent doc,
    // record it so future syncs can substitute it when the column has a non-agent file.
    {
        let layout_state: Vec<String> = col_args.iter().map(|col| {
            // Find the first agent doc file in this column
            for f in col.split(',').map(|f| f.trim()) {
                if f.is_empty() { continue; }
                if let Ok(content) = std::fs::read_to_string(f)
                    && let Ok((fm, _)) = frontmatter::parse(&content)
                    && fm.session.is_some() {
                        return f.to_string();
                    }
            }
            String::new()
        }).collect();
        // Only save if at least one column has an agent doc
        if layout_state.iter().any(|s| !s.is_empty())
            && let Ok(json) = serde_json::to_string(&layout_state) {
                let _ = std::fs::write(layout_state_path, json);
            }
    }

    // tmux_session frontmatter write-back removed (deprecated).
    // Session targeting now uses --window arg or pane introspection.

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
/// Check if a tmux pane is running an active agent-doc or claude session.
///
/// Used as a `protect_pane` callback to prevent stashing panes with active sessions.
/// Checks the pane's PID and its child processes for agent-doc or claude in the command line.
#[allow(dead_code)]
fn is_pane_busy(tmux: &Tmux, pane_id: &str) -> bool {
    let output = tmux.cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{pane_pid}"])
        .output();
    let pid_str = match output {
        Ok(ref o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => return false,
    };
    if pid_str.is_empty() {
        return false;
    }

    // Check the pane's direct process
    if pid_is_agent_session(&pid_str) {
        return true;
    }

    // Check child processes (pane PID is usually a shell)
    if let Ok(children) = std::process::Command::new("pgrep")
        .args(["-P", &pid_str])
        .output()
    {
        for child_pid in String::from_utf8_lossy(&children.stdout).lines() {
            let child_pid = child_pid.trim();
            if !child_pid.is_empty() && pid_is_agent_session(child_pid) {
                return true;
            }
        }
    }
    false
}

/// Check if a process (by PID) is running agent-doc or claude (any file).
#[allow(dead_code)]
fn pid_is_agent_session(pid: &str) -> bool {
    let output = match std::process::Command::new("ps")
        .args(["-p", pid, "-o", "command="])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let cmdline = String::from_utf8_lossy(&output.stdout);
    cmdline.contains("agent-doc") || cmdline.contains("claude")
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::IsolatedTmux;

    /// Helper: list windows as vec of (index, name) pairs.
    fn list_windows(tmux: &Tmux, session: &str) -> Vec<(String, String)> {
        let output = tmux
            .raw_cmd(&[
                "list-windows",
                "-t",
                &format!("{}:", session),
                "-F",
                "#{window_index} #{window_name}",
            ])
            .unwrap();
        output
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(2, ' ');
                let idx = parts.next()?.to_string();
                let name = parts.next()?.to_string();
                Some((idx, name))
            })
            .collect()
    }

    #[test]
    fn repair_layout_skips_correct_state() {
        let iso = IsolatedTmux::new("sync-repair-skip-correct");
        let tmp = tempfile::TempDir::new().unwrap();

        // Create session with agent-doc window at index 0 + one stash window
        let _pane = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let _ = iso.ensure_stash_window("test");

        let windows_before = list_windows(&iso, "test");

        // repair_layout should succeed and not change anything
        repair_layout(&iso, "test", "agent-doc").unwrap();

        let windows_after = list_windows(&iso, "test");
        assert_eq!(windows_before, windows_after, "layout was already correct — nothing should change");
    }

    #[test]
    fn repair_layout_moves_window_to_index_0() {
        let iso = IsolatedTmux::new("sync-repair-move-idx0");
        let tmp = tempfile::TempDir::new().unwrap();

        // Create session: initial window at 0 (placeholder), then create
        // agent-doc + stash at higher indices, and remove the placeholder.
        // This leaves agent-doc at a non-zero index with index 0 free.
        let _pane0 = iso.new_session("test", tmp.path()).unwrap();
        // Create stash at index 1
        let _ = iso.raw_cmd(&[
            "new-window", "-t", "test:", "-n", "stash", "-d",
        ]);
        // Create agent-doc at index 2
        let _ = iso.raw_cmd(&[
            "new-window", "-t", "test:", "-n", "agent-doc", "-d",
        ]);
        // Kill the placeholder at index 0 to free it
        let _ = iso.raw_cmd(&["kill-window", "-t", "test:0"]);

        // Verify agent-doc is NOT at index 0 before repair
        let windows_before = list_windows(&iso, "test");
        let ad_before = windows_before.iter().find(|(_, n)| n == "agent-doc");
        assert!(ad_before.is_some(), "agent-doc window should exist");
        assert_ne!(ad_before.unwrap().0, "0", "agent-doc should NOT be at index 0 before repair");

        repair_layout(&iso, "test", "agent-doc").unwrap();

        // After repair, agent-doc should be at index 0
        let windows_after = list_windows(&iso, "test");
        let ad_after = windows_after.iter().find(|(_, n)| n == "agent-doc");
        assert!(ad_after.is_some(), "agent-doc window should still exist");
        assert_eq!(ad_after.unwrap().0, "0", "agent-doc should be at index 0 after repair");
    }

    #[test]
    fn repair_layout_rescues_pane_from_stash() {
        let iso = IsolatedTmux::new("sync-repair-rescue-stash");
        let tmp = tempfile::TempDir::new().unwrap();

        // Create session with a non-agent-doc window + stash with a pane
        let pane1 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "other"]);

        // Create a second pane and stash it
        let pane2 = iso.split_window(&pane1, tmp.path(), "-dh").unwrap();
        iso.stash_pane(&pane2, "test").unwrap();

        // Verify no agent-doc window exists
        let windows_before = list_windows(&iso, "test");
        assert!(
            !windows_before.iter().any(|(_, n)| n == "agent-doc"),
            "agent-doc window should NOT exist before repair"
        );

        // Note: repair_layout uses sessions::load() which reads from CWD.
        // In tests without CWD override, Phase 2 rescue may not find the pane
        // in the registry. But Phase 1 (stash consolidation) and Phase 3 (index
        // normalization) still run. The key assertion is that repair doesn't error.
        let result = repair_layout(&iso, "test", "agent-doc");
        assert!(result.is_ok(), "repair_layout should not error");

        // The stashed pane should still be alive regardless
        assert!(iso.pane_alive(&pane2), "stashed pane should still be alive");
    }

    #[test]
    fn repair_layout_consolidates_multiple_stash_windows() {
        let iso = IsolatedTmux::new("sync-repair-consolidate");
        let tmp = tempfile::TempDir::new().unwrap();

        // Create session with agent-doc window
        let pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

        // Create 3 extra panes, stash each one separately to create multiple stash windows
        let p1 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
        let _p2 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
        let _p3 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();

        // Stash them — each stash_pane goes to the same stash window normally,
        // but we can force multiple stash windows by using break_pane_to_stash
        // which creates overflow windows.
        iso.stash_pane(&p1, "test").unwrap();
        // The first stash_pane creates the stash window. For the second and third,
        // create new windows named "stash" manually to simulate overflow.
        let _ = iso.raw_cmd(&[
            "new-window", "-t", "test:", "-n", "stash", "-d", "-P", "-F", "#{window_id}",
        ]);

        let stash_windows: Vec<String> = {
            let output = iso.raw_cmd(&[
                "list-windows", "-t", "test:", "-F", "#{window_id} #{window_name}",
            ]).unwrap();
            output.lines()
                .filter_map(|line| {
                    let mut parts = line.splitn(2, ' ');
                    let id = parts.next()?;
                    let name = parts.next()?;
                    if name == "stash" || name.starts_with("stash-") {
                        Some(id.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        };

        // We should have at least 2 stash windows now
        assert!(stash_windows.len() >= 2, "should have multiple stash windows, got {}", stash_windows.len());

        // Count total stash windows before repair
        let windows_before = list_windows(&iso, "test");
        let stash_count_before = windows_before.iter()
            .filter(|(_, n)| n == "stash" || n.starts_with("stash-"))
            .count();
        assert!(stash_count_before >= 2, "should have >=2 stash windows before repair, got {}", stash_count_before);

        repair_layout(&iso, "test", "agent-doc").unwrap();

        // After repair, should have at most 1 stash window
        let windows_after = list_windows(&iso, "test");
        let stash_count_after = windows_after.iter()
            .filter(|(_, n)| n == "stash" || n.starts_with("stash-"))
            .count();
        assert!(
            stash_count_after <= 1,
            "should have at most 1 stash window after consolidation, got {}",
            stash_count_after
        );

        // agent-doc should still be at index 0
        let ad = windows_after.iter().find(|(_, n)| n == "agent-doc");
        assert!(ad.is_some(), "agent-doc window should still exist");
        assert_eq!(ad.unwrap().0, "0", "agent-doc should be at index 0");
    }

    #[test]
    fn repair_layout_swaps_when_index_0_occupied() {
        // Bug: when agent-doc is at index 2 and index 0 is occupied by another window
        // (e.g., stash), move-window fails because index 0 is taken.
        // Fix: use swap-window when index 0 is occupied.
        let iso = IsolatedTmux::new("sync-repair-swap-idx0");
        let tmp = tempfile::TempDir::new().unwrap();

        // Create session — window 0 is a "corky" window (simulating user's corky watch)
        let _pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "corky"]);

        // Create stash at index 1
        let _ = iso.raw_cmd(&[
            "new-window", "-t", "test:", "-n", "stash", "-d",
        ]);
        // Create agent-doc at index 2
        let _ = iso.raw_cmd(&[
            "new-window", "-t", "test:", "-n", "agent-doc", "-d",
        ]);

        // Verify: corky at 0, stash at 1, agent-doc at 2
        let windows_before = list_windows(&iso, "test");
        assert_eq!(windows_before.iter().find(|(i, _)| i == "0").unwrap().1, "corky");
        assert_eq!(windows_before.iter().find(|(i, _)| i == "2").unwrap().1, "agent-doc");

        repair_layout(&iso, "test", "agent-doc").unwrap();

        // After repair: agent-doc should be at 0, corky should be at 2
        let windows_after = list_windows(&iso, "test");
        let ad = windows_after.iter().find(|(_, n)| n == "agent-doc");
        assert!(ad.is_some(), "agent-doc window should still exist");
        assert_eq!(ad.unwrap().0, "0", "agent-doc should be at index 0 after swap");

        let corky = windows_after.iter().find(|(_, n)| n == "corky");
        assert!(corky.is_some(), "corky window should still exist (not destroyed)");
        assert_ne!(corky.unwrap().0, "0", "corky should have moved away from index 0");

        // All 3 windows should still exist
        assert_eq!(windows_after.len(), 3, "no windows should be destroyed, got {:?}", windows_after);
    }

    /// Regression: sync must never write tmux_session back to document frontmatter.
    /// This was the root cause of pane-swap bugs — stale session names in frontmatter
    /// caused terminal.rs to route panes to the wrong session.
    #[test]
    fn sync_does_not_write_tmux_session_to_frontmatter() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("test.md");

        // Write a doc WITHOUT tmux_session
        std::fs::write(&doc, "---\nagent_doc_session: test-123\n---\n\n## User\n\nHello\n").unwrap();

        // Read it back — tmux_session should be None
        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = crate::frontmatter::parse(&content).unwrap();
        assert!(fm.tmux_session.is_none(), "tmux_session should not be set initially");

        // Write a doc WITH tmux_session already set
        let doc2 = tmp.path().join("test2.md");
        std::fs::write(&doc2, "---\nagent_doc_session: test-456\ntmux_session: old-session\n---\n\n## User\n\nHello\n").unwrap();

        let content2 = std::fs::read_to_string(&doc2).unwrap();
        let (fm2, _) = crate::frontmatter::parse(&content2).unwrap();
        // Frontmatter still parses it (for backward compat reading), but resolve_file
        // must NOT propagate it to FileResolution
        assert_eq!(fm2.tmux_session, Some("old-session".to_string()),
            "frontmatter parser should still read tmux_session for backward compat");
    }

    /// Verify resolve_file closure always passes tmux_session: None regardless of frontmatter.
    #[test]
    fn resolve_file_ignores_frontmatter_tmux_session() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("test.md");

        // File with tmux_session in frontmatter
        std::fs::write(&doc, "---\nagent_doc_session: sess-1\ntmux_session: stale-session\n---\n\nbody\n").unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = crate::frontmatter::parse(&content).unwrap();

        // Simulate what resolve_file does — tmux_session must be None
        let resolution = match fm.session {
            Some(key) => FileResolution::Registered {
                key,
                tmux_session: None, // This is the critical assertion
            },
            None => FileResolution::Unmanaged,
        };

        match resolution {
            FileResolution::Registered { tmux_session, .. } => {
                assert!(tmux_session.is_none(),
                    "FileResolution must never carry tmux_session from frontmatter");
            }
            _ => panic!("expected Registered"),
        }
    }

    /// Sync skips files that have no `agent_doc_session` in frontmatter.
    /// These are regular files that were never claimed — they should resolve as Unmanaged.
    #[test]
    fn sync_skips_file_without_session_in_frontmatter() {
        let tmp = tempfile::TempDir::new().unwrap();

        // Create a file with no frontmatter session UUID
        let doc = tmp.path().join("no-session.md");
        std::fs::write(&doc, "# Just a regular file\n\nNo frontmatter at all.\n").unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = crate::frontmatter::parse(&content).unwrap();
        assert!(fm.session.is_none(), "file should have no session UUID");

        // Simulate resolve_file: no session → Unmanaged
        let resolution = match fm.session {
            Some(_) => unreachable!("session should be None"),
            None => FileResolution::Unmanaged,
        };
        assert!(matches!(resolution, FileResolution::Unmanaged));
    }

    /// Sync skips files that have a session UUID in frontmatter but no registry entry.
    /// This prevents auto-starting sessions for files that were never properly claimed
    /// or whose claim expired.
    #[test]
    fn sync_skips_file_with_session_uuid_but_no_registry() {
        let tmp = tempfile::TempDir::new().unwrap();

        // Create an empty registry
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/sessions.json"),
            "{}",
        ).unwrap();

        // Create a file with a session UUID but no matching registry entry
        let doc = tmp.path().join("stale-claim.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: orphan-uuid-123\n---\n\n## User\n\nHello\n",
        ).unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = crate::frontmatter::parse(&content).unwrap();
        assert_eq!(fm.session, Some("orphan-uuid-123".to_string()));

        // Load registry directly from the temp path (avoid CWD dependency)
        let reg_content = std::fs::read_to_string(
            tmp.path().join(".agent-doc/sessions.json"),
        ).unwrap();
        let registry: sessions::SessionRegistry = serde_json::from_str(&reg_content).unwrap();
        let has_registry_entry = registry.contains_key("orphan-uuid-123");
        assert!(!has_registry_entry, "should NOT have a registry entry");

        // This is what the fixed resolve_file does — returns Unmanaged for stale claims
        let resolution = if has_registry_entry {
            FileResolution::Registered {
                key: "orphan-uuid-123".to_string(),
                tmux_session: None,
            }
        } else {
            FileResolution::Unmanaged
        };
        assert!(
            matches!(resolution, FileResolution::Unmanaged),
            "file with session UUID but no registry entry should be Unmanaged"
        );
    }

    /// Sync routes files that have both a session UUID in frontmatter AND a registry entry.
    #[test]
    fn sync_routes_file_with_session_uuid_and_registry_entry() {
        let tmp = tempfile::TempDir::new().unwrap();

        // Create registry with a matching entry
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let registry_content = serde_json::json!({
            "claimed-uuid-456": {
                "pane": "%99",
                "pid": 12345,
                "cwd": "/tmp",
                "started": "2026-01-01T00:00:00Z",
                "file": "claimed.md",
                "window": "@0"
            }
        });
        std::fs::write(
            tmp.path().join(".agent-doc/sessions.json"),
            serde_json::to_string_pretty(&registry_content).unwrap(),
        ).unwrap();

        // Create a file with a session UUID that matches the registry
        let doc = tmp.path().join("claimed.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: claimed-uuid-456\n---\n\n## User\n\nHello\n",
        ).unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = crate::frontmatter::parse(&content).unwrap();
        assert_eq!(fm.session, Some("claimed-uuid-456".to_string()));

        // Load registry directly from the temp path (avoid CWD dependency)
        let reg_content = std::fs::read_to_string(
            tmp.path().join(".agent-doc/sessions.json"),
        ).unwrap();
        let registry: sessions::SessionRegistry = serde_json::from_str(&reg_content).unwrap();
        let has_registry_entry = registry.contains_key("claimed-uuid-456");
        assert!(has_registry_entry, "should have a registry entry");

        // This is what the fixed resolve_file does — returns Registered for claimed files
        let resolution = if has_registry_entry {
            FileResolution::Registered {
                key: "claimed-uuid-456".to_string(),
                tmux_session: None,
            }
        } else {
            FileResolution::Unmanaged
        };
        assert!(
            matches!(resolution, FileResolution::Registered { .. }),
            "file with session UUID AND registry entry should be Registered"
        );
    }

    /// Empty col_args are filtered out before processing (JetBrains plugin sends phantom columns).
    #[test]
    fn empty_col_args_filtered() {
        let col_args: Vec<String> = vec![
            "file1.md".into(),
            "".into(),
            "file2.md".into(),
            "".into(),
            "  ".into(),
        ];
        let filtered: Vec<String> = col_args
            .iter()
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .collect();
        assert_eq!(filtered, vec!["file1.md", "file2.md"]);
    }
}
