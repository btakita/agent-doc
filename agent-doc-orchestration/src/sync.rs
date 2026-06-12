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
//! Usage: `agent-doc sync [--col plan.md,corky.md] [--col agent-doc.md] [--window @1] [--focus plan.md]`
//!
//! Each `--col` argument is a comma-separated list of files. Columns are arranged
//! left-to-right; files within a column stack top-to-bottom. Layout arithmetic is
//! delegated to `tmux-router::sync`. This module provides the agent-doc-specific
//! layers: frontmatter-based session resolution, auto-start for missing panes,
//! post-sync registry updates, layout repair, and column memory.
//!
//! ## Spec
//! - `run(col_args, window, focus)` is the primary entry point. When no explicit
//!   `col_args` are provided, it falls back to the recorded
//!   `.agent-doc/last_layout.json` for the current sync scope. It preserves empty
//!   col_args as positional placeholders through column-memory substitution, then
//!   drops any still-empty columns before tmux-router parsing. This lets editor
//!   plugins represent a non-markdown sibling split without losing left/right
//!   column identity. When `window` is omitted, sync still scopes the run to the
//!   current tmux session's `agent-doc` window instead of inheriting the
//!   currently focused window (which may itself be `stash`). Full sync first runs
//!   the file-scoped doctor repair path for a focused/session document when one is
//!   available, then prunes stale sessions, auto-starts missing panes, delegates
//!   to `tmux_router::sync`, and registers synced file→pane assignments. Passive
//!   `--no-autostart` editor sync skips the repair step and remains non-destructive
//!   around layout drift.
//! - `run_layout_only(col_args, window, focus)` keeps the non-destructive
//!   editor-sync contract: it still refuses to replace or override live /
//!   ambiguous owners, but it may cold-start a pane after sync proves there is
//!   no live owner left for the document.
//! - `run_with_tmux(col_args, window, focus, tmux)` injects a custom `Tmux` instance
//!   (test hook); auto-start is enabled.
//! - `repair_layout(tmux, session_name, target_window_name)` runs three phases:
//!   1. **Stash consolidation** — merges `stash-*` and duplicate `stash` panes into
//!      the primary stash window via `join-pane` while preserving any overflow
//!      windows that cannot be joined.
//!   2. **Target window rescue** — if the target window is missing, breaks a live
//!      registered pane out of the stash and renames the new window.
//!   3. **Index normalisation** — moves or swaps the target window to index 0,
//!      using `swap-window` when index 0 is occupied to avoid data loss, then
//!      renames and packs stash windows as `1:stash`, `2:stash`, and so on.
//!
//!   Phases 1 and 2 are skipped when the layout is already correct (target exists,
//!   single stash). Phase 3 always runs.
//! - `repair_file_state_with_tmux` is the tmux-layout portion of the doctor repair
//!   path used by both `agent-doc session doctor <FILE> --repair` and full sync.
//! - The `resolve_file` closure reads each file's frontmatter session UUID and
//!   produces a `FileResolution::Registered` (or `Unmanaged` when no UUID is present).
//!   Files with session UUIDs are always treated as registered, even if the registry
//!   entry was pruned — sync will auto-start a new session for them. This enables the
//!   declarative layout flow: navigating to a file in a split creates a tmux pane.
//!   It never propagates `tmux_session` from frontmatter — that field is deprecated.
//! - When a registered pane is found in a stash window, sync treats it as alive and
//!   defers placement to the reconciler. The reconciler's SWAP fast path handles
//!   1-in/1-out transitions atomically via `swap-pane`, avoiding the 3-pane bounce
//!   loop that occurs when sync rescues a stashed pane before reconcile (which then
//!   stashes another pane, creating continuous churn on tab switches).
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
//! - Before sync auto-starts a replacement pane, it must clear any startup-miss
//!   marker already superseded by a newer registered owner. If the current
//!   registered pane is still alive and still owns the active startup-miss
//!   marker, sync fails closed for that file instead of rebinding the document
//!   to yet another pane era.
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
//! - `run_layout_only` is safe for passive editor sync: it will not replace a
//!   live or ambiguous owner, but it may start a new pane after a cleanly
//!   closed/missing prior session leaves no live owner to rescue.
//! - `register_synced_files` holds `RegistryLock` for the duration of its write and
//!   saves only when at least one entry changed.
//! - `is_file_rename` is pure (no tmux dependency): it compares two paths and checks
//!   disk existence of the old one. Safe to call from any context.
//! - File rename re-registration reuses the existing `sessions::register` path, so
//!   single-session-per-pane invariant and `RegistryLock` apply as normal.
//! - **Column memory:** `.agent-doc/last_layout.json` persists a column→agent-doc mapping.
//!   When a column has no agent doc (user switches to a non-session file), sync substitutes
//!   the last known agent doc for that column index. Empty `--col` placeholders from editor
//!   split detection keep the original column position stable, so a right-hand markdown file
//!   can still restore a remembered left-hand agent pane. The state file is updated after each
//!   successful sync with any columns that contain an agent doc.
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
//! - Open closeout panes stay alive during sync reconcile: if an unwanted pane owns
//!   a document with an open `preflight_started`, `response_captured`, or
//!   `write_applied` cycle, sync logs that fact but still lets tmux-router stash the
//!   pane. Stashing is non-destructive, so the closeout keeps running without
//!   forcing a third visible pane into a two-document editor projection.
//!
//! ## Evals
//! - repair_layout_skips_correct_state: session with agent-doc at index 0 and one
//!   stash → repair is a no-op, window list unchanged.
//! - repair_layout_moves_window_to_index_0: agent-doc at index 2 with index 0 free →
//!   repair moves agent-doc to index 0.
//! - repair_layout_swaps_when_index_0_occupied: agent-doc at index 2 with a different
//!   window at index 0 → repair swaps the two windows, both windows preserved.
//! - repair_layout_consolidates_multiple_stash_windows: multiple `stash`/`stash-*`
//!   windows → repair merges joinable panes into `1:stash` and leaves only
//!   overflow stash windows at adjacent indices.
//! - repair_layout_rescues_pane_from_stash: no agent-doc window, pane in stash →
//!   repair does not error; stashed pane remains alive.
//! - sync_does_not_write_tmux_session_to_frontmatter: after sync, the document file
//!   must not contain a `tmux_session` frontmatter key.
//! - resolve_file_ignores_frontmatter_tmux_session: `FileResolution::Registered` always
//!   has `tmux_session: None` regardless of what the frontmatter contains.
//! - find_alive_pane_for_file: pane whose child process tree still includes the
//!   long-lived `agent-doc start <file>` owner command is returned; control-surface
//!   utility invocations like `agent-doc route <file>` are skipped.
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
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

use crate::sessions::{PaneMoveOp, Tmux};
use crate::{component, frontmatter, resync, route, sessions, snapshot};

use tmux_router::FileResolution;

const RENAME_DEBOUNCE_TTL_SECS: u64 = 5;
const SYNC_FRONTMATTER_STATUS_PREFIX: &str = "[agent-doc sync] malformed frontmatter";
const SYNC_WINDOW_RESOLUTION_BUDGET: Duration = Duration::from_millis(250);
const SYNC_PRUNE_BUDGET: Duration = Duration::from_millis(1_000);
const SYNC_PRUNE_SUBPHASE_BUDGET: Duration = Duration::from_millis(250);
const SYNC_LOCK_WAIT_LATENCY_BUDGET: Duration = Duration::from_millis(100);
const SYNC_PRELOCK_ACTOR_FOCUS_BUDGET: Duration = Duration::from_millis(300);
const SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET: Duration = Duration::from_millis(250);
const SYNC_PROJECTION_REFRESH_BUDGET: Duration = Duration::from_millis(250);
const SYNC_OWNERSHIP_PROOF_BUDGET: Duration = Duration::from_millis(750);
const SYNC_ROUTER_BUDGET: Duration = Duration::from_millis(1_000);
const SYNC_SAFE_PASSIVE_TOTAL_BUDGET: Duration = Duration::from_millis(1_000);
const SYNC_LOCK_WAIT_BUDGET: Duration = Duration::from_secs(3);
const SYNC_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SAFE_PASSIVE_STASH_CLEANUP_THROTTLE: Duration = Duration::from_secs(2);
const SAFE_PASSIVE_SYNC_LOCK_SKIPPED_MARKER: &str =
    "[sync] safe_passive_sync_lock_contention_retry";
const STALE_SYNC_LOCK_OWNER_AGE: Duration = Duration::from_secs(300);

fn latency_budget_status(elapsed: Duration, budget: Duration) -> &'static str {
    if elapsed >= budget {
        "over_budget"
    } else {
        "ok"
    }
}

fn sync_latency_message(
    phase: &str,
    elapsed: Duration,
    budget: Duration,
    auto_start_mode: AutoStartMode,
) -> String {
    format!(
        "sync_latency phase={} elapsed_ms={} budget_ms={} status={} mode={}",
        phase,
        elapsed.as_millis(),
        budget.as_millis(),
        latency_budget_status(elapsed, budget),
        auto_start_mode.log_label()
    )
}

#[derive(Debug)]
enum SyncLockAcquire {
    Acquired(File),
    Contended,
    Unavailable,
}

impl SyncLockAcquire {
    fn is_acquired(&self) -> bool {
        if let Self::Acquired(file) = self {
            let _ = file;
            true
        } else {
            false
        }
    }
}

fn acquire_sync_lock(lock_path: &Path, wait_budget: Duration) -> SyncLockAcquire {
    if let Some(parent) = lock_path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        sync_log(&format!(
            "sync lock unavailable — failed to create {}: {}",
            parent.display(),
            err
        ));
        return SyncLockAcquire::Unavailable;
    }

    let file = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
    {
        Ok(file) => file,
        Err(err) => {
            sync_log(&format!(
                "sync lock unavailable — failed to open {}: {}",
                lock_path.display(),
                err
            ));
            return SyncLockAcquire::Unavailable;
        }
    };

    let started = Instant::now();
    let mut stale_owner_cleanup_attempted = false;
    loop {
        use fs2::FileExt;
        match file.try_lock_exclusive() {
            Ok(()) => return SyncLockAcquire::Acquired(file),
            Err(err) if started.elapsed() >= wait_budget => {
                if !stale_owner_cleanup_attempted && reap_stale_orphaned_sync_lock_owners(lock_path)
                {
                    stale_owner_cleanup_attempted = true;
                    continue;
                }
                sync_log(&format!(
                    "sync lock contention exceeded {}ms at {}: {}",
                    wait_budget.as_millis(),
                    lock_path.display(),
                    err
                ));
                return SyncLockAcquire::Contended;
            }
            Err(_) => std::thread::sleep(SYNC_LOCK_POLL_INTERVAL),
        }
    }
}

#[derive(Clone, Debug)]
struct SyncLockProcess {
    pid: u32,
    ppid: u32,
    age: Duration,
    cmdline: Vec<String>,
    has_lock_fd: bool,
}

fn is_stale_orphaned_sync_lock_owner(process: &SyncLockProcess) -> bool {
    process.ppid == 1
        && process.age >= STALE_SYNC_LOCK_OWNER_AGE
        && process.has_lock_fd
        && process
            .cmdline
            .first()
            .is_some_and(|bin| bin.ends_with("agent-doc"))
        && process.cmdline.iter().any(|arg| arg == "sync")
}

#[cfg(target_os = "linux")]
fn reap_stale_orphaned_sync_lock_owners(lock_path: &Path) -> bool {
    let lock_path = lock_path
        .canonicalize()
        .unwrap_or_else(|_| lock_path.to_path_buf());
    let current_pid = std::process::id();
    let mut reaped_any = false;

    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };

    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == current_pid {
            continue;
        }

        let process = sync_lock_process_from_proc(pid, &lock_path);
        if !is_stale_orphaned_sync_lock_owner(&process) {
            continue;
        }

        let rc = unsafe { libc::kill(process.pid as libc::pid_t, libc::SIGTERM) };
        if rc == 0 {
            reaped_any = true;
            let message = format!(
                "[sync] stale_sync_lock_owner_reaped pid={} age_ms={} cmd={}",
                process.pid,
                process.age.as_millis(),
                process.cmdline.join(" ")
            );
            eprintln!("{}", message);
            sync_log(&message);
        }
    }

    reaped_any
}

#[cfg(not(target_os = "linux"))]
fn reap_stale_orphaned_sync_lock_owners(_lock_path: &Path) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn sync_lock_process_from_proc(pid: u32, lock_path: &Path) -> SyncLockProcess {
    let proc_dir = PathBuf::from("/proc").join(pid.to_string());
    let ppid = read_proc_ppid(&proc_dir).unwrap_or(0);
    let age = read_proc_age(&proc_dir).unwrap_or(Duration::ZERO);
    let cmdline = read_proc_cmdline(&proc_dir);
    let has_lock_fd = proc_has_fd_for_path(&proc_dir, lock_path);

    SyncLockProcess {
        pid,
        ppid,
        age,
        cmdline,
        has_lock_fd,
    }
}

#[cfg(target_os = "linux")]
fn read_proc_ppid(proc_dir: &Path) -> Option<u32> {
    let status = std::fs::read_to_string(proc_dir.join("status")).ok()?;
    status.lines().find_map(|line| {
        let value = line.strip_prefix("PPid:")?.trim();
        value.parse::<u32>().ok()
    })
}

#[cfg(target_os = "linux")]
fn read_proc_cmdline(proc_dir: &Path) -> Vec<String> {
    std::fs::read(proc_dir.join("cmdline"))
        .ok()
        .map(|bytes| {
            bytes
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .filter_map(|part| String::from_utf8(part.to_vec()).ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn read_proc_age(proc_dir: &Path) -> Option<Duration> {
    let stat = std::fs::read_to_string(proc_dir.join("stat")).ok()?;
    let after_comm = stat.rsplit_once(") ")?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let start_ticks = fields.get(19)?.parse::<u64>().ok()?;
    let uptime_secs = std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()?;
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        return None;
    }
    let start_secs = start_ticks as f64 / ticks_per_second as f64;
    if uptime_secs < start_secs {
        return Some(Duration::ZERO);
    }
    Some(Duration::from_secs_f64(uptime_secs - start_secs))
}

#[cfg(target_os = "linux")]
fn proc_has_fd_for_path(proc_dir: &Path, target: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(proc_dir.join("fd")) else {
        return false;
    };
    entries.flatten().any(|entry| {
        std::fs::read_link(entry.path())
            .ok()
            .map(|path| path == target)
            .unwrap_or(false)
    })
}

fn safe_passive_lock_contention_message(elapsed: Duration, budget: Duration) -> String {
    format!(
        "{} phase=sync_lock_wait elapsed_ms={} budget_ms={} status=over_budget coalesced=skipped_stale action=retry",
        SAFE_PASSIVE_SYNC_LOCK_SKIPPED_MARKER,
        elapsed.as_millis(),
        budget.as_millis()
    )
}

fn safe_passive_focus_path_and_session(focus: Option<&str>) -> Option<(PathBuf, String)> {
    let focus = focus?.trim();
    if focus.is_empty() {
        return None;
    }
    let focus_path = PathBuf::from(focus);
    let session_id = frontmatter::read_session_id(&focus_path)?;
    Some((focus_path, session_id))
}

fn safe_passive_local_actor_record_state(
    focus_path: &Path,
) -> Option<Option<crate::session_actor::ActorRecord>> {
    let canonical = focus_path
        .canonicalize()
        .ok()
        .unwrap_or_else(|| focus_path.to_path_buf());
    let base_dir = crate::snapshot::find_project_root(&canonical)?;
    crate::session_actor::load_record_in(&base_dir, &canonical.to_string_lossy()).ok()
}

fn safe_passive_registry_pane_state(focus_path: &Path, session_id: &str) -> Option<Option<String>> {
    let canonical = focus_path
        .canonicalize()
        .ok()
        .unwrap_or_else(|| focus_path.to_path_buf());
    let base_dir = crate::snapshot::find_project_root(&canonical)?;
    crate::sessions::lookup_in(&base_dir, session_id).ok()
}

/// Move-before-select for the passive fast-handoff focus path
/// (`#tmux-switch-lag`). If the actor pane is parked in a `stash` window,
/// reparent it into the working `agent-doc` window *before* selecting it, so the
/// doc-to-doc switch never shows an intermediate stash frame (the visible flash
/// the operator sees mid-switch). tmux preserves the pane id across the
/// `join-pane`/`break-pane` move, so the caller keeps selecting the same id.
/// Best-effort: a non-stashed pane or a failed promote leaves the later
/// `select_pane` to surface it in place, exactly as before.
fn promote_stashed_pane_before_focus(tmux: &Tmux, focus_path: &Path, pane_id: &str) {
    if !pane_in_stash_window(tmux, pane_id) {
        return;
    }
    match promote_pane_to_agent_doc_window(tmux, pane_id) {
        Ok(true) => sync_log(&format!(
            "safe_passive_move_before_select_promoted file={} pane={} (#tmux-switch-lag)",
            focus_path.display(),
            pane_id
        )),
        Ok(false) => sync_log(&format!(
            "safe_passive_move_before_select_noop file={} pane={} (#tmux-switch-lag)",
            focus_path.display(),
            pane_id
        )),
        Err(err) => {
            eprintln!(
                "[sync] warning: move-before-select promote of pane {} for {} failed: {}",
                pane_id,
                focus_path.display(),
                err
            );
            sync_log(&format!(
                "warning: safe_passive_move_before_select_failed file={} pane={} err={}",
                focus_path.display(),
                pane_id,
                err
            ));
        }
    }
}

fn safe_passive_select_prelock_pane(
    tmux: &Tmux,
    focus_path: &Path,
    pane_id: &str,
    source: &str,
) -> Option<String> {
    // Move-before-select: surface the pane out of stash before selecting it so
    // the switch shows no intermediate stash frame (#tmux-switch-lag).
    promote_stashed_pane_before_focus(tmux, focus_path, pane_id);
    if let Err(err) = tmux.select_pane(pane_id) {
        eprintln!(
            "[sync] warning: failed safe-passive pre-lock focus of actor pane {} for {}: {}",
            pane_id,
            focus_path.display(),
            err
        );
        sync_log(&format!(
            "warning: safe_passive_prelock_actor_focus_failed file={} pane={} source={} err={}",
            focus_path.display(),
            pane_id,
            source,
            err
        ));
        return None;
    }
    eprintln!(
        "[sync] safe_passive_prelock_actor_focus pane={} file={} source={}",
        pane_id,
        focus_path.display(),
        source
    );
    sync_log(&format!(
        "safe_passive_prelock_actor_focus file={} pane={} source={}",
        focus_path.display(),
        pane_id,
        source
    ));
    Some(pane_id.to_string())
}

fn safe_passive_prelock_provision_focus_pane(
    tmux: &Tmux,
    focus_path: &Path,
    session_id: &str,
    window: Option<&str>,
    col_args: &[String],
) -> Option<String> {
    if has_rename_debounce(focus_path) {
        let message = format!(
            "safe_passive_prelock_autostart_skipped file={} reason=rename_debounce",
            focus_path.display()
        );
        eprintln!("[sync] {}", message);
        sync_log(&message);
        return None;
    }

    match skip_auto_start_for_recent_session_loss(focus_path, session_id) {
        Ok(true) => return None,
        Ok(false) => {}
        Err(err) => {
            let message = format!(
                "warning: safe_passive_prelock_autostart_recent_loss_check_failed file={} err={}",
                focus_path.display(),
                err
            );
            eprintln!("[sync] {}", message);
            sync_log(&message);
            return None;
        }
    }

    match passive_autostart_skip_reason(tmux, focus_path, session_id, None) {
        Ok(Some(reason)) => {
            let message = format!(
                "safe_passive_prelock_autostart_skipped file={} reason={}",
                focus_path.display(),
                reason
            );
            eprintln!("[sync] {}", message);
            sync_log(&message);
            return None;
        }
        Ok(None) => {}
        Err(err) => {
            let message = format!(
                "warning: safe_passive_prelock_autostart_skip_check_failed file={} err={}",
                focus_path.display(),
                err
            );
            eprintln!("[sync] {}", message);
            sync_log(&message);
            return None;
        }
    }

    let context_session = window.and_then(|target| session_name_for_target_window(tmux, target));
    let file_str = focus_path.to_string_lossy().to_string();
    match route::try_provision_pane(
        tmux,
        focus_path,
        session_id,
        &file_str,
        context_session.as_deref(),
        col_args,
    ) {
        Ok(Some(pane_id)) => {
            eprintln!(
                "[sync] safe_passive_prelock_autostart pane={} file={}",
                pane_id,
                focus_path.display()
            );
            sync_log(&format!(
                "safe_passive_prelock_autostart file={} pane={}",
                focus_path.display(),
                pane_id
            ));
            Some(pane_id)
        }
        Ok(None) => {
            sync_log(&format!(
                "safe_passive_prelock_autostart_skipped file={} reason=startup_lock_busy",
                focus_path.display()
            ));
            None
        }
        Err(err) => {
            let message = format!(
                "warning: safe_passive_prelock_autostart_failed file={} err={}",
                focus_path.display(),
                err
            );
            eprintln!("[sync] {}", message);
            sync_log(&message);
            None
        }
    }
}

fn safe_passive_focus_actor_before_sync_lock(
    tmux: &Tmux,
    focus: Option<&str>,
    window: Option<&str>,
    col_args: &[String],
) -> Option<String> {
    let (focus_path, session_id) = safe_passive_focus_path_and_session(focus)?;
    if let Some(pane_id) =
        crate::focus::local_actor_projection_pane_for_document(&focus_path, &session_id, tmux)
    {
        return safe_passive_select_prelock_pane(tmux, &focus_path, &pane_id, "local_projection");
    }

    match safe_passive_local_actor_record_state(&focus_path)? {
        Some(record) => {
            sync_log(&format!(
                "safe_passive_prelock_actor_focus_deferred file={} reason=local_actor_record_not_live record_session={} record_pane={} record_state={:?}",
                focus_path.display(),
                record.session_id,
                record.pane_id,
                record.state
            ));
            None
        }
        None => match safe_passive_registry_pane_state(&focus_path, &session_id)? {
            Some(pane_id) if tmux.pane_alive(&pane_id) => {
                safe_passive_select_prelock_pane(tmux, &focus_path, &pane_id, "sessions_registry")
            }
            Some(pane_id) => {
                sync_log(&format!(
                    "safe_passive_prelock_actor_focus_deferred file={} reason=registry_pane_not_live registry_pane={}",
                    focus_path.display(),
                    pane_id
                ));
                None
            }
            None => safe_passive_prelock_provision_focus_pane(
                tmux,
                &focus_path,
                &session_id,
                window,
                col_args,
            ),
        },
    }
}

fn safe_passive_focus_actor_after_sync_lock(
    tmux: &Tmux,
    focus: Option<&str>,
    proof_cache: &SyncProofCache,
) -> Option<String> {
    let (focus_path, session_id) = safe_passive_focus_path_and_session(focus)?;
    let (pane_id, generation, source) = if let Some(pane_id) =
        crate::focus::local_actor_projection_pane_for_document(&focus_path, &session_id, tmux)
    {
        (pane_id, None, "local_projection")
    } else {
        let record = load_live_authoritative_actor_record_cached(
            tmux,
            &focus_path,
            &session_id,
            proof_cache,
        )?;
        (record.pane_id, Some(record.generation), "controller")
    };
    // Move-before-select: surface the pane out of stash before selecting it so
    // the switch shows no intermediate stash frame (#tmux-switch-lag).
    promote_stashed_pane_before_focus(tmux, &focus_path, &pane_id);
    if let Err(err) = tmux.select_pane(&pane_id) {
        eprintln!(
            "[sync] warning: failed safe-passive post-lock focus of actor pane {} for {}: {}",
            pane_id,
            focus_path.display(),
            err
        );
        sync_log(&format!(
            "warning: safe_passive_postlock_actor_focus_failed file={} pane={} source={} err={}",
            focus_path.display(),
            pane_id,
            source,
            err
        ));
        return None;
    }
    let generation = generation
        .map(|generation| generation.to_string())
        .unwrap_or_else(|| "projection".to_string());
    eprintln!(
        "[sync] safe_passive_postlock_actor_focus pane={} file={} generation={} source={}",
        pane_id,
        focus_path.display(),
        generation,
        source
    );
    sync_log(&format!(
        "safe_passive_postlock_actor_focus file={} pane={} generation={} source={}",
        focus_path.display(),
        pane_id,
        generation,
        source
    ));
    Some(pane_id)
}

fn log_sync_latency(
    focus: Option<&str>,
    phase: &str,
    elapsed: Duration,
    budget: Duration,
    auto_start_mode: AutoStartMode,
) {
    let message = sync_latency_message(phase, elapsed, budget, auto_start_mode);
    sync_log(&message);
    if latency_budget_status(elapsed, budget) == "over_budget" {
        eprintln!(
            "[sync] latency budget exceeded: phase {} took {}ms (budget {}ms, mode={})",
            phase,
            elapsed.as_millis(),
            budget.as_millis(),
            auto_start_mode.log_label()
        );
    }
    if let Some(focus) = focus {
        let path = Path::new(focus);
        if path.exists() {
            crate::ops_log::log_op(path, &message);
        }
    }
}

fn parse_frontmatter_for_sync<'a>(
    content: &'a str,
    file: &Path,
    phase: &str,
) -> Result<(frontmatter::Frontmatter, &'a str)> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    frontmatter::parse_for_file_with_context(content, file, &rc)
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
    closeout_recovery_phase: Option<String>,
    closeout_recovery_outcome: Option<crate::repair::RepairOutcome>,
    closeout_recovery_error: Option<String>,
    block_auto_start_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MissingRegisteredPaneRepairMode {
    InspectOnly,
    ExplicitRepair,
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
        crate::cycle_state::CyclePhase::Abandoned => "abandoned",
    };
    Some(label.to_string())
}

fn repair_outcome_label(outcome: crate::repair::RepairOutcome) -> &'static str {
    match outcome {
        crate::repair::RepairOutcome::Noop => "noop",
        crate::repair::RepairOutcome::ReplayedResponse => "replayed_response",
        crate::repair::RepairOutcome::AlreadyApplied => "already_applied",
        crate::repair::RepairOutcome::ManualTailRemovalRespected => "manual_tail_removal_respected",
        crate::repair::RepairOutcome::StaleCaptureRetired => "stale_capture_retired",
        crate::repair::RepairOutcome::StalePreflightLockRepaired => "stale_preflight_lock_repaired",
        crate::repair::RepairOutcome::StalePreflightCycleAbandoned => {
            "stale_preflight_cycle_abandoned"
        }
        crate::repair::RepairOutcome::CommitBoundaryRecovered => "commit_boundary_recovered",
        crate::repair::RepairOutcome::TemplateNormalized => "template_normalized",
        crate::repair::RepairOutcome::CompletedBacklogReaped => "completed_backlog_reaped",
    }
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

fn canonicalize_sync_file(file: &Path) -> Option<PathBuf> {
    let candidate = if file.is_absolute() {
        file.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(file)
    };
    Some(candidate.canonicalize().unwrap_or(candidate))
}

fn registry_location_for_file(file: &Path) -> Option<(PathBuf, PathBuf, String)> {
    let canonical = canonicalize_sync_file(file)?;
    let project_root = crate::snapshot::find_project_root(&canonical)?;
    let registry_key =
        sessions::canonical_registry_key_in(&project_root, canonical.to_string_lossy().as_ref());
    Some((canonical, project_root, registry_key))
}

fn registry_relative_file_path(project_root: &Path, canonical_file: &Path) -> String {
    canonical_file
        .strip_prefix(project_root)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| canonical_file.to_string_lossy().to_string())
}

fn first_agent_doc_in_col(col: &str) -> Option<String> {
    col.split(',').find_map(|f| {
        let f = f.trim();
        if f.is_empty() {
            return None;
        }
        if let Ok(content) = std::fs::read_to_string(f)
            && let Ok((fm, _)) = frontmatter::parse(&content)
            && fm.session.is_some()
        {
            return Some(f.to_string());
        }
        None
    })
}

fn column_has_agent_doc(col: &str) -> bool {
    first_agent_doc_in_col(col).is_some()
}

fn apply_column_memory(col_args: &[String], saved_layout: &[String]) -> Vec<String> {
    let visible_docs: HashSet<String> = col_args
        .iter()
        .filter_map(|col| first_agent_doc_in_col(col))
        .collect();
    let mut reserved_docs = visible_docs.clone();
    col_args
        .iter()
        .enumerate()
        .map(|(i, col)| {
            if column_has_agent_doc(col) {
                col.trim().to_string()
            } else if let Some(remembered) = saved_layout.get(i) {
                let remembered = remembered.trim();
                if !remembered.is_empty() && !reserved_docs.contains(remembered) {
                    sync_log(&format!(
                        "column {} has no agent doc, substituting remembered: {}",
                        i, remembered
                    ));
                    reserved_docs.insert(remembered.to_string());
                    remembered.to_string()
                } else {
                    col.trim().to_string()
                }
            } else {
                col.trim().to_string()
            }
        })
        .collect()
}

fn build_layout_state(col_args: &[String], saved_layout: &[String]) -> Vec<String> {
    let current_docs: Vec<Option<String>> = col_args
        .iter()
        .map(|col| first_agent_doc_in_col(col))
        .collect();
    let mut current_counts = HashMap::new();
    for doc in current_docs.iter().flatten() {
        *current_counts.entry(doc.clone()).or_insert(0usize) += 1;
    }
    let mut duplicate_keepers = HashMap::new();
    for (i, current_doc) in current_docs.iter().enumerate() {
        let Some(current_doc) = current_doc else {
            continue;
        };
        if current_counts.get(current_doc).copied().unwrap_or_default() <= 1 {
            duplicate_keepers.entry(current_doc.clone()).or_insert(i);
            continue;
        }
        if saved_layout
            .get(i)
            .map(|saved| saved.trim() == current_doc)
            .unwrap_or(false)
        {
            duplicate_keepers.entry(current_doc.clone()).or_insert(i);
        }
    }
    for (i, current_doc) in current_docs.iter().enumerate() {
        if let Some(current_doc) = current_doc {
            duplicate_keepers.entry(current_doc.clone()).or_insert(i);
        }
    }
    let mut reserved_docs = HashSet::new();
    current_docs
        .iter()
        .enumerate()
        .map(|(i, current_doc)| {
            if let Some(current_doc) = current_doc {
                let keep_current = duplicate_keepers
                    .get(current_doc)
                    .copied()
                    .is_some_and(|keeper| keeper == i);
                if keep_current && reserved_docs.insert(current_doc.clone()) {
                    return current_doc.clone();
                }
            }
            if let Some(remembered) = saved_layout.get(i) {
                let remembered = remembered.trim();
                if !remembered.is_empty()
                    && column_has_agent_doc(remembered)
                    && reserved_docs.insert(remembered.to_string())
                {
                    return remembered.to_string();
                }
            }
            if let Some(current_doc) = current_doc
                && reserved_docs.insert(current_doc.clone())
            {
                return current_doc.clone();
            }
            String::new()
        })
        .collect()
}

fn active_pane_column_index(
    tmux: &Tmux,
    target_session: Option<&str>,
    window: Option<&str>,
    layout_len: usize,
) -> Option<usize> {
    if layout_len < 2 {
        return None;
    }
    let session = target_session?;
    let window = window?;
    let active = tmux.active_pane(session)?;
    let ordered = tmux.list_panes_ordered(window).ok()?;
    if ordered.len() < 2 {
        return None;
    }
    let active_index = ordered.iter().position(|pane| pane == &active)?;
    Some(active_index.min(layout_len.saturating_sub(1)))
}

fn visible_registered_layout(tmux: &Tmux, window: Option<&str>) -> Vec<String> {
    let Some(window) = window else {
        return Vec::new();
    };
    let Ok(ordered_panes) = tmux.list_panes_ordered(window) else {
        return Vec::new();
    };
    if ordered_panes.len() < 2 {
        return Vec::new();
    }
    let registry = sessions::load().unwrap_or_default();
    ordered_panes
        .iter()
        .map(|pane| {
            registry
                .values()
                .find(|entry| entry.pane == *pane && !entry.file.trim().is_empty())
                .map(|entry| entry.file.trim().to_string())
                .unwrap_or_default()
        })
        .collect()
}

fn same_sync_file(lhs: &str, rhs: &str) -> bool {
    let lhs = lhs.trim();
    let rhs = rhs.trim();
    if lhs.is_empty() || rhs.is_empty() {
        return false;
    }
    if lhs == rhs {
        return true;
    }
    match (
        canonicalize_sync_file(Path::new(lhs)),
        canonicalize_sync_file(Path::new(rhs)),
    ) {
        (Some(lhs), Some(rhs)) => lhs == rhs,
        _ => false,
    }
}

fn focused_column_index(remembered_layout: &[String], focus: Option<&str>) -> Option<usize> {
    let focus = focus?.trim();
    if focus.is_empty() {
        return None;
    }
    remembered_layout.iter().position(|col| {
        col.split(',')
            .map(str::trim)
            .any(|candidate| same_sync_file(candidate, focus))
    })
}

fn expand_focus_only_columns_for_editor_switch(
    col_args: &[String],
    remembered_layout: &[String],
    active_column_index: Option<usize>,
    auto_start_mode: AutoStartMode,
) -> Vec<String> {
    if !matches!(auto_start_mode, AutoStartMode::SafePassive)
        || col_args.len() != 1
        || remembered_layout.len() < 2
    {
        return col_args.to_vec();
    }
    let Some(active_column_index) =
        active_column_index.filter(|index| *index < remembered_layout.len())
    else {
        return col_args.to_vec();
    };
    let focused_column = col_args[0].trim();
    if focused_column.is_empty() {
        return col_args.to_vec();
    }

    let mut expanded: Vec<String> = remembered_layout
        .iter()
        .map(|col| col.trim().to_string())
        .collect();
    expanded[active_column_index] = focused_column.to_string();
    for (index, col) in expanded.iter_mut().enumerate() {
        if index != active_column_index && col == focused_column {
            col.clear();
        }
    }
    sync_log(&format!(
        "safe_passive_focus_only_editor_switch_expanded active_column={} columns={:?}",
        active_column_index, expanded
    ));
    expanded
}

fn apply_focus_only_expansion_policy(
    col_args: &[String],
    remembered_layout: &[String],
    active_column_index: Option<usize>,
    auto_start_mode: AutoStartMode,
    exact_visible_projection: bool,
) -> Vec<String> {
    if exact_visible_projection {
        sync_log(&format!(
            "safe_passive_exact_visible_projection columns={:?}",
            col_args
        ));
        col_args.to_vec()
    } else {
        expand_focus_only_columns_for_editor_switch(
            col_args,
            remembered_layout,
            active_column_index,
            auto_start_mode,
        )
    }
}

fn lookup_registry_entry_for_file_session(
    file: &Path,
    session_id: &str,
) -> Option<sessions::SessionEntry> {
    let (_, _project_root, registry_key) = registry_location_for_file(file)?;
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let registry = rc.session_registry();
    let entry = registry.get(&registry_key)?.clone();
    (entry.session_id == session_id).then_some(entry)
}

#[derive(Debug, Clone)]
struct SyntheticRegistryCandidate {
    session_id: String,
    file_path: PathBuf,
    entry: sessions::SessionEntry,
    live_owner_match: bool,
    pane_root_match: bool,
}

fn filter_duplicate_synthetic_registry_candidates(
    candidates: Vec<SyntheticRegistryCandidate>,
) -> Vec<SyntheticRegistryCandidate> {
    let mut pane_claims: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (idx, candidate) in candidates.iter().enumerate() {
        pane_claims
            .entry(candidate.entry.pane.clone())
            .or_default()
            .push(idx);
    }

    let mut keep = vec![true; candidates.len()];
    for (pane_id, claimants) in pane_claims {
        if claimants.len() < 2 {
            continue;
        }

        let live_owner_matches: Vec<usize> = claimants
            .iter()
            .copied()
            .filter(|idx| candidates[*idx].live_owner_match)
            .collect();
        let pane_root_matches: Vec<usize> = claimants
            .iter()
            .copied()
            .filter(|idx| candidates[*idx].pane_root_match)
            .collect();

        let winners = if live_owner_matches.len() == 1 {
            live_owner_matches
        } else if live_owner_matches.is_empty() && pane_root_matches.len() == 1 {
            pane_root_matches
        } else {
            Vec::new()
        };

        if let Some(&winner_idx) = winners.first() {
            let winner = &candidates[winner_idx];
            eprintln!(
                "[sync] synthetic tmux-router registry keeps pane {} for {} and drops {} duplicate claimant(s)",
                pane_id,
                winner.file_path.display(),
                claimants.len() - 1
            );
            sync_log(&format!(
                "router_registry_duplicate_kept pane={} winner={} duplicates={} basis={}",
                pane_id,
                winner.file_path.display(),
                claimants.len() - 1,
                if winner.live_owner_match {
                    "live_owner"
                } else {
                    "pane_root"
                }
            ));
            for idx in claimants {
                keep[idx] = idx == winner_idx;
            }
            continue;
        }

        let duplicate_files = claimants
            .iter()
            .map(|idx| candidates[*idx].file_path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "[sync] synthetic tmux-router registry dropping ambiguous duplicate pane {} for {}",
            pane_id, duplicate_files
        );
        sync_log(&format!(
            "router_registry_duplicate_dropped pane={} files={}",
            pane_id, duplicate_files
        ));
        for idx in claimants {
            keep[idx] = false;
        }
    }

    candidates
        .into_iter()
        .enumerate()
        .filter_map(|(idx, candidate)| keep[idx].then_some(candidate))
        .collect()
}

fn build_tmux_router_sync_registry(
    tmux: &Tmux,
    col_args: &[String],
    proof_cache: &SyncProofCache,
) -> Result<Option<NamedTempFile>> {
    let mut candidates = Vec::new();

    for file_path in col_args
        .iter()
        .flat_map(|arg| arg.split(','))
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        let path = Path::new(file_path);
        if !path.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let (fm, _) = match frontmatter::parse(&content) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let Some(session_id) = fm.session else {
            continue;
        };
        let Some(entry) = lookup_registry_entry_for_file_session(path, &session_id) else {
            continue;
        };
        let Some((_, project_root, _)) = registry_location_for_file(path) else {
            continue;
        };
        let live_owner_match = sync_actor_or_live_owner_matches_cached(
            tmux,
            path,
            &session_id,
            &entry.pane,
            proof_cache,
        );
        let pane_root_match =
            pane_assignment_matches_document_root(tmux, &entry.pane, &project_root);
        candidates.push(SyntheticRegistryCandidate {
            session_id,
            file_path: path.to_path_buf(),
            entry,
            live_owner_match,
            pane_root_match,
        });
    }

    let mut registry = tmux_router::Registry::new();
    for candidate in filter_duplicate_synthetic_registry_candidates(candidates) {
        registry.insert(candidate.session_id, candidate.entry);
    }

    if registry.is_empty() {
        return Ok(None);
    }

    // Snapshot the synthetic registry under an absolute path so later cwd
    // drift in other parallel tests cannot make tmux-router read the wrong
    // registry file for this sync cycle.
    let temp_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".agent-doc/router-sync");
    std::fs::create_dir_all(&temp_dir).with_context(|| {
        format!(
            "failed to create synthetic tmux-router registry dir {}",
            temp_dir.display()
        )
    })?;
    let temp_file = NamedTempFile::new_in(&temp_dir).with_context(|| {
        format!(
            "failed to create synthetic tmux-router registry in {}",
            temp_dir.display()
        )
    })?;
    tmux_router::registry::save_registry(temp_file.path(), &registry).with_context(|| {
        format!(
            "failed to save synthetic tmux-router registry {}",
            temp_file.path().display()
        )
    })?;
    Ok(Some(temp_file))
}

fn claimed_sync_pane_owner(
    claimed_panes: &RefCell<std::collections::HashMap<String, PathBuf>>,
    pane_id: &str,
    file_path: &Path,
) -> Option<PathBuf> {
    let claimed = claimed_panes.borrow();
    let owner = claimed.get(pane_id)?;
    (owner != file_path).then_some(owner.clone())
}

fn reserve_sync_pane(
    claimed_panes: &RefCell<std::collections::HashMap<String, PathBuf>>,
    pane_id: &str,
    file_path: &Path,
) {
    claimed_panes
        .borrow_mut()
        .insert(pane_id.to_string(), file_path.to_path_buf());
}

/// Build the unique candidate document paths for the auto-start pre-sync pass
/// from the requested column arguments.
///
/// Each `col_args` entry is a comma-joined column of documents, and the same
/// document may legitimately appear in more than one requested column (column
/// memory, focus + column overlap, repeated layout requests). The auto-start
/// pass must make at most ONE pane decision per document: if the same path is
/// processed twice, the first occurrence can auto-start a fresh pane that the
/// second occurrence's registry / session-log lookup cannot see yet (the new
/// pane has not recorded its binding), so the second occurrence cold-starts a
/// second pane — the duplicate-editor-pane regression ("3 tmux panes with 2
/// editor panes"). Dedup by path, preserving first-seen order so column/focus
/// precedence is unchanged. The reconciler already dedups panes per column, so
/// collapsing duplicate auto-start candidates here keeps the two passes aligned.
fn auto_start_candidate_files(col_args: &[String]) -> Vec<PathBuf> {
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut out: Vec<PathBuf> = Vec::new();
    for path in col_args
        .iter()
        .flat_map(|arg| arg.split(','))
        .map(|s| PathBuf::from(s.trim()))
    {
        if seen.insert(path.clone()) {
            out.push(path);
        }
    }
    out
}

#[derive(Default)]
struct SyncProofCache {
    actor_records: RefCell<HashMap<(PathBuf, String), Option<crate::session_actor::ActorRecord>>>,
    live_owner_matches: RefCell<HashMap<(PathBuf, String, String), bool>>,
}

fn sync_proof_file_key(file: &Path) -> PathBuf {
    file.canonicalize().unwrap_or_else(|_| file.to_path_buf())
}

fn registered_pane_proves_live_owner(
    tmux: &Tmux,
    file_path: &Path,
    session_id: &str,
    pane_id: &str,
    proof_cache: &SyncProofCache,
) -> bool {
    if !tmux.pane_alive(pane_id) {
        return false;
    }
    sync_actor_or_live_owner_matches_cached(tmux, file_path, session_id, pane_id, proof_cache)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProtectedRegisteredPaneState {
    reason: String,
    last_visible_excerpt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenCycleProtectedPaneState {
    file: PathBuf,
    phase: &'static str,
}

fn resolve_harness_for_sync(file: &Path) -> crate::harness::HarnessConfig {
    let content = std::fs::read_to_string(file).unwrap_or_default();
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    rc.set_doc_content(content);
    let fm = rc.frontmatter();
    let global_config = rc.global_config();
    crate::harness::HarnessConfig::from_context(&fm, &global_config)
}

fn protected_registered_pane_state(
    tmux: &Tmux,
    file: &Path,
    pane_id: &str,
) -> Option<ProtectedRegisteredPaneState> {
    if !tmux.pane_alive(pane_id) {
        return None;
    }

    let capture = sessions::capture_pane(tmux, pane_id).ok()?;
    protected_registered_pane_state_from_capture(file, &capture)
}

fn protected_registered_pane_state_from_capture(
    file: &Path,
    capture: &str,
) -> Option<ProtectedRegisteredPaneState> {
    let harness = resolve_harness_for_sync(file);
    let reason = harness.protected_prompt_input_reason(capture)?;
    if reason == "active permission prompt" {
        crate::input_diag::log_prompt_detection(
            Some(file),
            "sync.protected_registered_pane",
            "registered_pane",
            &harness.binary,
            &reason,
            "active",
        );
    }
    Some(ProtectedRegisteredPaneState {
        reason,
        last_visible_excerpt: last_visible_excerpt(capture),
    })
}

fn open_cycle_protected_file_state(file: &Path) -> Option<OpenCycleProtectedPaneState> {
    let state = crate::cycle_state::load(file).ok().flatten()?;
    let phase = match state.phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed | crate::cycle_state::CyclePhase::Abandoned => {
            return None;
        }
    };
    Some(OpenCycleProtectedPaneState {
        file: file.to_path_buf(),
        phase,
    })
}

fn registered_file_for_pane(tmux: &Tmux, pane_id: &str) -> Option<PathBuf> {
    let project_root = pane_project_root(tmux, pane_id)?;
    let registry = sessions::load_in(&project_root).ok()?;
    let entry = registry
        .values()
        .find(|entry| entry.pane == pane_id && !entry.file.is_empty())?;
    let file = Path::new(&entry.file);
    Some(if file.is_absolute() {
        file.to_path_buf()
    } else {
        project_root.join(file)
    })
}

fn open_cycle_protected_pane_state(
    tmux: &Tmux,
    pane_id: &str,
) -> Option<OpenCycleProtectedPaneState> {
    let file = registered_file_for_pane(tmux, pane_id)?;
    open_cycle_protected_file_state(&file)
}

fn projected_sync_pane_count(col_args: &[String]) -> usize {
    col_args
        .iter()
        .flat_map(|arg| arg.split(','))
        .map(str::trim)
        .filter(|file| !file.is_empty())
        .map(|file| {
            let path = PathBuf::from(file);
            path.canonicalize().unwrap_or(path)
        })
        .collect::<HashSet<_>>()
        .len()
}

fn select_visible_focus_pane_if_present(
    tmux: &Tmux,
    window: &str,
    focus: Option<&str>,
) -> Option<String> {
    let focus = focus?.trim();
    if focus.is_empty() {
        return None;
    }
    let focus_path = PathBuf::from(focus);
    let canonical_focus = focus_path.canonicalize().unwrap_or(focus_path);
    for pane in tmux.list_window_panes(window).unwrap_or_default() {
        let Some(file) = registered_file_for_pane(tmux, &pane) else {
            continue;
        };
        let canonical_file = file.canonicalize().unwrap_or(file);
        if canonical_file != canonical_focus || !tmux.pane_alive(&pane) {
            continue;
        }
        if let Err(err) = tmux.select_pane(&pane) {
            let warning = format!(
                "[sync] warning: failed to reselect visible focus pane {} for {} while preserving layout: {}",
                pane,
                canonical_focus.display(),
                err
            );
            eprintln!("{}", warning);
            sync_log(&warning);
            return None;
        }
        return Some(pane);
    }
    None
}

fn emit_preserved_layout_focus_marker(pane: &str, reason: &str) {
    let marker = format!(
        "[sync] safe_passive_layout_preserved_reselected_focus pane={} reason={}",
        pane, reason
    );
    eprintln!("{}", marker);
    sync_log(&marker);
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

    let _ = crate::startup_miss::append_session_log_event(
        file,
        session_id,
        &format!(
            "pane_death_cleanup pane={pane_id} action=keep_dead policy=normal_sync_never_kills"
        ),
    );

    Ok(Some(DeadPaneDiagnostics {
        observed_window,
        dead_status,
        cycle_phase,
        capture_path,
        last_visible_excerpt,
        pane_killed: false,
    }))
}

fn recover_missing_pane_closeout(
    file: &Path,
    session_id: &str,
    pane_id: &str,
) -> (
    Option<String>,
    Option<crate::repair::RepairOutcome>,
    Option<String>,
) {
    let state = match crate::cycle_state::load(file) {
        Ok(state) => state,
        Err(err) => {
            return (
                None,
                None,
                sanitize_excerpt(&format!("failed to load cycle state: {err}")),
            );
        }
    };
    let Some(state) = state else {
        return (None, None, None);
    };
    let phase = match state.phase {
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        _ => return (None, None, None),
    };
    let capture_present = crate::capture::load_active(file).ok().flatten().is_some();
    let _ = crate::startup_miss::append_session_log_event(
        file,
        session_id,
        &format!(
            "sync_missing_pane_closeout_recovery_start pane={pane_id} cycle={} phase={phase} durable_capture={capture_present}",
            state.cycle_id
        ),
    );
    match crate::repair::repair(file) {
        Ok(outcome) => {
            let _ = crate::startup_miss::append_session_log_event(
                file,
                session_id,
                &format!(
                    "sync_missing_pane_closeout_recovery_result pane={pane_id} cycle={} phase={phase} outcome={}",
                    state.cycle_id,
                    repair_outcome_label(outcome)
                ),
            );
            (Some(phase.to_string()), Some(outcome), None)
        }
        Err(err) => {
            let detail =
                sanitize_excerpt(&err.to_string()).unwrap_or_else(|| "unknown".to_string());
            let _ = crate::startup_miss::append_session_log_event(
                file,
                session_id,
                &format!(
                    "sync_missing_pane_closeout_recovery_failed pane={pane_id} cycle={} phase={phase} reason={detail}",
                    state.cycle_id
                ),
            );
            (Some(phase.to_string()), None, Some(detail))
        }
    }
}

fn pending_missing_pane_repair_phase(file: &Path) -> Option<&'static str> {
    let state = crate::cycle_state::load(file).ok().flatten()?;
    match state.phase {
        crate::cycle_state::CyclePhase::PreflightStarted => Some("preflight_started"),
        crate::cycle_state::CyclePhase::ResponseCaptured => Some("response_captured"),
        crate::cycle_state::CyclePhase::WriteApplied => Some("write_applied"),
        crate::cycle_state::CyclePhase::Committed | crate::cycle_state::CyclePhase::Abandoned => {
            None
        }
    }
}

fn missing_pane_manual_repair_reason(file: &Path, phase: &str) -> String {
    if let Ok(Some(message)) = crate::session_check::detect_uncommitted_closeout_drift(file) {
        return message;
    }
    let detail = match phase {
        "preflight_started" => "stale preflight state is still open",
        "response_captured" => "a captured response still needs explicit closeout recovery",
        "write_applied" => "the write reached disk but the commit boundary is still open",
        _ => "manual repair is still required",
    };
    format!(
        "normal sync will not auto-repair {phase} for {} ({}). Run `agent-doc repair {}` or `agent-doc session doctor {} --repair` before syncing again",
        file.display(),
        detail,
        file.display(),
        file.display()
    )
}

fn missing_pane_closeout_block_reason(file: &Path, phase: &str, error: Option<&str>) -> String {
    if let Ok(Some(message)) = crate::session_check::detect_uncommitted_closeout_drift(file) {
        return message;
    }
    let detail = error.unwrap_or("unknown");
    format!(
        "closeout recovery for {phase} failed and needs manual repair ({detail}). Re-run `agent-doc repair {}` before syncing again",
        file.display()
    )
}

fn repair_missing_registered_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
    last_known_window: Option<&str>,
    mode: MissingRegisteredPaneRepairMode,
) -> Result<MissingRegisteredPaneRepair> {
    let dead_pane =
        capture_dead_pane_diagnostics(tmux, file, session_id, pane_id, last_known_window)?;
    let (closeout_recovery_phase, closeout_recovery_outcome, closeout_recovery_error) = match mode {
        MissingRegisteredPaneRepairMode::InspectOnly => (
            pending_missing_pane_repair_phase(file).map(str::to_string),
            None,
            None,
        ),
        MissingRegisteredPaneRepairMode::ExplicitRepair => {
            recover_missing_pane_closeout(file, session_id, pane_id)
        }
    };
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
    let repaired_stale_preflight = match mode {
        MissingRegisteredPaneRepairMode::InspectOnly => false,
        MissingRegisteredPaneRepairMode::ExplicitRepair if closeout_recovery_phase.is_none() => {
            matches!(
                crate::repair::repair_stale_preflight_started_cycle(file)?,
                crate::repair::RepairOutcome::StalePreflightLockRepaired
            )
        }
        MissingRegisteredPaneRepairMode::ExplicitRepair => false,
    };
    let block_auto_start_reason = match mode {
        MissingRegisteredPaneRepairMode::InspectOnly => closeout_recovery_phase
            .as_deref()
            .map(|phase| missing_pane_manual_repair_reason(file, phase)),
        MissingRegisteredPaneRepairMode::ExplicitRepair
            if closeout_recovery_phase.is_some() && closeout_recovery_outcome.is_none() =>
        {
            Some(missing_pane_closeout_block_reason(
                file,
                closeout_recovery_phase.as_deref().unwrap_or("unknown"),
                closeout_recovery_error.as_deref(),
            ))
        }
        MissingRegisteredPaneRepairMode::ExplicitRepair => None,
    };
    Ok(MissingRegisteredPaneRepair {
        dead_pane,
        recorded_session_loss,
        repaired_stale_preflight,
        closeout_recovery_phase,
        closeout_recovery_outcome,
        closeout_recovery_error,
        block_auto_start_reason,
    })
}

pub fn repair_file_state(file: &Path) -> Result<Vec<String>> {
    let tmux = Tmux::default_server();
    repair_file_state_with_tmux(&tmux, file)
}

pub fn repair_file_state_with_tmux(tmux: &Tmux, file: &Path) -> Result<Vec<String>> {
    let canonical = file
        .canonicalize()
        .unwrap_or_else(|_| crate::git::resolve_absolute_file_path(file));
    let mut actions = Vec::new();

    let columns = vec![canonical.to_string_lossy().to_string()];
    if let Some(session_name) = resolve_sync_target_session(tmux, None, &columns, None) {
        repair_layout(tmux, &session_name, "agent-doc")?;
        actions.push(format!(
            "Repaired `agent-doc`/`stash` layout in tmux session `{session_name}`."
        ));
    }

    let content = std::fs::read_to_string(&canonical)
        .with_context(|| format!("failed to read {}", canonical.display()))?;
    let (frontmatter, _) =
        parse_frontmatter_for_sync(&content, &canonical, "session doctor --repair")?;
    let Some(session_id) = frontmatter.session else {
        return Ok(actions);
    };
    let Some(entry) = lookup_registry_entry_for_file_session(&canonical, &session_id) else {
        return Ok(actions);
    };
    if tmux.pane_alive(&entry.pane) {
        return Ok(actions);
    }

    let repair = repair_missing_registered_pane(
        tmux,
        &canonical,
        &session_id,
        &entry.pane,
        Some(entry.window.as_str()),
        MissingRegisteredPaneRepairMode::ExplicitRepair,
    )?;
    if repair.recorded_session_loss {
        actions.push(format!(
            "Recorded missing-pane diagnostics for `{}` on pane `{}`.",
            canonical.display(),
            entry.pane
        ));
    }
    if let Some(phase) = repair.closeout_recovery_phase.as_deref() {
        if let Some(outcome) = repair.closeout_recovery_outcome {
            actions.push(format!(
                "Recovered `{phase}` closeout for `{}` ({}) before replacement logic resumes.",
                canonical.display(),
                repair_outcome_label(outcome)
            ));
        } else if let Some(err) = repair.closeout_recovery_error.as_deref() {
            actions.push(format!(
                "Explicit repair still could not finish `{phase}` closeout for `{}`: {}",
                canonical.display(),
                err
            ));
        }
    } else if repair.repaired_stale_preflight {
        actions.push(format!(
            "Closed a stale `preflight_started` cycle for `{}`.",
            canonical.display()
        ));
    }

    Ok(actions)
}

fn skip_auto_start_for_recent_session_loss(file: &Path, session_id: &str) -> Result<bool> {
    let Some(window) = crate::startup_miss::recent_session_loss_window(file, session_id)? else {
        return Ok(false);
    };

    let first = crate::startup_miss::format_timestamp(window.first_timestamp);
    let last = crate::startup_miss::format_timestamp(window.last_timestamp);
    let latest_reason = window.latest_reason.as_deref().unwrap_or("unknown");
    eprintln!(
        "[sync] repeated pane-loss window for {} ({} events since {}, latest reason={} at {}) — skipping auto-start until manual recovery",
        file.display(),
        window.count,
        first,
        latest_reason,
        last
    );
    sync_log(&format!(
        "repeated pane-loss window file={} session={} count={} first={} last={} latest_reason={} action=skip_auto_start",
        file.display(),
        session_id,
        window.count,
        first,
        last,
        latest_reason
    ));
    Ok(true)
}

pub fn run(col_args: &[String], window: Option<&str>, focus: Option<&str>) -> Result<()> {
    tracing::debug!(cols = ?col_args, window, focus, "sync::run start");
    run_with_options(col_args, window, focus, AutoStartMode::Full)
}

fn run_with_options(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
) -> Result<()> {
    run_with_options_internal(
        col_args,
        window,
        focus,
        auto_start_mode,
        false,
        &Tmux::default_server(),
    )
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

/// Run sync in the passive editor mode.
///
/// This mode remains non-destructive around live or ambiguous owners, but it can
/// cold-start a pane when sync proves the document no longer has a live owner.
#[allow(dead_code)]
pub fn run_layout_only(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
) -> Result<()> {
    run_with_options(col_args, window, focus, AutoStartMode::SafePassive)
}

/// Run sync in passive editor mode with an exact editor-visible projection.
///
/// Unlike generic focus-only safe-passive sync, this mode does not expand a
/// single provided column from remembered layout state. Editors use it when
/// their selection snapshot already represents the full visible markdown set.
pub fn run_layout_only_exact_visible(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
) -> Result<()> {
    run_with_options_internal(
        col_args,
        window,
        focus,
        AutoStartMode::SafePassive,
        true,
        &Tmux::default_server(),
    )
}

pub fn run_with_tmux(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    run_with_options_internal(col_args, window, focus, AutoStartMode::Full, false, tmux)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoStartMode {
    Full,
    SafePassive,
}

impl AutoStartMode {
    fn log_label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::SafePassive => "safe-passive",
        }
    }
}

fn load_live_authoritative_actor_record_uncached(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
) -> Option<crate::session_actor::ActorRecord> {
    let canonical = file
        .canonicalize()
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    let base_dir = crate::snapshot::find_project_root(&canonical)?;
    let record = crate::project_controller::authoritative_actor_binding(&base_dir, &canonical)
        .ok()
        .flatten()?;
    if record.session_id != session_id || !tmux.pane_alive(&record.pane_id) {
        return None;
    }
    Some(record)
}

fn load_live_authoritative_actor_record_cached(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    proof_cache: &SyncProofCache,
) -> Option<crate::session_actor::ActorRecord> {
    let key = (sync_proof_file_key(file), session_id.to_string());
    if let Some(record) = proof_cache.actor_records.borrow().get(&key) {
        return record.clone();
    }

    let record = load_live_authoritative_actor_record_uncached(tmux, file, session_id);
    proof_cache
        .actor_records
        .borrow_mut()
        .insert(key, record.clone());
    record
}

pub fn authoritative_actor_pane_for_document(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
) -> Option<String> {
    load_live_authoritative_actor_record_uncached(tmux, file, session_id)
        .map(|record| record.pane_id)
}

fn authoritative_actor_pane_for_document_cached(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    proof_cache: &SyncProofCache,
) -> Option<String> {
    load_live_authoritative_actor_record_cached(tmux, file, session_id, proof_cache)
        .map(|record| record.pane_id)
}

fn project_authoritative_actor_binding(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
    proof_cache: &SyncProofCache,
) -> Option<String> {
    if matches!(auto_start_mode, AutoStartMode::SafePassive)
        && let Some(pane_id) =
            crate::focus::local_actor_projection_pane_for_document(file, session_id, tmux)
    {
        log_sync_latency(
            focus,
            "controller_actor_lookup",
            Duration::ZERO,
            SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET,
            auto_start_mode,
        );
        sync_log(&format!(
            "controller_actor_lookup_skipped file={} pane={} source=local_projection",
            file.display(),
            pane_id
        ));
        return Some(pane_id);
    }

    let lookup_start = Instant::now();
    let record = load_live_authoritative_actor_record_cached(tmux, file, session_id, proof_cache);
    log_sync_latency(
        focus,
        "controller_actor_lookup",
        lookup_start.elapsed(),
        SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET,
        auto_start_mode,
    );
    let record = record?;
    let actor_pane = record.pane_id.clone();
    if lookup_registry_entry_for_file_session(file, session_id)
        .as_ref()
        .map(|entry| entry.pane.as_str())
        != Some(actor_pane.as_str())
    {
        let projection_start = Instant::now();
        eprintln!(
            "[sync] authoritative actor generation {} keeps {} on pane {} — refreshing sessions.json as a projection",
            record.generation,
            file.display(),
            actor_pane
        );
        sync_log(&format!(
            "actor_projection_refresh file={} pane={} generation={}",
            file.display(),
            actor_pane,
            record.generation
        ));
        if let Err(err) = reregister_recovered_owner(tmux, file, session_id, &actor_pane) {
            eprintln!(
                "[sync] warning: failed to project authoritative actor pane {} for {} into sessions.json: {}",
                actor_pane,
                file.display(),
                err
            );
            sync_log(&format!(
                "warning: actor_projection_refresh_failed file={} pane={} err={}",
                file.display(),
                actor_pane,
                err
            ));
        }
        log_sync_latency(
            focus,
            "projection_refresh",
            projection_start.elapsed(),
            SYNC_PROJECTION_REFRESH_BUDGET,
            auto_start_mode,
        );
    }
    Some(actor_pane)
}

#[cfg(test)]
fn sync_actor_or_live_owner_matches(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
) -> bool {
    let proof_cache = SyncProofCache::default();
    sync_actor_or_live_owner_matches_cached(tmux, file, session_id, pane_id, &proof_cache)
}

fn sync_actor_or_live_owner_matches_cached(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
    proof_cache: &SyncProofCache,
) -> bool {
    let key = (
        sync_proof_file_key(file),
        session_id.to_string(),
        pane_id.to_string(),
    );
    if let Some(matches) = proof_cache.live_owner_matches.borrow().get(&key) {
        return *matches;
    }

    let matches = authoritative_actor_pane_for_document_cached(tmux, file, session_id, proof_cache)
        .as_deref()
        == Some(pane_id)
        || find_normal_path_owner_pane_excluding_quiet(tmux, file, session_id, None).as_deref()
            == Some(pane_id);
    proof_cache
        .live_owner_matches
        .borrow_mut()
        .insert(key, matches);
    matches
}

fn passive_autostart_skip_reason(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    unresolved_startup_miss: Option<&crate::startup_miss::StartupMiss>,
) -> Result<Option<String>> {
    if unresolved_startup_miss.is_some() {
        return Ok(Some(
            "startup-miss is still unresolved for this document".to_string(),
        ));
    }

    let Some(status) = crate::startup_miss::session_log_status(file, session_id)? else {
        return Ok(None);
    };

    if !status.latest_session_closed() {
        return Ok(Some(format!(
            "latest session log is still open or ambiguous (last_event={})",
            crate::startup_miss::latest_log_last_event(&status)
        )));
    }

    let last_event = crate::startup_miss::latest_log_last_event(&status);
    if last_event.starts_with("session_end origin=registry_rebind ")
        && let Some(successor) =
            find_alive_pane_via_registry_rebind_successor(tmux, file, session_id, None, false)
    {
        return Ok(Some(format!(
            "latest session ended via registry_rebind and successor pane {successor} is still alive (last_event={last_event})"
        )));
    }

    Ok(None)
}

fn open_session_log_owner_fail_closed_diagnostic(
    file: &Path,
    session_id: &str,
    pane_id: &str,
) -> Result<Option<String>> {
    let Some(status) = crate::startup_miss::session_log_status(file, session_id)? else {
        return Ok(None);
    };
    if status.latest_start_pane.as_deref() != Some(pane_id) || !status.latest_session_open() {
        return Ok(None);
    }
    crate::startup_miss::session_log_diagnostic(file, session_id)
}

#[cfg(test)]
fn rescue_missing_agent_doc_window_from_candidates(
    tmux: &Tmux,
    session_name: &str,
    target_window_name: &str,
    rescue_candidates: &[String],
) -> bool {
    for pane in rescue_candidates {
        if !tmux.pane_alive(pane) {
            continue;
        }
        if tmux.pane_session(pane).ok().as_deref() != Some(session_name) {
            continue;
        }
        let Ok(window_id) = tmux.pane_window(pane) else {
            continue;
        };
        let Some(window_name) = window_name_for_window_id(tmux, &window_id) else {
            continue;
        };
        if !is_stash_window_name(&window_name) {
            continue;
        }

        eprintln!(
            "[sync] rescuing pane {} from {} to recreate '{}'",
            pane, window_name, target_window_name
        );
        if tmux.break_pane(pane).is_ok() {
            if let Ok(new_win) = tmux.pane_window(pane) {
                let _ = tmux.raw_cmd(&["rename-window", "-t", &new_win, target_window_name]);
                eprintln!(
                    "[sync] recreated window {} as {}",
                    new_win, target_window_name
                );
            }
            return true;
        }
    }

    false
}

/// Normalize the tmux layout by consolidating stash windows and ensuring
/// the agent-doc window exists.
///
/// Phase 1: Stash consolidation — merge all joinable `stash-*` and extra
/// `stash` panes into the primary stash window. Any panes that cannot be joined
/// stay alive in overflow stash windows.
///
/// Phase 2: Ensure the target window exists — if missing, break a registered
/// alive pane out of the stash to recreate it.
///
/// Phase 3: Window index normalization — keep `agent-doc` at `0`, then pack
/// stash windows as `1:stash`, `2:stash`, and so on.
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
        .filter(|w| is_stash_window_name(&w.name))
        .count();
    let has_exact_stash = windows.iter().any(|w| w.name == "stash");
    // Check if Phase 1+2 can be skipped (target exists, exactly one canonical stash)
    let skip_phase_1_2 = has_target && stash_count == 1 && has_exact_stash;
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
            } else if is_stash_window_name(&w.name) {
                secondary_stash_ids.push(w.id.clone());
            }
        }

        if stash_count == 0 {
            match tmux.ensure_stash_window(session_name) {
                Ok(id) => eprintln!("[repair] created missing stash window {}", id),
                Err(e) => {
                    eprintln!("[repair] failed to create stash window: {}", e);
                    return Ok(());
                }
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
                // if it has no panes left; otherwise keep it as an overflow stash.
                let remaining = tmux.list_window_panes(sec_id).unwrap_or_default();
                if remaining.is_empty() {
                    // Window should have auto-deleted, but try to kill just in case
                    let _ = tmux.raw_cmd(&["kill-window", "-t", sec_id]);
                    eprintln!("[repair] killed empty stash window {}", sec_id);
                } else {
                    normalize_stash_window_name(tmux, sec_id);
                    eprintln!(
                        "[repair] preserving overflow stash window {} with {} pane(s)",
                        sec_id,
                        remaining.len()
                    );
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

            // Load the registry and find a same-session stashed pane we can
            // safely break back out into a visible agent-doc window.
            if let Ok(registry) = sessions::load() {
                let mut rescued = false;
                for entry in registry.values() {
                    if !tmux.pane_alive(&entry.pane) {
                        continue;
                    }
                    if tmux.pane_session(&entry.pane).ok().as_deref() != Some(session_name) {
                        continue;
                    }
                    let Ok(window_id) = tmux.pane_window(&entry.pane) else {
                        continue;
                    };
                    let Some(window_name) = window_name_for_window_id(tmux, &window_id) else {
                        continue;
                    };
                    if !is_stash_window_name(&window_name) {
                        continue;
                    }

                    eprintln!("[repair] rescuing pane {} from {}", entry.pane, window_name);
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
                if !rescued {
                    eprintln!(
                        "[repair] no alive registered panes found, sync will auto-start later"
                    );
                }
            }
        }
    } // end skip_phase_1_2 else

    // ── Phase 3: Normalize window indices (always runs) ──
    // agent-doc should be at index 0, stash windows should directly follow it.
    let windows = list_session_windows(tmux, session_name);
    if let Some((_, target_window_id, _)) = windows
        .iter()
        .find(|(_, _, name)| name == target_window_name)
        .cloned()
    {
        normalize_window_to_index(tmux, session_name, &target_window_id, 0, "repair");
    }

    let stash_index_plan = planned_stash_window_indices(&list_session_windows(tmux, session_name));
    for (stash_window_id, desired_index) in stash_index_plan {
        normalize_stash_window_name(tmux, &stash_window_id);
        normalize_window_to_index(
            tmux,
            session_name,
            &stash_window_id,
            desired_index,
            "repair_stash",
        );
    }

    Ok(())
}

fn planned_stash_window_indices(windows: &[(String, String, String)]) -> Vec<(String, usize)> {
    windows
        .iter()
        .filter(|(_, _, name)| is_stash_window_name(name))
        .enumerate()
        .map(|(offset, (_, id, _))| (id.clone(), offset + 1))
        .collect()
}

fn normalize_stash_window_name(tmux: &Tmux, window_id: &str) {
    let _ = tmux.raw_cmd(&["rename-window", "-t", window_id, "stash"]);
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

fn current_tmux_session_name(tmux: &Tmux) -> Option<String> {
    tmux.current_session()
}

fn session_name_for_target_window(tmux: &Tmux, window: &str) -> Option<String> {
    tmux.cmd()
        .args(["display-message", "-t", window, "-p", "#{session_name}"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|name| !name.is_empty())
}

fn sync_candidate_files(col_args: &[String], focus: Option<&str>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(focused) = focus.map(str::trim).filter(|path| !path.is_empty()) {
        files.push(PathBuf::from(focused));
    }
    files.extend(
        col_args
            .iter()
            .flat_map(|arg| arg.split(','))
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from),
    );
    files
}

fn sync_doctor_repair_candidate(col_args: &[String], focus: Option<&str>) -> Option<PathBuf> {
    sync_candidate_files(col_args, focus)
        .into_iter()
        .find_map(|path| {
            if !path.exists() || frontmatter::read_session_id(&path).is_none() {
                return None;
            }
            Some(path.canonicalize().unwrap_or(path))
        })
}

fn canonical_sync_candidate_files(col_args: &[String], focus: Option<&str>) -> Vec<PathBuf> {
    sync_candidate_files(col_args, focus)
        .into_iter()
        .filter(|path| path.exists())
        .map(|path| path.canonicalize().unwrap_or(path))
        .collect()
}

fn common_ancestor_dir(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut iter = paths.iter();
    let first = iter.next()?;
    let mut common = if first.is_dir() {
        first.clone()
    } else {
        first.parent()?.to_path_buf()
    };

    for path in iter {
        let other = if path.is_dir() {
            path.clone()
        } else {
            path.parent()?.to_path_buf()
        };
        while !other.starts_with(&common) {
            common = common.parent()?.to_path_buf();
        }
    }

    Some(common)
}

pub fn shared_sync_scope_root(col_args: &[String], focus: Option<&str>) -> Option<PathBuf> {
    let files = canonical_sync_candidate_files(col_args, focus);
    let mut current = common_ancestor_dir(&files)?;
    loop {
        if current.join(".agent-doc").is_dir() {
            return Some(current);
        }
        current = current.parent()?.to_path_buf();
    }
}

fn sync_scope_root(col_args: &[String], focus: Option<&str>) -> Option<PathBuf> {
    shared_sync_scope_root(col_args, focus)
        .or_else(|| {
            focus
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .and_then(|path| crate::snapshot::find_project_root(Path::new(path)))
        })
        .or_else(|| {
            let cwd = std::env::current_dir().ok()?;
            crate::snapshot::find_project_root(&cwd)
                .or_else(|| cwd.join(".agent-doc").is_dir().then_some(cwd))
        })
}

fn layout_state_scope_root_for_sync(col_args: &[String], focus: Option<&str>) -> PathBuf {
    sync_scope_root(col_args, focus)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn layout_state_path_for_sync(col_args: &[String], focus: Option<&str>) -> PathBuf {
    layout_state_scope_root_for_sync(col_args, focus)
        .join(".agent-doc")
        .join("last_layout.json")
}

fn sync_prune_state_path_for_sync(col_args: &[String], focus: Option<&str>) -> PathBuf {
    let base = sync_scope_root(col_args, focus)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    base.join(".agent-doc").join("sync-prune-state.json")
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct SyncPruneState {
    fingerprint: String,
    last_full_cleanup_ms: u64,
}

fn epoch_millis_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn sync_prune_fingerprint(col_args: &[String], window: Option<&str>) -> String {
    serde_json::json!({
        "window": window.unwrap_or(""),
        "columns": col_args,
    })
    .to_string()
}

fn safe_passive_prune_cleanup_mode_at(
    state_path: &Path,
    col_args: &[String],
    window: Option<&str>,
    now_ms: u64,
) -> resync::PruneCleanupMode {
    let fingerprint = sync_prune_fingerprint(col_args, window);
    let throttle_ms = SAFE_PASSIVE_STASH_CLEANUP_THROTTLE.as_millis() as u64;
    let fresh_unchanged = std::fs::read_to_string(state_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<SyncPruneState>(&raw).ok())
        .is_some_and(|state| {
            state.fingerprint == fingerprint
                && now_ms.saturating_sub(state.last_full_cleanup_ms) < throttle_ms
        });

    if !fresh_unchanged {
        if let Some(parent) = state_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let state = SyncPruneState {
            fingerprint,
            last_full_cleanup_ms: now_ms,
        };
        if let Ok(raw) = serde_json::to_string(&state) {
            let _ = std::fs::write(state_path, raw);
        }
    }
    resync::PruneCleanupMode::SkipExpensiveStashCleanup
}

fn safe_passive_prune_cleanup_mode(
    auto_start_mode: AutoStartMode,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
) -> resync::PruneCleanupMode {
    if !matches!(auto_start_mode, AutoStartMode::SafePassive) {
        return resync::PruneCleanupMode::Full;
    }
    // Editor-driven safe-passive sync is the fast handoff path. It still prunes
    // stale registry rows and retained dead non-stash panes, but it must not
    // spend the selection budget scanning stash panes before tmux-router can
    // detach any extra visible pane from the active editor projection.
    let state_path = sync_prune_state_path_for_sync(col_args, focus);
    let _ = safe_passive_prune_cleanup_mode_at(&state_path, col_args, window, epoch_millis_now());
    resync::PruneCleanupMode::SkipExpensiveStashCleanup
}

fn effective_sync_columns(
    col_args: &[String],
    saved_layout: &[String],
    layout_state_path: &Path,
) -> Result<Vec<String>> {
    if !col_args.is_empty() {
        return Ok(col_args.to_vec());
    }

    if saved_layout.iter().all(|col| col.trim().is_empty()) {
        anyhow::bail!(
            "no sync columns provided and no recorded layout exists at {}",
            layout_state_path.display()
        );
    }

    Ok(saved_layout
        .iter()
        .map(|col| col.trim().to_string())
        .collect())
}

pub fn configured_session_for_root(tmux: &Tmux, root: &Path) -> Option<String> {
    let config_path = root.join(".agent-doc").join("config.toml");
    let configured = crate::project_config::load_project_from(&config_path).tmux_session;
    match configured {
        Some(session) if tmux.session_alive(&session) => Some(session),
        Some(session) => {
            eprintln!(
                "[sync] configured tmux_session '{}' is not alive for scope {}, ignoring stale pin",
                session,
                root.display()
            );
            None
        }
        None => None,
    }
}

fn resolve_sync_target_session(
    tmux: &Tmux,
    window: Option<&str>,
    col_args: &[String],
    focus: Option<&str>,
) -> Option<String> {
    let context_session = window.and_then(|target| session_name_for_target_window(tmux, target));
    if context_session.is_some() {
        return crate::route::resolve_preferred_session(tmux, context_session.as_deref(), "[sync]");
    }

    if let Some(scope_root) = shared_sync_scope_root(col_args, focus) {
        if let Some(session) = configured_session_for_root(tmux, &scope_root) {
            return Some(session);
        }
        return current_tmux_session_name(tmux);
    }

    crate::route::resolve_preferred_session(tmux, None, "[sync]")
}

fn resolve_agent_doc_window_id(
    tmux: &Tmux,
    session_name: &str,
    target_window_name: &str,
) -> Option<String> {
    let listing = tmux
        .raw_cmd(&[
            "list-windows",
            "-t",
            &format!("{}:", session_name),
            "-F",
            "#{window_id} #{window_name}",
        ])
        .ok()?;
    listing.lines().find_map(|line| {
        let mut parts = line.splitn(2, ' ');
        match (parts.next(), parts.next()) {
            (Some(window_id), Some(window_name)) if window_name == target_window_name => {
                Some(window_id.to_string())
            }
            _ => None,
        }
    })
}

fn window_name_for_window_id(tmux: &Tmux, window_id: &str) -> Option<String> {
    tmux.cmd()
        .args(["display-message", "-t", window_id, "-p", "#{window_name}"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|name| !name.is_empty())
}

fn target_is_agent_doc_window(tmux: &Tmux, target: &str) -> bool {
    window_name_for_window_id(tmux, target).as_deref() == Some("agent-doc")
}

fn is_stash_window_name(window_name: &str) -> bool {
    window_name == "stash" || window_name.starts_with("stash-")
}

/// Returns `true` when `pane_id` currently lives in a `stash` window.
///
/// Default focus uses this to avoid selecting a stashed pane in place:
/// selecting a pane that lives in the stash window surfaces editor focus
/// *inside* the stash instead of in the working `agent-doc` window
/// (`#jb-tsift-pane-sync`). When the target pane is stashed, the deferred sync
/// reconciler's atomic SWAP owns surfacing and selecting it (it swaps the
/// stashed pane into `agent-doc` and stashes the displaced pane in one
/// operation, then selects the focus pane), so exactly one path performs the
/// in/out transition. Best-effort: an unresolved window returns `false` so the
/// caller falls back to selecting in place.
pub fn pane_in_stash_window(tmux: &Tmux, pane_id: &str) -> bool {
    let Ok(window_id) = tmux.pane_window(pane_id) else {
        return false;
    };
    match window_name_for_window_id(tmux, &window_id) {
        Some(name) => is_stash_window_name(&name),
        None => false,
    }
}

/// Promote a live-owner pane out of a `stash` window into its session's
/// `agent-doc` window so editor focus surfaces it in the working layout instead
/// of selecting it in place inside the stash (`#stash-pane-promote-on-focus`).
///
/// The focus live-owner fix (submodule e6dafa52) selects the correct pane but
/// leaves it parked in the stash window; this reparents it. tmux preserves the
/// pane id across `join-pane` / `break-pane`, so callers keep selecting the same
/// pane id afterward.
///
/// Returns `Ok(true)` when the pane was reparented, `Ok(false)` when no
/// promotion was needed (pane not alive, window unresolved, or not a stash
/// window) or the move could not be completed. Best-effort: a failed move is
/// logged and reported as `Ok(false)` so focus still selects the pane in place.
pub fn promote_pane_to_agent_doc_window(tmux: &Tmux, pane_id: &str) -> Result<bool> {
    if !tmux.pane_alive(pane_id) {
        return Ok(false);
    }
    let Ok(window_id) = tmux.pane_window(pane_id) else {
        return Ok(false);
    };
    let Some(window_name) = window_name_for_window_id(tmux, &window_id) else {
        return Ok(false);
    };
    if !is_stash_window_name(&window_name) {
        return Ok(false);
    }
    let Ok(session_name) = tmux.pane_session(pane_id) else {
        return Ok(false);
    };

    let agent_doc_window = list_session_windows(tmux, &session_name)
        .into_iter()
        .find(|(_, _, name)| name == "agent-doc")
        .map(|(_, id, _)| id);

    if let Some(adw) = agent_doc_window {
        let target = tmux.largest_pane_in_window(&adw).or_else(|| {
            tmux.list_window_panes(&adw)
                .unwrap_or_default()
                .into_iter()
                .next()
        });
        let Some(target) = target.filter(|t| !t.is_empty()) else {
            return Ok(false);
        };
        sync_log(&format!(
            "promote_on_focus_action=join-pane src={} dst={} agent_doc_window={}",
            pane_id, target, adw
        ));
        match PaneMoveOp::new(tmux, pane_id, &target).join("-dh") {
            Ok(()) => {
                eprintln!(
                    "[focus] promoted live-owner pane {} from {} into the agent-doc window",
                    pane_id, window_name
                );
                sync_log(&format!(
                    "promote_on_focus_result=join-pane ok=true src={} dst={}",
                    pane_id, target
                ));
                Ok(true)
            }
            Err(e) => {
                eprintln!(
                    "[focus] promote join-pane {} → {} failed: {}, leaving in stash",
                    pane_id, target, e
                );
                sync_log(&format!(
                    "promote_on_focus_result=join-pane ok=false src={} dst={} err={}",
                    pane_id, target, e
                ));
                Ok(false)
            }
        }
    } else {
        // No agent-doc window exists yet: break the pane into its own window and
        // name it `agent-doc`, mirroring the stash rescue path.
        sync_log(&format!(
            "promote_on_focus_action=break-pane src={} from={}",
            pane_id, window_name
        ));
        if tmux.break_pane(pane_id).is_ok() {
            if let Ok(new_win) = tmux.pane_window(pane_id) {
                let _ = tmux.raw_cmd(&["rename-window", "-t", &new_win, "agent-doc"]);
            }
            eprintln!(
                "[focus] promoted live-owner pane {} from {} into a new agent-doc window",
                pane_id, window_name
            );
            sync_log(&format!(
                "promote_on_focus_result=break-pane ok=true src={}",
                pane_id
            ));
            Ok(true)
        } else {
            sync_log(&format!(
                "promote_on_focus_result=break-pane ok=false src={}",
                pane_id
            ));
            Ok(false)
        }
    }
}

fn list_session_windows(tmux: &Tmux, session_name: &str) -> Vec<(String, String, String)> {
    let Ok(output) = tmux.raw_cmd(&[
        "list-windows",
        "-t",
        &format!("{}:", session_name),
        "-F",
        "#{window_index} #{window_id} #{window_name}",
    ]) else {
        return Vec::new();
    };
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, ' ');
            let index = parts.next()?.to_string();
            let id = parts.next()?.to_string();
            let name = parts.next()?.to_string();
            Some((index, id, name))
        })
        .collect()
}

fn normalize_window_to_index(
    tmux: &Tmux,
    session_name: &str,
    window_id: &str,
    desired_index: usize,
    log_prefix: &str,
) {
    let windows = list_session_windows(tmux, session_name);
    let desired = desired_index.to_string();
    let Some((current_index, _, current_name)) =
        windows.iter().find(|(_, id, _)| id == window_id).cloned()
    else {
        return;
    };
    if current_index == desired {
        return;
    }

    if let Some((_, occupant_id, occupant_name)) = windows
        .iter()
        .find(|(index, _, _)| index == &desired)
        .cloned()
    {
        if occupant_id == window_id {
            return;
        }
        sync_log(&format!(
            "{}_action=swap-window src={} dst={} session={} src_name={} dst_name={}",
            log_prefix, current_index, desired, session_name, current_name, occupant_name
        ));
        let result = tmux.raw_cmd(&["swap-window", "-s", window_id, "-t", &occupant_id]);
        sync_log(&format!(
            "{}_result=swap-window ok={} src={} dst={}",
            log_prefix,
            result.is_ok(),
            current_index,
            desired
        ));
        let _ = result;
    } else {
        sync_log(&format!(
            "{}_action=move-window src={} dst={} session={} name={}",
            log_prefix, current_index, desired, session_name, current_name
        ));
        let result = tmux.raw_cmd(&[
            "move-window",
            "-s",
            window_id,
            "-t",
            &format!("{session_name}:{desired}"),
        ]);
        sync_log(&format!(
            "{}_result=move-window ok={} src={} dst={}",
            log_prefix,
            result.is_ok(),
            current_index,
            desired
        ));
        let _ = result;
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

fn run_with_options_internal(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
    exact_visible_projection: bool,
    tmux: &Tmux,
) -> Result<()> {
    let window = normalize_scope_arg(window);
    let focus = normalize_scope_arg(focus);
    tracing::debug!(
        cols = ?col_args,
        window,
        focus,
        auto_start_mode = auto_start_mode.log_label(),
        "sync::run_with_options start"
    );
    let sync_total_start = Instant::now();

    if matches!(auto_start_mode, AutoStartMode::SafePassive) {
        let prelock_focus_start = Instant::now();
        let _ = safe_passive_focus_actor_before_sync_lock(tmux, focus, window, col_args);
        log_sync_latency(
            focus,
            "prelock_actor_focus",
            prelock_focus_start.elapsed(),
            SYNC_PRELOCK_ACTOR_FOCUS_BUDGET,
            auto_start_mode,
        );
    }

    // Serialize sync calls via file lock. Concurrent syncs (from rapid tab switches)
    // race against each other's stash operations, causing pane bouncing. Contention
    // is bounded so a stuck prior editor sync cannot starve later selections forever.
    let lock_path = std::path::Path::new(".agent-doc/sync.lock");
    let sync_lock_wait_budget = if matches!(auto_start_mode, AutoStartMode::SafePassive) {
        SYNC_LOCK_WAIT_LATENCY_BUDGET
    } else {
        SYNC_LOCK_WAIT_BUDGET
    };
    let sync_lock_start = Instant::now();
    let lock_guard = acquire_sync_lock(lock_path, sync_lock_wait_budget);
    let sync_lock_elapsed = sync_lock_start.elapsed();
    log_sync_latency(
        focus,
        "sync_lock_wait",
        sync_lock_elapsed,
        SYNC_LOCK_WAIT_LATENCY_BUDGET,
        auto_start_mode,
    );
    if matches!(auto_start_mode, AutoStartMode::SafePassive) && !lock_guard.is_acquired() {
        let message =
            safe_passive_lock_contention_message(sync_lock_elapsed, sync_lock_wait_budget);
        eprintln!("{}", message);
        sync_log(&message);
        return Ok(());
    }
    let _lock_guard = lock_guard;

    // Check for new build and clear stale caches
    check_build_stamp();
    if let Ok(cwd) = std::env::current_dir()
        && let Some(project_root) = crate::snapshot::find_project_root(&cwd)
    {
        match crate::project_controller::close_stale_starting_actors_for_caller(
            &project_root,
            std::time::Duration::from_secs(3600),
            false,
            "sync",
        ) {
            Ok((closed, kept)) if closed > 0 => {
                eprintln!(
                    "[sync] actors: {} stale starting closed, {} still active",
                    closed, kept
                );
            }
            Ok(_) => {}
            Err(e) => eprintln!("[sync] actor gc warning: {}", e),
        }
    }

    // Column memory: for columns with non-agent files, substitute the last known
    // agent doc so the reconciler preserves the pane from the previous layout.
    // When sync is called without explicit columns, fall back to that recorded layout.
    let layout_state_root = layout_state_scope_root_for_sync(col_args, focus);
    let layout_state_path = layout_state_path_for_sync(col_args, focus);
    let saved_layout = match crate::project_controller::load_layout_state(&layout_state_root) {
        Ok(layout) => layout,
        Err(err) => {
            eprintln!(
                "[sync] warning: failed to load controller layout state from {}: {}",
                layout_state_root.display(),
                err
            );
            std::fs::read_to_string(&layout_state_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        }
    };

    let input_cols = effective_sync_columns(col_args, &saved_layout, &layout_state_path)?;
    let mut col_args: Vec<String> = apply_column_memory(&input_cols, &saved_layout)
        .into_iter()
        .filter(|col| !col.is_empty())
        .collect();
    let proof_cache = SyncProofCache::default();

    // Resolve the target session/window. Full/manual sync delegates repair to
    // the same file-scoped doctor path that operators can run explicitly;
    // passive editor sync keeps layout repair explicit.
    let window_resolution_start = Instant::now();
    let target_session = resolve_sync_target_session(tmux, window, &col_args, focus);
    let full_sync = matches!(auto_start_mode, AutoStartMode::Full);
    let doctor_repair_candidate = if full_sync {
        sync_doctor_repair_candidate(&col_args, focus)
    } else {
        None
    };
    if full_sync {
        if let Some(file) = doctor_repair_candidate.as_deref() {
            let notes = repair_file_state_with_tmux(tmux, file)?;
            for note in notes {
                let message = format!("[sync] doctor repair: {note}");
                eprintln!("{message}");
                sync_log(&message);
            }
        } else if window.is_none()
            && let Some(session_name) = target_session.as_deref()
        {
            repair_layout(tmux, session_name, "agent-doc")?;
        }
    }
    let mut effective_window = match (window, target_session.as_deref()) {
        (Some(w), _) => Some(w.to_string()),
        (None, Some(session_name)) => Some(format!("{session_name}:agent-doc")),
        (None, None) => None,
    };
    let explicit_window_is_agent_doc =
        window.is_some_and(|target| target_is_agent_doc_window(tmux, target));
    if let Some(ref session_name) = target_session {
        if let Some(resolved_window_id) =
            resolve_agent_doc_window_id(tmux, session_name, "agent-doc")
            && effective_window.as_deref() != Some(resolved_window_id.as_str())
        {
            if let Some(previous) = effective_window.as_deref() {
                eprintln!(
                    "[sync] resolved target window after repair: {} → {}",
                    previous, resolved_window_id
                );
            }
            effective_window = Some(resolved_window_id);
        } else {
            let warning = format!(
                "[sync] session {} has no visible agent-doc window; normal sync will not auto-repair stash/layout drift. Use `agent-doc fix` or `agent-doc session doctor <file> --repair` if you want layout repair.",
                session_name
            );
            eprintln!("{}", warning);
            sync_log(&warning);
            if let Some(target) = window
                && !explicit_window_is_agent_doc
            {
                let refusal = format!(
                    "[sync] explicit window {} is not an agent-doc window; preserving layout instead of reconciling onto the wrong tmux window",
                    target
                );
                eprintln!("{}", refusal);
                sync_log(&refusal);
                return Ok(());
            }
        }
    }
    let window = effective_window.as_deref();
    let remembered_layout = if saved_layout.len() >= 2 {
        saved_layout.clone()
    } else {
        visible_registered_layout(tmux, window)
    };
    let active_column_index = if matches!(auto_start_mode, AutoStartMode::SafePassive) {
        focused_column_index(&remembered_layout, focus).or_else(|| {
            active_pane_column_index(
                tmux,
                target_session.as_deref(),
                window,
                remembered_layout.len(),
            )
        })
    } else {
        None
    };
    log_sync_latency(
        focus,
        "window_resolution",
        window_resolution_start.elapsed(),
        SYNC_WINDOW_RESOLUTION_BUDGET,
        auto_start_mode,
    );
    if matches!(auto_start_mode, AutoStartMode::SafePassive) {
        let focus_start = Instant::now();
        let _ = safe_passive_focus_actor_after_sync_lock(tmux, focus, &proof_cache);
        log_sync_latency(
            focus,
            "postlock_actor_focus",
            focus_start.elapsed(),
            SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET,
            auto_start_mode,
        );
    }
    col_args = apply_focus_only_expansion_policy(
        &col_args,
        &remembered_layout,
        active_column_index,
        auto_start_mode,
        exact_visible_projection,
    )
    .into_iter()
    .filter(|col| !col.is_empty())
    .collect();
    let col_args = col_args.as_slice();
    sync_log(&format!(
        "=== sync start: col_args={:?} window={:?} focus={:?} auto_start_mode={}",
        col_args,
        window,
        focus,
        auto_start_mode.log_label()
    ));

    // Diagnostic: log pane count at key checkpoints to find where stashed panes reappear
    if let Some(w) = window {
        let pane_count = tmux.list_window_panes(w).map(|p| p.len()).unwrap_or(0);
        let pane_list: Vec<String> = tmux.list_window_panes(w).unwrap_or_default();
        sync_log(&format!(
            "checkpoint:post-window-resolution window={} panes={} list={:?}",
            w, pane_count, pane_list
        ));
    }

    let prune_start = Instant::now();
    let prune_cleanup_mode =
        safe_passive_prune_cleanup_mode(auto_start_mode, col_args, window, focus);
    if matches!(
        prune_cleanup_mode,
        resync::PruneCleanupMode::SkipExpensiveStashCleanup
    ) {
        sync_log("safe_passive_prune_cleanup_skipped reason=unchanged_recent_layout");
    }
    let prune_timings = match resync::prune_with_tmux_timed_in_mode(tmux, prune_cleanup_mode) {
        Ok((_removed, timings)) => timings,
        Err(err) => {
            eprintln!("[sync] warning: prune failed: {}", err);
            sync_log(&format!("warning: prune failed: {}", err));
            Vec::new()
        }
    }; // Clean stale entries before layout calculation
    for timing in prune_timings {
        log_sync_latency(
            focus,
            timing.phase,
            timing.elapsed,
            SYNC_PRUNE_SUBPHASE_BUDGET,
            auto_start_mode,
        );
    }
    log_sync_latency(
        focus,
        "prune",
        prune_start.elapsed(),
        SYNC_PRUNE_BUDGET,
        auto_start_mode,
    );

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
    let blocked_unresolved_files: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());

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
                let has_registry = lookup_registry_entry_for_file_session(path, key).is_some();
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

    // Pre-sync: auto-start agent sessions for files that have session UUIDs
    // but no alive panes. Full mode may provision after recovery/fail-closed
    // checks. Safe-passive mode keeps those same checks, but only provisions
    // when the latest session log proves the prior owner already closed.
    {
        let ownership_proof_start = Instant::now();
        let mut auto_started_panes: Vec<(String, String)> = Vec::new();
        let claimed_sync_panes: RefCell<std::collections::HashMap<String, PathBuf>> =
            RefCell::new(std::collections::HashMap::new());

        // Parse file paths from col_args (each arg is "file1.md,file2.md").
        // Dedup so the auto-start pass makes one pane decision per document and
        // cannot start a second editor pane for a document requested in more
        // than one column (see `auto_start_candidate_files`).
        let all_files: Vec<PathBuf> = auto_start_candidate_files(col_args);

        // Determine the target session for auto-start:
        // 1. From frontmatter tmux_session (if alive)
        // 2. From --window argument
        // 3. Falls back to None (current session)
        let context_session: Option<String> = target_session
            .clone()
            .or_else(|| window.and_then(|w| session_name_for_target_window(tmux, w)));
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

            let authoritative_actor_pane = project_authoritative_actor_binding(
                tmux,
                file_path,
                &session_id,
                focus,
                auto_start_mode,
                &proof_cache,
            );
            let registered_entry = lookup_registry_entry_for_file_session(file_path, &session_id);
            let registered_pane = authoritative_actor_pane
                .or_else(|| registered_entry.as_ref().map(|entry| entry.pane.clone()));
            if let Some((miss, supersession)) =
                crate::startup_miss::take_superseded_startup_miss(file_path)?
            {
                let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
                eprintln!(
                    "[sync] clearing stale startup-miss on pane {} from {} for {} because newer registered owner {} already took over",
                    miss.pane_id,
                    miss_ts,
                    file_path.display(),
                    supersession.registered_pane
                );
                sync_log(&format!(
                    "startup_miss_cleared_superseded file={} stale_pane={} registered_pane={} miss_timestamp={} latest_start_timestamp={}",
                    file_path.display(),
                    miss.pane_id,
                    supersession.registered_pane,
                    miss_ts,
                    supersession.latest_start_timestamp
                ));
            }
            let unresolved_startup_miss = crate::startup_miss::load(file_path).ok().flatten();

            // Files with session UUIDs but no registry entry are auto-started.
            // The registry was likely pruned when the pane died. The user's intent
            // (navigating to the file in a split) is clear — create a pane for it.
            eprintln!(
                "[sync] auto-start check: {} session={} registered_pane={}",
                file_path.display(),
                &session_id[..8.min(session_id.len())],
                registered_pane.as_deref().unwrap_or("none")
            );
            let claimed_owner = registered_pane
                .as_ref()
                .and_then(|pane| claimed_sync_pane_owner(&claimed_sync_panes, pane, file_path));
            if let (Some(pane), Some(owner)) = (registered_pane.as_ref(), claimed_owner.as_ref()) {
                eprintln!(
                    "[sync] pane {} is already reserved for {} in this sync run; treating {} as unresolved so layout can rehydrate a distinct pane",
                    pane,
                    owner.display(),
                    file_path.display()
                );
                sync_log(&format!(
                    "duplicate_live_pane_claim pane={} owner={} duplicate={}",
                    pane,
                    owner.display(),
                    file_path.display()
                ));
            }
            if matches!(auto_start_mode, AutoStartMode::SafePassive)
                && let Some(pane_id) = registered_pane.as_ref()
                && claimed_owner.is_none()
                && registered_pane_proves_live_owner(
                    tmux,
                    file_path,
                    &session_id,
                    pane_id,
                    &proof_cache,
                )
            {
                eprintln!(
                    "[sync] safe passive sync reusing authoritative actor or supervisor-backed registered pane {} for {}",
                    pane_id,
                    file_path.display()
                );
                sync_log(&format!(
                    "safe_passive_reuse_registered_projection file={} pane={}",
                    file_path.display(),
                    pane_id
                ));
                reserve_sync_pane(&claimed_sync_panes, pane_id, file_path);
                continue;
            }
            let registered_live_owner = registered_pane.as_ref().is_some_and(|pane| {
                registered_pane_proves_live_owner(tmux, file_path, &session_id, pane, &proof_cache)
            });
            if let Some(pane) = registered_pane.as_ref()
                && tmux.pane_alive(pane)
                && !registered_live_owner
            {
                if claimed_owner.is_none()
                    && let Some(diagnostic) =
                        open_session_log_owner_fail_closed_diagnostic(file_path, &session_id, pane)?
                {
                    eprintln!(
                        "[sync] pane {} for {} is alive but ownership proof weakened while the session log still shows that pane as the latest open owner ({}) — failing closed instead of recording registered_pane_missing",
                        pane,
                        file_path.display(),
                        diagnostic
                    );
                    sync_log(&format!(
                        "registered_pane_open_session_log_owner file={} pane={} action=fail_closed diagnostic={}",
                        file_path.display(),
                        pane,
                        diagnostic
                    ));
                    let reason = sanitize_excerpt(&diagnostic).unwrap_or_else(|| {
                        "latest open session-log owner still points to this pane".to_string()
                    });
                    let _ = crate::startup_miss::append_session_log_event(
                        file_path,
                        &session_id,
                        &format!(
                            "registered_pane_open_session_log_owner pane={} reason={} action=fail_closed",
                            pane, reason
                        ),
                    );
                    blocked_unresolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
                    continue;
                }
                if claimed_owner.is_none()
                    && let Some(protected) = protected_registered_pane_state(tmux, file_path, pane)
                {
                    let reason = sanitize_excerpt(&protected.reason)
                        .unwrap_or_else(|| "drafted prompt input".to_string());
                    let excerpt = protected
                        .last_visible_excerpt
                        .as_deref()
                        .unwrap_or("<none>");
                    eprintln!(
                        "[sync] pane {} for {} is alive but shows protected Codex input ({}) — failing closed instead of recording registered_pane_missing",
                        pane,
                        file_path.display(),
                        reason
                    );
                    sync_log(&format!(
                        "registered_pane_protected file={} pane={} reason={} excerpt={}",
                        file_path.display(),
                        pane,
                        reason,
                        excerpt
                    ));
                    let mut event = format!(
                        "registered_pane_protected pane={} reason={} action=fail_closed",
                        pane, reason
                    );
                    if let Some(excerpt) = protected.last_visible_excerpt.as_deref() {
                        event.push_str(&format!(" last_visible_excerpt={excerpt}"));
                    }
                    let _ = crate::startup_miss::append_session_log_event(
                        file_path,
                        &session_id,
                        &event,
                    );
                    blocked_unresolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
                    continue;
                }
                eprintln!(
                    "[sync] pane {} for {} is alive but no longer proves ownership — treating it as unresolved instead of reusing the stale binding",
                    pane,
                    file_path.display()
                );
                sync_log(&format!(
                    "registered_pane_unowned file={} pane={} action=treat_unresolved",
                    file_path.display(),
                    pane
                ));
            }
            let has_alive_pane = claimed_owner.is_none()
                && registered_pane
                    .as_ref()
                    .map(|pane| {
                        if !registered_live_owner {
                            return false;
                        }
                        // Stashed panes are alive — don't rescue here. The reconciler's
                        // SWAP fast path handles 1-in/1-out atomically via swap-pane,
                        // avoiding the 3-pane bounce (rescue→reconcile→stash another).
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
                                        "[sync] pane {} for {} is in session '{}' stash; cross-session — treating as alive",
                                        pane, file_path.display(), pane_session
                                    );
                                    sync_log(&format!(
                                        "stash_pane_cross_session pane={} file={} actual_session={} target_session={}",
                                        pane, file_path.display(), pane_session, target_sess
                                    ));
                                } else {
                                    eprintln!(
                                        "[sync] pane {} for {} is in stash — deferring rescue to reconciler",
                                        pane, file_path.display()
                                    );
                                    sync_log(&format!(
                                        "stash_pane_deferred pane={} file={} stash_window={}",
                                        pane, file_path.display(), win_name
                                    ));
                                }
                                return true;
                            }
                        }
                        true
                    })
                .unwrap_or(false);

            if has_alive_pane {
                if let Some(ref pane) = registered_pane {
                    reserve_sync_pane(&claimed_sync_panes, pane, file_path);
                }
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
                        if let Err(e) =
                            reregister_recovered_owner(tmux, file_path, &session_id, pane)
                        {
                            eprintln!("[sync] warning: re-register failed: {}", e);
                        }
                    }
                }
                continue;
            }

            if should_skip_autostart_for_unresolved_startup_miss(
                registered_pane.as_deref(),
                registered_pane
                    .as_deref()
                    .is_some_and(|pane| tmux.pane_alive(pane)),
                unresolved_startup_miss.as_ref(),
            ) {
                let miss = unresolved_startup_miss
                    .as_ref()
                    .expect("guard checked presence");
                let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
                eprintln!(
                    "[sync] unresolved startup-miss {} still belongs to alive pane {} for {} — skipping auto-start instead of rebinding over the existing owner",
                    miss_ts,
                    miss.pane_id,
                    file_path.display()
                );
                sync_log(&format!(
                    "startup_miss_skip_autostart file={} pane={} miss_timestamp={}",
                    file_path.display(),
                    miss.pane_id,
                    miss_ts
                ));
                blocked_unresolved_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
                continue;
            }

            if matches!(auto_start_mode, AutoStartMode::SafePassive) && registered_pane.is_none() {
                if has_rename_debounce(file_path) {
                    eprintln!(
                        "[sync] skipping auto-start for {} (rename debounce active)",
                        file_path.display()
                    );
                    sync_log(&format!(
                        "rename-debounce: skipped auto-start for {}",
                        file_path.display()
                    ));
                    blocked_unresolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
                    continue;
                }

                if skip_auto_start_for_recent_session_loss(file_path, &session_id)? {
                    blocked_unresolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
                    continue;
                }

                if let Some(reason) = passive_autostart_skip_reason(
                    tmux,
                    file_path,
                    &session_id,
                    unresolved_startup_miss.as_ref(),
                )? {
                    eprintln!(
                        "[sync] safe passive sync is not auto-starting {} ({})",
                        file_path.display(),
                        reason
                    );
                    sync_log(&format!(
                        "safe_passive_autostart_skipped file={} reason={}",
                        file_path.display(),
                        reason
                    ));
                    blocked_unresolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
                    continue;
                }

                sync_log(&format!(
                    "safe_passive_fast_autostart file={} mode={}",
                    file_path.display(),
                    auto_start_mode.log_label()
                ));
                eprintln!(
                    "[sync] safe passive sync is cold-starting {} after no matching pane or registered owner was found",
                    file_path.display()
                );
                let file_str = file_path.to_string_lossy().to_string();
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
                        reserve_sync_pane(&claimed_sync_panes, &pane_id, file_path);
                        auto_started_panes.push((pane_id, file_str.clone()));
                    }
                    Err(e) => {
                        eprintln!(
                            "[sync] warning: auto-start failed for {}: {}",
                            file_path.display(),
                            e
                        );
                        blocked_unresolved_files
                            .borrow_mut()
                            .insert(file_path.to_path_buf());
                    }
                }
                continue;
            }

            // No alive pane in registry. Before auto-starting, check if any
            // alive pane in the target session is already running agent-doc
            // for this file (registry may have been pruned or stale).
            // This prevents creating duplicate panes.
            let associated_candidates = find_associated_panes(tmux, file_path, &session_id);
            match resolve_associated_panes(associated_candidates.clone(), window) {
                AssociatedPaneResolution::Selected { winner, redundant } => {
                    let detail = std::iter::once(&winner)
                        .chain(redundant.iter())
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
                        "[sync] found legacy associated pane evidence for {} but normal sync will not reclaim ownership from {} automatically; run an explicit claim/repair instead",
                        file_path.display(),
                        winner.pane_id
                    );
                    sync_log(&format!(
                        "associated_pane_requires_explicit_repair file={} pane={} sources={} redundant={} candidates={}",
                        file_path.display(),
                        winner.pane_id,
                        winner.source_summary(),
                        redundant.len(),
                        detail
                    ));
                    blocked_unresolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
                    continue;
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
                        "[sync] found multiple legacy associated panes for {}; normal sync will not re-elect ownership. Resolve with explicit claim/repair.",
                        file_path.display()
                    );
                    sync_log(&format!(
                        "associated_pane_ambiguous_requires_explicit_repair file={} candidates={}",
                        file_path.display(),
                        detail
                    ));
                    blocked_unresolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
                    continue;
                }
                AssociatedPaneResolution::None => {}
            }

            if let Some(ref pane) = registered_pane {
                match repair_missing_registered_pane(
                    tmux,
                    file_path,
                    &session_id,
                    pane,
                    registered_entry.as_ref().map(|entry| entry.window.as_str()),
                    MissingRegisteredPaneRepairMode::InspectOnly,
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
                        if let Some(phase) = repair.closeout_recovery_phase.as_deref() {
                            if let Some(outcome) = repair.closeout_recovery_outcome {
                                eprintln!(
                                    "[sync] recovered {} closeout for {} after pane {} disappeared ({})",
                                    phase,
                                    file_path.display(),
                                    pane,
                                    repair_outcome_label(outcome)
                                );
                                sync_log(&format!(
                                    "missing-pane closeout recovered file={} pane={} phase={} outcome={}",
                                    file_path.display(),
                                    pane,
                                    phase,
                                    repair_outcome_label(outcome)
                                ));
                            } else if let Some(err) = repair.closeout_recovery_error.as_deref() {
                                eprintln!(
                                    "[sync] warning: failed to recover {} closeout for {} after pane {} disappeared: {}",
                                    phase,
                                    file_path.display(),
                                    pane,
                                    err
                                );
                                sync_log(&format!(
                                    "warning: missing-pane closeout recovery failed file={} pane={} phase={} err={}",
                                    file_path.display(),
                                    pane,
                                    phase,
                                    err
                                ));
                            }
                        }
                        if let Some(reason) = repair.block_auto_start_reason.as_deref() {
                            eprintln!("[sync] {}", reason);
                            sync_log(&format!(
                                "missing-pane auto-start blocked file={} pane={} reason={}",
                                file_path.display(),
                                pane,
                                reason
                            ));
                            blocked_unresolved_files
                                .borrow_mut()
                                .insert(file_path.to_path_buf());
                            continue;
                        } else if let Some(err) = repair.closeout_recovery_error.as_deref() {
                            eprintln!(
                                "[sync] warning: failed to inspect closeout state for {} after pane {} disappeared: {}",
                                file_path.display(),
                                pane,
                                err
                            );
                            sync_log(&format!(
                                "warning: missing-pane closeout state inspection failed file={} pane={} err={}",
                                file_path.display(),
                                pane,
                                err
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
                blocked_unresolved_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
                continue;
            }

            if skip_auto_start_for_recent_session_loss(file_path, &session_id)? {
                blocked_unresolved_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
                continue;
            }

            if matches!(auto_start_mode, AutoStartMode::SafePassive)
                && let Some(reason) = passive_autostart_skip_reason(
                    tmux,
                    file_path,
                    &session_id,
                    unresolved_startup_miss.as_ref(),
                )?
            {
                eprintln!(
                    "[sync] safe passive sync is not auto-starting {} ({})",
                    file_path.display(),
                    reason
                );
                sync_log(&format!(
                    "safe_passive_autostart_skipped file={} reason={}",
                    file_path.display(),
                    reason
                ));
                blocked_unresolved_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
                continue;
            }

            sync_log(&format!(
                "auto-starting session for {} (no alive pane, mode={})",
                file_path.display(),
                auto_start_mode.log_label()
            ));
            eprintln!(
                "[sync] auto-starting session for {} (no alive pane, mode={})",
                file_path.display(),
                auto_start_mode.log_label()
            );
            let file_str = file_path.to_string_lossy().to_string();
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
                    reserve_sync_pane(&claimed_sync_panes, &pane_id, file_path);
                    auto_started_panes.push((pane_id, file_str.clone()));
                }
                Err(e) => {
                    eprintln!(
                        "[sync] warning: auto-start failed for {}: {}",
                        file_path.display(),
                        e
                    );
                    blocked_unresolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
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
        log_sync_latency(
            focus,
            "ownership_proof",
            ownership_proof_start.elapsed(),
            SYNC_OWNERSHIP_PROOF_BUDGET,
            auto_start_mode,
        );
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

    let tmux_router_registry = match build_tmux_router_sync_registry(tmux, col_args, &proof_cache) {
        Ok(registry) => registry,
        Err(err) => {
            let warning = format!(
                "[sync] warning: failed to build synthetic tmux-router registry: {}",
                err
            );
            eprintln!("{}", warning);
            sync_log(&warning);
            None
        }
    };
    if matches!(auto_start_mode, AutoStartMode::SafePassive) {
        let mut blocked_files: Vec<String> = blocked_unresolved_files
            .borrow()
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        blocked_files.sort();
        if !blocked_files.is_empty() {
            let blocked_summary = blocked_files.join(", ");
            eprintln!(
                "[sync] safe passive sync preserved the current tmux layout because unresolved files remain blocked: {}",
                blocked_summary
            );
            if let Some(target_window) = window
                && let Some(pane) = select_visible_focus_pane_if_present(tmux, target_window, focus)
            {
                emit_preserved_layout_focus_marker(&pane, "blocked_files");
            }
            sync_log(&format!(
                "safe_passive_layout_preserved blocked_files={}",
                blocked_summary
            ));
            return Ok(());
        }
    }
    let tmux_router_registry_path = tmux_router_registry
        .as_ref()
        .map(|file| file.path())
        .unwrap_or(registry_path.as_path());
    let allow_unresolved_pane_assignment =
        |path: &Path| !blocked_unresolved_files.borrow().contains(path);
    let logged_open_cycle_panes: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    let log_open_cycle_detach = |pane_id: &str| {
        let Some(protected) = open_cycle_protected_pane_state(tmux, pane_id) else {
            return false;
        };
        if logged_open_cycle_panes
            .borrow_mut()
            .insert(pane_id.to_string())
        {
            eprintln!(
                "[sync] allowing pane {} for {} to be stashed during reconcile while its {} cycle remains open",
                pane_id,
                protected.file.display(),
                protected.phase
            );
            sync_log(&format!(
                "reconcile_open_cycle_detachable pane={} file={} phase={}",
                pane_id,
                protected.file.display(),
                protected.phase
            ));
        }
        true
    };
    let allow_open_cycle_detach = |pane_id: &str| {
        let _ = log_open_cycle_detach(pane_id);
        false
    };
    let tmux_router_options = tmux_router::SyncOptions {
        protect_pane: Some(&allow_open_cycle_detach),
        allow_unresolved_pane_assignment: Some(&allow_unresolved_pane_assignment),
    };

    // Open-cycle panes are logged during DETACH, but they are not kept visible.
    // Stashing preserves the process and avoids recurring 3-pane projections.
    let router_start = Instant::now();
    let result = tmux_router::sync_with_options(
        col_args,
        window,
        focus,
        tmux,
        tmux_router_registry_path,
        &resolve_file,
        &tmux_router_options,
    )?;
    log_sync_latency(
        focus,
        "tmux_router",
        router_start.elapsed(),
        SYNC_ROUTER_BUDGET,
        auto_start_mode,
    );

    // Log pane count after tmux_router::sync
    if let Some(w) = window {
        let pane_count = tmux.list_window_panes(w).map(|p| p.len()).unwrap_or(0);
        sync_log(&format!(
            "post-tmux_router::sync: window={} panes={} file_panes={}",
            w,
            pane_count,
            result.file_panes.len()
        ));
        let projected = projected_sync_pane_count(col_args);
        if projected > 0 && pane_count > projected {
            let wanted_panes: HashSet<String> = result
                .file_panes
                .iter()
                .map(|(_, pane)| pane.clone())
                .collect();
            let extra_panes: Vec<String> = tmux
                .list_window_panes(w)
                .unwrap_or_default()
                .into_iter()
                .filter(|pane| !wanted_panes.contains(pane))
                .collect();
            let unprotected_extras: Vec<String> = extra_panes
                .iter()
                .filter(|pane| !log_open_cycle_detach(pane))
                .cloned()
                .collect();
            if unprotected_extras.is_empty() {
                sync_log(&format!(
                    "layout_projection_exceeded_by_open_cycles window={} desired_panes={} actual_visible_panes={} extras={:?}",
                    w, projected, pane_count, extra_panes
                ));
            } else {
                let warning = format!(
                    "[sync] warning: visible pane count {} exceeds requested editor projection {} after sync for window {}",
                    pane_count, projected, w
                );
                eprintln!("{}", warning);
                sync_log(&format!(
                    "layout_projection_exceeded window={} desired_panes={} actual_visible_panes={} unprotected_extras={:?}",
                    w, projected, pane_count, unprotected_extras
                ));
            }
        }
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
        let layout_state = build_layout_state(col_args, &saved_layout);
        // Only save if at least one column has an agent doc
        if layout_state.iter().any(|s| !s.is_empty())
            && let Err(err) =
                crate::project_controller::store_layout_state(&layout_state_root, &layout_state)
        {
            eprintln!(
                "[sync] warning: failed to persist controller layout state for {}: {}",
                layout_state_root.display(),
                err
            );
        }
    }

    // tmux_session frontmatter write-back removed (deprecated).
    // Session targeting now uses --window arg or pane introspection.

    // Post-sync: register/update claims for all synced files using the
    // file→pane assignments from tmux-router. This ensures autoclaim works
    // for files arranged by sync, even if they were never individually claimed.
    register_synced_files_with_cache(
        tmux,
        &session_files.borrow(),
        &result.file_panes,
        &proof_cache,
    );

    // Post-sync: validate session state (report only, no kill).
    // Disabled --fix because auto_start with context_session intentionally places
    // cross-session panes — resync --fix would kill them (lesson: context_session override).
    if let Err(e) = resync::run(false, None, None) {
        eprintln!("[sync] warning: post-sync resync failed: {}", e);
    }
    if full_sync {
        if let Some(file) = doctor_repair_candidate.as_deref() {
            let notes = repair_file_state_with_tmux(tmux, file)?;
            for note in notes {
                let message = format!("[sync] post-sync doctor repair: {note}");
                eprintln!("{message}");
                sync_log(&message);
            }
        } else if window.is_none()
            && let Some(session_name) = target_session.as_deref()
        {
            repair_layout(tmux, session_name, "agent-doc")?;
        }
    }

    if matches!(auto_start_mode, AutoStartMode::SafePassive) {
        log_sync_latency(
            focus,
            "safe_passive_total",
            sync_total_start.elapsed(),
            SYNC_SAFE_PASSIVE_TOTAL_BUDGET,
            auto_start_mode,
        );
    }
    if let Some(focus) = focus.map(str::trim).filter(|path| !path.is_empty()) {
        crate::editor_route_errors::clear_for_success(Path::new(focus), "sync_success");
    }

    Ok(())
}

/// Register or update registry entries for synced files.
///
/// Uses the file→pane assignments from `SyncResult::file_panes` to create
/// registry entries for files that don't have one yet, and update file paths
/// for existing entries.
#[cfg(test)]
fn register_synced_files(
    tmux: &Tmux,
    session_files: &[(String, PathBuf)],
    file_panes: &[(PathBuf, String)],
) {
    let proof_cache = SyncProofCache::default();
    register_synced_files_with_cache(tmux, session_files, file_panes, &proof_cache);
}

fn register_synced_files_with_cache(
    tmux: &Tmux,
    session_files: &[(String, PathBuf)],
    file_panes: &[(PathBuf, String)],
    proof_cache: &SyncProofCache,
) {
    if session_files.is_empty() || file_panes.is_empty() {
        return;
    }

    // Build file→pane lookup from sync result
    let pane_lookup: std::collections::HashMap<&Path, &str> = file_panes
        .iter()
        .map(|(p, id)| (p.as_path(), id.as_str()))
        .collect();
    let mut pane_claim_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (_, file_path) in session_files {
        let Some(&pane_id) = pane_lookup.get(file_path.as_path()) else {
            continue;
        };
        *pane_claim_counts.entry(pane_id.to_string()).or_default() += 1;
    }
    let mut acceptable_duplicate_claims: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (session_id, file_path) in session_files {
        let Some(&pane_id) = pane_lookup.get(file_path.as_path()) else {
            continue;
        };
        if pane_claim_counts.get(pane_id).copied().unwrap_or(0) < 2 {
            continue;
        }
        let Some((_, project_root, registry_key)) = registry_location_for_file(file_path) else {
            continue;
        };
        let registry_root_matches = sessions::load_in(&project_root)
            .ok()
            .and_then(|registry| registry.get(&registry_key).cloned())
            .is_some_and(|entry| {
                entry.session_id == *session_id
                    && entry.pane == pane_id
                    && registry_entry_matches_document_root(&entry, &project_root)
            });
        let live_owner_matches = sync_actor_or_live_owner_matches_cached(
            tmux,
            file_path,
            session_id,
            pane_id,
            proof_cache,
        );
        if pane_assignment_matches_document_root(tmux, pane_id, &project_root)
            || registry_root_matches
            || live_owner_matches
        {
            *acceptable_duplicate_claims
                .entry(pane_id.to_string())
                .or_default() += 1;
        }
    }

    for (session_id, file_path) in session_files {
        let Some(&pane_id) = pane_lookup.get(file_path.as_path()) else {
            continue;
        };
        if let Some(actor_pane) = authoritative_actor_pane_for_document(tmux, file_path, session_id)
            && pane_id != actor_pane
        {
            eprintln!(
                "[sync] refusing geometry-only pane assignment {} for {} because authoritative actor pane {} still owns the document",
                pane_id,
                file_path.display(),
                actor_pane
            );
            sync_log(&format!(
                "register_synced_files_skip_actor_projection file={} pane={} authoritative_pane={}",
                file_path.display(),
                pane_id,
                actor_pane
            ));
            continue;
        }
        let live_owner_matches = sync_actor_or_live_owner_matches_cached(
            tmux,
            file_path,
            session_id,
            pane_id,
            proof_cache,
        );
        let fail_closed_binding_guard = crate::startup_miss::load(file_path)
            .ok()
            .flatten()
            .is_some()
            || crate::startup_miss::recent_session_loss_window(file_path, session_id)
                .ok()
                .flatten()
                .is_some();
        let Some((canonical_file, project_root, registry_key)) =
            registry_location_for_file(file_path)
        else {
            continue;
        };
        let registry_path = sessions::registry_path_in(&project_root);
        let Ok(_lock) = sessions::RegistryLock::acquire(&registry_path) else {
            continue;
        };
        let Ok(mut registry) = sessions::load_in(&project_root) else {
            continue;
        };
        if fail_closed_binding_guard && !live_owner_matches {
            if let Some(entry) = registry.get(&registry_key)
                && entry.pane == pane_id
            {
                eprintln!(
                    "[sync] removing fail-closed geometry-only pane binding for {} → {}",
                    file_path.display(),
                    pane_id
                );
                registry.remove(&registry_key);
                let _ = sessions::save_in(&project_root, &registry);
            }
            eprintln!(
                "[sync] refusing geometry-only pane assignment {} for {} while fail-closed recovery is active",
                pane_id,
                file_path.display()
            );
            sync_log(&format!(
                "register_synced_files_skip_unowned_fail_closed file={} pane={} guard_active=true",
                file_path.display(),
                pane_id
            ));
            continue;
        }
        let duplicate_claim_count = pane_claim_counts.get(pane_id).copied().unwrap_or(0);
        if duplicate_claim_count > 1 {
            let registry_root_matches = registry
                .get(&registry_key)
                .is_some_and(|entry| registry_entry_matches_document_root(entry, &project_root));
            let claim_acceptable =
                pane_assignment_matches_document_root(tmux, pane_id, &project_root)
                    || registry_root_matches
                    || live_owner_matches;
            let acceptable_claim_count = acceptable_duplicate_claims
                .get(pane_id)
                .copied()
                .unwrap_or(0);
            if !claim_acceptable || acceptable_claim_count != 1 {
                if let Some(entry) = registry.get(&registry_key)
                    && entry.pane == pane_id
                    && !claim_acceptable
                {
                    eprintln!(
                        "[sync] removing stale duplicate pane binding for {} → {}",
                        file_path.display(),
                        pane_id
                    );
                    registry.remove(&registry_key);
                    let _ = sessions::save_in(&project_root, &registry);
                }
                eprintln!(
                    "[sync] refusing duplicate pane assignment {} for {} (claims={}, acceptable={})",
                    pane_id,
                    file_path.display(),
                    duplicate_claim_count,
                    acceptable_claim_count
                );
                continue;
            }
        }

        let file_str = registry_relative_file_path(&project_root, &canonical_file);
        let pane_pid = pane_pid_from_tmux(tmux, pane_id).unwrap_or(std::process::id());
        let window = tmux.pane_window(pane_id).unwrap_or_default();
        let cwd = project_root.to_string_lossy().to_string();
        let mut changed = false;

        let stale_keys: Vec<String> = registry
            .iter()
            .filter(|(key, entry)| {
                *key != &registry_key && (entry.session_id == *session_id || entry.pane == pane_id)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in stale_keys {
            registry.remove(&key);
            changed = true;
        }

        if let Some(entry) = registry.get_mut(&registry_key) {
            if entry.session_id != *session_id {
                entry.session_id = session_id.clone();
                changed = true;
            }
            if entry.file != file_str {
                eprintln!(
                    "[sync] updating file path for session {} → {}",
                    &session_id[..8.min(session_id.len())],
                    file_path.display()
                );
                entry.file = file_str.clone();
                changed = true;
            }
            if entry.pane != pane_id {
                eprintln!(
                    "[sync] updating pane for {} → {}",
                    file_path.display(),
                    pane_id
                );
                entry.pane = pane_id.to_string();
                changed = true;
            }
            if entry.pid != pane_pid {
                entry.pid = pane_pid;
                changed = true;
            }
            if entry.cwd != cwd {
                entry.cwd = cwd.clone();
                changed = true;
            }
            if entry.window != window {
                entry.window = window.clone();
                changed = true;
            }
        } else {
            eprintln!(
                "[sync] registering {} → pane {} (session {})",
                file_path.display(),
                pane_id,
                &session_id[..8.min(session_id.len())]
            );
            registry.insert(
                registry_key,
                sessions::SessionEntry {
                    pane: pane_id.to_string(),
                    pid: pane_pid,
                    cwd,
                    started: String::new(),
                    session_id: session_id.clone(),
                    file: file_str,
                    window,
                    supervisor_instance_id: String::new(),
                },
            );
            changed = true;
        }

        if changed {
            let _ = sessions::save_in(&project_root, &registry);
        }
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
pub fn find_alive_pane_for_file(tmux: &Tmux, file_path: &str) -> Option<String> {
    find_alive_pane_for_file_inner(tmux, file_path, None, true)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AssociatedPaneSource {
    Registered,
    SessionLog,
    RegistryRebind,
    ProcessTree,
    SupervisorPid,
}

impl AssociatedPaneSource {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::SessionLog => "session-log",
            Self::RegistryRebind => "registry-rebind",
            Self::ProcessTree => "process-tree",
            Self::SupervisorPid => "supervisor-pid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssociatedPaneCandidate {
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
pub enum AssociatedPaneResolution {
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

pub fn find_associated_panes(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
) -> Vec<AssociatedPaneCandidate> {
    let file_path = file.to_string_lossy().to_string();
    let inventory = list_associated_pane_inventory(tmux);
    let process_tree_matches = collect_process_tree_matches(&inventory, &file_path);
    let registered =
        lookup_registry_entry_for_file_session(file, session_id).map(|entry| entry.pane);
    let supervisor_match = find_alive_pane_via_supervisor_pid(tmux, file, session_id);
    let session_log_match =
        find_alive_pane_via_open_session_log(tmux, file, session_id, None, false);
    let registry_rebind_match =
        find_alive_pane_via_registry_rebind_successor(tmux, file, session_id, None, false);

    let mut associated: Vec<AssociatedPaneCandidate> = inventory
        .into_iter()
        .filter_map(|mut candidate| {
            if registered.as_deref() == Some(candidate.pane_id.as_str()) {
                candidate.sources.insert(AssociatedPaneSource::Registered);
            }
            if session_log_match.as_deref() == Some(candidate.pane_id.as_str()) {
                candidate.sources.insert(AssociatedPaneSource::SessionLog);
            }
            if registry_rebind_match.as_deref() == Some(candidate.pane_id.as_str()) {
                candidate
                    .sources
                    .insert(AssociatedPaneSource::RegistryRebind);
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
                .contains(&AssociatedPaneSource::SessionLog)
                || candidate
                    .sources
                    .contains(&AssociatedPaneSource::RegistryRebind)
                || candidate
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

pub fn resolve_associated_panes(
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

#[cfg(test)]
fn log_stashed_associated_pane(tmux: &Tmux, pane_id: &str, file_path: &Path) {
    eprintln!(
        "[sync] associated pane {} for {} is in stash — deferring rescue to reconciler",
        pane_id,
        file_path.display()
    );
    let win_name = tmux
        .pane_window(pane_id)
        .ok()
        .and_then(|win_id| {
            tmux.cmd()
                .args(["display-message", "-t", &win_id, "-p", "#{window_name}"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_default();
    sync_log(&format!(
        "stash_associated_pane_deferred pane={} file={} stash_window={}",
        pane_id,
        file_path.display(),
        win_name
    ));
}

#[cfg(test)]
enum ExistingAssociatedPaneRecovery {
    Recovered(String),
    Ambiguous,
    None,
}

#[cfg(test)]
fn recover_existing_associated_pane(
    tmux: &Tmux,
    file_path: &Path,
    session_id: &str,
    window: Option<&str>,
    claimed_panes: &RefCell<std::collections::HashMap<String, PathBuf>>,
) -> ExistingAssociatedPaneRecovery {
    let mut candidates = find_associated_panes(tmux, file_path, session_id);
    let mut reserved_conflicts = Vec::new();
    candidates.retain(|candidate| {
        if let Some(owner) = claimed_sync_pane_owner(claimed_panes, &candidate.pane_id, file_path) {
            reserved_conflicts.push((candidate.clone(), owner));
            return false;
        }
        true
    });
    for (candidate, owner) in reserved_conflicts {
        eprintln!(
            "[sync] skipping associated pane {} for {} because it is already reserved for {} in this sync run",
            candidate.pane_id,
            file_path.display(),
            owner.display()
        );
        sync_log(&format!(
            "associated_pane_reserved file={} pane={} owner={} sources={}",
            file_path.display(),
            candidate.pane_id,
            owner.display(),
            candidate.source_summary()
        ));
    }
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
            let winner_pane = winner.pane_id.clone();
            if let Err(e) = reregister_recovered_owner(tmux, file_path, session_id, &winner.pane_id)
            {
                eprintln!(
                    "[sync] warning: re-register failed for {} via associated pane {}: {}",
                    file_path.display(),
                    winner.pane_id,
                    e
                );
            }
            if winner.is_stash() {
                log_stashed_associated_pane(tmux, &winner.pane_id, file_path);
            }
            ExistingAssociatedPaneRecovery::Recovered(winner_pane)
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

pub fn find_live_owner_pane(tmux: &Tmux, file: &Path, session_id: &str) -> Option<String> {
    find_live_owner_pane_excluding(tmux, file, session_id, None)
}

/// Quiet variant of [`find_live_owner_pane`] that suppresses the per-hit
/// stderr diagnostics. Used by `focus` on every editor navigation, where the
/// happy path re-resolves the same owner and logging each hit would be noise.
pub fn find_live_owner_pane_quiet(tmux: &Tmux, file: &Path, session_id: &str) -> Option<String> {
    find_live_owner_pane_excluding_with_logging(tmux, file, session_id, None, false)
}

pub fn find_normal_path_owner_pane(tmux: &Tmux, file: &Path, session_id: &str) -> Option<String> {
    find_normal_path_owner_pane_excluding(tmux, file, session_id, None)
}

pub fn find_normal_path_owner_pane_excluding(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
) -> Option<String> {
    find_normal_path_owner_pane_excluding_with_logging(tmux, file, session_id, excluded_pane, true)
}

pub fn find_normal_path_owner_pane_excluding_quiet(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
) -> Option<String> {
    find_normal_path_owner_pane_excluding_with_logging(tmux, file, session_id, excluded_pane, false)
}

fn find_normal_path_owner_pane_excluding_with_logging(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
    log_hits: bool,
) -> Option<String> {
    let candidate = authoritative_actor_pane_for_document(tmux, file, session_id)
        .filter(|pane| excluded_pane != Some(pane.as_str()))
        .or_else(|| {
            find_registered_pane_via_path_provenance(
                tmux,
                file,
                session_id,
                excluded_pane,
                log_hits,
            )
        });
    // Cross-document guard (#jb-tsift-pane-sync): navigation / sync / autostart
    // owner resolution must never surface or reuse a pane that is actually
    // running a *different* document's agent-doc/codex session. Stale registry
    // provenance or geometry-only reconciliation can otherwise return the
    // currently-visible pane (e.g. one owning `agent-doc-bugs2.md`) as the owner
    // for the navigated file (e.g. `tsift.md`). Reject the wrong-document pane so
    // the caller cold-starts a correct owner instead of aliasing two documents
    // onto one pane.
    reject_cross_document_owner_pane(tmux, candidate, file, log_hits)
}

/// Reject an owner-pane candidate that is running a *different* document's
/// agent-doc/codex session so the normal navigation/sync/autostart owner path
/// never binds the navigated file to a wrong-document pane
/// (#jb-tsift-pane-sync cross-document variant). Returns `None` (forcing a
/// correct cold-start / proper-owner path) when the candidate owns another
/// document; otherwise returns the candidate unchanged. A candidate that owns
/// `file` itself, or a bare non-owner pane, is preserved (see
/// `cmdline_owns_other_document`).
fn reject_cross_document_owner_pane(
    tmux: &Tmux,
    candidate: Option<String>,
    file: &Path,
    log_hits: bool,
) -> Option<String> {
    let pane = candidate?;
    if pane_runs_other_document_owner(tmux, &pane, file) {
        if log_hits {
            crate::ops_log::log_op(
                file,
                &format!(
                    "[sync] owner candidate pane {} runs another document; not surfacing for {} (cross-document guard #jb-tsift-pane-sync)",
                    pane,
                    file.display()
                ),
            );
        }
        return None;
    }
    Some(pane)
}

pub fn find_live_owner_pane_excluding(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
) -> Option<String> {
    find_live_owner_pane_excluding_with_logging(tmux, file, session_id, excluded_pane, true)
}

fn find_live_owner_pane_excluding_with_logging(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
    log_hits: bool,
) -> Option<String> {
    // Full heuristic recovery remains available for explicit repair/resync paths.
    // Normal route/start/sync ownership decisions must use
    // `find_normal_path_owner_pane*` instead.
    let candidate =
        find_registered_pane_via_path_provenance(tmux, file, session_id, excluded_pane, log_hits)
            .or_else(|| {
                find_alive_pane_via_supervisor_pid(tmux, file, session_id)
                    .filter(|pane| excluded_pane != Some(pane.as_str()))
            })
            .or_else(|| {
                find_alive_pane_via_open_session_log(
                    tmux,
                    file,
                    session_id,
                    excluded_pane,
                    log_hits,
                )
            })
            .or_else(|| {
                find_alive_pane_via_registry_rebind_successor(
                    tmux,
                    file,
                    session_id,
                    excluded_pane,
                    log_hits,
                )
            })
            .or_else(|| {
                let file_path = file.to_string_lossy();
                find_alive_pane_for_file_inner(tmux, file_path.as_ref(), excluded_pane, log_hits)
            });
    // Cross-document guard (#jb-tsift-pane-sync): the focus path
    // (`focus.rs` -> `find_live_owner_pane_quiet`) and resync recovery resolve
    // owners through this heuristic resolver, not `find_normal_path_owner_pane*`.
    // Stale registry provenance or a process-tree match can otherwise surface
    // the currently-visible pane (e.g. one owning `agent-doc-bugs2.md`) as the
    // owner for the navigated file (e.g. `tsift.md`), aliasing two documents
    // onto one pane. Reject the wrong-document candidate so the caller
    // cold-starts / fails closed instead of focusing a contaminated pane.
    reject_cross_document_owner_pane(tmux, candidate, file, log_hits)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SupervisorIdentity {
    pid: u32,
    instance_id: String,
}

fn query_supervisor_identity(file: &Path, session_id: &str) -> Option<SupervisorIdentity> {
    let project_root = crate::snapshot::find_project_root(file)?;
    let sock = crate::supervisor::ipc::socket_path(&project_root, session_id);
    if !sock.exists() {
        return None;
    }
    let response =
        crate::supervisor::ipc::send_command(&sock, &crate::supervisor::ipc::IpcMethod::State)
            .ok()?;
    if !response.ok {
        return None;
    }
    let data = response.data?;
    let pid = data
        .get("supervisor_pid")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())?;
    let instance_id = data
        .get("supervisor_instance_id")
        .and_then(|value| value.as_str())?
        .to_string();
    if pid == 0 || instance_id.is_empty() {
        return None;
    }
    Some(SupervisorIdentity { pid, instance_id })
}

fn pane_pid_from_tmux(tmux: &Tmux, pane_id: &str) -> Option<u32> {
    let output = tmux
        .cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{pane_pid}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .ok()
}

fn pane_project_root(tmux: &Tmux, pane_id: &str) -> Option<PathBuf> {
    let output = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-p",
            "#{pane_current_path}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let current_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if current_path.is_empty() {
        return None;
    }
    let path = PathBuf::from(current_path);
    crate::snapshot::find_project_root(&path).or(Some(path))
}

fn registry_entry_matches_document_root(
    entry: &sessions::SessionEntry,
    project_root: &Path,
) -> bool {
    let cwd = Path::new(entry.cwd.trim());
    if cwd.as_os_str().is_empty() {
        return false;
    }
    crate::snapshot::find_project_root(cwd)
        .or_else(|| cwd.is_dir().then_some(cwd.to_path_buf()))
        .is_some_and(|root| root == project_root)
}

fn pane_assignment_matches_document_root(tmux: &Tmux, pane_id: &str, project_root: &Path) -> bool {
    pane_project_root(tmux, pane_id)
        .map(|pane_root| pane_root == project_root)
        .unwrap_or(false)
}

fn pane_contains_supervisor_pid(tmux: &Tmux, pane_id: &str, target_pid: u32) -> bool {
    let Some(pane_pid) = pane_pid_from_tmux(tmux, pane_id) else {
        return false;
    };
    pane_process_tree_contains_pid(&pane_pid.to_string(), target_pid)
}

pub fn reregister_recovered_owner(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
) -> anyhow::Result<()> {
    let Some((canonical_file, project_root, _registry_key)) = registry_location_for_file(file)
    else {
        return sessions::register(session_id, pane_id, &file.to_string_lossy());
    };
    let file_str = registry_relative_file_path(&project_root, &canonical_file);

    if let Some(entry) = lookup_registry_entry_for_file_session(file, session_id)
        && entry.pane == pane_id
        && entry.pid != 0
        && !entry.supervisor_instance_id.is_empty()
        && pane_contains_supervisor_pid(tmux, pane_id, entry.pid)
    {
        return sessions::register_supervisor_in(
            &project_root,
            session_id,
            pane_id,
            &file_str,
            entry.pid,
            &entry.supervisor_instance_id,
        );
    }

    if let Some(identity) = query_supervisor_identity(file, session_id)
        && pane_contains_supervisor_pid(tmux, pane_id, identity.pid)
    {
        return sessions::register_supervisor_in(
            &project_root,
            session_id,
            pane_id,
            &file_str,
            identity.pid,
            &identity.instance_id,
        );
    }

    let pid = pane_pid_from_tmux(tmux, pane_id).unwrap_or(std::process::id());
    let window = tmux.pane_window(pane_id).unwrap_or_default();
    sessions::register_full_with_cwd_in(
        &project_root,
        session_id,
        pane_id,
        &file_str,
        pid,
        &window,
        &project_root.to_string_lossy(),
    )
}

fn find_registered_pane_via_path_provenance(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
    log_hits: bool,
) -> Option<String> {
    let (_, project_root, registry_key) = registry_location_for_file(file)?;
    let registry = sessions::load_in(&project_root).ok()?;
    let entry = registry.get(&registry_key)?;
    if entry.session_id != session_id
        || entry.pid == 0
        || entry.supervisor_instance_id.is_empty()
        || excluded_pane == Some(entry.pane.as_str())
        || !tmux.pane_alive(&entry.pane)
    {
        return None;
    }

    let identity = query_supervisor_identity(file, session_id)?;
    if identity.pid != entry.pid || identity.instance_id != entry.supervisor_instance_id {
        return None;
    }

    let pane_pid = tmux
        .cmd()
        .args(["display-message", "-t", &entry.pane, "-p", "#{pane_pid}"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())?;
    if pane_pid.is_empty() || !pane_process_tree_contains_pid(&pane_pid, identity.pid) {
        return None;
    }

    if log_hits {
        eprintln!(
            "[sync] recovered live pane {} for session {} via path provenance pid={} instance={}",
            entry.pane,
            &session_id[..std::cmp::min(8, session_id.len())],
            identity.pid,
            identity.instance_id
        );
    }
    Some(entry.pane.clone())
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

fn find_alive_pane_via_open_session_log(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
    log_hits: bool,
) -> Option<String> {
    let status = crate::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten()?;
    if !status.latest_session_open() {
        return None;
    }
    let pane_id = status.latest_start_pane.as_deref()?;
    if excluded_pane == Some(pane_id) || !tmux.pane_alive(pane_id) {
        return None;
    }

    let project_root = crate::snapshot::find_project_root(file)?;
    if !pane_assignment_matches_document_root(tmux, pane_id, &project_root) {
        return None;
    }

    if log_hits {
        eprintln!(
            "[sync] recovered live pane {} for session {} via latest open session-log owner",
            pane_id,
            &session_id[..std::cmp::min(8, session_id.len())],
        );
    }

    Some(pane_id.to_string())
}

fn find_alive_pane_via_registry_rebind_successor(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
    log_hits: bool,
) -> Option<String> {
    let status = crate::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten()?;
    if !status.latest_session_closed() {
        return None;
    }
    let pane_id = crate::startup_miss::latest_registry_rebind_successor(&status)?;
    if excluded_pane == Some(pane_id) || !tmux.pane_alive(pane_id) {
        return None;
    }

    let project_root = crate::snapshot::find_project_root(file)?;
    if !pane_assignment_matches_document_root(tmux, pane_id, &project_root) {
        return None;
    }

    if log_hits {
        eprintln!(
            "[sync] recovered live pane {} for session {} via registry-rebind successor",
            pane_id,
            &session_id[..std::cmp::min(8, session_id.len())],
        );
    }

    Some(pane_id.to_string())
}

/// Check if a process (by PID) is running agent-doc for a specific file.
///
/// Uses `ps -p <pid> -o command=` which works on both Linux and macOS.
/// Check if a tmux pane is running an active agent session.
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

/// Check if a process (by PID) is running an agent session.
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
    cmdline.contains("agent-doc")
        || cmdline.contains("claude")
        || cmdline.contains("codex")
        || cmdline.contains("opencode")
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

fn should_skip_autostart_for_unresolved_startup_miss(
    registered_pane: Option<&str>,
    pane_alive: bool,
    miss: Option<&crate::startup_miss::StartupMiss>,
) -> bool {
    pane_alive && registered_pane.is_some_and(|pane| miss.is_some_and(|miss| miss.pane_id == pane))
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

fn token_basename(token: &str) -> &str {
    Path::new(token)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(token)
}

fn token_is_agent_doc_binary(token: &str) -> bool {
    token_basename(token).starts_with("agent-doc")
}

fn token_is_harness_binary(token: &str) -> bool {
    matches!(
        token_basename(token),
        "claude" | "codex" | "opencode" | "bun" | "node"
    )
}

fn token_is_non_owner_agent_doc_subcommand(token: &str) -> bool {
    matches!(token, "route" | "claim")
}

fn agent_doc_cmdline_is_owner(cmdline: &str, file_path: &str) -> bool {
    cmdline_has_file_match(cmdline, file_path) && cmdline_is_agent_doc_owner_session(cmdline)
}

/// File-agnostic half of [`agent_doc_cmdline_is_owner`]: true when `cmdline` is a
/// long-lived agent-doc/codex owner invocation (a supervisor `start`, an owner
/// subcommand, or a harness binary) for *some* document, regardless of which.
fn cmdline_is_agent_doc_owner_session(cmdline: &str) -> bool {
    let tokens = cmdline.split_whitespace().collect::<Vec<_>>();
    if let Some(idx) = tokens
        .iter()
        .position(|token| token_is_agent_doc_binary(token))
    {
        let Some(next) = tokens.get(idx + 1) else {
            return false;
        };
        if *next == "start" {
            return true;
        }
        return !token_is_non_owner_agent_doc_subcommand(next);
    }

    tokens.iter().any(|token| token_is_harness_binary(token))
}

/// True when `cmdline` references at least one `.md` document path token.
fn cmdline_references_md_document(cmdline: &str) -> bool {
    cmdline.split_whitespace().any(|token| {
        token
            .trim_matches(|c| c == '"' || c == '\'')
            .ends_with(".md")
    })
}

/// True when `cmdline` is a live agent-doc/codex owner session for a document
/// OTHER than `claimed_file`. Cross-root safe: it is keyed on the live process
/// command line, so it recognizes a pane owned by a document rooted in another
/// project/submodule whose session registry the calling root cannot see. Used to
/// keep `claim`/`route` from commandeering such a pane.
pub(crate) fn cmdline_owns_other_document(cmdline: &str, claimed_file: &str) -> bool {
    cmdline_is_agent_doc_owner_session(cmdline)
        && cmdline_references_md_document(cmdline)
        && !agent_doc_cmdline_is_owner(cmdline, claimed_file)
}

/// First `.md` document path token in `cmdline` (the document an
/// agent-doc/codex owner session is bound to), for cross-document diagnostics.
fn owner_document_from_cmdline(cmdline: &str) -> Option<String> {
    cmdline
        .split_whitespace()
        .map(|token| token.trim_matches(|c| c == '"' || c == '\''))
        .find(|token| token.ends_with(".md"))
        .map(|token| token.to_string())
}

/// Diagnostic sibling of [`pane_runs_other_document_owner`]: returns the foreign
/// document path the pane's live process tree owns (a document other than
/// `claimed_file`), or `None`. Used only for cross-document execution logging.
fn pane_owned_document_other_than(
    tmux: &Tmux,
    pane_id: &str,
    claimed_file: &Path,
) -> Option<String> {
    let pane_pid = pane_pid_from_tmux(tmux, pane_id)?;
    let claimed = claimed_file.to_string_lossy();
    let mut pids = vec![pane_pid.to_string()];
    if let Ok(children) = std::process::Command::new("pgrep")
        .args(["-P", &pane_pid.to_string()])
        .output()
    {
        for child in String::from_utf8_lossy(&children.stdout).lines() {
            let child = child.trim();
            if !child.is_empty() {
                pids.push(child.to_string());
            }
        }
    }
    for pid in pids {
        let cmdline = match std::process::Command::new("ps")
            .args(["-p", &pid, "-o", "command="])
            .output()
        {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).to_string()
            }
            _ => continue,
        };
        if cmdline_owns_other_document(&cmdline, &claimed) {
            return owner_document_from_cmdline(&cmdline);
        }
    }
    None
}

/// `#jb-tsift-pane-sync` cross-document execution diagnostic. Logs (best-effort,
/// never blocks) when an agent-doc cycle for `file` is executing inside a tmux
/// pane whose live process tree owns a *different* document — the contamination
/// vector where, e.g., a `tsift.md`-owned pane runs `agent-doc-bugs2.md`'s cycle
/// (a cross-document child `agent-doc <FILE>` invocation). The same-document
/// recursion guard (`owned_pane_self_invocation_detail`) does not catch this
/// because the invoked document differs from the pane's own, and route/dispatch
/// logs only record the pane's own document — so without this line the exact
/// vector is invisible in the logs. `origin` names the entry point (e.g. `run`,
/// `preflight`) so a future repro pins where the cross-document cycle started.
pub fn log_cross_document_execution_context(file: &Path, origin: &str) {
    let current_pane = match crate::sessions::current_pane() {
        Ok(pane) if !pane.is_empty() => pane,
        _ => return,
    };
    let tmux = Tmux::default_server();
    if let Some(other) = pane_owned_document_other_than(&tmux, &current_pane, file) {
        crate::ops_log::log_op(
            file,
            &format!(
                "cross_document_execution_context file={} origin={} current_pane={} pane_owns={} note=agent-doc cycle running inside a pane that owns a different document (#jb-tsift-pane-sync contamination vector)",
                file.display(),
                origin,
                current_pane,
                other
            ),
        );
    }
}

/// Walk the pane's process tree (pane pid + direct children) and return true if
/// any live process is an agent-doc/codex owner session for a document other than
/// `claimed_file`. Keyed on the live cmdline rather than any single root's
/// `sessions.json`, so it enforces the one-live-pane-per-document binding
/// invariant across project/submodule roots.
pub(crate) fn pane_runs_other_document_owner(
    tmux: &Tmux,
    pane_id: &str,
    claimed_file: &Path,
) -> bool {
    let Some(pane_pid) = pane_pid_from_tmux(tmux, pane_id) else {
        return false;
    };
    let claimed = claimed_file.to_string_lossy();
    let mut pids = vec![pane_pid.to_string()];
    if let Ok(children) = std::process::Command::new("pgrep")
        .args(["-P", &pane_pid.to_string()])
        .output()
    {
        for child in String::from_utf8_lossy(&children.stdout).lines() {
            let child = child.trim();
            if !child.is_empty() {
                pids.push(child.to_string());
            }
        }
    }
    for pid in pids {
        let cmdline = match std::process::Command::new("ps")
            .args(["-p", &pid, "-o", "command="])
            .output()
        {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).to_string()
            }
            _ => continue,
        };
        if cmdline_owns_other_document(&cmdline, &claimed) {
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
    agent_doc_cmdline_is_owner(&cmdline, file_path)
}

/// Detect whether a file has been renamed: the registered path differs from
/// the current path and the old path no longer exists on disk.
pub fn is_file_rename(registered_path: &str, current_path: &str) -> bool {
    registered_path != current_path && !Path::new(registered_path).exists()
}

#[cfg(test)]
mod tests;
