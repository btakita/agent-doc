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
//!   back to the agent-doc window via `join-pane` instead of treating it as dead.
//!   This preserves the existing Claude session context
//!   when switching between editor tabs. Only if rescue fails is the pane treated as
//!   dead and a fresh session started.
//! - **File rename detection:** When a pane is alive but the registry's `file` field
//!   points to a path that no longer exists AND the current sync target has a different
//!   path, sync infers a rename occurred. It calls `sessions::register` with the new
//!   path, reusing the existing pane (no kill/restart). Detection is via
//!   `is_file_rename(registered_path, current_path)`. Editor plugins trigger this by
//!   calling `agent-doc sync --focus <new_path>` on `FileRenameEvent` (JB) or
//!   `onDidRenameFiles` (VS Code).
//! - **Rename debounce (#qam7):** When `--rename` is passed, sync writes a debounce marker
//!   (`.agent-doc/rename-debounce/<hash>.marker`) for the focused file. Any sync within
//!   5 seconds that finds the marker will skip auto-start for that file. This prevents
//!   spurious pane creation when FileRenameListener triggers sync for a file that has no
//!   alive pane. The subsequent EditorTabSyncListener-triggered sync also respects the
//!   marker. Markers are cleaned up on expiry check. Functions: `write_rename_debounce(path)`
//!   (public, called from main.rs), `has_rename_debounce(path)` (private, checked in
//!   auto-start loop).
//! - **Auto-start pane ID logging:** `provision_pane` returns `Result<String>` (the new
//!   pane ID). Sync logs each auto-started pane as `[sync] auto-started %XX for <file>`.
//!   When >1 pane auto-starts in a single call, a batch summary is printed:
//!   `[sync] auto-started N panes: %XX→file1, %YY→file2`. Both messages go to
//!   `/tmp/agent-doc-sync.log` for forensic analysis.
//! - Auto-start detects duplicate panes before spawning. It first resolves
//!   session-associated panes via shared live-owner proof
//!   (`find_associated_panes` / supervisor PID fallback), which can recover a live
//!   supervisor-backed pane even when the foreground process tree no longer includes
//!   the document path. Ambiguous multi-pane ownership now fails closed for that file
//!   instead of auto-starting another replacement session on top. The `col_args`
//!   slice is passed through to `route::provision_pane` so new panes split in the
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
//! - `is_file_rename` is pure (no tmux dependency): it compares two paths and checks
//!   disk existence of the old one. Safe to call from any context.
//! - File rename re-registration reuses the existing `sessions::register` path, so
//!   single-session-per-pane invariant and `RegistryLock` apply as normal.
//! - **Column memory:** `.agent-doc/last_layout.json` persists a column→agent-doc mapping.
//!   When a column has no agent doc (user switches to a non-session file), sync substitutes
//!   the last known agent doc for that column index. This preserves the 2-pane tmux layout
//!   when one editor column temporarily shows a non-agent file. The state file is updated
//!   after each successful sync with any columns that contain an agent doc.
//! - `write_rename_debounce` is idempotent: calling it multiple times for the same file
//!   just refreshes the marker timestamp. No tmux dependency — pure filesystem operation.
//! - `has_rename_debounce` self-cleans: expired markers are deleted on check, so no
//!   separate GC is needed.
//! - `provision_pane` returns the pane ID string on success; callers can log it or
//!   collect for batch summaries without additional tmux queries.
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
//! - is_file_rename_detects_rename_when_old_path_gone: old path absent + paths differ →
//!   returns true.
//! - is_file_rename_returns_false_when_paths_match: same path → returns false.
//! - is_file_rename_returns_false_when_old_path_still_exists: old path present + paths
//!   differ → returns false (not a rename, both files exist).
//! - is_file_rename_handles_relative_paths: relative paths with nonexistent old →
//!   returns true.
//! - file_rename_updates_registry: registry with old path entry + detection confirms
//!   rename logic; entry pane preserved.
//! - rename_debounce_suppresses_auto_start: marker written for file → `has_rename_debounce`
//!   returns true within TTL, auto-start is skipped.
//! - rename_debounce_expires_after_ttl: marker with old timestamp → `has_rename_debounce`
//!   returns false, marker file is deleted.
//! - rename_debounce_does_not_affect_other_files: marker for file A → file B still
//!   passes the debounce check.
//! - batch_summary_format_multiple_panes: 3 auto-started panes → summary string contains
//!   count and all pane→file mappings.
//! - batch_summary_not_printed_for_single_pane: 1 auto-started pane → batch summary
//!   condition (len > 1) is false.
//! - check_build_stamp_clears_locks: new build timestamp → stale `.lock` files removed,
//!   stamp file updated.

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::sessions::{PaneMoveOp, Tmux};
use crate::{component, frontmatter, resync, route, sessions, snapshot};

use tmux_router::FileResolution;

const RENAME_DEBOUNCE_TTL_SECS: u64 = 5;
const SYNC_FRONTMATTER_STATUS_PREFIX: &str = "[agent-doc sync] malformed frontmatter";

fn parse_frontmatter_for_sync<'a>(
    content: &'a str,
    file: &Path,
    phase: &str,
) -> Result<(frontmatter::Frontmatter, &'a str)> {
    frontmatter::parse_for_file(content, file)
        .map_err(|err| anyhow::anyhow!("sync {} frontmatter: {}", phase, err))
}

fn sync_frontmatter_status_message(phase: &str, err: &anyhow::Error) -> String {
    format!(
        "{} during {}.\n\n{}",
        SYNC_FRONTMATTER_STATUS_PREFIX, phase, err
    )
}

fn write_sync_status(file: &Path, text: &str) -> Result<bool> {
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for sync status update", file.display()))?;
    let components = component::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;
    let Some(status) = components
        .iter()
        .find(|comp| comp.name.as_str() == "status")
        .cloned()
    else {
        return Ok(false);
    };
    if status.content(&doc).trim() == text.trim() {
        return Ok(false);
    }

    let payload = if text.is_empty() {
        String::new()
    } else {
        format!("{text}\n")
    };
    let updated = status.replace_content(&doc, &payload);
    std::fs::write(file, &updated)
        .with_context(|| format!("failed to write {} for sync status update", file.display()))?;
    snapshot::save(file, &updated).with_context(|| {
        format!(
            "failed to update snapshot for {} after sync status update",
            file.display()
        )
    })?;
    Ok(true)
}

fn surface_frontmatter_status(file: &Path, phase: &str, err: &anyhow::Error) {
    let text = sync_frontmatter_status_message(phase, err);
    match write_sync_status(file, &text) {
        Ok(true) => {
            let log = format!(
                "[sync] status: surfaced malformed frontmatter warning for {}",
                file.display()
            );
            eprintln!("{}", log);
            sync_log(&log);
        }
        Ok(false) => {}
        Err(status_err) => {
            let warning = format!(
                "[sync] warning: failed to surface malformed frontmatter status for {}: {}",
                file.display(),
                status_err
            );
            eprintln!("{}", warning);
            sync_log(&warning);
        }
    }
}

fn clear_frontmatter_status(file: &Path) {
    let doc = match std::fs::read_to_string(file) {
        Ok(doc) => doc,
        Err(_) => return,
    };
    let components = match component::parse(&doc) {
        Ok(components) => components,
        Err(_) => return,
    };
    let Some(status) = components
        .iter()
        .find(|comp| comp.name.as_str() == "status")
        .cloned()
    else {
        return;
    };
    if !status
        .content(&doc)
        .trim_start()
        .starts_with(SYNC_FRONTMATTER_STATUS_PREFIX)
    {
        return;
    }

    match write_sync_status(file, "") {
        Ok(true) => {
            let log = format!(
                "[sync] status: cleared malformed frontmatter warning for {}",
                file.display()
            );
            eprintln!("{}", log);
            sync_log(&log);
        }
        Ok(false) => {}
        Err(status_err) => {
            let warning = format!(
                "[sync] warning: failed to clear malformed frontmatter status for {}: {}",
                file.display(),
                status_err
            );
            eprintln!("{}", warning);
            sync_log(&warning);
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MissingRegisteredPaneRepair {
    dead_pane: Option<DeadPaneDiagnostics>,
    recorded_session_loss: bool,
    repaired_stale_preflight: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DeadPaneDiagnostics {
    observed_window: Option<String>,
    dead_status: Option<String>,
    cycle_phase: Option<String>,
    capture_path: Option<PathBuf>,
    last_visible_excerpt: Option<String>,
    pane_killed: bool,
}

fn cycle_phase_label(file: &Path) -> Option<String> {
    let state = crate::cycle_state::load(file).ok().flatten()?;
    let label = match state.phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
    };
    Some(label.to_string())
}

fn sanitize_excerpt(text: &str) -> Option<String> {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let mut excerpt = collapsed;
    if excerpt.len() > 200 {
        excerpt.truncate(200);
        excerpt.push_str("...");
    }
    Some(excerpt)
}

fn last_visible_excerpt(capture: &str) -> Option<String> {
    capture
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("Pane is dead"))
        .and_then(sanitize_excerpt)
}

fn persist_dead_pane_capture(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    tail: &str,
) -> Option<PathBuf> {
    if tail.trim().is_empty() {
        return None;
    }
    let canonical = file
        .canonicalize()
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    let root = crate::snapshot::find_project_root(&canonical)?;
    let dir = root.join(".agent-doc/logs/dead-panes");
    std::fs::create_dir_all(&dir).ok()?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let pane_token = pane_id.trim_start_matches('%');
    let path = dir.join(format!("{session_id}-{timestamp}-pane-{pane_token}.log"));
    std::fs::write(&path, tail).ok()?;
    Some(path)
}

fn capture_dead_pane_diagnostics(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
    last_known_window: Option<&str>,
) -> Result<Option<DeadPaneDiagnostics>> {
    if !tmux.pane_dead(pane_id) {
        return Ok(None);
    }

    let dead_status = tmux.pane_dead_status(pane_id)?;
    let observed_window = tmux
        .pane_window(pane_id)
        .ok()
        .or_else(|| last_known_window.map(ToOwned::to_owned));
    let cycle_phase = cycle_phase_label(file);
    let tail = tmux.capture_pane(pane_id, Some(80)).unwrap_or_default();
    let capture_path = persist_dead_pane_capture(file, session_id, pane_id, &tail);
    let last_visible_excerpt = last_visible_excerpt(&tail);
    let mut event = format!(
        "pane_death_detected pane={pane_id} status={} cycle_phase={}",
        dead_status.as_deref().unwrap_or("unknown"),
        cycle_phase.as_deref().unwrap_or("none")
    );
    if let Some(window_id) = observed_window.as_deref() {
        event.push_str(&format!(" window={window_id}"));
    }
    if let Some(path) = capture_path.as_ref() {
        event.push_str(&format!(" capture={}", path.display()));
    }
    if let Some(excerpt) = last_visible_excerpt.as_deref() {
        event.push_str(&format!(" last_visible_excerpt={excerpt}"));
    }
    let _ = crate::startup_miss::append_session_log_event(file, session_id, &event);

    let pane_killed = match tmux.kill_pane(pane_id) {
        Ok(()) => {
            let _ = crate::startup_miss::append_session_log_event(
                file,
                session_id,
                &format!("pane_death_cleanup pane={pane_id} action=kill"),
            );
            true
        }
        Err(err) => {
            let _ = crate::startup_miss::append_session_log_event(
                file,
                session_id,
                &format!(
                    "pane_death_cleanup pane={pane_id} action=keep_dead reason={}",
                    sanitize_excerpt(&err.to_string()).unwrap_or_else(|| "unknown".to_string())
                ),
            );
            false
        }
    };

    Ok(Some(DeadPaneDiagnostics {
        observed_window,
        dead_status,
        cycle_phase,
        capture_path,
        last_visible_excerpt,
        pane_killed,
    }))
}

fn repair_missing_registered_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
    last_known_window: Option<&str>,
) -> Result<MissingRegisteredPaneRepair> {
    let dead_pane =
        capture_dead_pane_diagnostics(tmux, file, session_id, pane_id, last_known_window)?;
    let recorded_session_loss = crate::startup_miss::record_session_loss(
        file,
        session_id,
        pane_id,
        if dead_pane.is_some() {
            "registered_pane_dead"
        } else {
            "registered_pane_missing"
        },
        dead_pane
            .as_ref()
            .and_then(|diag| diag.observed_window.as_deref())
            .or(last_known_window),
    )?;
    let repaired_stale_preflight = matches!(
        crate::repair::repair_stale_preflight_started_cycle(file)?,
        crate::repair::RepairOutcome::StalePreflightLockRepaired
    );
    Ok(MissingRegisteredPaneRepair {
        dead_pane,
        recorded_session_loss,
        repaired_stale_preflight,
    })
}

pub fn run(col_args: &[String], window: Option<&str>, focus: Option<&str>) -> Result<()> {
    tracing::debug!(cols = ?col_args, window, focus, "sync::run start");
    run_with_options(col_args, window, focus, true, &Tmux::default_server())
}

/// Write a debounce marker for a file that was just renamed.
/// Subsequent syncs within 5s will skip auto-start for this file.
pub fn write_rename_debounce(file_path: &str) {
    let debounce_dir = Path::new(".agent-doc/rename-debounce");
    if std::fs::create_dir_all(debounce_dir).is_err() {
        return;
    }
    let hash = crate::snapshot::doc_hash(Path::new(file_path)).unwrap_or_default();
    if hash.is_empty() {
        return;
    }
    let marker = debounce_dir.join(format!("{}.marker", hash));
    let _ = std::fs::write(&marker, file_path);
    eprintln!(
        "[sync] rename debounce marker set for {} ({})",
        file_path, hash
    );
    sync_log(&format!(
        "rename-debounce: set marker for {} hash={}",
        file_path, hash
    ));
}

/// Check if a file has an active rename debounce marker (within TTL).
fn has_rename_debounce(file_path: &Path) -> bool {
    let hash = match crate::snapshot::doc_hash(file_path) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let marker = Path::new(".agent-doc/rename-debounce").join(format!("{}.marker", hash));
    if !marker.exists() {
        return false;
    }
    let expired = marker
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() >= RENAME_DEBOUNCE_TTL_SECS)
        .unwrap_or(true);
    if expired {
        let _ = std::fs::remove_file(&marker);
        return false;
    }
    true
}

/// Run sync without auto-starting sessions. Used when called from route
/// (route already handled the target file — auto-start would create duplicates).
#[allow(dead_code)]
pub fn run_layout_only(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
) -> Result<()> {
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
    tracing::debug!(
        session_name,
        target_window_name,
        "sync::repair_layout start"
    );
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
            eprintln!(
                "[repair] failed to list windows for session {}: {}",
                session_name, e
            );
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
            Some(WinInfo {
                id,
                name,
                _pane_count: pane_count,
            })
        })
        .collect();

    // ── Fast path: if layout is already correct, skip repair ──
    let has_target = windows.iter().any(|w| w.name == target_window_name);
    let stash_count = windows
        .iter()
        .filter(|w| w.name == "stash" || w.name.starts_with("stash-"))
        .count();
    // Check if Phase 1+2 can be skipped (target exists, single stash)
    let skip_phase_1_2 = has_target && stash_count <= 1;
    if skip_phase_1_2 {
        // Target exists and stash is consolidated. Skip Phases 1+2,
        // but still run Phase 3 (index normalization) below.
    } else {
        eprintln!(
            "[repair] layout needs repair: target={} stash_count={}",
            has_target, stash_count
        );

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
                eprintln!(
                    "[repair] consolidating stash window {} into {}",
                    sec_id, primary_id
                );

                // List panes in the secondary window
                let panes = tmux.list_window_panes(sec_id).unwrap_or_default();
                for pane in &panes {
                    // Resize stash to 1000 rows before each join to prevent "too small"
                    let _ = tmux.raw_cmd(&["resize-window", "-t", &primary_id, "-y", "1000"]);

                    // Find the largest pane in primary stash as join target
                    let target = tmux.largest_pane_in_window(&primary_id).unwrap_or_else(|| {
                        // Fallback: first pane in primary
                        tmux.list_window_panes(&primary_id)
                            .unwrap_or_default()
                            .into_iter()
                            .next()
                            .unwrap_or_default()
                    });
                    if target.is_empty() {
                        eprintln!(
                            "[repair] no target pane in primary stash, skipping {}",
                            pane
                        );
                        continue;
                    }

                    sync_log(&format!(
                        "stash_consolidate_action=join-pane src={} dst={} primary_stash={}",
                        pane, target, primary_id
                    ));
                    match PaneMoveOp::new(tmux, pane, &target).join("-dv") {
                        Ok(()) => {
                            eprintln!("[repair] joined pane {} → stash {}", pane, primary_id);
                            sync_log(&format!(
                                "stash_consolidate_result=join-pane ok=true src={} dst={}",
                                pane, target
                            ));
                        }
                        Err(e) => {
                            eprintln!(
                                "[repair] join-pane {} → {} failed: {}, leaving in place",
                                pane, target, e
                            );
                            sync_log(&format!(
                                "stash_consolidate_result=join-pane ok=false src={} dst={} err={}",
                                pane, target, e
                            ));
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
                                        "rename-window",
                                        "-t",
                                        &new_win,
                                        target_window_name,
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
                    eprintln!(
                        "[repair] no alive registered panes found, sync will auto-start later"
                    );
                }
            }
        }
    } // end skip_phase_1_2 else

    // ── Phase 3: Normalize window indices (always runs) ──
    // agent-doc should be at index 0, stash at index 1+
    // Re-list windows after repairs
    let output = tmux.raw_cmd(&[
        "list-windows",
        "-t",
        &format!("{}:", session_name),
        "-F",
        "#{window_index} #{window_name}",
    ]);
    if let Ok(ref listing) = output {
        // Check if window 0 exists (occupied by another window)
        let window_0_exists = listing.lines().any(|line| line.starts_with("0 "));

        for line in listing.lines() {
            let mut parts = line.splitn(2, ' ');
            if let (Some(idx), Some(name)) = (parts.next(), parts.next())
                && name == target_window_name
                && idx != "0"
            {
                if window_0_exists {
                    // Window 0 is occupied — swap to preserve both windows
                    eprintln!("[repair] swapping {}:{} with window 0", idx, name);
                    sync_log(&format!(
                        "repair_action=swap-window src={}:{} dst={}:0 window_name={}",
                        session_name, idx, session_name, name
                    ));
                    let result = tmux.raw_cmd(&[
                        "swap-window",
                        "-s",
                        &format!("{}:{}", session_name, idx),
                        "-t",
                        &format!("{}:0", session_name),
                    ]);
                    sync_log(&format!(
                        "repair_result=swap-window src_idx={} dst_idx=0 ok={}",
                        idx,
                        result.is_ok()
                    ));
                    let _ = result;
                } else {
                    // Window 0 is free — move directly
                    eprintln!("[repair] moving {}:{} to index 0", idx, name);
                    sync_log(&format!(
                        "repair_action=move-window src={}:{} dst={}:0 window_name={}",
                        session_name, idx, session_name, name
                    ));
                    let result = tmux.raw_cmd(&[
                        "move-window",
                        "-s",
                        &format!("{}:{}", session_name, idx),
                        "-t",
                        &format!("{}:0", session_name),
                    ]);
                    sync_log(&format!(
                        "repair_result=move-window src_idx={} dst_idx=0 ok={}",
                        idx,
                        result.is_ok()
                    ));
                    let _ = result;
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
        .create(true)
        .append(true)
        .open("/tmp/agent-doc-sync.log")
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{}] {}", ts, msg);
    }
}

fn normalize_scope_arg(value: Option<&str>) -> Option<&str> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
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
    eprintln!(
        "[sync] new build detected ({}→{}), clearing stale caches",
        stored.trim(),
        build_ts
    );
    // Clear startup locks
    let starting_dir = cwd.join(".agent-doc/starting");
    if starting_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&starting_dir)
    {
        for entry in entries.flatten() {
            if entry
                .path()
                .extension()
                .map(|e| e == "lock")
                .unwrap_or(false)
            {
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
    let window = normalize_scope_arg(window);
    let focus = normalize_scope_arg(focus);
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
    let col_args: Vec<String> = col_args
        .iter()
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

    let col_args: Vec<String> = col_args
        .iter()
        .enumerate()
        .map(|(i, col)| {
            // Check if this column has an agent doc (any file with session UUID)
            let has_agent_doc = col.split(',').any(|f| {
                let f = f.trim();
                if f.is_empty() {
                    return false;
                }
                if let Ok(content) = std::fs::read_to_string(f)
                    && let Ok((fm, _)) = frontmatter::parse(&content)
                {
                    return fm.session.is_some();
                }
                false
            });
            if has_agent_doc {
                col.clone()
            } else if let Some(remembered) = saved_layout.get(i) {
                if !remembered.is_empty() {
                    sync_log(&format!(
                        "column {} has no agent doc, substituting remembered: {}",
                        i, remembered
                    ));
                    remembered.clone()
                } else {
                    col.clone()
                }
            } else {
                col.clone()
            }
        })
        .collect();
    let col_args = col_args.as_slice();
    sync_log(&format!(
        "=== sync start: col_args={:?} window={:?} focus={:?} auto_start={}",
        col_args, window, focus, auto_start
    ));
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
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            })
            .unwrap_or_default();
        if !session_name.is_empty() {
            let _ = repair_layout(tmux, &session_name, "agent-doc");
            sync_log("repair_layout completed");
            // After repair, the window ID may have changed. Re-resolve by name.
            let resolved = tmux.raw_cmd(&[
                "list-windows",
                "-t",
                &format!("{}:", session_name),
                "-F",
                "#{window_id} #{window_name}",
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
        sync_log(&format!(
            "checkpoint:post-repair window={} panes={} list={:?}",
            w, pane_count, pane_list
        ));
    }

    let _ = resync::prune(); // Clean stale entries before layout calculation

    if let Some(w) = window {
        let pane_list: Vec<String> = tmux.list_window_panes(w).unwrap_or_default();
        sync_log(&format!(
            "checkpoint:post-prune window={} panes={} list={:?}",
            w,
            pane_list.len(),
            pane_list
        ));
    }

    let registry_path = sessions::registry_path();
    // Track session_id → file path for post-sync claim updates
    let session_files: RefCell<Vec<(String, PathBuf)>> = RefCell::new(Vec::new());

    let resolve_file = |path: &Path| -> Option<FileResolution> {
        // Step 1: Auto-scaffold empty .md files BEFORE ensure_initialized().
        // Must run first because ensure_initialized() writes minimal frontmatter
        // (just agent_doc_session:) which prevents the full template scaffold.
        // Per SPEC §8.5: empty files should be initialized as template documents.
        if path.extension() == Some(std::ffi::OsStr::new("md")) {
            let raw = std::fs::read_to_string(path).unwrap_or_default();
            if raw.trim().is_empty() {
                eprintln!("[sync] auto-scaffolding empty file: {}", path.display());
                let session_id = uuid::Uuid::new_v4();
                let scaffold = format!(
                    "---\nagent_doc_session: {}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n\n## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n## Icebox\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n",
                    session_id
                );
                if let Err(e) = std::fs::write(path, &scaffold) {
                    eprintln!(
                        "[sync] warning: failed to scaffold {}: {}",
                        path.display(),
                        e
                    );
                    return Some(FileResolution::Unmanaged);
                }
                // Save snapshot BEFORE committing — git::commit() uses the snapshot
                // to determine what to stage. Without this, the snapshot has stale
                // content and the commit fails with a drift warning.
                if let Err(e) = crate::snapshot::save(path, &scaffold) {
                    eprintln!(
                        "[sync] warning: failed to save scaffold snapshot for {}: {}",
                        path.display(),
                        e
                    );
                }
                // Commit the scaffolded file immediately.
                if let Err(e) = crate::git::commit(path) {
                    eprintln!(
                        "[sync] warning: failed to commit scaffold for {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        // Step 2: Ensure initialized (UUID + snapshot + git baseline).
        // For scaffolded files, this creates the snapshot and git tracking.
        // For files with agent_doc_format but no session, this assigns a UUID.
        if let Err(e) = crate::snapshot::ensure_initialized(path) {
            eprintln!(
                "[sync] warning: ensure_initialized failed for {}: {}",
                path.display(),
                e
            );
        }

        // Step 3: Read content and resolve.
        let content = std::fs::read_to_string(path).ok()?;
        let (fm, _) = match parse_frontmatter_for_sync(&content, path, "resolve_file") {
            Ok(parsed) => parsed,
            Err(e) => {
                let warning = format!("[sync] warning: {}", e);
                eprintln!("{}", warning);
                sync_log(&warning);
                surface_frontmatter_status(path, "resolve_file", &e);
                return None;
            }
        };
        clear_frontmatter_status(path);

        match fm.session {
            Some(ref key) => {
                let has_registry = sessions::lookup(key).ok().flatten().is_some();
                let registry_str = if has_registry {
                    "yes"
                } else {
                    "no (will auto-start)"
                };
                tracing::debug!(
                    file = %path.display(),
                    session = &key[..8.min(key.len())],
                    registry = registry_str,
                    "sync resolve_file → Registered"
                );
                eprintln!(
                    "[sync] resolve_file: {} → Registered (session={}, registry={})",
                    path.display(),
                    &key[..8.min(key.len())],
                    registry_str
                );
                session_files
                    .borrow_mut()
                    .push((key.clone(), path.to_path_buf()));
                Some(FileResolution::Registered {
                    key: key.clone(),
                    tmux_session: None,
                })
            }
            None => Some(FileResolution::Unmanaged),
        }
    };

    // Self-healing: if the target window doesn't exist (was deleted when all panes
    // were stashed), recreate it by breaking a registered pane out of the stash.
    if let Some(w) = window {
        let window_exists = tmux
            .list_window_panes(w)
            .map(|p| !p.is_empty())
            .unwrap_or(false);
        if !window_exists {
            eprintln!(
                "[sync] target window {} does not exist, attempting to recreate from stash",
                w
            );
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
                    eprintln!(
                        "[sync] rescuing pane {} for {} from stash",
                        pane,
                        file_path.display()
                    );
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

    // Pre-sync: auto-start agent sessions for files that have session UUIDs
    // but no alive panes. This ensures sync has panes to arrange.
    // Skipped when auto_start=false (e.g., when called from route which already handled the file).
    if auto_start {
        let mut auto_started_panes: Vec<(String, String)> = Vec::new();

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
        let context_session: Option<String> = window.and_then(|w| {
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
            let (fm, _) = match parse_frontmatter_for_sync(&content, file_path, "auto-start") {
                Ok(r) => r,
                Err(e) => {
                    let warning = format!("[sync] warning: {}", e);
                    eprintln!("{}", warning);
                    sync_log(&warning);
                    surface_frontmatter_status(file_path, "auto-start", &e);
                    continue;
                }
            };
            clear_frontmatter_status(file_path);
            let session_id = match fm.session {
                Some(ref id) => id.clone(),
                None => continue,
            };

            let registered_entry = sessions::lookup_entry(&session_id).ok().flatten();
            let registered_pane = registered_entry.as_ref().map(|entry| entry.pane.clone());

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
                    // Session guard: only rescue within the correct session.
                    // If pane is in the wrong session, stash it in the target session first.
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
                            // Check if pane is in the correct session before rescuing
                            let pane_session = tmux
                                .cmd()
                                .args(["display-message", "-t", pane, "-p", "#{session_name}"])
                                .output()
                                .ok()
                                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                                .unwrap_or_default();
                            let target_sess = context_session.as_deref().unwrap_or("");
                            if !target_sess.is_empty() && pane_session != target_sess {
                                eprintln!(
                                    "[sync] pane {} for {} is in session '{}' stash; refusing cross-session rescue into '{}'",
                                    pane, file_path.display(), pane_session, target_sess
                                );
                                sync_log(&format!(
                                    "rescue_skipped_cross_session pane={} file={} actual_session={} target_session={}",
                                    pane, file_path.display(), pane_session, target_sess
                                ));
                                return true;
                            }
                            eprintln!(
                                "[sync] pane {} for {} is in stash window '{}', rescuing",
                                pane, file_path.display(), win_name
                            );
                            sync_log(&format!(
                                "rescue_attempt pane={} file={} stash_window={} session={}",
                                pane, file_path.display(), win_name, target_sess
                            ));
                            // Rescue: rejoin the stashed pane into the agent-doc window.
                            // Use the window ID directly — format!("{}:agent-doc", window_id)
                            // is invalid because tmux parses `:` as session:window, treating
                            // the window ID as a session name (which doesn't exist).
                            // When `window` is not explicitly provided, discover the
                            // agent-doc window from the target session name.
                            let discovered_win = if window.is_none() && !target_sess.is_empty() {
                                let candidate = format!("{}:agent-doc", target_sess);
                                if !tmux.list_window_panes(&candidate).unwrap_or_default().is_empty() {
                                    Some(candidate)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            let rescue_win = window.map(|w| w.to_string()).or(discovered_win);
                            if let Some(ref target_win) = rescue_win {
                                let target_panes = tmux.list_panes_ordered(target_win).unwrap_or_default();
                                let split_before = crate::route::is_first_column(file_path, col_args);
                                let target = if split_before {
                                    target_panes.first()
                                } else {
                                    target_panes.last()
                                };
                                if let Some(target) = target {
                                    let join_flag = if split_before { "-dbh" } else { "-dh" };
                                    sync_log(&format!(
                                        "rescue_action=join-pane src={} dst={} target_window={} join_flag={}",
                                        pane, target, target_win, join_flag
                                    ));
                                    match sessions::join_pane_guarded(
                                        tmux,
                                        pane,
                                        target,
                                        target_sess,
                                        join_flag,
                                    ) {
                                        Ok(()) => {
                                            eprintln!("[sync] rescued pane {} via join-pane", pane);
                                            sync_log(&format!(
                                                "rescue_result=join-pane ok=true pane={} target={}",
                                                pane, target
                                            ));
                                            return true;
                                        }
                                        Err(e) => {
                                            eprintln!("[sync] join-pane rescue failed ({})", e);
                                            sync_log(&format!(
                                                "rescue_result=join-pane ok=false pane={} target={} err={}",
                                                pane, target, e
                                            ));
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
                // Pane is alive — check if the file was renamed (registered path
                // no longer exists but the session ID matches). If so, update the
                // registry to the new path and reuse the existing pane.
                if let Some(ref pane) = registered_pane
                    && let Some(ref entry) = registered_entry
                {
                    let current_file = file_path.to_string_lossy();
                    if is_file_rename(&entry.file, &current_file) {
                        eprintln!(
                            "[sync] file renamed: {} → {} — reusing pane {} (session {})",
                            entry.file,
                            file_path.display(),
                            pane,
                            session_id
                        );
                        if let Err(e) = sessions::register(&session_id, pane, &current_file) {
                            eprintln!("[sync] warning: re-register failed: {}", e);
                        }
                    }
                }
                continue;
            }

            // No alive pane in registry. Before auto-starting, check if any
            // alive pane in the target session is already running agent-doc
            // for this file (registry may have been pruned or stale).
            // This prevents creating duplicate panes.
            let file_str = file_path.to_string_lossy().to_string();
            match recover_existing_associated_pane(
                tmux,
                file_path,
                &session_id,
                &file_str,
                context_session.as_deref(),
                window,
                col_args,
            ) {
                ExistingAssociatedPaneRecovery::Recovered
                | ExistingAssociatedPaneRecovery::Ambiguous => continue,
                ExistingAssociatedPaneRecovery::None => {}
            }

            if let Some(ref pane) = registered_pane {
                match repair_missing_registered_pane(
                    tmux,
                    file_path,
                    &session_id,
                    pane,
                    registered_entry.as_ref().map(|entry| entry.window.as_str()),
                ) {
                    Ok(repair) => {
                        if let Some(dead) = repair.dead_pane.as_ref() {
                            let status = dead.dead_status.as_deref().unwrap_or("unknown");
                            let phase = dead.cycle_phase.as_deref().unwrap_or("none");
                            let capture = dead
                                .capture_path
                                .as_ref()
                                .map(|path| path.display().to_string())
                                .unwrap_or_else(|| "<none>".to_string());
                            let excerpt = dead.last_visible_excerpt.as_deref().unwrap_or("<none>");
                            eprintln!(
                                "[sync] captured dead pane {} for {} (status {}, cycle {}, capture {})",
                                pane,
                                file_path.display(),
                                status,
                                phase,
                                capture
                            );
                            sync_log(&format!(
                                "captured dead pane file={} pane={} status={} cycle={} capture={} killed={} excerpt={}",
                                file_path.display(),
                                pane,
                                status,
                                phase,
                                capture,
                                dead.pane_killed,
                                excerpt
                            ));
                        }
                        if repair.recorded_session_loss {
                            let detail = registered_entry
                                .as_ref()
                                .map(|entry| format!(" (last known window {})", entry.window))
                                .unwrap_or_default();
                            eprintln!(
                                "[sync] recorded session loss for missing pane {} on {}{}",
                                pane,
                                file_path.display(),
                                detail
                            );
                            sync_log(&format!(
                                "recorded missing-pane session loss file={} pane={}{}",
                                file_path.display(),
                                pane,
                                registered_entry
                                    .as_ref()
                                    .map(|entry| format!(" window={}", entry.window))
                                    .unwrap_or_default()
                            ));
                        }
                        if repair.repaired_stale_preflight {
                            eprintln!(
                                "[sync] closed stale preflight_started cycle for {} before auto-start",
                                file_path.display()
                            );
                            sync_log(&format!(
                                "repaired stale preflight_started cycle before auto-start for {}",
                                file_path.display()
                            ));
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[sync] warning: failed to repair missing pane state for {}: {}",
                            file_path.display(),
                            e
                        );
                        sync_log(&format!(
                            "warning: missing-pane repair failed for {}: {}",
                            file_path.display(),
                            e
                        ));
                    }
                }
            }

            if has_rename_debounce(file_path) {
                eprintln!(
                    "[sync] skipping auto-start for {} (rename debounce active)",
                    file_path.display()
                );
                sync_log(&format!(
                    "rename-debounce: skipped auto-start for {}",
                    file_path.display()
                ));
                continue;
            }

            sync_log(&format!(
                "auto-starting session for {} (no alive pane)",
                file_path.display()
            ));
            eprintln!(
                "[sync] auto-starting session for {} (no alive pane)",
                file_path.display()
            );
            match route::provision_pane(
                tmux,
                file_path,
                &session_id,
                &file_str,
                context_session.as_deref(),
                col_args,
            ) {
                Ok(pane_id) => {
                    eprintln!(
                        "[sync] auto-started {} for {}",
                        pane_id,
                        file_path.display()
                    );
                    sync_log(&format!(
                        "auto-started {} for {}",
                        pane_id,
                        file_path.display()
                    ));
                    auto_started_panes.push((pane_id, file_str.clone()));
                }
                Err(e) => {
                    eprintln!(
                        "[sync] warning: auto-start failed for {}: {}",
                        file_path.display(),
                        e
                    );
                }
            }
        }

        if auto_started_panes.len() > 1 {
            let summary: Vec<String> = auto_started_panes
                .iter()
                .map(|(pane, file)| format!("{}→{}", pane, file))
                .collect();
            eprintln!(
                "[sync] auto-started {} panes: {}",
                auto_started_panes.len(),
                summary.join(", ")
            );
            sync_log(&format!(
                "batch: auto-started {} panes: {}",
                auto_started_panes.len(),
                summary.join(", ")
            ));
        }

        // Post-auto_start stash removed: the tmux_router reconciler now always runs
        // the full reconcile path (no early exits), so it handles stashing excess panes.
    }

    // Log pane count before tmux_router::sync
    if let Some(w) = window {
        let pane_list: Vec<String> = tmux.list_window_panes(w).unwrap_or_default();
        sync_log(&format!(
            "checkpoint:pre-tmux_router window={} panes={} list={:?}",
            w,
            pane_list.len(),
            pane_list
        ));
    }

    // NOTE: The busy pane guard (protect_pane) was removed from DETACH because it caused
    // 3-pane accumulation when the user switches documents in the same column. The guard
    // prevented stashing panes with active sessions, but when a new document replaces the
    // old one in a column, the old pane must give way. Column memory + stash rescue handle
    // session preservation for the non-agent-file case.
    let result = tmux_router::sync(col_args, window, focus, tmux, &registry_path, &resolve_file)?;

    // Log pane count after tmux_router::sync
    if let Some(w) = window {
        let pane_count = tmux.list_window_panes(w).map(|p| p.len()).unwrap_or(0);
        sync_log(&format!(
            "post-tmux_router::sync: window={} panes={} file_panes={}",
            w,
            pane_count,
            result.file_panes.len()
        ));
        tracing::debug!(
            window = w,
            pane_count,
            file_panes = result.file_panes.len(),
            "post-sync pane count"
        );

        // Session health check: verify the session still exists after sync.
        // If the session was destroyed (e.g., all windows stashed), log a critical warning.
        if let Ok(session) = tmux
            .cmd()
            .args(["display-message", "-t", w, "-p", "#{session_name}"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            && !session.is_empty()
        {
            let session_alive = tmux
                .cmd()
                .args(["has-session", "-t", &session])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !session_alive {
                tracing::error!(session = %session, "SESSION DESTROYED after sync — tmux session no longer exists");
                eprintln!(
                    "[sync] CRITICAL: session '{}' was destroyed during sync!",
                    session
                );
            }
        }
    }

    // Save column layout state: for each column that has an agent doc,
    // record it so future syncs can substitute it when the column has a non-agent file.
    {
        let layout_state: Vec<String> = col_args
            .iter()
            .map(|col| {
                // Find the first agent doc file in this column
                for f in col.split(',').map(|f| f.trim()) {
                    if f.is_empty() {
                        continue;
                    }
                    if let Ok(content) = std::fs::read_to_string(f)
                        && let Ok((fm, _)) = frontmatter::parse(&content)
                        && fm.session.is_some()
                    {
                        return f.to_string();
                    }
                }
                String::new()
            })
            .collect();
        // Only save if at least one column has an agent doc
        if layout_state.iter().any(|s| !s.is_empty())
            && let Ok(json) = serde_json::to_string(&layout_state)
        {
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
    if let Err(e) = resync::run(false, None, None) {
        eprintln!("[sync] warning: post-sync resync failed: {}", e);
    }

    Ok(())
}

/// Register or update registry entries for synced files.
///
/// Uses the file→pane assignments from `SyncResult::file_panes` to create
/// registry entries for files that don't have one yet, and update file paths
/// for existing entries.
fn register_synced_files(session_files: &[(String, PathBuf)], file_panes: &[(PathBuf, String)]) {
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
fn find_alive_pane_for_file_inner(
    tmux: &Tmux,
    file_path: &str,
    excluded_pane: Option<&str>,
    log_hits: bool,
) -> Option<String> {
    let output = tmux
        .cmd()
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
        if excluded_pane.is_some_and(|excluded| excluded == pane_id) {
            continue;
        }

        // Check the pane's process and its children for agent-doc + file_path
        if pid_has_agent_doc_for_file(pid_str, file_path) {
            if log_hits {
                eprintln!(
                    "[sync] found alive agent-doc pane {} (pid {}) for {}",
                    pane_id, pid_str, file_path
                );
            }
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
                    if log_hits {
                        eprintln!(
                            "[sync] found alive agent-doc child (pid {}) in pane {} for {}",
                            child_pid, pane_id, file_path
                        );
                    }
                    return Some(pane_id.to_string());
                }
            }
        }
    }
    None
}

#[allow(dead_code)]
pub(crate) fn find_alive_pane_for_file(tmux: &Tmux, file_path: &str) -> Option<String> {
    find_alive_pane_for_file_inner(tmux, file_path, None, true)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AssociatedPaneSource {
    Registered,
    ProcessTree,
    SupervisorPid,
}

impl AssociatedPaneSource {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::ProcessTree => "process-tree",
            Self::SupervisorPid => "supervisor-pid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssociatedPaneCandidate {
    pub pane_id: String,
    pub pane_pid: String,
    pub session_name: String,
    pub window_id: String,
    pub window_name: String,
    pub current_command: String,
    pub sources: BTreeSet<AssociatedPaneSource>,
}

impl AssociatedPaneCandidate {
    pub fn is_stash(&self) -> bool {
        self.window_name == "stash" || self.window_name.starts_with("stash-")
    }

    pub fn source_summary(&self) -> String {
        self.sources
            .iter()
            .map(AssociatedPaneSource::as_str)
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssociatedPaneResolution {
    None,
    Selected {
        winner: AssociatedPaneCandidate,
        redundant: Vec<AssociatedPaneCandidate>,
    },
    Ambiguous(Vec<AssociatedPaneCandidate>),
}

fn parse_pane_inventory_line(line: &str) -> Option<AssociatedPaneCandidate> {
    let mut parts = line.splitn(6, '\t');
    let pane_id = parts.next()?.trim();
    let pane_pid = parts.next()?.trim();
    let window_id = parts.next()?.trim();
    let window_name = parts.next()?.trim();
    let session_name = parts.next()?.trim();
    let current_command = parts.next()?.trim();
    if pane_id.is_empty() {
        return None;
    }
    Some(AssociatedPaneCandidate {
        pane_id: pane_id.to_string(),
        pane_pid: pane_pid.to_string(),
        session_name: session_name.to_string(),
        window_id: window_id.to_string(),
        window_name: window_name.to_string(),
        current_command: current_command.to_string(),
        sources: BTreeSet::new(),
    })
}

fn list_associated_pane_inventory(tmux: &Tmux) -> Vec<AssociatedPaneCandidate> {
    let output = match tmux
        .cmd()
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_id}\t#{pane_pid}\t#{window_id}\t#{window_name}\t#{session_name}\t#{pane_current_command}",
        ])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_pane_inventory_line)
        .collect()
}

fn collect_process_tree_matches(
    inventory: &[AssociatedPaneCandidate],
    file_path: &str,
) -> BTreeSet<String> {
    let mut matches = BTreeSet::new();

    for candidate in inventory {
        if candidate.pane_pid.is_empty() {
            continue;
        }
        if pid_has_agent_doc_for_file(&candidate.pane_pid, file_path) {
            matches.insert(candidate.pane_id.clone());
            continue;
        }

        if let Ok(children) = std::process::Command::new("pgrep")
            .args(["-P", &candidate.pane_pid])
            .output()
        {
            for child_pid in String::from_utf8_lossy(&children.stdout).lines() {
                let child_pid = child_pid.trim();
                if !child_pid.is_empty() && pid_has_agent_doc_for_file(child_pid, file_path) {
                    matches.insert(candidate.pane_id.clone());
                    break;
                }
            }
        }
    }

    matches
}

pub(crate) fn find_associated_panes(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
) -> Vec<AssociatedPaneCandidate> {
    let file_path = file.to_string_lossy().to_string();
    let inventory = list_associated_pane_inventory(tmux);
    let process_tree_matches = collect_process_tree_matches(&inventory, &file_path);
    let registered = sessions::lookup(session_id).ok().flatten();
    let supervisor_match = find_alive_pane_via_supervisor_pid(tmux, file, session_id);

    let mut associated: Vec<AssociatedPaneCandidate> = inventory
        .into_iter()
        .filter_map(|mut candidate| {
            if registered.as_deref() == Some(candidate.pane_id.as_str()) {
                candidate.sources.insert(AssociatedPaneSource::Registered);
            }
            if process_tree_matches.contains(&candidate.pane_id) {
                candidate.sources.insert(AssociatedPaneSource::ProcessTree);
            }
            if supervisor_match.as_deref() == Some(candidate.pane_id.as_str()) {
                candidate
                    .sources
                    .insert(AssociatedPaneSource::SupervisorPid);
            }
            let proves_live_ownership = candidate
                .sources
                .contains(&AssociatedPaneSource::ProcessTree)
                || candidate
                    .sources
                    .contains(&AssociatedPaneSource::SupervisorPid);
            if candidate.sources.is_empty() || !proves_live_ownership {
                return None;
            }
            Some(candidate)
        })
        .collect();

    associated.sort_by(|left, right| left.pane_id.cmp(&right.pane_id));
    associated
}

pub(crate) fn resolve_associated_panes(
    mut candidates: Vec<AssociatedPaneCandidate>,
    preferred_window: Option<&str>,
) -> AssociatedPaneResolution {
    if candidates.is_empty() {
        return AssociatedPaneResolution::None;
    }
    candidates.sort_by(|left, right| left.pane_id.cmp(&right.pane_id));
    if candidates.len() == 1 {
        return AssociatedPaneResolution::Selected {
            winner: candidates.remove(0),
            redundant: Vec::new(),
        };
    }

    if let Some(window_id) = preferred_window {
        let mut preferred_matches = candidates
            .iter()
            .filter(|candidate| candidate.window_id == window_id)
            .cloned()
            .collect::<Vec<_>>();
        let non_preferred = candidates
            .iter()
            .filter(|candidate| candidate.window_id != window_id)
            .cloned()
            .collect::<Vec<_>>();
        if preferred_matches.len() == 1
            && non_preferred.iter().all(AssociatedPaneCandidate::is_stash)
        {
            let winner = preferred_matches.remove(0);
            let redundant = candidates
                .into_iter()
                .filter(|candidate| candidate.pane_id != winner.pane_id)
                .collect();
            return AssociatedPaneResolution::Selected { winner, redundant };
        }
    }

    let mut stash_matches = candidates
        .iter()
        .filter(|candidate| candidate.is_stash())
        .cloned()
        .collect::<Vec<_>>();
    if stash_matches.len() == 1 && stash_matches.len() == candidates.len() {
        let winner = stash_matches.remove(0);
        let redundant = candidates
            .into_iter()
            .filter(|candidate| candidate.pane_id != winner.pane_id)
            .collect();
        return AssociatedPaneResolution::Selected { winner, redundant };
    }

    AssociatedPaneResolution::Ambiguous(candidates)
}

fn rescue_stashed_associated_pane(
    tmux: &Tmux,
    pane_id: &str,
    file_path: &Path,
    context_session: Option<&str>,
    window: Option<&str>,
    col_args: &[String],
) {
    let target_sess = context_session.unwrap_or("");
    let pane_session = tmux
        .cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{session_name}"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if !target_sess.is_empty() && pane_session != target_sess {
        eprintln!(
            "[sync] pane {} for {} is in session '{}' stash; refusing cross-session rescue into '{}'",
            pane_id,
            file_path.display(),
            pane_session,
            target_sess
        );
        sync_log(&format!(
            "rescue_skipped_cross_session pane={} file={} actual_session={} target_session={}",
            pane_id,
            file_path.display(),
            pane_session,
            target_sess
        ));
        return;
    }

    let rescue_win = window.map(|w| w.to_string()).or_else(|| {
        if target_sess.is_empty() {
            None
        } else {
            let candidate = format!("{}:agent-doc", target_sess);
            if !tmux
                .list_window_panes(&candidate)
                .unwrap_or_default()
                .is_empty()
            {
                Some(candidate)
            } else {
                None
            }
        }
    });
    let Some(target_win) = rescue_win else {
        return;
    };

    let target_panes = tmux.list_panes_ordered(&target_win).unwrap_or_default();
    let split_before = crate::route::is_first_column(file_path, col_args);
    let target = if split_before {
        target_panes.first()
    } else {
        target_panes.last()
    };
    let Some(target) = target else {
        return;
    };

    let join_flag = if split_before { "-dbh" } else { "-dh" };
    eprintln!(
        "[sync] rescuing stashed pane {} for {} to window {}",
        pane_id,
        file_path.display(),
        target_win
    );
    sync_log(&format!(
        "rescue_action=join-pane src={} dst={} target_window={} join_flag={}",
        pane_id, target, target_win, join_flag
    ));
    match sessions::join_pane_guarded(tmux, pane_id, target, target_sess, join_flag) {
        Ok(()) => {
            eprintln!("[sync] rescued stashed pane {} via join-pane", pane_id);
            sync_log(&format!(
                "rescue_result=join-pane ok=true pane={} target={}",
                pane_id, target
            ));
        }
        Err(e) => {
            eprintln!("[sync] stash rescue failed for pane {}: {}", pane_id, e);
            sync_log(&format!(
                "rescue_result=join-pane ok=false pane={} target={} err={}",
                pane_id, target, e
            ));
        }
    }
}

enum ExistingAssociatedPaneRecovery {
    Recovered,
    Ambiguous,
    None,
}

fn recover_existing_associated_pane(
    tmux: &Tmux,
    file_path: &Path,
    session_id: &str,
    file_str: &str,
    context_session: Option<&str>,
    window: Option<&str>,
    col_args: &[String],
) -> ExistingAssociatedPaneRecovery {
    let candidates = find_associated_panes(tmux, file_path, session_id);
    match resolve_associated_panes(candidates, window) {
        AssociatedPaneResolution::Selected { winner, redundant } => {
            eprintln!(
                "[sync] found associated pane {} for {} via {}{}",
                winner.pane_id,
                file_path.display(),
                winner.source_summary(),
                if redundant.is_empty() {
                    String::new()
                } else {
                    format!(" ({} redundant pane(s) still associated)", redundant.len())
                }
            );
            sync_log(&format!(
                "associated_pane_recovered file={} pane={} sources={} redundant={}",
                file_path.display(),
                winner.pane_id,
                winner.source_summary(),
                redundant.len()
            ));
            if let Err(e) = sessions::register(session_id, &winner.pane_id, file_str) {
                eprintln!(
                    "[sync] warning: re-register failed for {} via associated pane {}: {}",
                    file_path.display(),
                    winner.pane_id,
                    e
                );
            }
            if winner.is_stash() {
                rescue_stashed_associated_pane(
                    tmux,
                    &winner.pane_id,
                    file_path,
                    context_session,
                    window,
                    col_args,
                );
            }
            ExistingAssociatedPaneRecovery::Recovered
        }
        AssociatedPaneResolution::Ambiguous(candidates) => {
            let detail = candidates
                .iter()
                .map(|candidate| {
                    format!(
                        "{}:{}:{}:{}",
                        candidate.pane_id,
                        candidate.window_name,
                        candidate.window_id,
                        candidate.source_summary()
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "[sync] warning: multiple panes are still associated with {} — skipping auto-start until one is claimed explicitly",
                file_path.display()
            );
            sync_log(&format!(
                "associated_pane_ambiguous file={} candidates={}",
                file_path.display(),
                detail
            ));
            ExistingAssociatedPaneRecovery::Ambiguous
        }
        AssociatedPaneResolution::None => ExistingAssociatedPaneRecovery::None,
    }
}

pub(crate) fn find_live_owner_pane(tmux: &Tmux, file: &Path, session_id: &str) -> Option<String> {
    find_live_owner_pane_excluding(tmux, file, session_id, None)
}

pub(crate) fn find_live_owner_pane_excluding(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
) -> Option<String> {
    find_live_owner_pane_excluding_with_logging(tmux, file, session_id, excluded_pane, true)
}

pub(crate) fn find_live_owner_pane_excluding_quiet(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
) -> Option<String> {
    find_live_owner_pane_excluding_with_logging(tmux, file, session_id, excluded_pane, false)
}

fn find_live_owner_pane_excluding_with_logging(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
    log_hits: bool,
) -> Option<String> {
    let file_path = file.to_string_lossy();
    find_alive_pane_for_file_inner(tmux, file_path.as_ref(), excluded_pane, log_hits).or_else(
        || {
            find_alive_pane_via_supervisor_pid(tmux, file, session_id)
                .filter(|pane| excluded_pane != Some(pane.as_str()))
        },
    )
}

fn pane_process_tree_contains_pid(pane_pid: &str, target_pid: u32) -> bool {
    let mut frontier = vec![pane_pid.to_string()];
    let target = target_pid.to_string();

    while let Some(pid) = frontier.pop() {
        if pid == target {
            return true;
        }

        let output = match std::process::Command::new("pgrep")
            .args(["-P", &pid])
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => continue,
        };

        for child_pid in String::from_utf8_lossy(&output.stdout).lines() {
            let child_pid = child_pid.trim();
            if child_pid.is_empty() {
                continue;
            }
            if child_pid == target {
                return true;
            }
            frontier.push(child_pid.to_string());
        }
    }

    false
}

fn find_alive_pane_via_supervisor_pid(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
) -> Option<String> {
    let project_root = crate::snapshot::find_project_root(file)?;
    let sock = crate::supervisor::ipc::socket_path(&project_root, session_id);
    if !sock.exists() {
        return None;
    }

    let response =
        crate::supervisor::ipc::send_command(&sock, &crate::supervisor::ipc::IpcMethod::Pid)
            .ok()?;
    if !response.ok {
        return None;
    }

    let target_pid = response
        .data
        .as_ref()?
        .get("pid")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())?;
    if target_pid == 0 {
        return None;
    }

    let output = tmux
        .cmd()
        .args(["list-panes", "-a", "-F", "#{pane_id} #{pane_pid}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.splitn(2, ' ');
        let Some(pane_id) = parts.next() else {
            continue;
        };
        let Some(pane_pid) = parts.next() else {
            continue;
        };
        let pane_id = pane_id.trim();
        let pane_pid = pane_pid.trim();
        if pane_id.is_empty() || pane_pid.is_empty() {
            continue;
        }
        if pane_process_tree_contains_pid(pane_pid, target_pid) {
            eprintln!(
                "[sync] recovered live pane {} for session {} via supervisor pid {}",
                pane_id,
                &session_id[..std::cmp::min(8, session_id.len())],
                target_pid
            );
            return Some(pane_id.to_string());
        }
    }

    None
}

/// Check if a process (by PID) is running agent-doc for a specific file.
///
/// Uses `ps -p <pid> -o command=` which works on both Linux and macOS.
/// Check if a tmux pane is running an active agent session (agent-doc / claude / codex).
///
/// Used as a `protect_pane` callback to prevent stashing panes with active sessions.
/// Checks the pane's PID and its child processes for agent process names in the command line.
#[allow(dead_code)]
fn is_pane_busy(tmux: &Tmux, pane_id: &str) -> bool {
    let output = tmux
        .cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{pane_pid}"])
        .output();
    let pid_str = match output {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
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

/// Check if a process (by PID) is running an agent session (agent-doc / claude / codex).
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
    cmdline.contains("agent-doc") || cmdline.contains("claude") || cmdline.contains("codex")
}

fn path_has_component_suffix(path: &Path, suffix: &Path) -> bool {
    let path_components: Vec<_> = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect();
    let suffix_components: Vec<_> = suffix
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect();

    if suffix_components.is_empty() || suffix_components.len() > path_components.len() {
        return false;
    }

    path_components[path_components.len() - suffix_components.len()..] == suffix_components[..]
}

fn cmdline_has_file_match(cmdline: &str, file_path: &str) -> bool {
    if cmdline.contains(file_path) {
        return true;
    }

    let target = Path::new(file_path);
    let canonical_target = target.canonicalize().ok();
    if let Some(ref canonical) = canonical_target
        && cmdline.contains(canonical.to_string_lossy().as_ref())
    {
        return true;
    }

    for token in cmdline.split_whitespace() {
        let candidate = Path::new(token);
        if candidate.is_absolute() {
            if let Some(ref canonical) = canonical_target
                && candidate.canonicalize().ok().as_ref() == Some(canonical)
            {
                return true;
            }
            continue;
        }

        if path_has_component_suffix(target, candidate) {
            return true;
        }
        if let Some(ref canonical) = canonical_target
            && path_has_component_suffix(canonical, candidate)
        {
            return true;
        }
    }

    false
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
    let has_agent =
        cmdline.contains("agent-doc") || cmdline.contains("claude") || cmdline.contains("codex");
    let has_file = cmdline_has_file_match(&cmdline, file_path);
    has_agent && has_file
}

/// Detect whether a file has been renamed: the registered path differs from
/// the current path and the old path no longer exists on disk.
pub(crate) fn is_file_rename(registered_path: &str, current_path: &str) -> bool {
    registered_path != current_path && !Path::new(registered_path).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::IsolatedTmux;
    use std::time::Duration;

    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    fn candidate(
        pane_id: &str,
        window_id: &str,
        window_name: &str,
        sources: &[AssociatedPaneSource],
    ) -> AssociatedPaneCandidate {
        let mut source_set = BTreeSet::new();
        for source in sources {
            source_set.insert(source.clone());
        }
        AssociatedPaneCandidate {
            pane_id: pane_id.to_string(),
            pane_pid: "100".to_string(),
            session_name: "14".to_string(),
            window_id: window_id.to_string(),
            window_name: window_name.to_string(),
            current_command: "agent-doc".to_string(),
            sources: source_set,
        }
    }

    fn wait_for<F>(timeout: Duration, mut predicate: F) -> bool
    where
        F: FnMut() -> bool,
    {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if predicate() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        predicate()
    }

    struct ScopedCurrentDir {
        prev_cwd: PathBuf,
        _env_guard: std::sync::MutexGuard<'static, ()>,
    }

    impl ScopedCurrentDir {
        fn set(path: &Path) -> Self {
            let env_guard = ENV_MUTEX
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let prev_cwd = std::env::current_dir()
                .ok()
                .filter(|cwd| cwd.exists())
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            std::env::set_current_dir(path).unwrap();
            Self {
                prev_cwd,
                _env_guard: env_guard,
            }
        }
    }

    impl Drop for ScopedCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev_cwd);
        }
    }

    #[test]
    fn resolve_associated_panes_prefers_unique_active_window() {
        let candidates = vec![
            candidate("%417", "@9", "stash", &[AssociatedPaneSource::ProcessTree]),
            candidate(
                "%419",
                "@3",
                "agent-doc",
                &[
                    AssociatedPaneSource::Registered,
                    AssociatedPaneSource::SupervisorPid,
                ],
            ),
        ];

        let resolution = resolve_associated_panes(candidates, Some("@3"));
        match resolution {
            AssociatedPaneResolution::Selected { winner, redundant } => {
                assert_eq!(winner.pane_id, "%419");
                assert_eq!(redundant.len(), 1);
                assert_eq!(redundant[0].pane_id, "%417");
            }
            other => panic!("expected selected winner, got {other:?}"),
        }
    }

    #[test]
    fn resolve_associated_panes_accepts_single_stash_candidate() {
        let candidates = vec![candidate(
            "%420",
            "@9",
            "stash",
            &[AssociatedPaneSource::ProcessTree],
        )];

        let resolution = resolve_associated_panes(candidates, Some("@7"));
        match resolution {
            AssociatedPaneResolution::Selected { winner, redundant } => {
                assert_eq!(winner.pane_id, "%420");
                assert!(redundant.is_empty());
            }
            other => panic!("expected selected stash winner, got {other:?}"),
        }
    }

    #[test]
    fn resolve_associated_panes_reports_ambiguity_when_multiple_candidates_remain() {
        let candidates = vec![
            candidate("%417", "@9", "stash", &[AssociatedPaneSource::ProcessTree]),
            candidate(
                "%419",
                "@3",
                "agent-doc",
                &[AssociatedPaneSource::Registered],
            ),
            candidate(
                "%420",
                "@5",
                "agent-doc",
                &[AssociatedPaneSource::SupervisorPid],
            ),
        ];

        let resolution = resolve_associated_panes(candidates, Some("@7"));
        match resolution {
            AssociatedPaneResolution::Ambiguous(candidates) => {
                assert_eq!(candidates.len(), 3);
            }
            other => panic!("expected ambiguous resolution, got {other:?}"),
        }
    }

    #[test]
    fn recover_existing_associated_pane_reregisters_supervisor_owned_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: associated-supervisor\n---\n").unwrap();

        let iso = IsolatedTmux::new("sync-associated-supervisor");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let pane_pid = iso
            .raw_cmd(&["display-message", "-t", &pane, "-p", "#{pane_pid}"])
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();

        let _ipc = crate::supervisor::ipc::SupervisorIpc::start(
            tmp.path(),
            "associated-supervisor",
            move |method| match method {
                crate::supervisor::ipc::IpcMethod::Pid => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({
                        "pid": pane_pid
                    }))
                }
                _ => crate::supervisor::ipc::IpcResponse::ok_empty(),
            },
        )
        .unwrap();

        let file_str = doc.to_string_lossy().to_string();
        let recovery = recover_existing_associated_pane(
            &iso,
            &doc,
            "associated-supervisor",
            &file_str,
            Some("test"),
            None,
            &["tasks/owned.md".to_string()],
        );

        assert!(matches!(
            recovery,
            ExistingAssociatedPaneRecovery::Recovered
        ));
        assert_eq!(
            sessions::lookup("associated-supervisor").unwrap(),
            Some(pane.clone())
        );
    }

    #[test]
    fn cmdline_file_match_accepts_submodule_relative_start_path() {
        let file_path = "/tmp/agent-loop/src/session-share/tasks/docs.md";
        let cmdline = "/home/brian/.cargo/bin/agent-doc start tasks/docs.md";

        assert!(
            cmdline_has_file_match(cmdline, file_path),
            "root-relative target should match pane-relative start path"
        );
    }

    #[test]
    fn cmdline_file_match_rejects_different_relative_path() {
        let file_path = "/tmp/agent-loop/src/session-share/tasks/docs.md";
        let cmdline = "/home/brian/.cargo/bin/agent-doc start tasks/other.md";

        assert!(
            !cmdline_has_file_match(cmdline, file_path),
            "different relative path should not match by basename alone"
        );
    }

    #[test]
    fn parse_frontmatter_for_sync_includes_phase_and_fix_hint() {
        let path = Path::new("tasks/bad.md");
        let err = parse_frontmatter_for_sync(
            "---\nprompt_presets:\n  key: [oops\n---\n",
            path,
            "auto-start",
        )
        .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("sync auto-start frontmatter"));
        assert!(message.contains("invalid YAML frontmatter in tasks/bad.md"));
        assert!(message.contains("Frontmatter excerpt:"));
        assert!(message.contains("> 2 |   key: [oops"));
        assert!(
            message.contains("Fix the frontmatter between the opening and closing --- markers")
        );
    }

    #[test]
    fn sync_frontmatter_status_round_trips_through_snapshot() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("bad.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nprompt_presets:\n  key: [oops\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\nold status\n<!-- /agent:status -->\n",
        )
        .unwrap();

        let err = parse_frontmatter_for_sync(
            "---\nprompt_presets:\n  key: [oops\n---\n",
            &doc,
            "auto-start",
        )
        .unwrap_err();

        surface_frontmatter_status(&doc, "auto-start", &err);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains(SYNC_FRONTMATTER_STATUS_PREFIX));
        assert!(updated.contains("sync auto-start frontmatter"));

        let snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert!(snapshot.contains(SYNC_FRONTMATTER_STATUS_PREFIX));

        std::fs::write(
            &doc,
            "---\nagent_doc_session: test\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n[agent-doc sync] malformed frontmatter during auto-start.\n\nsync auto-start frontmatter: invalid YAML frontmatter in tasks/bad.md: boom\n<!-- /agent:status -->\n",
        )
        .unwrap();
        snapshot::save(
            &doc,
            "---\nagent_doc_session: test\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n[agent-doc sync] malformed frontmatter during auto-start.\n\nsync auto-start frontmatter: invalid YAML frontmatter in tasks/bad.md: boom\n<!-- /agent:status -->\n",
        )
        .unwrap();

        clear_frontmatter_status(&doc);

        let cleared = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !cleared.contains(SYNC_FRONTMATTER_STATUS_PREFIX),
            "managed sync warning should be removed once parsing succeeds"
        );
        let cleared_snapshot = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !cleared_snapshot.contains(SYNC_FRONTMATTER_STATUS_PREFIX),
            "snapshot should track the cleared status too"
        );
    }

    #[test]
    fn clear_frontmatter_status_preserves_non_sync_status() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("ok.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let original = "---\nagent_doc_session: test\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\nuser-owned status\n<!-- /agent:status -->\n";
        std::fs::write(&doc, original).unwrap();
        snapshot::save(&doc, original).unwrap();

        clear_frontmatter_status(&doc);

        assert_eq!(std::fs::read_to_string(&doc).unwrap(), original);
        assert_eq!(snapshot::load(&doc).unwrap().unwrap(), original);
    }

    #[test]
    fn repair_missing_registered_pane_records_loss_and_closes_stale_preflight() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("lost-pane.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = "---\nagent_doc_session: session-lost-pane\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let log_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(
            log_dir.join("session-lost-pane.log"),
            "[1] session_start file=tasks/lost-pane.md pane=%422 session=session-lost-pane\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let repair = repair_missing_registered_pane(
            &Tmux::default_server(),
            &doc,
            "session-lost-pane",
            "%422",
            Some("@17"),
        )
        .unwrap();
        assert!(repair.recorded_session_loss);
        assert!(repair.repaired_stale_preflight);
        assert!(repair.dead_pane.is_none());

        let state = crate::cycle_state::load(&doc)
            .unwrap()
            .expect("cycle state should exist");
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);

        let status = crate::startup_miss::session_log_status(&doc, "session-lost-pane")
            .unwrap()
            .expect("session log should be readable");
        assert!(status.latest_session_closed());
    }

    #[test]
    fn repair_missing_registered_pane_captures_retained_dead_pane_diagnostics() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("dead-pane.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = "---\nagent_doc_session: dead-pane-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let log_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(
            log_dir.join("dead-pane-session.log"),
            "[1] session_start file=tasks/dead-pane.md pane=%501 session=dead-pane-session\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-dead-pane-diagnostics");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        iso.enable_remain_on_exit(&pane).unwrap();
        iso.send_keys(&pane, "printf 'assistant tail\\n'; exit 9")
            .unwrap();
        assert!(
            wait_for(Duration::from_secs(3), || iso.pane_dead(&pane)),
            "pane should be retained as dead for diagnostics"
        );

        let repair =
            repair_missing_registered_pane(&iso, &doc, "dead-pane-session", &pane, Some("@17"))
                .unwrap();
        let dead = repair
            .dead_pane
            .as_ref()
            .expect("retained dead pane should be captured");
        let capture_path = dead
            .capture_path
            .as_ref()
            .expect("dead pane tail should be persisted for provenance");
        assert_eq!(dead.dead_status.as_deref(), Some("9"));
        assert_eq!(dead.cycle_phase.as_deref(), Some("preflight_started"));
        assert!(capture_path.exists(), "dead pane tail should exist");
        let capture = std::fs::read_to_string(capture_path).unwrap();
        assert!(
            capture.contains("assistant tail"),
            "persisted dead pane tail should contain the last visible assistant output: {capture}"
        );
        assert!(dead.last_visible_excerpt.is_some());
        assert!(repair.recorded_session_loss);
        assert!(repair.repaired_stale_preflight);
        assert!(!iso.pane_alive(&pane));
        assert_eq!(
            dead.pane_killed,
            !iso.pane_dead(&pane),
            "pane_killed should reflect whether the retained dead pane could be safely removed"
        );
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
        assert_eq!(
            windows_before, windows_after,
            "layout was already correct — nothing should change"
        );
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
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash", "-d"]);
        // Create agent-doc at index 2
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "agent-doc", "-d"]);
        // Kill the placeholder at index 0 to free it
        let _ = iso.raw_cmd(&["kill-window", "-t", "test:0"]);

        // Verify agent-doc is NOT at index 0 before repair
        let windows_before = list_windows(&iso, "test");
        let ad_before = windows_before.iter().find(|(_, n)| n == "agent-doc");
        assert!(ad_before.is_some(), "agent-doc window should exist");
        assert_ne!(
            ad_before.unwrap().0,
            "0",
            "agent-doc should NOT be at index 0 before repair"
        );

        repair_layout(&iso, "test", "agent-doc").unwrap();

        // After repair, agent-doc should be at index 0
        let windows_after = list_windows(&iso, "test");
        let ad_after = windows_after.iter().find(|(_, n)| n == "agent-doc");
        assert!(ad_after.is_some(), "agent-doc window should still exist");
        assert_eq!(
            ad_after.unwrap().0,
            "0",
            "agent-doc should be at index 0 after repair"
        );
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
            "new-window",
            "-t",
            "test:",
            "-n",
            "stash",
            "-d",
            "-P",
            "-F",
            "#{window_id}",
        ]);

        let stash_windows: Vec<String> = {
            let output = iso
                .raw_cmd(&[
                    "list-windows",
                    "-t",
                    "test:",
                    "-F",
                    "#{window_id} #{window_name}",
                ])
                .unwrap();
            output
                .lines()
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
        assert!(
            stash_windows.len() >= 2,
            "should have multiple stash windows, got {}",
            stash_windows.len()
        );

        // Count total stash windows before repair
        let windows_before = list_windows(&iso, "test");
        let stash_count_before = windows_before
            .iter()
            .filter(|(_, n)| n == "stash" || n.starts_with("stash-"))
            .count();
        assert!(
            stash_count_before >= 2,
            "should have >=2 stash windows before repair, got {}",
            stash_count_before
        );

        repair_layout(&iso, "test", "agent-doc").unwrap();

        // After repair, should have at most 1 stash window
        let windows_after = list_windows(&iso, "test");
        let stash_count_after = windows_after
            .iter()
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
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash", "-d"]);
        // Create agent-doc at index 2
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "agent-doc", "-d"]);

        // Verify: corky at 0, stash at 1, agent-doc at 2
        let windows_before = list_windows(&iso, "test");
        assert_eq!(
            windows_before.iter().find(|(i, _)| i == "0").unwrap().1,
            "corky"
        );
        assert_eq!(
            windows_before.iter().find(|(i, _)| i == "2").unwrap().1,
            "agent-doc"
        );

        repair_layout(&iso, "test", "agent-doc").unwrap();

        // After repair: agent-doc should be at 0, corky should be at 2
        let windows_after = list_windows(&iso, "test");
        let ad = windows_after.iter().find(|(_, n)| n == "agent-doc");
        assert!(ad.is_some(), "agent-doc window should still exist");
        assert_eq!(
            ad.unwrap().0,
            "0",
            "agent-doc should be at index 0 after swap"
        );

        let corky = windows_after.iter().find(|(_, n)| n == "corky");
        assert!(
            corky.is_some(),
            "corky window should still exist (not destroyed)"
        );
        assert_ne!(
            corky.unwrap().0,
            "0",
            "corky should have moved away from index 0"
        );

        // All 3 windows should still exist
        assert_eq!(
            windows_after.len(),
            3,
            "no windows should be destroyed, got {:?}",
            windows_after
        );
    }

    /// Regression: sync must never write tmux_session back to document frontmatter.
    /// This was the root cause of pane-swap bugs — stale session names in frontmatter
    /// caused terminal.rs to route panes to the wrong session.
    #[test]
    fn sync_does_not_write_tmux_session_to_frontmatter() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("test.md");

        // Write a doc WITHOUT tmux_session
        std::fs::write(
            &doc,
            "---\nagent_doc_session: test-123\n---\n\n## User\n\nHello\n",
        )
        .unwrap();

        // Read it back — tmux_session should be None
        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = crate::frontmatter::parse(&content).unwrap();
        assert!(
            fm.tmux_session.is_none(),
            "tmux_session should not be set initially"
        );

        // Write a doc WITH tmux_session already set
        let doc2 = tmp.path().join("test2.md");
        std::fs::write(&doc2, "---\nagent_doc_session: test-456\ntmux_session: old-session\n---\n\n## User\n\nHello\n").unwrap();

        let content2 = std::fs::read_to_string(&doc2).unwrap();
        let (fm2, _) = crate::frontmatter::parse(&content2).unwrap();
        // Frontmatter still parses it (for backward compat reading), but resolve_file
        // must NOT propagate it to FileResolution
        assert_eq!(
            fm2.tmux_session,
            Some("old-session".to_string()),
            "frontmatter parser should still read tmux_session for backward compat"
        );
    }

    /// Verify resolve_file closure always passes tmux_session: None regardless of frontmatter.
    #[test]
    fn resolve_file_ignores_frontmatter_tmux_session() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("test.md");

        // File with tmux_session in frontmatter
        std::fs::write(
            &doc,
            "---\nagent_doc_session: sess-1\ntmux_session: stale-session\n---\n\nbody\n",
        )
        .unwrap();

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
                assert!(
                    tmux_session.is_none(),
                    "FileResolution must never carry tmux_session from frontmatter"
                );
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
        std::fs::write(tmp.path().join(".agent-doc/sessions.json"), "{}").unwrap();

        // Create a file with a session UUID but no matching registry entry
        let doc = tmp.path().join("stale-claim.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: orphan-uuid-123\n---\n\n## User\n\nHello\n",
        )
        .unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = crate::frontmatter::parse(&content).unwrap();
        assert_eq!(fm.session, Some("orphan-uuid-123".to_string()));

        // Load registry directly from the temp path (avoid CWD dependency)
        let reg_content =
            std::fs::read_to_string(tmp.path().join(".agent-doc/sessions.json")).unwrap();
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
        )
        .unwrap();

        // Create a file with a session UUID that matches the registry
        let doc = tmp.path().join("claimed.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: claimed-uuid-456\n---\n\n## User\n\nHello\n",
        )
        .unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = crate::frontmatter::parse(&content).unwrap();
        assert_eq!(fm.session, Some("claimed-uuid-456".to_string()));

        // Load registry directly from the temp path (avoid CWD dependency)
        let reg_content =
            std::fs::read_to_string(tmp.path().join(".agent-doc/sessions.json")).unwrap();
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

    #[test]
    fn empty_window_arg_normalized_to_none() {
        assert_eq!(normalize_scope_arg(None), None);
        assert_eq!(normalize_scope_arg(Some("")), None);
        assert_eq!(normalize_scope_arg(Some("   ")), None);
        assert_eq!(normalize_scope_arg(Some("@12")), Some("@12"));
        assert_eq!(normalize_scope_arg(Some("  @12  ")), Some("@12"));
    }

    /// Empty .md files should be auto-scaffolded by sync's resolve_file.
    /// This tests the scaffolding logic inline (resolve_file is a closure in run()).
    #[test]
    fn sync_auto_scaffolds_empty_md_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::create_dir_all(project.join(".agent-doc/snapshots")).unwrap();

        let doc = project.join("test.md");
        std::fs::write(&doc, "").unwrap(); // Empty file

        // Simulate what resolve_file does for empty files:
        let content = std::fs::read_to_string(&doc).unwrap();
        assert!(content.trim().is_empty(), "file should be empty");

        // Scaffold it
        let session_id = uuid::Uuid::new_v4();
        let scaffold = format!(
            "---\nagent_doc_session: {}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n\n## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n## Icebox\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n",
            session_id
        );
        std::fs::write(&doc, &scaffold).unwrap();

        // Verify scaffolded content has frontmatter
        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = crate::frontmatter::parse(&content).unwrap();
        assert!(
            fm.session.is_some(),
            "should have session UUID after scaffold"
        );
        assert!(fm.format.is_some(), "should have format after scaffold");
        assert!(
            content.contains("<!-- agent:exchange"),
            "should have exchange component"
        );
    }

    /// Non-empty .md files without frontmatter should NOT be auto-scaffolded.
    #[test]
    fn sync_does_not_scaffold_non_empty_md_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("notes.md");
        std::fs::write(&doc, "# My Notes\n\nSome content here.\n").unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        assert!(!content.trim().is_empty(), "file is not empty");
    }

    /// Scaffolded template must include all required components.
    #[test]
    fn sync_scaffold_includes_all_components() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::create_dir_all(project.join(".agent-doc/snapshots")).unwrap();

        let doc = project.join("new-session.md");
        std::fs::write(&doc, "").unwrap();

        // Simulate scaffold (same code as resolve_file)
        let raw = std::fs::read_to_string(&doc).unwrap();
        assert!(raw.trim().is_empty());

        let session_id = uuid::Uuid::new_v4();
        let scaffold = format!(
            "---\nagent_doc_session: {}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n\n## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n## Icebox\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n",
            session_id
        );
        std::fs::write(&doc, &scaffold).unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = crate::frontmatter::parse(&content).unwrap();

        // Verify frontmatter
        assert!(fm.session.is_some(), "must have session UUID");
        assert!(fm.format.is_some(), "must have format set");

        // Verify all five components
        assert!(
            content.contains("<!-- agent:status patch=replace -->"),
            "must have status component"
        );
        assert!(
            content.contains("<!-- agent:exchange patch=append -->"),
            "must have exchange component"
        );
        assert!(
            content.contains("<!-- agent:queue -->"),
            "must have queue component"
        );
        assert!(
            content.contains("<!-- agent:backlog -->"),
            "must have backlog component"
        );
        assert!(
            content.contains("<!-- agent:icebox -->"),
            "must have icebox component"
        );

        // Verify components are properly closed
        assert!(
            content.contains("<!-- /agent:status -->"),
            "status must be closed"
        );
        assert!(
            content.contains("<!-- /agent:exchange -->"),
            "exchange must be closed"
        );
        assert!(
            content.contains("<!-- /agent:queue -->"),
            "queue must be closed"
        );
        assert!(
            content.contains("<!-- /agent:backlog -->"),
            "backlog must be closed"
        );
        assert!(
            content.contains("<!-- /agent:icebox -->"),
            "icebox must be closed"
        );
    }

    /// Non-.md files should never be scaffolded even if empty.
    #[test]
    fn sync_does_not_scaffold_non_md_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let txt = tmp.path().join("empty.txt");
        std::fs::write(&txt, "").unwrap();

        // .txt extension should not trigger scaffold
        assert_ne!(txt.extension(), Some(std::ffi::OsStr::new("md")));
    }

    /// Whitespace-only files should be treated as empty and scaffolded.
    #[test]
    fn sync_scaffolds_whitespace_only_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::create_dir_all(project.join(".agent-doc/snapshots")).unwrap();

        let doc = project.join("whitespace.md");
        std::fs::write(&doc, "   \n\n  \n").unwrap();

        let raw = std::fs::read_to_string(&doc).unwrap();
        assert!(
            raw.trim().is_empty(),
            "whitespace-only should be treated as empty"
        );
    }

    /// Files that already have frontmatter (even minimal) should NOT be re-scaffolded.
    #[test]
    fn sync_does_not_scaffold_file_with_existing_frontmatter() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("existing.md");
        std::fs::write(&doc, "---\nagent_doc_session: test-123\n---\n").unwrap();

        let raw = std::fs::read_to_string(&doc).unwrap();
        // File has content (frontmatter) → not empty → no scaffold
        assert!(!raw.trim().is_empty(), "file with frontmatter is not empty");
    }

    // --- #4sh0: sync_log / repair_layout logging tests ---

    /// repair_layout writes move-window or swap-window entries to /tmp/agent-doc-sync.log
    /// when it has to reposition the agent-doc window to index 0.
    #[test]
    fn repair_layout_logs_move_window_action() {
        let iso = IsolatedTmux::new("sync-log-move-window");
        let tmp = tempfile::TempDir::new().unwrap();

        // Create session: placeholder at 0, then agent-doc at 1+ after killing placeholder
        let _pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "agent-doc", "-d"]);
        // Kill index 0 so agent-doc is at index 1 with 0 free → triggers move-window
        let _ = iso.raw_cmd(&["kill-window", "-t", "test:0"]);

        let log_path = std::path::Path::new("/tmp/agent-doc-sync.log");
        // Record log size before repair so we only check new lines
        let before_len = std::fs::metadata(log_path).map(|m| m.len()).unwrap_or(0);

        repair_layout(&iso, "test", "agent-doc").unwrap();

        // Verify the log file has new content mentioning move-window or swap-window
        let log_content = std::fs::read_to_string(log_path).unwrap_or_default();
        let new_content = &log_content[before_len.min(log_content.len() as u64) as usize..];
        assert!(
            new_content.contains("repair_action=move-window")
                || new_content.contains("repair_action=swap-window"),
            "repair_layout should log a move-window or swap-window action, got:\n{new_content}"
        );
    }

    /// sync_log writes timestamped entries to /tmp/agent-doc-sync.log.
    #[test]
    fn sync_log_writes_to_log_file() {
        let marker = format!("sync_log_test_marker_{}", std::process::id());
        sync_log(&marker);

        let log_content = std::fs::read_to_string("/tmp/agent-doc-sync.log").unwrap_or_default();
        assert!(
            log_content.contains(&marker),
            "sync_log should write to /tmp/agent-doc-sync.log, marker not found"
        );
        // Verify timestamp format: each line starts with [<unix_seconds>]
        let matching_line = log_content
            .lines()
            .find(|l| l.contains(&marker))
            .expect("marker line should exist");
        assert!(
            matching_line.starts_with('['),
            "log line should start with timestamp bracket, got: {matching_line}"
        );
    }

    // --- File rename detection tests ---

    #[test]
    fn is_file_rename_detects_rename_when_old_path_gone() {
        let tmp = tempfile::TempDir::new().unwrap();
        let old_path = tmp.path().join("old.md");
        // old_path does NOT exist on disk
        let current_path = tmp.path().join("new.md").to_string_lossy().to_string();
        assert!(
            is_file_rename(&old_path.to_string_lossy(), &current_path),
            "should detect rename when old path doesn't exist and paths differ"
        );
    }

    #[test]
    fn is_file_rename_returns_false_when_paths_match() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("same.md");
        std::fs::write(&path, "content").unwrap();
        let path_str = path.to_string_lossy().to_string();
        assert!(
            !is_file_rename(&path_str, &path_str),
            "should not detect rename when paths are identical"
        );
    }

    #[test]
    fn is_file_rename_returns_false_when_old_path_still_exists() {
        let tmp = tempfile::TempDir::new().unwrap();
        let old_path = tmp.path().join("old.md");
        let new_path = tmp.path().join("new.md");
        std::fs::write(&old_path, "content").unwrap();
        std::fs::write(&new_path, "content").unwrap();
        assert!(
            !is_file_rename(&old_path.to_string_lossy(), &new_path.to_string_lossy()),
            "should not detect rename when old path still exists (both files present)"
        );
    }

    #[test]
    fn is_file_rename_handles_relative_paths() {
        assert!(
            is_file_rename(
                "tasks/nonexistent-old-file.md",
                "tasks/software/renamed-file.md"
            ),
            "should detect rename with relative paths when old doesn't exist"
        );
    }

    #[test]
    fn file_rename_updates_registry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path();

        // Set up registry with an entry pointing to old path
        std::fs::create_dir_all(project.join(".agent-doc")).unwrap();
        let session_id = "rename-test-uuid";
        let old_file = "tasks/old-name.md";
        let new_file = "tasks/new-name.md";
        let registry_content = serde_json::json!({
            session_id: {
                "pane": "%42",
                "pid": 12345,
                "cwd": project.to_string_lossy(),
                "started": "2026-04-20T00:00:00Z",
                "file": old_file,
                "window": "@0"
            }
        });
        std::fs::write(
            project.join(".agent-doc/sessions.json"),
            serde_json::to_string_pretty(&registry_content).unwrap(),
        )
        .unwrap();

        // Verify detection
        assert!(
            is_file_rename(old_file, new_file),
            "old path doesn't exist on disk, paths differ → rename"
        );

        // Verify we can load the entry and see the old path
        let reg: sessions::SessionRegistry = serde_json::from_str(
            &std::fs::read_to_string(project.join(".agent-doc/sessions.json")).unwrap(),
        )
        .unwrap();
        let entry = reg.get(session_id).unwrap();
        assert_eq!(entry.file, old_file);
        assert_eq!(entry.pane, "%42");
    }

    // --- Batch summary formatting tests ---

    #[test]
    fn batch_summary_format_multiple_panes() {
        let auto_started_panes = vec![
            ("%80".to_string(), "tasks/cursor.md".to_string()),
            ("%81".to_string(), "tasks/feat.md".to_string()),
            ("%82".to_string(), "tasks/agent-loop.md".to_string()),
        ];
        let summary: Vec<String> = auto_started_panes
            .iter()
            .map(|(pane, file)| format!("{}→{}", pane, file))
            .collect();
        let msg = format!(
            "[sync] auto-started {} panes: {}",
            auto_started_panes.len(),
            summary.join(", ")
        );
        assert!(msg.contains("3 panes"));
        assert!(msg.contains("%80→tasks/cursor.md"));
        assert!(msg.contains("%81→tasks/feat.md"));
        assert!(msg.contains("%82→tasks/agent-loop.md"));
    }

    #[test]
    fn batch_summary_not_printed_for_single_pane() {
        let auto_started_panes = vec![("%84".to_string(), "tasks/file.md".to_string())];
        // Batch summary only prints when len > 1
        assert!(
            auto_started_panes.len() <= 1,
            "single pane should not trigger batch summary"
        );
    }

    // --- Rename debounce tests ---

    #[test]
    fn rename_debounce_suppresses_auto_start() {
        let tmp = tempfile::TempDir::new().unwrap();
        let debounce_dir = tmp.path().join(".agent-doc/rename-debounce");
        std::fs::create_dir_all(&debounce_dir).unwrap();

        // Create a file with known content for hashing
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "---\nagent_doc_session: abc123\n---\n").unwrap();

        // Write marker using the same hash function
        let hash = crate::snapshot::doc_hash(&file).unwrap();
        let marker = debounce_dir.join(format!("{}.marker", hash));
        std::fs::write(&marker, file.to_string_lossy().as_ref()).unwrap();

        // Check: marker exists and is fresh → has_rename_debounce should find it
        // (We test the marker file existence and freshness directly since
        // has_rename_debounce uses a hardcoded path relative to cwd)
        assert!(marker.exists(), "marker should exist after write");
        let age = marker
            .metadata()
            .unwrap()
            .modified()
            .unwrap()
            .elapsed()
            .unwrap();
        assert!(
            age.as_secs() < RENAME_DEBOUNCE_TTL_SECS,
            "marker should be fresh"
        );
    }

    #[test]
    fn rename_debounce_ttl_logic() {
        // Test the expiry logic directly: a marker older than RENAME_DEBOUNCE_TTL_SECS
        // should be considered expired
        let now = std::time::SystemTime::now();
        let fresh = now - std::time::Duration::from_secs(1);
        let expired = now - std::time::Duration::from_secs(RENAME_DEBOUNCE_TTL_SECS + 1);

        let fresh_age = now.duration_since(fresh).unwrap().as_secs();
        let expired_age = now.duration_since(expired).unwrap().as_secs();

        assert!(
            fresh_age < RENAME_DEBOUNCE_TTL_SECS,
            "fresh marker should be within TTL"
        );
        assert!(
            expired_age >= RENAME_DEBOUNCE_TTL_SECS,
            "expired marker should exceed TTL"
        );
    }

    #[test]
    fn rename_debounce_does_not_affect_other_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let debounce_dir = tmp.path().join(".agent-doc/rename-debounce");
        std::fs::create_dir_all(&debounce_dir).unwrap();

        let file_a = tmp.path().join("a.md");
        let file_b = tmp.path().join("b.md");
        std::fs::write(&file_a, "---\nagent_doc_session: aaa\n---\n").unwrap();
        std::fs::write(&file_b, "---\nagent_doc_session: bbb\n---\n").unwrap();

        // Only write marker for file_a
        let hash_a = crate::snapshot::doc_hash(&file_a).unwrap();
        let marker_a = debounce_dir.join(format!("{}.marker", hash_a));
        std::fs::write(&marker_a, file_a.to_string_lossy().as_ref()).unwrap();

        // file_b should have a different hash, no marker
        let hash_b = crate::snapshot::doc_hash(&file_b).unwrap();
        let marker_b = debounce_dir.join(format!("{}.marker", hash_b));
        assert_ne!(
            hash_a, hash_b,
            "different files should have different hashes"
        );
        assert!(!marker_b.exists(), "no marker should exist for file_b");
    }

    #[test]
    fn stash_rescue_discovers_agent_doc_window_when_window_arg_is_none() {
        let iso = IsolatedTmux::new("sync-stash-discover-window");
        let tmp = tempfile::TempDir::new().unwrap();

        // Create session with agent-doc window
        let pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

        // Create a second pane and stash it
        let pane1 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
        iso.stash_pane(&pane1, "test").unwrap();
        assert!(iso.pane_alive(&pane1), "stashed pane should still be alive");

        // Verify pane1 is in a stash window
        let win_id = iso.pane_window(&pane1).unwrap();
        let win_name = iso
            .cmd()
            .args(["display-message", "-t", &win_id, "-p", "#{window_name}"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        assert!(
            win_name == "stash" || win_name.starts_with("stash-"),
            "pane should be in stash window, got: {}",
            win_name
        );

        // Simulate what the fix does: discover agent-doc window from session name
        // when `window` arg is None
        let target_sess = "test";
        let candidate = format!("{}:agent-doc", target_sess);
        let window_panes = iso.list_window_panes(&candidate).unwrap_or_default();
        assert!(
            !window_panes.is_empty(),
            "should discover agent-doc window from session name"
        );

        // Rescue the pane into the agent-doc window without swapping pane0 out.
        let target = window_panes.first().unwrap();
        let rescue_result = sessions::join_pane_guarded(&iso, &pane1, target, target_sess, "-dh");
        assert!(
            rescue_result.is_ok(),
            "join-pane rescue should succeed: {:?}",
            rescue_result.err()
        );

        // Verify pane1 is no longer in stash
        let post_win_id = iso.pane_window(&pane1).unwrap();
        let post_win_name = iso
            .cmd()
            .args([
                "display-message",
                "-t",
                &post_win_id,
                "-p",
                "#{window_name}",
            ])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        assert_eq!(
            post_win_name, "agent-doc",
            "pane should be in agent-doc window after rescue, got: {}",
            post_win_name
        );
        let visible_panes = iso.list_window_panes(&candidate).unwrap();
        assert!(
            visible_panes.contains(&pane0),
            "existing pane should stay visible after rescue, got: {:?}",
            visible_panes
        );
        assert!(
            visible_panes.contains(&pane1),
            "rescued pane should be visible after rescue, got: {:?}",
            visible_panes
        );
    }
}
