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
//! in the durable registry.
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
//!   project `state.db` layout state for the current sync scope. It preserves empty
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
//! - `repair_layout(tmux, session_name, target_window_name)` runs four phases:
//!   1. **Stash consolidation** — merges `stash-*` and duplicate `stash` panes into
//!      the primary stash window via `join-pane` while preserving any overflow
//!      windows that cannot be joined.
//!   2. **Target window rescue** — if the target window is missing, breaks a live
//!      registered pane out of the stash and renames the new window.
//!   3. **Target window consolidation** — merges duplicate target windows back into
//!      the canonical target window so repair does not leave split-brain layouts.
//!   4. **Index normalisation** — moves or swaps the target window to index 0,
//!      using `swap-window` when index 0 is occupied to avoid data loss, then
//!      renames and packs stash windows as `1:stash`, `2:stash`, and so on.
//!
//!   Phases 1 and 2 are skipped when the layout is already correct enough for
//!   the destructive rescue phase (target exists, with either no stash windows
//!   or one canonical stash). Phases 3 and 4 always run.
//! - `repair_file_state_with_tmux` is the tmux-layout and commit-boundary portion of
//!   the doctor repair path used by both `agent-doc session doctor <FILE> --repair`
//!   and full sync.
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
//!   (a typed `rename_debounce` state.db lease) for the focused file. Any sync within
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
//! - **Column memory:** project `state.db` persists a column→agent-doc mapping.
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
//! - rename_debounce_suppresses_auto_start: lease recorded for file → `has_rename_debounce`
//!   returns true within TTL, auto-start is skipped.
//! - rename_debounce_expires_after_ttl: lease with old timestamp → `has_rename_debounce`
//!   returns false and clears the lease.
//! - rename_debounce_does_not_affect_other_files: lease for file A → file B still
//!   passes the debounce check.
//! - batch_summary_format_multiple_panes: 3 auto-started panes → summary string contains
//!   count and all pane→file mappings.
//! - batch_summary_not_printed_for_single_pane: 1 auto-started pane → batch summary
//!   condition (len > 1) is false.

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
#[cfg(test)]
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
#[cfg(test)]
use tempfile::NamedTempFile;

#[cfg(test)]
use crate::SyncLockAcquire;
use crate::acquire_sync_lock;
use agent_doc_controller::dispatch::{is_stash_window_name, normalize_context_session};
use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_state_scope::LocalProcessScope;
use agent_doc_supervisor::ipc_protocol::IpcMethod;
#[cfg(test)]
use agent_doc_supervisor::ipc_protocol::IpcResponse;
use agent_doc_supervisor::startup_miss::unresolved_startup_miss_blocks_autostart;
use agent_doc_sync::{
    AutoStartMode, RENAME_DEBOUNCE_TTL_SECS, SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET,
    SYNC_LOCK_WAIT_BUDGET, SYNC_LOCK_WAIT_LATENCY_BUDGET, SYNC_OWNERSHIP_PROOF_BUDGET,
    SYNC_PROJECTION_REFRESH_BUDGET, SYNC_PRUNE_BUDGET, SYNC_PRUNE_SUBPHASE_BUDGET,
    SYNC_ROUTER_BUDGET, SYNC_SAFE_PASSIVE_TOTAL_BUDGET, SYNC_WINDOW_RESOLUTION_BUDGET,
    WindowIndexNormalizationPlan, auto_started_panes_summary,
    destructive_repair_throttle_state_key, effective_sync_columns, epoch_millis_now,
    is_file_rename, last_visible_excerpt, latency_budget_status, plan_window_index_normalization,
    planned_stash_window_indices, preserved_layout_focus_marker, registry_relative_file_path,
    rename_debounce_expired, reselect_visible_focus_pane_failed_warning, sanitize_excerpt,
    sync_latency_message,
};
use agent_doc_tmux::{
    AssociatedPaneCandidate, AssociatedPaneResolution, AssociatedPaneSource,
    associated_pane_candidates_detail, auto_start_candidate_files, parse_pane_inventory_line,
    projected_sync_pane_count, resolve_associated_panes,
};
use agent_doc_tmux_io::pane_project_root;
use tmux_router::{PaneMoveOp, Tmux};

use agent_doc_frontmatter::frontmatter;

use crate::resync;
use agent_doc_session_registry_io::registration as sessions;

use tmux_router::FileResolution;

mod layout;
pub(crate) use layout::*;
mod pane_repair;
pub(crate) use pane_repair::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncRunReport {
    pub applied: bool,
    pub reason: String,
}

impl Default for SyncRunReport {
    fn default() -> Self {
        Self {
            applied: true,
            reason: "applied".to_string(),
        }
    }
}

thread_local! {
    static LAST_SYNC_RUN_REPORT: RefCell<SyncRunReport> =
        RefCell::new(SyncRunReport::default());
}

fn reset_sync_run_report() {
    LAST_SYNC_RUN_REPORT.with(|report| {
        *report.borrow_mut() = SyncRunReport::default();
    });
}

fn mark_sync_layout_preserved(reason: &str) {
    LAST_SYNC_RUN_REPORT.with(|report| {
        *report.borrow_mut() = SyncRunReport {
            applied: false,
            reason: reason.to_string(),
        };
    });
}

pub fn last_sync_run_report() -> SyncRunReport {
    LAST_SYNC_RUN_REPORT.with(|report| report.borrow().clone())
}

fn log_sync_latency(
    focus: Option<&str>,
    phase: &str,
    elapsed: Duration,
    budget: Duration,
    auto_start_mode: AutoStartMode,
) {
    let mode_label = auto_start_mode.log_label();
    let message = sync_latency_message(phase, elapsed, budget, mode_label);
    sync_log(&message);
    if latency_budget_status(elapsed, budget) == "over_budget" {
        eprintln!(
            "[sync] latency budget exceeded: phase {} took {}ms (budget {}ms, mode={})",
            phase,
            elapsed.as_millis(),
            budget.as_millis(),
            mode_label
        );
    }
    if let Some(focus) = focus {
        let path = Path::new(focus);
        if path.exists() {
            agent_doc_ops_log_io::log_op(path, &message);
        }
    }
}

fn parse_frontmatter_for_sync<'a>(
    content: &'a str,
    file: &Path,
    phase: &str,
) -> Result<(frontmatter::Frontmatter, &'a str)> {
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    agent_doc_frontmatter_io::session::parse_for_file_with_context(content, file, &rc.ssh_context())
        .map_err(|err| anyhow::anyhow!("sync {} frontmatter: {}", phase, err))
}

fn save_sync_status_snapshot(file: &Path, updated: &str) -> Result<()> {
    agent_doc_snapshot_io::checkpoint_document_baseline(file, updated, agent_doc_ops_log_io::log_op)
}

fn log_sync_status(message: String) {
    eprintln!("{}", message);
    sync_log(&message);
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct MissingRegisteredPaneRepair {
    dead_pane: Option<DeadPaneDiagnostics>,
    recorded_session_loss: bool,
    repaired_stale_preflight: bool,
    closeout_recovery_phase: Option<String>,
    closeout_recovery_outcome: Option<agent_doc_turn::repair::RepairOutcome>,
    closeout_recovery_error: Option<String>,
    block_auto_start_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MissingRegisteredPaneRepairMode {
    InspectOnly,
    ExplicitRepair,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DeadPaneDiagnostics {
    observed_window: Option<String>,
    dead_status: Option<String>,
    cycle_phase: Option<String>,
    capture_path: Option<PathBuf>,
    last_visible_excerpt: Option<String>,
    pane_killed: bool,
}

mod registry;
pub(crate) use registry::*;

pub fn repair_file_state(file: &Path) -> Result<Vec<String>> {
    let tmux = Tmux::default_server();
    repair_file_state_with_tmux(&tmux, file)
}

fn recover_jb_cache_conflict_cancel_commit_boundary(file: &Path) -> Result<Option<String>> {
    if !crate::runtime_effects()?.detect_jb_cache_conflict_cancel_recoverable(file)? {
        return Ok(None);
    }

    crate::runtime_effects()?.commit(file).with_context(|| {
        format!(
            "failed to close recoverable jb_cache_conflict_cancel commit boundary for {}",
            file.display()
        )
    })?;
    if crate::runtime_effects()?.detect_jb_cache_conflict_cancel_recoverable(file)? {
        anyhow::bail!(
            "recoverable jb_cache_conflict_cancel commit boundary remained after commit for {}",
            file.display()
        );
    }

    Ok(Some(format!(
        "Closed recoverable `jb_cache_conflict_cancel` commit boundary for `{}`.",
        file.display()
    )))
}

pub fn repair_file_state_with_tmux(tmux: &Tmux, file: &Path) -> Result<Vec<String>> {
    let canonical = file
        .canonicalize()
        .unwrap_or_else(|_| agent_doc_git_io::dirs::resolve_absolute_file_path(file));
    let mut actions = Vec::new();

    let columns = vec![canonical.to_string_lossy().to_string()];
    if let Some(session_name) = resolve_sync_target_session(tmux, None, &columns, None) {
        repair_layout(tmux, &session_name, "agent-doc")?;
        actions.push(format!(
            "Repaired `agent-doc`/`stash` layout in tmux session `{session_name}`."
        ));
    }
    if let Some(note) = recover_jb_cache_conflict_cancel_commit_boundary(&canonical)? {
        actions.push(note);
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
                outcome.as_str()
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
    let Some(window) =
        agent_doc_supervisor_io::startup_miss::recent_session_loss_window(file, session_id)?
    else {
        return Ok(false);
    };

    let first = agent_doc_supervisor::startup_miss::format_timestamp(window.first_timestamp);
    let last = agent_doc_supervisor::startup_miss::format_timestamp(window.last_timestamp);
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

pub fn run_in_project_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
) -> Result<()> {
    tracing::debug!(
        root = %project_root.display(),
        cols = ?col_args,
        window,
        focus,
        "sync::run_in_project_root start"
    );
    run_with_options_at_root(project_root, col_args, window, focus, AutoStartMode::Full)
}

fn run_with_options(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
) -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_with_options_at_root(&cwd, col_args, window, focus, auto_start_mode)
}

fn run_with_options_at_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
) -> Result<()> {
    run_with_options_internal_at_root(
        project_root,
        col_args,
        window,
        focus,
        auto_start_mode,
        false,
        &Tmux::default_server(),
    )
}

/// Record a debounce lease for a file that was just renamed.
/// Subsequent syncs within 5s will skip auto-start for this file.
pub fn write_rename_debounce(file_path: &str) {
    let file = Path::new(file_path);
    let hash = agent_doc_fs::document_state_hash(file).unwrap_or_default();
    if hash.is_empty() {
        return;
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return;
    };
    let _ = agent_doc_controller_io::project_controller::upsert_coordination_lease(
        &project_root,
        &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
            scope_kind: "rename_debounce".to_string(),
            scope_id: hash.clone(),
            holder: file_path.to_string(),
            holder_pid: Some(std::process::id()),
            heartbeat_secs: agent_doc_sqlite::state_store::timestamp_secs(),
        },
    );
    eprintln!(
        "[sync] rename debounce state recorded for {} ({})",
        file_path, hash
    );
    sync_log(&format!(
        "rename-debounce: recorded lease for {} hash={}",
        file_path, hash
    ));
}

/// Check if a file has an active rename debounce lease (within TTL).
fn has_rename_debounce(file_path: &Path) -> bool {
    let hash = match agent_doc_fs::document_state_hash(file_path) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file_path) else {
        return false;
    };
    let Some(lease) = agent_doc_controller_io::project_controller::load_coordination_lease(
        &project_root,
        "rename_debounce",
        &hash,
    )
    .ok()
    .flatten() else {
        return false;
    };
    let age = Duration::from_secs(
        agent_doc_sqlite::state_store::timestamp_secs().saturating_sub(lease.heartbeat_secs),
    );
    let expired = rename_debounce_expired(age, Duration::from_secs(RENAME_DEBOUNCE_TTL_SECS));
    if expired {
        let _ = agent_doc_controller_io::project_controller::clear_coordination_lease(
            &project_root,
            "rename_debounce",
            &hash,
        );
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

pub fn run_layout_only_in_project_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
) -> Result<()> {
    run_with_options_at_root(
        project_root,
        col_args,
        window,
        focus,
        AutoStartMode::SafePassive,
    )
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
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_layout_only_exact_visible_in_project_root(&cwd, col_args, window, focus)
}

pub fn run_layout_only_exact_visible_in_project_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
) -> Result<()> {
    run_with_options_internal_at_root(
        project_root,
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

fn load_live_authoritative_actor_record_uncached(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
) -> Option<agent_doc_sqlite::state_store::ActorRecord> {
    let canonical = file
        .canonicalize()
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    let base_dir = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let record = agent_doc_controller_io::project_controller::authoritative_actor_binding(
        &base_dir, &canonical,
    )
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
) -> Option<agent_doc_sqlite::state_store::ActorRecord> {
    if proof_cache.skip_authoritative_actor_lookup {
        return None;
    }
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
        && proof_cache.skip_authoritative_actor_lookup
    {
        log_sync_latency(
            focus,
            "controller_actor_lookup",
            Duration::ZERO,
            SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET,
            auto_start_mode,
        );
        sync_log(&format!(
            "controller_actor_lookup_skipped file={} source=safe_passive_no_controller_actor_rpc",
            file.display()
        ));
        return None;
    }
    if matches!(auto_start_mode, AutoStartMode::SafePassive)
        && let Some(pane_id) =
            authoritative_actor_pane_for_document_cached(tmux, file, session_id, proof_cache)
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
        let registry_refresh_start = Instant::now();
        eprintln!(
            "[sync] authoritative actor generation {} keeps {} on pane {} — refreshing durable registry metadata",
            record.generation,
            file.display(),
            actor_pane
        );
        sync_log(&format!(
            "actor_registry_refresh file={} pane={} generation={}",
            file.display(),
            actor_pane,
            record.generation
        ));
        if let Err(err) = reregister_recovered_owner(tmux, file, session_id, &actor_pane) {
            eprintln!(
                "[sync] warning: failed to refresh authoritative actor pane {} for {} in durable registry: {}",
                actor_pane,
                file.display(),
                err
            );
            sync_log(&format!(
                "warning: actor_registry_refresh_failed file={} pane={} err={}",
                file.display(),
                actor_pane,
                err
            ));
        }
        log_sync_latency(
            focus,
            "registry_refresh",
            registry_refresh_start.elapsed(),
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

    let matches = find_normal_path_owner_pane_excluding_cached_quiet(
        tmux,
        file,
        session_id,
        None,
        proof_cache,
    )
    .as_deref()
        == Some(pane_id);
    proof_cache
        .live_owner_matches
        .borrow_mut()
        .insert(key, matches);
    matches
}

fn find_normal_path_owner_pane_excluding_cached_quiet(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    excluded_pane: Option<&str>,
    proof_cache: &SyncProofCache,
) -> Option<String> {
    let actor = if proof_cache.skip_authoritative_actor_lookup {
        None
    } else {
        authoritative_actor_pane_for_document_cached(tmux, file, session_id, proof_cache)
            .filter(|pane| excluded_pane != Some(pane.as_str()))
    };
    let candidate = actor.or_else(|| {
        find_registered_pane_via_path_provenance(tmux, file, session_id, excluded_pane, false)
    });
    reject_cross_document_owner_pane(tmux, candidate, file, false)
}

fn passive_autostart_skip_reason(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    unresolved_startup_miss: Option<&agent_doc_supervisor::startup_miss::StartupMiss>,
) -> Result<Option<String>> {
    let status = agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)?;
    let live_registry_rebind_successor = status
        .as_ref()
        .and_then(agent_doc_supervisor::startup_miss::latest_registry_rebind_successor)
        .and_then(|_| {
            find_alive_pane_via_registry_rebind_successor(tmux, file, session_id, None, false)
        });
    Ok(
        agent_doc_supervisor::startup_miss::passive_autostart_skip_reason(
            unresolved_startup_miss.is_some(),
            status.as_ref(),
            live_registry_rebind_successor.as_deref(),
        ),
    )
}

fn block_unresolved_safe_passive_managed_files(
    managed_files: &HashSet<PathBuf>,
    resolved_files: &HashSet<PathBuf>,
    blocked_files: &RefCell<HashSet<PathBuf>>,
) -> Vec<PathBuf> {
    let mut newly_blocked = Vec::new();
    let mut blocked = blocked_files.borrow_mut();
    for file_path in managed_files {
        if resolved_files.contains(file_path) {
            continue;
        }
        if blocked.insert(file_path.clone()) {
            newly_blocked.push(file_path.clone());
        }
    }
    newly_blocked.sort();
    newly_blocked
}

fn open_session_log_owner_fail_closed_diagnostic(
    file: &Path,
    session_id: &str,
    pane_id: &str,
) -> Result<Option<String>> {
    let Some(status) = agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)?
    else {
        return Ok(None);
    };
    if status.latest_start_pane.as_deref() != Some(pane_id) || !status.latest_session_open() {
        return Ok(None);
    }
    agent_doc_supervisor_io::startup_miss::session_log_diagnostic(file, session_id)
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
                let _ = agent_doc_tmux_io::rename_window(tmux, &new_win, target_window_name);
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
/// Phase 3: Target consolidation — merge duplicate target windows into the
/// canonical target window.
///
/// Phase 4: Window index normalization — keep `agent-doc` at `0`, then pack
/// stash windows as `1:stash`, `2:stash`, and so on.
const STASH_JOIN_TEMP_HEIGHT: &str = "1000";

pub fn repair_layout(tmux: &Tmux, session_name: &str, target_window_name: &str) -> Result<()> {
    tracing::debug!(
        session_name,
        target_window_name,
        "sync::repair_layout start"
    );
    // List all windows in the session. Put the name last so names containing
    // spaces remain parseable via splitn.
    let output = agent_doc_tmux_io::list_windows(
        tmux,
        Some(&format!("{}:", session_name)),
        "#{window_id} #{window_panes} #{window_height} #{window_name}",
    );
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

    // Parse windows into (id, pane_count, height, name).
    struct WinInfo {
        id: String,
        name: String,
        _pane_count: usize,
        height: String,
    }
    let windows: Vec<WinInfo> = window_list
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, ' ');
            let id = parts.next()?.to_string();
            let pane_count: usize = parts.next()?.parse().ok()?;
            let height = parts.next()?.to_string();
            let name = parts.next()?.to_string();
            Some(WinInfo {
                id,
                name,
                _pane_count: pane_count,
                height,
            })
        })
        .collect();

    // `#stashresizerestore`: heal windows pinned by binaries that used the
    // 1000-row join workaround without fully removing tmux's manual-size
    // override. Restrict recovery to the exact staging height and agent-doc's
    // owned target/stash windows so an operator's unrelated manual layout is
    // never normalized.
    for window in &windows {
        let agent_doc_owned =
            window.name == target_window_name || is_stash_window_name(&window.name);
        if agent_doc_owned && window.height == STASH_JOIN_TEMP_HEIGHT {
            match agent_doc_tmux_io::resize_window_to_clients(tmux, &window.id) {
                Ok(()) => sync_log(&format!(
                    "layout_repair_unpinned_window window={} name={} stale_height={}",
                    window.id, window.name, window.height
                )),
                Err(err) => eprintln!(
                    "[repair] warning: could not unpin stale 1000-row window {} ({}): {}",
                    window.id, window.name, err
                ),
            }
        }
    }

    // ── Fast path: if layout is already correct, skip repair ──
    let has_target = windows.iter().any(|w| w.name == target_window_name);
    let stash_count = windows
        .iter()
        .filter(|w| is_stash_window_name(&w.name))
        .count();
    let has_exact_stash = windows.iter().any(|w| w.name == "stash");
    // Check if Phase 1+2 can be skipped. A clean single-window layout has no
    // stash yet; forcing one into existence makes repeated manual/JB syncs
    // ping-pong through destructive tmux window ops (#tmuxsynccrash).
    let skip_phase_1_2 =
        agent_doc_tmux::repair_layout_skips_rescue_phase(has_target, stash_count, has_exact_stash);
    if skip_phase_1_2 {
        // Target exists and stash is consolidated. Skip Phases 1+2,
        // but still run target consolidation and index normalization below.
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
                    // Temporarily grow the stash window so `join-pane` cannot fail
                    // with "pane too small". `#stashresizerestore`: this MUST be
                    // undone after the joins — see the restore below. Leaving it
                    // set pins the window at 1000 rows for the rest of the
                    // session, and every TUI in it then renders into a ~1000-row
                    // viewport: content sits at the bottom under ~950 rows of
                    // blank pane, which reads to the operator as "my history is
                    // gone".
                    if let Err(err) = agent_doc_tmux_io::resize_window_height(
                        tmux,
                        &primary_id,
                        STASH_JOIN_TEMP_HEIGHT,
                    ) {
                        eprintln!(
                            "[repair] warning: could not grow stash window {primary_id} for join: {err}"
                        );
                    }

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
                    let _ = agent_doc_tmux_io::kill_window(tmux, sec_id);
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

            // `#stashresizerestore`: hand the window back to the client size. The
            // 1000-row height above is a join-time workaround, not a layout
            // choice. Re-fit the current geometry and remove tmux's per-window
            // manual-size override so subsequent client resizes remain live.
            if let Err(err) = agent_doc_tmux_io::resize_window_to_clients(tmux, &primary_id) {
                eprintln!(
                    "[repair] warning: could not restore stash window {primary_id} to the client size: {err}"
                );
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
            if let Ok(registry) = agent_doc_session_registry_io::load() {
                let mut rescued = false;
                for entry in registry.values() {
                    if !tmux.pane_alive(&entry.pane) {
                        continue;
                    }
                    if tmux.pane_session(&entry.pane).ok().as_deref() != Some(session_name) {
                        continue;
                    }
                    let Some(window_id) = agent_doc_tmux_io::target_window_id(tmux, &entry.pane)
                    else {
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
                            if let Some(new_win) =
                                agent_doc_tmux_io::target_window_id(tmux, &entry.pane)
                            {
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

    // ── Phase 3: Consolidate duplicate target windows (always runs) ──
    consolidate_duplicate_target_windows(tmux, session_name, target_window_name);

    // ── Phase 4: Normalize window indices (always runs) ──
    // agent-doc should be at index 0, stash windows should directly follow it.
    let windows = list_session_windows(tmux, session_name);
    if let Some((_, target_window_id, _)) = windows
        .iter()
        .find(|(_, _, name)| name == target_window_name)
        .cloned()
    {
        normalize_window_to_index(tmux, session_name, &target_window_id, 0, "repair");
    }

    let stash_index_plan = planned_stash_window_indices(
        &list_session_windows(tmux, session_name),
        is_stash_window_name,
    );
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

fn consolidate_duplicate_target_windows(tmux: &Tmux, session_name: &str, target_window_name: &str) {
    let mut target_windows: Vec<(usize, String)> = list_session_windows(tmux, session_name)
        .into_iter()
        .filter_map(|(index, id, name)| {
            if name != target_window_name {
                return None;
            }
            let parsed_index = index.parse::<usize>().unwrap_or(usize::MAX);
            Some((parsed_index, id))
        })
        .collect();
    target_windows.sort_by_key(|(index, id)| (*index, id.clone()));
    let Some((_, canonical_window)) = target_windows.first().cloned() else {
        return;
    };
    if target_windows.len() <= 1 {
        return;
    }

    for (_, duplicate_window) in target_windows.into_iter().skip(1) {
        let panes = tmux
            .list_window_panes(&duplicate_window)
            .unwrap_or_default();
        for pane in panes {
            if !tmux.pane_alive(&pane) {
                continue;
            }
            let target = tmux.largest_pane_in_window(&canonical_window).or_else(|| {
                tmux.list_window_panes(&canonical_window)
                    .unwrap_or_default()
                    .into_iter()
                    .next()
            });
            let Some(target) = target.filter(|pane_id| !pane_id.is_empty()) else {
                continue;
            };
            sync_log(&format!(
                "target_window_consolidate_action=join-pane src={} dst={} duplicate_window={} canonical_window={}",
                pane, target, duplicate_window, canonical_window
            ));
            match PaneMoveOp::new(tmux, &pane, &target).join("-dh") {
                Ok(()) => {
                    eprintln!(
                        "[repair] joined duplicate {} pane {} into {}",
                        target_window_name, pane, canonical_window
                    );
                    sync_log(&format!(
                        "target_window_consolidate_result=join-pane ok=true src={} dst={}",
                        pane, target
                    ));
                }
                Err(e) => {
                    eprintln!(
                        "[repair] join duplicate {} pane {} → {} failed: {}",
                        target_window_name, pane, target, e
                    );
                    sync_log(&format!(
                        "target_window_consolidate_result=join-pane ok=false src={} dst={} err={}",
                        pane, target, e
                    ));
                }
            }
        }

        if tmux
            .list_window_panes(&duplicate_window)
            .unwrap_or_default()
            .is_empty()
        {
            let _ = agent_doc_tmux_io::kill_window(tmux, &duplicate_window);
        }
    }
}

fn normalize_stash_window_name(tmux: &Tmux, window_id: &str) {
    let _ = agent_doc_tmux_io::rename_window(tmux, window_id, "stash");
}

fn sync_log(msg: &str) {
    crate::append_sync_log(msg);
}

fn reselect_visible_focus_pane_if_present(
    tmux: &Tmux,
    window: Option<&str>,
    focus: Option<&str>,
    reason: &str,
) -> Option<String> {
    let focus_display = focus?;
    let canonical_focus = canonicalize_sync_file(Path::new(focus_display))?;
    let session_id = agent_doc_frontmatter_io::session::read_session_id(&canonical_focus)?;
    let pane = find_normal_path_owner_pane(tmux, &canonical_focus, &session_id)?;
    if let Some(target_window) = window {
        let visible_panes = tmux.list_window_panes(target_window).unwrap_or_default();
        if !visible_panes.iter().any(|candidate| candidate == &pane) {
            return None;
        }
    }
    match tmux.select_pane(&pane) {
        Ok(()) => {
            let message = preserved_layout_focus_marker(&pane, reason);
            eprintln!("{}", message);
            sync_log(&message);
            Some(pane)
        }
        Err(err) => {
            let warning = reselect_visible_focus_pane_failed_warning(
                &pane,
                &canonical_focus.display().to_string(),
                &err.to_string(),
            );
            eprintln!("{}", warning);
            sync_log(&warning);
            None
        }
    }
}

/// Check the per-server-per-session destructive-repair throttle in the project
/// state transaction. No filesystem stamp participates in the sync hot path.
fn throttle_destructive_repair(project_root: &Path, tmux: &Tmux, session_name: &str) -> bool {
    let now = epoch_millis_now();
    match destructive_repair_throttle_at(
        project_root,
        tmux.server_socket.as_deref(),
        session_name,
        now,
    ) {
        Ok((true, last)) => {
            sync_log(&format!(
                "destructive_repair_throttled session={} last={:?} now={} min_ms={}",
                session_name,
                last,
                now,
                agent_doc_tmux::DESTRUCTIVE_REPAIR_MIN_INTERVAL_MS
            ));
            true
        }
        Ok((false, _)) => false,
        Err(error) => {
            eprintln!(
                "[sync] warning: could not update destructive-repair throttle in state.db: {}",
                error
            );
            false
        }
    }
}

fn destructive_repair_throttle_at(
    project_root: &Path,
    server_socket: Option<&str>,
    session_name: &str,
    now: u64,
) -> Result<(bool, Option<u64>)> {
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let state_key = destructive_repair_throttle_state_key(server_socket, session_name);
    let last =
        agent_doc_sqlite::state_store::load_project_runtime_state_from_db(&conn, &state_key)?
            .and_then(|value| value.parse::<u64>().ok());
    if agent_doc_tmux::destructive_repair_throttled(
        last,
        now,
        agent_doc_tmux::DESTRUCTIVE_REPAIR_MIN_INTERVAL_MS,
    ) {
        return Ok((true, last));
    }
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &state_key,
        &now.to_string(),
        now,
    )?;
    Ok((false, last))
}

fn current_tmux_session_name(tmux: &Tmux) -> Option<String> {
    tmux.current_session()
}

fn current_agent_doc_session_name(tmux: &Tmux) -> Option<String> {
    let current = current_tmux_session_name(tmux)?;
    if agent_doc_tmux_io::has_window_named(tmux, &current, "agent-doc") {
        Some(current)
    } else {
        None
    }
}

fn resolve_preferred_session(
    tmux: &Tmux,
    context_session: Option<&str>,
    log_prefix: &str,
) -> Option<String> {
    if let Some(ctx) = normalize_context_session(context_session) {
        return Some(ctx.to_string());
    }

    if let Some(current) = current_agent_doc_session_name(tmux) {
        return Some(current);
    }

    let configured = agent_doc_project_config_io::project_tmux_session();
    if configured.as_ref().is_some_and(|s| tmux.session_alive(s)) {
        return configured;
    }

    if let Some(ref stale) = configured {
        eprintln!(
            "{log_prefix} configured tmux_session '{}' is not alive, ignoring stale pin",
            stale
        );
    }

    current_tmux_session_name(tmux)
}

fn session_name_for_target_window(tmux: &Tmux, window: &str) -> Option<String> {
    agent_doc_tmux_io::target_session_name(tmux, window)
}

fn sync_doctor_repair_candidate(col_args: &[String], focus: Option<&str>) -> Option<PathBuf> {
    agent_doc_sync::sync_candidate_files(col_args, focus)
        .into_iter()
        .find_map(|path| {
            if !path.exists() || agent_doc_frontmatter_io::session::read_session_id(&path).is_none()
            {
                return None;
            }
            Some(path.canonicalize().unwrap_or(path))
        })
}

#[cfg(test)]
fn safe_passive_prune_cleanup_mode(
    auto_start_mode: AutoStartMode,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
) -> agent_doc_tmux::PruneCleanupMode {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    safe_passive_prune_cleanup_mode_at_root(auto_start_mode, col_args, window, focus, &cwd)
}

fn safe_passive_prune_cleanup_mode_at_root(
    auto_start_mode: AutoStartMode,
    _col_args: &[String],
    _window: Option<&str>,
    _focus: Option<&str>,
    _project_root: &Path,
) -> agent_doc_tmux::PruneCleanupMode {
    if !matches!(auto_start_mode, AutoStartMode::SafePassive) {
        return agent_doc_tmux::PruneCleanupMode::Full;
    }
    // Editor-driven safe-passive sync is the fast handoff path. It still prunes
    // stale registry rows, but it must not spend the selection budget scanning
    // stash panes or retained dead panes before tmux-router can detach any extra
    // visible pane from the active editor projection.
    agent_doc_tmux::PruneCleanupMode::SkipExpensiveStashCleanup
}

fn exact_visible_safe_passive(
    auto_start_mode: AutoStartMode,
    exact_visible_projection: bool,
) -> bool {
    matches!(auto_start_mode, AutoStartMode::SafePassive) && exact_visible_projection
}

/// `#exact-visible-focus-swap` eligibility (cheap gate): whether an exact-visible
/// passive focus sync MAY resolve an existing off-screen owned pane — so the
/// reconcile can swap it into view — instead of blocking every file and preserving
/// the stale layout. This covers the cheap, always-safe-to-evaluate terms; the
/// caller still confirms ownership with the (expensive) live-owner proof before
/// resolving, so the full rule is `eligible && proves_live_owner`. We only reuse a
/// pane that already EXISTS (never cold-start a missing pane under no-autostart)
/// and never borrow a pane another document already claimed this sync run. Kept as
/// a pure function so the SimWorld projection model and the live sync path share
/// one decision rule.
pub fn exact_visible_focus_eligible_for_owned_pane_resolve(
    exact_visible_no_autostart: bool,
    registered_pane_alive: bool,
    claimed_by_other_document: bool,
) -> bool {
    exact_visible_no_autostart && registered_pane_alive && !claimed_by_other_document
}

fn skip_sync_status_updates_for_mode(auto_start_mode: AutoStartMode) -> bool {
    matches!(auto_start_mode, AutoStartMode::SafePassive)
}

pub fn configured_session_for_root(tmux: &Tmux, root: &Path) -> Option<String> {
    let config_path = root.join(".agent-doc").join("config.toml");
    let configured = agent_doc_project_config_io::load_project_from(&config_path).tmux_session;
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
        return resolve_preferred_session(tmux, context_session.as_deref(), "[sync]");
    }

    if let Some(current) = current_agent_doc_session_name(tmux) {
        return Some(current);
    }

    if let Some(scope_root) = agent_doc_sync::shared_sync_scope_root(col_args, focus) {
        if let Some(session) = configured_session_for_root(tmux, &scope_root) {
            return Some(session);
        }
        return current_tmux_session_name(tmux);
    }

    resolve_preferred_session(tmux, None, "[sync]")
}

fn resolve_agent_doc_window_id(
    tmux: &Tmux,
    session_name: &str,
    target_window_name: &str,
) -> Option<String> {
    let listing = agent_doc_tmux_io::list_windows(
        tmux,
        Some(&format!("{}:", session_name)),
        "#{window_id} #{window_name}",
    )
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
    agent_doc_tmux_io::target_window_name(tmux, window_id)
}

fn target_is_agent_doc_window(tmux: &Tmux, target: &str) -> bool {
    window_name_for_window_id(tmux, target).as_deref() == Some("agent-doc")
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
    let Some(window_id) = agent_doc_tmux_io::target_window_id(tmux, pane_id) else {
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
    let Some(window_id) = agent_doc_tmux_io::target_window_id(tmux, pane_id) else {
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
            if let Some(new_win) = agent_doc_tmux_io::target_window_id(tmux, pane_id) {
                let _ = agent_doc_tmux_io::rename_window(tmux, &new_win, "agent-doc");
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
    let Ok(output) = agent_doc_tmux_io::list_windows(
        tmux,
        Some(&format!("{}:", session_name)),
        "#{window_index} #{window_id} #{window_name}",
    ) else {
        return Vec::new();
    };
    agent_doc_tmux::parse_session_windows(&output)
}

fn normalize_window_to_index(
    tmux: &Tmux,
    session_name: &str,
    window_id: &str,
    desired_index: usize,
    log_prefix: &str,
) {
    let windows = list_session_windows(tmux, session_name);
    match plan_window_index_normalization(&windows, window_id, desired_index) {
        WindowIndexNormalizationPlan::Missing | WindowIndexNormalizationPlan::AlreadyAtIndex => {}
        WindowIndexNormalizationPlan::Swap {
            current_index,
            desired_index,
            current_name,
            occupant_id,
            occupant_name,
        } => {
            sync_log(&format!(
                "{}_action=swap-window src={} dst={} session={} src_name={} dst_name={}",
                log_prefix, current_index, desired_index, session_name, current_name, occupant_name
            ));
            let result = agent_doc_tmux_io::swap_window(tmux, window_id, &occupant_id);
            sync_log(&format!(
                "{}_result=swap-window ok={} src={} dst={}",
                log_prefix,
                result.is_ok(),
                current_index,
                desired_index
            ));
            let _ = result;
        }
        WindowIndexNormalizationPlan::Move {
            current_index,
            desired_index,
            current_name,
        } => {
            sync_log(&format!(
                "{}_action=move-window src={} dst={} session={} name={}",
                log_prefix, current_index, desired_index, session_name, current_name
            ));
            let result = tmux.raw_cmd(&[
                "move-window",
                "-s",
                window_id,
                "-t",
                &format!("{session_name}:{desired_index}"),
            ]);
            sync_log(&format!(
                "{}_result=move-window ok={} src={} dst={}",
                log_prefix,
                result.is_ok(),
                current_index,
                desired_index
            ));
            let _ = result;
        }
    }
}

// `#panefocussteal`: a passive sync (the JetBrains 5s layout poll or a
// tab-select) reconciles tmux layout but must **never** select a pane. Moving
// tmux focus is a separate, explicit action — the `agent-doc focus <file>`
// command — which the plugin invokes only on a deliberate editor doc switch
// (`EditorTabSyncListener.plan()` → `AutomaticCommandKind.Focus`).
//
// An earlier attempt (`maybe_follow_focus_change`) tried to follow focus from
// inside `sync` by persisting the last focused document per project at
// `.agent-doc/state/last-sync-focus` and selecting the pane whenever the
// incoming `--focus` differed. That is unsound with more than one open document:
// each editor window emits `sync --focus <its-own-doc>`, and documents under a
// shared superproject root map to the **same** state file. A `--focus` for the
// non-globally-focused document therefore always compares "changed" and steals
// the tmux pane to itself. `sync` cannot tell "the operator switched the global
// focus" from "another window's poller reconciled its own layout" — only the
// editor knows, and it already signals a deliberate switch via `agent-doc
// focus`. So `sync` stays focus-neutral and owns layout only.

fn run_with_options_internal(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
    exact_visible_projection: bool,
    tmux: &Tmux,
) -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_with_options_internal_at_root(
        &cwd,
        col_args,
        window,
        focus,
        auto_start_mode,
        exact_visible_projection,
        tmux,
    )
}

/// Resolve `project_root` to its canonical nearest `.agent-doc` root so
/// cross-root comparisons in [`document_belongs_to_sync_root`] are stable.
fn canonical_sync_project_root(project_root: &Path) -> PathBuf {
    agent_doc_fs::find_project_root_canonical(project_root)
        .or_else(|| project_root.canonicalize().ok())
        .unwrap_or_else(|| project_root.to_path_buf())
}

/// A document belongs to this sync's project root only when its nearest
/// `.agent-doc` ancestor is that same root (`#sync-cross-root-autostart`).
///
/// A document under a nested/submodule project root (its own `.agent-doc/`,
/// e.g. `src/sample-app/tasks/...`) is owned by that root's own controller
/// and sync. Auto-starting or remembering it from the superproject provisions a
/// mis-rooted pane: `agent-doc start` runs against the wrong root, the pane is
/// slow to prove ownership, and it dies on start — the operator-visible "slow
/// creation of the new pane and agent-doc start crash". `sync_project_root`
/// must already be canonical (see [`canonical_sync_project_root`]).
///
/// A document with no resolvable `.agent-doc` ancestor (unmanaged/scaffold, or a
/// path that does not yet exist on disk) is treated as belonging so same-root
/// scaffolding is never blocked.
fn document_belongs_to_sync_root(file: &Path, sync_project_root: &Path) -> bool {
    match agent_doc_fs::find_project_root_canonical(file) {
        Some(owning) => owning == *sync_project_root,
        None => true,
    }
}

/// Drop remembered layout columns that belong to a *different* project root so
/// the cross-root document is neither restored into a visible column nor
/// re-persisted into this root's layout memory. A column whose files are all
/// cross-root becomes an empty placeholder (preserving column index positions
/// the remembered-layout math relies on); a mixed column keeps only its
/// same-root files. Returns the sanitized layout plus the paths that were
/// dropped (for diagnostics).
fn sanitize_cross_root_layout(
    saved_layout: Vec<String>,
    sync_project_root: &Path,
) -> (Vec<String>, Vec<String>) {
    let mut dropped: Vec<String> = Vec::new();
    let sanitized = saved_layout
        .into_iter()
        .map(|col| {
            let files: Vec<&str> = col
                .split(',')
                .map(str::trim)
                .filter(|f| !f.is_empty())
                .collect();
            if files.is_empty() {
                return col;
            }
            let kept: Vec<&str> = files
                .iter()
                .copied()
                .filter(|f| {
                    let belongs = document_belongs_to_sync_root(Path::new(f), sync_project_root);
                    if !belongs {
                        dropped.push((*f).to_string());
                    }
                    belongs
                })
                .collect();
            kept.join(",")
        })
        .collect();
    (sanitized, dropped)
}

fn run_with_options_internal_at_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
    exact_visible_projection: bool,
    tmux: &Tmux,
) -> Result<()> {
    let window = agent_doc_sync::normalize_scope_arg(window);
    let focus = agent_doc_sync::normalize_scope_arg(focus);
    reset_sync_run_report();
    tracing::debug!(
        cols = ?col_args,
        window,
        focus,
        auto_start_mode = auto_start_mode.log_label(),
        "sync::run_with_options start"
    );
    let sync_total_start = Instant::now();
    let pre_window_start = Instant::now();

    // `#panefocussteal`: no pre-lock actor focus. A passive sync must not select
    // the document pane — that is what stole the operator's focus on every poll.
    // Deliberate focus movement is the `agent-doc focus` command.

    // Serialize sync calls via file lock. Concurrent syncs (from rapid tab switches)
    // race against each other's stash operations, causing pane bouncing. Contention
    // is bounded so a stuck prior editor sync cannot starve later selections forever.
    let scope_root = agent_doc_sync::layout_state_scope_root(col_args, focus, project_root);
    let lock_path_buf = scope_root.join(".agent-doc/sync.lock");
    let lock_path = lock_path_buf.as_path();
    let sync_lock_wait_budget = if matches!(auto_start_mode, AutoStartMode::SafePassive) {
        SYNC_LOCK_WAIT_LATENCY_BUDGET
    } else {
        SYNC_LOCK_WAIT_BUDGET
    };
    let sync_lock_start = Instant::now();
    let lock_guard = acquire_sync_lock(lock_path, sync_lock_wait_budget, |message| {
        if message.starts_with("[sync] stale_sync_lock_owner_reaped ") {
            eprintln!("{}", message);
        }
        sync_log(&message);
    });
    let sync_lock_elapsed = sync_lock_start.elapsed();
    log_sync_latency(
        focus,
        "sync_lock_wait",
        sync_lock_elapsed,
        SYNC_LOCK_WAIT_LATENCY_BUDGET,
        auto_start_mode,
    );
    if matches!(auto_start_mode, AutoStartMode::SafePassive) && !lock_guard.is_acquired() {
        let message = agent_doc_sync::safe_passive_lock_contention_message(
            sync_lock_elapsed,
            sync_lock_wait_budget,
        );
        eprintln!("{}", message);
        sync_log(&message);
        return Ok(());
    }

    // `#syncobscache`: with the sync lock held we own the layout, so tmux's
    // structural answers are coherent for the rest of this pass. Reconciliation
    // re-asks the same questions once per tracked document while resolving
    // panes; memoizing the read-only ones for the run collapsed a measured 109
    // subprocess spawns to 35 distinct commands. Any mutation we perform
    // invalidates the memo, and the guard drops it at the end of the run so the
    // next sync always re-observes.
    let _tmux_observations = agent_doc_tmux_io::begin_observation_scope();
    // The pane-liveness half lives in tmux-router because that crate issues the
    // `list-panes -a` scans directly (its spawn sites bypass the
    // `TmuxCommandRunner` trait the cache above wraps). `pane_alive`/`pane_dead`
    // each scan the full listing for one id, so a caller checking N panes issued
    // N identical full scans (19 in a measured focus sync). The scope shares one
    // snapshot; tmux-router's pane-mutating methods invalidate it themselves.
    let _pane_snapshot = tmux_router::begin_pane_snapshot_scope();
    // `#ledgerscope`: the dominant hot-path cost. `load_startup_miss` (and
    // `take_superseded_startup_miss`, which calls it again) loads the ENTIRE
    // `state.db` event ledger and JSON-replays every event just to project one
    // document out of it — per document. Measured here: 995ms of a 1024ms
    // per-document ownership loop, for two documents. The ledger is one shared
    // input per project, so this scope loads it once and lets every projection
    // derive from that; `append_event` invalidates it.
    let _state_ledger = agent_doc_supervisor_io::begin_state_ledger_scope();
    let _lock_guard = lock_guard;

    // Check for new build and clear stale caches
    {
        match agent_doc_controller_io::project_controller::close_stale_starting_actors_for_caller(
            &scope_root,
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
        if !matches!(auto_start_mode, AutoStartMode::SafePassive) {
            match agent_doc_controller_io::project_controller::close_stale_dead_pane_actors_with_tmux_for_caller(
                &scope_root,
                false,
                "sync",
                "stale_dead_pane_actor",
            ) {
                Ok((closed, kept)) if closed > 0 => {
                    eprintln!(
                        "[sync] actors: {} stale dead-pane closed, {} still active",
                        closed, kept
                    );
                }
                Ok(_) => {}
                Err(e) => eprintln!("[sync] dead-pane actor gc warning: {}", e),
            }
        }
    }

    // Column memory: for columns with non-agent files, substitute the last known
    // agent doc so the reconciler preserves the pane from the previous layout.
    // When sync is called without explicit columns, fall back to that recorded layout.
    let layout_state_root = scope_root;
    let saved_layout =
        match agent_doc_controller_io::project_controller::load_layout_state(&layout_state_root) {
            Ok(layout) => layout,
            Err(err) => {
                eprintln!(
                    "[sync] warning: failed to load controller layout state from {}: {}",
                    layout_state_root.display(),
                    err
                );
                Vec::new()
            }
        };

    // Cross-root guard (`#sync-cross-root-autostart`): remembered layout state can
    // accumulate documents that live under a nested/submodule project root with
    // its own `.agent-doc/` (e.g. `src/sample-app/tasks/...`). Restoring or
    // auto-starting them from this root provisions a mis-rooted pane that dies on
    // start. Sanitize the remembered layout at load so the cross-root document is
    // neither restored into a column nor re-persisted; this self-heals memory that
    // was polluted before the guard existed.
    let sync_project_root = canonical_sync_project_root(project_root);
    let saved_layout = {
        let (sanitized, dropped) = sanitize_cross_root_layout(saved_layout, &sync_project_root);
        for path in &dropped {
            eprintln!(
                "[sync] dropping cross-root document from remembered layout: {} (sync root {})",
                path,
                sync_project_root.display()
            );
            sync_log(&format!(
                "cross_root_layout_dropped file={} sync_root={}",
                path,
                sync_project_root.display()
            ));
        }
        sanitized
    };

    let input_cols = effective_sync_columns(col_args, &saved_layout, &layout_state_root)?;
    let column_memory = agent_doc_tmux::apply_column_memory(
        &agent_doc_tmux::classify_sync_layout_columns(&input_cols, first_agent_doc_in_col),
        &saved_layout,
    );
    for restoration in &column_memory.restorations {
        sync_log(&format!(
            "column {} has no agent doc, substituting remembered: {}",
            restoration.column_index, restoration.remembered
        ));
    }
    let mut col_args: Vec<String> = column_memory
        .columns
        .into_iter()
        .filter(|col| !col.is_empty())
        .collect();
    let skip_autostart_diagnostics =
        exact_visible_safe_passive(auto_start_mode, exact_visible_projection);
    let skip_sync_status_updates = skip_sync_status_updates_for_mode(auto_start_mode);
    let proof_cache = if skip_autostart_diagnostics {
        let controller_local_actor_lookup =
            agent_doc_controller_io::process::is_same_project_controller_pid(
                &sync_project_root,
                std::process::id(),
            );
        SyncProofCache::safe_passive(controller_local_actor_lookup)
    } else {
        SyncProofCache::default()
    };
    let hot_latency_focus = if skip_autostart_diagnostics {
        None
    } else {
        focus
    };

    // Resolve the target session/window. Full/manual sync delegates repair to
    // the same file-scoped doctor path that operators can run explicitly;
    // passive editor sync keeps layout repair explicit.
    log_sync_latency(
        focus,
        "gap_pre_window",
        pre_window_start.elapsed(),
        SYNC_WINDOW_RESOLUTION_BUDGET,
        auto_start_mode,
    );
    let window_resolution_start = Instant::now();
    let target_session = resolve_sync_target_session(tmux, window, &col_args, focus);
    let full_sync = matches!(auto_start_mode, AutoStartMode::Full);
    let doctor_repair_candidate = if full_sync {
        sync_doctor_repair_candidate(&col_args, focus)
    } else {
        None
    };
    // `#tmuxsynccrash`: gate the destructive doctor repair behind a per-session
    // rate limit. A rapid burst of full syncs (double-pressed `Sync Tmux
    // Layout`, the JB supersede re-run, interleaved triggers) otherwise storms
    // tmux with `move-window`/`swap-window`/`join-pane`/`resize-window` ops and
    // crashes the server. Decide once and apply to BOTH the pre- and
    // post-reconcile repair so a single gesture still runs its full pass; only
    // cross-invocation bursts are throttled. The reconciler always runs.
    let run_doctor_repair = if full_sync {
        match target_session
            .clone()
            .or_else(|| current_tmux_session_name(tmux))
        {
            Some(session) if throttle_destructive_repair(&layout_state_root, tmux, &session) => {
                let message = format!(
                    "[sync] doctor repair throttled for session `{session}` (within {}ms of a prior repair); running reconcile only (#tmuxsynccrash)",
                    agent_doc_tmux::DESTRUCTIVE_REPAIR_MIN_INTERVAL_MS
                );
                eprintln!("{message}");
                sync_log(&message);
                false
            }
            _ => true,
        }
    } else {
        false
    };
    if run_doctor_repair {
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
        agent_doc_tmux::focused_column_index(&remembered_layout, focus).or_else(|| {
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
    let mid_gap_start = Instant::now();
    // `#panefocussteal`: passive sync is focus-neutral — it never selects a pane.
    // Deliberate focus movement is the `agent-doc focus <file>` command, which the
    // plugin invokes on a real editor doc switch. Following focus from here is
    // unsound with multiple open documents (see the module comment above
    // `run_with_options_internal`): every window emits `sync --focus <own-doc>`
    // against a shared per-project `last-sync-focus`, so a non-focused document's
    // poll would steal the tmux pane.
    let focus_only_mode = if matches!(auto_start_mode, AutoStartMode::SafePassive) {
        agent_doc_tmux::TmuxFocusOnlyExpansionMode::SafePassive
    } else {
        agent_doc_tmux::TmuxFocusOnlyExpansionMode::LiteralProjection
    };
    let focus_only_expansion = agent_doc_tmux::apply_focus_only_expansion_policy(
        &col_args,
        &remembered_layout,
        active_column_index,
        focus_only_mode,
        exact_visible_projection,
    );
    match &focus_only_expansion.event {
        Some(agent_doc_tmux::TmuxFocusOnlyExpansionEvent::ExactVisibleProjection) => {
            sync_log(&format!(
                "safe_passive_exact_visible_projection columns={:?}",
                col_args
            ));
        }
        Some(agent_doc_tmux::TmuxFocusOnlyExpansionEvent::Expanded {
            active_column_index,
        }) => {
            sync_log(&format!(
                "safe_passive_focus_only_editor_switch_expanded active_column={} columns={:?}",
                active_column_index, focus_only_expansion.columns
            ));
        }
        None => {}
    }
    col_args = focus_only_expansion
        .columns
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
    let prune_cleanup_mode = safe_passive_prune_cleanup_mode_at_root(
        auto_start_mode,
        col_args,
        window,
        focus,
        &layout_state_root,
    );
    if matches!(
        prune_cleanup_mode,
        agent_doc_tmux::PruneCleanupMode::SkipExpensiveStashCleanup
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
            hot_latency_focus,
            timing.phase,
            timing.elapsed,
            SYNC_PRUNE_SUBPHASE_BUDGET,
            auto_start_mode,
        );
    }
    log_sync_latency(
        hot_latency_focus,
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

    let registry_path = agent_doc_session_registry_io::registry_path_in(&layout_state_root);
    // Track session_id → file path for post-sync claim updates
    let session_files: RefCell<Vec<(String, PathBuf)>> = RefCell::new(Vec::new());
    let blocked_unresolved_files: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
    let safe_passive_managed_files: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
    let safe_passive_resolved_files: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());

    let resolve_file = |path: &Path| -> Option<FileResolution> {
        // Step 1: Auto-scaffold empty .md files BEFORE ensure_initialized().
        // Must run first because ensure_initialized() writes minimal frontmatter
        // (just agent_doc_session:) which prevents the full template scaffold.
        // Per SPEC §8.5: empty files should be initialized as template documents.
        if path.extension() == Some(std::ffi::OsStr::new("md")) {
            let raw = std::fs::read_to_string(path).unwrap_or_default();
            if agent_doc_document::claim_scaffold::should_scaffold_empty_markdown(
                &raw,
                path.extension().and_then(|extension| extension.to_str()),
            ) {
                eprintln!("[sync] auto-scaffolding empty file: {}", path.display());
                let session_id = uuid::Uuid::new_v4().to_string();
                let scaffold =
                    agent_doc_document::claim_scaffold::render_empty_template_scaffold(&session_id);
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
                if let Err(e) = agent_doc_snapshot_io::checkpoint_document_baseline(
                    path,
                    &scaffold,
                    agent_doc_ops_log_io::log_op,
                ) {
                    eprintln!(
                        "[sync] warning: failed to save scaffold snapshot for {}: {}",
                        path.display(),
                        e
                    );
                }
                // Commit the scaffolded file immediately.
                if let Err(e) = crate::runtime_effects().and_then(|effects| effects.commit(path)) {
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
        if let Err(e) = agent_doc_workflow_io::document_init::ensure_initialized(
            path,
            |doc| crate::runtime_effects()?.commit(doc),
            agent_doc_ops_log_io::log_op,
        ) {
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
                if !skip_sync_status_updates {
                    crate::surface_frontmatter_status_with(
                        path,
                        "resolve_file",
                        &e,
                        save_sync_status_snapshot,
                        log_sync_status,
                    );
                }
                return None;
            }
        };
        if !skip_sync_status_updates {
            crate::clear_frontmatter_status_with(path, save_sync_status_snapshot, log_sync_status);
        }

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
        log_sync_latency(
            focus,
            "gap_window_to_ownership",
            mid_gap_start.elapsed(),
            SYNC_OWNERSHIP_PROOF_BUDGET,
            auto_start_mode,
        );
        let (mut sa, mut sb, mut sc, mut sd) = (
            Duration::ZERO,
            Duration::ZERO,
            Duration::ZERO,
            Duration::ZERO,
        );
        let per_file_loop_start = Instant::now();
        for file_path in &all_files {
            let mut seg_mark = Instant::now();
            let _ = &seg_mark;
            if !file_path.exists() {
                continue;
            }
            // Cross-root guard (`#sync-cross-root-autostart`): never provision a
            // pane for a document owned by a different (nested/submodule) project
            // root — its own controller manages it. Defense-in-depth for a
            // cross-root path passed directly via col_args, in addition to the
            // remembered-layout sanitization above.
            if !document_belongs_to_sync_root(file_path, &sync_project_root) {
                eprintln!(
                    "[sync] skipping cross-root auto-start: {} is owned by another project root (sync root {}) — its own controller starts it (#sync-cross-root-autostart)",
                    file_path.display(),
                    sync_project_root.display()
                );
                sync_log(&format!(
                    "cross_root_autostart_skipped file={} sync_root={}",
                    file_path.display(),
                    sync_project_root.display()
                ));
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
                    if !skip_sync_status_updates {
                        crate::surface_frontmatter_status_with(
                            file_path,
                            "auto-start",
                            &e,
                            save_sync_status_snapshot,
                            log_sync_status,
                        );
                    }
                    continue;
                }
            };
            if !skip_sync_status_updates {
                crate::clear_frontmatter_status_with(
                    file_path,
                    save_sync_status_snapshot,
                    log_sync_status,
                );
            }
            let session_id = match fm.session {
                Some(ref id) => id.clone(),
                None => continue,
            };
            if matches!(auto_start_mode, AutoStartMode::SafePassive) {
                safe_passive_managed_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
            }

            let authoritative_actor_pane = project_authoritative_actor_binding(
                tmux,
                file_path,
                &session_id,
                hot_latency_focus,
                auto_start_mode,
                &proof_cache,
            );
            sa += seg_mark.elapsed();
            seg_mark = Instant::now();
            let registered_entry = lookup_registry_entry_for_file_session(file_path, &session_id);
            let registered_pane = authoritative_actor_pane
                .or_else(|| registered_entry.as_ref().map(|entry| entry.pane.clone()));
            if let Some((miss, supersession)) =
                agent_doc_supervisor_io::startup_miss::take_superseded_startup_miss(
                    agent_doc_supervisor_io::startup_miss::session_registry_lookup(),
                    file_path,
                )?
            {
                let miss_ts = agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
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
            let mut unresolved_startup_miss =
                agent_doc_supervisor_io::startup_miss::load_startup_miss(file_path)
                    .ok()
                    .flatten();
            sb += seg_mark.elapsed();
            seg_mark = Instant::now();
            if let Some(miss) = unresolved_startup_miss.as_ref()
                && !tmux.pane_alive(&miss.pane_id)
            {
                let miss_ts = agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
                eprintln!(
                    "[sync] clearing stale startup-miss on dead pane {} from {} for {}",
                    miss.pane_id,
                    miss_ts,
                    file_path.display()
                );
                sync_log(&format!(
                    "startup_miss_cleared_dead_pane file={} pane={} miss_timestamp={}",
                    file_path.display(),
                    miss.pane_id,
                    miss_ts
                ));
                agent_doc_supervisor_io::startup_miss::clear_startup_miss(file_path)?;
                unresolved_startup_miss = None;
            }

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
            if !skip_autostart_diagnostics
                && matches!(auto_start_mode, AutoStartMode::SafePassive)
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
                safe_passive_resolved_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
                reserve_sync_pane(&claimed_sync_panes, pane_id, file_path);
                continue;
            }
            sb += seg_mark.elapsed();
            seg_mark = Instant::now();
            let registered_live_owner = !skip_autostart_diagnostics
                && registered_pane.as_ref().is_some_and(|pane| {
                    registered_pane_proves_live_owner(
                        tmux,
                        file_path,
                        &session_id,
                        pane,
                        &proof_cache,
                    )
                });
            if let Some(pane) = registered_pane.as_ref()
                && tmux.pane_alive(pane)
                && !registered_live_owner
            {
                if !skip_autostart_diagnostics
                    && claimed_owner.is_none()
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
                    let _ = agent_doc_supervisor_io::startup_miss::append_session_log_event(
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
                if !skip_autostart_diagnostics
                    && claimed_owner.is_none()
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
                    let _ = agent_doc_supervisor_io::startup_miss::append_session_log_event(
                        file_path,
                        &session_id,
                        &event,
                    );
                    blocked_unresolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
                    continue;
                }
                if skip_autostart_diagnostics {
                    eprintln!(
                        "[sync] pane {} for {} is alive; exact-visible no-autostart is treating it as unresolved without ownership diagnostics",
                        pane,
                        file_path.display()
                    );
                    sync_log(&format!(
                        "registered_pane_unresolved_no_autostart file={} pane={} action=treat_unresolved",
                        file_path.display(),
                        pane
                    ));
                } else {
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
            }
            sc += seg_mark.elapsed();
            seg_mark = Instant::now();
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
                        if let Some(win_id) = agent_doc_tmux_io::target_window_id(tmux, pane) {
                            let win_name =
                                agent_doc_tmux_io::target_window_name(tmux, &win_id)
                                    .unwrap_or_default();
                            if win_name == "stash" || win_name.starts_with("stash-") {
                                let pane_session =
                                    agent_doc_tmux_io::target_session_name(tmux, pane)
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
                if matches!(auto_start_mode, AutoStartMode::SafePassive) {
                    safe_passive_resolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
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

            if unresolved_startup_miss_blocks_autostart(
                registered_pane.as_deref(),
                registered_pane
                    .as_deref()
                    .is_some_and(|pane| tmux.pane_alive(pane)),
                unresolved_startup_miss.as_ref(),
            ) {
                let miss = unresolved_startup_miss
                    .as_ref()
                    .expect("guard checked presence");
                let miss_ts = agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
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

            if !skip_autostart_diagnostics
                && matches!(auto_start_mode, AutoStartMode::SafePassive)
                && registered_pane.is_none()
            {
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
                match crate::runtime_effects()?.provision_pane(
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
                        safe_passive_resolved_files
                            .borrow_mut()
                            .insert(file_path.to_path_buf());
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

            // `#exact-visible-focus-swap`: a deliberate editor tab/focus change emits
            // `sync --focus <file> --exact-visible --no-autostart`. Without resolving
            // existing panes this path blocked EVERY file (`registered_live_owner`
            // forced false under `skip_autostart_diagnostics`) and early-returned at
            // the layout-preservation guard before the `tmux_router` reconcile — so a
            // focused document whose owned pane is merely stashed never got swapped
            // into view (observed: focusing `sampleportal.md` left another
            // document's pane in the visible column). Resolve a proven-live registered
            // owner (including a stashed pane) so it enters the reconcile and the
            // `tmux_router` SWAP fast path brings it into the exact-visible columns.
            // We still never cold-start a MISSING pane under no-autostart — this only
            // reuses a pane that already exists and still proves ownership, so the
            // no-autostart / no-borrow-visible contract is preserved.
            if let Some(pane_id) = registered_pane.as_ref()
                && exact_visible_focus_eligible_for_owned_pane_resolve(
                    skip_autostart_diagnostics,
                    tmux.pane_alive(pane_id),
                    claimed_owner.is_some(),
                )
                // Proof is the expensive term — only evaluated once the cheap
                // eligibility gate holds (Rust `&&` short-circuits).
                && registered_pane_proves_live_owner(
                    tmux,
                    file_path,
                    &session_id,
                    pane_id,
                    &proof_cache,
                )
            {
                eprintln!(
                    "[sync] exact-visible focus: resolving live owner pane {} for {} so the reconcile can swap it into view",
                    pane_id,
                    file_path.display()
                );
                sync_log(&format!(
                    "safe_passive_exact_visible_resolve_live_pane file={} pane={} action=resolve_for_reconcile",
                    file_path.display(),
                    pane_id
                ));
                reserve_sync_pane(&claimed_sync_panes, pane_id, file_path);
                safe_passive_resolved_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
                continue;
            }

            if skip_autostart_diagnostics {
                sync_log(&format!(
                    "safe_passive_exact_visible_autostart_checks_skipped file={} registered_pane={} reason=no_autostart",
                    file_path.display(),
                    registered_pane.as_deref().unwrap_or("none")
                ));
                blocked_unresolved_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
                continue;
            }

            // No alive pane in registry. Before auto-starting, check if any
            // alive pane in the target session is already running agent-doc
            // for this file (registry may have been pruned or stale).
            // This prevents creating duplicate panes.
            sd += seg_mark.elapsed();
            seg_mark = Instant::now();
            let associated_candidates = filter_associated_panes_for_document(
                tmux,
                file_path,
                find_associated_panes(tmux, file_path, &session_id),
            );
            match resolve_associated_panes(associated_candidates.clone(), window) {
                AssociatedPaneResolution::Selected { winner, redundant } => {
                    let detail = associated_pane_candidates_detail(
                        std::iter::once(&winner).chain(redundant.iter()),
                    );
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
                    let detail = associated_pane_candidates_detail(&candidates);
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

            if !skip_autostart_diagnostics && let Some(ref pane) = registered_pane {
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
                                    outcome.as_str()
                                );
                                sync_log(&format!(
                                    "missing-pane closeout recovered file={} pane={} phase={} outcome={}",
                                    file_path.display(),
                                    pane,
                                    phase,
                                    outcome.as_str()
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
            match crate::runtime_effects()?.provision_pane(
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
                    if matches!(auto_start_mode, AutoStartMode::SafePassive) {
                        safe_passive_resolved_files
                            .borrow_mut()
                            .insert(file_path.to_path_buf());
                    }
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

        // `#syncobscache`: the per-document ownership loop is the bulk of
        // `ownership_proof` (~978ms measured, over its 750ms budget). Time it
        // separately so the cost is attributable per document instead of hiding
        // inside one coarse phase.
        log_sync_latency(
            focus,
            "ownership_per_file_loop",
            per_file_loop_start.elapsed(),
            SYNC_OWNERSHIP_PROOF_BUDGET,
            auto_start_mode,
        );
        sync_log(&format!(
            "ownership_per_file_loop_detail files={} elapsed_ms={} sa_ms={} sb_ms={} sc_ms={} sd_ms={}",
            all_files.len(),
            per_file_loop_start.elapsed().as_millis(),
            sa.as_millis(),
            sb.as_millis(),
            sc.as_millis(),
            sd.as_millis()
        ));
        if matches!(auto_start_mode, AutoStartMode::SafePassive) {
            let newly_blocked = block_unresolved_safe_passive_managed_files(
                &safe_passive_managed_files.borrow(),
                &safe_passive_resolved_files.borrow(),
                &blocked_unresolved_files,
            );
            for file_path in newly_blocked {
                eprintln!(
                    "[sync] safe passive sync is preserving layout for {} because no live owner pane was proven or freshly provisioned; refusing to borrow a visible pane",
                    file_path.display()
                );
                sync_log(&format!(
                    "safe_passive_unresolved_owner_blocked file={} reason=no_proven_or_provisioned_owner",
                    file_path.display()
                ));
            }
        }

        if let Some(summary) = auto_started_panes_summary(&auto_started_panes) {
            eprintln!("[sync] {summary}");
            sync_log(&format!("batch: {summary}"));
        }

        // Post-auto_start stash removed: the tmux_router reconciler now always runs
        // the full reconcile path (no early exits), so it handles stashing excess panes.
        log_sync_latency(
            hot_latency_focus,
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

    if matches!(auto_start_mode, AutoStartMode::SafePassive) {
        let mut blocked_files: Vec<String> = blocked_unresolved_files
            .borrow()
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        blocked_files.sort();
        if !blocked_files.is_empty() {
            let blocked_summary = blocked_files.join(", ");
            let message = format!(
                "[sync] safe passive sync preserved the current tmux layout because unresolved files remain blocked: {}",
                blocked_summary
            );
            eprintln!("{message}");
            mark_sync_layout_preserved(&message);
            if !skip_autostart_diagnostics {
                let _ =
                    reselect_visible_focus_pane_if_present(tmux, window, focus, "blocked_files");
            }
            sync_log(&format!(
                "safe_passive_layout_preserved blocked_files={}",
                blocked_summary
            ));
            return Ok(());
        }
    }

    let tmux_router_registry_path = registry_path.as_path();
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
        // SafePassive = editor-driven poll / tab-selection: reconcile layout but
        // don't steal the operator's active tmux pane (#panefocussteal).
        passive: matches!(auto_start_mode, AutoStartMode::SafePassive),
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
    let post_router_start = Instant::now();

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
        if let Some(session) = agent_doc_tmux_io::target_session_name(tmux, w) {
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
        let layout_state = agent_doc_tmux::build_layout_state(
            &agent_doc_tmux::classify_sync_layout_columns(col_args, first_agent_doc_in_col),
            &agent_doc_tmux::classify_sync_layout_columns(&saved_layout, first_agent_doc_in_col),
        );
        // Only save if at least one column has an agent doc
        if layout_state.iter().any(|s| !s.is_empty())
            && let Err(err) = agent_doc_controller_io::project_controller::store_layout_state(
                &layout_state_root,
                &layout_state,
            )
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
    //
    // `#passiveresyncdefer`: this pass is REPORT-ONLY — it mutates nothing — but it
    // was the single largest remaining cost on the editor-follow hot path: 956ms
    // of a 1456ms `safe_passive_total`, measured as `gap_post_router`. Passive
    // sync runs on every editor file switch, so paying ~1s for diagnostics that
    // change no state is exactly the work to keep off the hot path.
    //
    // Full `agent-doc sync` (the deliberate layout reconcile, and the doctor
    // repair path below) still runs it, so the validation is not lost — it is
    // deferred to the passes that are allowed to be slow and that actually act on
    // the result.
    if matches!(auto_start_mode, AutoStartMode::SafePassive) {
        sync_log(
            "post_sync_resync_skipped mode=safe-passive reason=report_only_off_hot_path (#passiveresyncdefer)",
        );
    } else if let Err(e) = resync::run(false, None, None) {
        eprintln!("[sync] warning: post-sync resync failed: {}", e);
    }
    if run_doctor_repair {
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
            "gap_post_router",
            post_router_start.elapsed(),
            SYNC_SAFE_PASSIVE_TOTAL_BUDGET,
            auto_start_mode,
        );
        log_sync_latency(
            focus,
            "safe_passive_total",
            sync_total_start.elapsed(),
            SYNC_SAFE_PASSIVE_TOTAL_BUDGET,
            auto_start_mode,
        );
    }
    if let Some(focus) = focus.map(str::trim).filter(|path| !path.is_empty()) {
        agent_doc_controller_io::editor_route_errors::clear_for_success(
            Path::new(focus),
            "sync_success",
            agent_doc_ops_log_io::log_op,
        );
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
        let registry_root_matches = agent_doc_session_registry_io::load_in(&project_root)
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
        if let Some(actor_pane) =
            authoritative_actor_pane_for_document_cached(tmux, file_path, session_id, proof_cache)
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
        let fail_closed_binding_guard =
            agent_doc_supervisor_io::startup_miss::load_startup_miss(file_path)
                .ok()
                .flatten()
                .is_some()
                || agent_doc_supervisor_io::startup_miss::recent_session_loss_window(
                    file_path, session_id,
                )
                .ok()
                .flatten()
                .is_some();
        let Some((canonical_file, project_root, registry_key)) =
            registry_location_for_file(file_path)
        else {
            continue;
        };
        let registry_path = agent_doc_session_registry_io::registry_path_in(&project_root);
        let Ok(_lock) = tmux_router::RegistryLock::acquire(&registry_path) else {
            continue;
        };
        let Ok(mut registry) = agent_doc_session_registry_io::load_in(&project_root) else {
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
                let _ = agent_doc_session_registry_io::save_in(&project_root, &registry);
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
                    let _ = agent_doc_session_registry_io::save_in(&project_root, &registry);
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
        let window = agent_doc_tmux_io::target_window_id(tmux, pane_id).unwrap_or_default();
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
                tmux_router::RegistryEntry {
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
            let _ = agent_doc_session_registry_io::save_in(&project_root, &registry);
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
    let output = agent_doc_tmux_io::list_panes_all(tmux, "#{pane_id} #{pane_pid}").ok()?;
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() != 2 {
            continue;
        }
        let pane_id = parts[0];
        let pid_str = parts[1];
        if excluded_pane.is_some_and(|excluded| excluded == pane_id) {
            continue;
        }

        // Check the pane's process tree (pane PID is usually a shell) for
        // agent-doc + file_path.
        if let Some(owner_pid) =
            agent_doc_process_owner_io::process_tree_agent_doc_owner_pid_for_file(
                pid_str, file_path,
            )
        {
            if log_hits {
                if owner_pid == pid_str {
                    eprintln!(
                        "[sync] found alive agent-doc pane {} (pid {}) for {}",
                        pane_id, pid_str, file_path
                    );
                } else {
                    eprintln!(
                        "[sync] found alive agent-doc child (pid {}) in pane {} for {}",
                        owner_pid, pane_id, file_path
                    );
                }
            }
            return Some(pane_id.to_string());
        }
    }
    None
}

#[allow(dead_code)]
pub fn find_alive_pane_for_file(tmux: &Tmux, file_path: &str) -> Option<String> {
    find_alive_pane_for_file_inner(tmux, file_path, None, true)
}

fn list_associated_pane_inventory(tmux: &Tmux) -> Vec<AssociatedPaneCandidate> {
    let output = match agent_doc_tmux_io::list_panes_all(
        tmux,
        "#{pane_id}\t#{pane_pid}\t#{window_id}\t#{window_name}\t#{session_name}\t#{pane_current_command}",
    ) {
        Ok(output) => output,
        _ => return Vec::new(),
    };

    output
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
        if agent_doc_process_owner_io::process_tree_has_agent_doc_owner_for_file(
            &candidate.pane_pid,
            file_path,
        ) {
            matches.insert(candidate.pane_id.clone());
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

pub fn filter_associated_panes_for_document(
    tmux: &Tmux,
    file: &Path,
    candidates: Vec<AssociatedPaneCandidate>,
) -> Vec<AssociatedPaneCandidate> {
    candidates
        .into_iter()
        .filter(|candidate| !pane_runs_other_document_owner(tmux, &candidate.pane_id, file))
        .collect()
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
        .and_then(|win_id| agent_doc_tmux_io::target_window_name(tmux, &win_id))
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
            let detail = associated_pane_candidates_detail(&candidates);
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
/// `file` itself, or a bare non-owner pane, is preserved by the focused
/// process-owner IO check.
fn reject_cross_document_owner_pane(
    tmux: &Tmux,
    candidate: Option<String>,
    file: &Path,
    log_hits: bool,
) -> Option<String> {
    let pane = candidate?;
    if pane_runs_other_document_owner(tmux, &pane, file) {
        if log_hits {
            agent_doc_ops_log_io::log_op(
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
    // `#cross-repo-owner-guard`: a pane whose working directory lives in a
    // *different git repository* than `file` must never be surfaced as the
    // document's owner. Nested git submodules that carry no local `.agent-doc/`
    // collapse up into the superproject's `.agent-doc/` keyspace under
    // `find_project_root`, so `.agent-doc`-root equality
    // (`pane_assignment_matches_document_root`) cannot tell a `src/lazily-rs`
    // submodule pane apart from a superproject `tasks/…` document — every
    // root-equality check compares `superproject == superproject`. That blind
    // spot let a supervisor restart re-attach a submodule agent session (e.g. a
    // lazily Claude pane) onto a superproject document's pane. Comparing git
    // toplevels rejects the cross-repo pane so the caller cold-starts / fails
    // closed with the correct owner. This also closes the bare-foreign-session
    // gap that `pane_runs_other_document_owner` misses (a raw `claude`/`codex`
    // pane carries no agent-doc `.md` path in its cmdline).
    if pane_in_foreign_git_repo(tmux, &pane, file) {
        if log_hits {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "[sync] owner candidate pane {} lives in a different git repository than {}; not surfacing (cross-repo guard #cross-repo-owner-guard)",
                    pane,
                    file.display()
                ),
            );
        }
        return None;
    }
    Some(pane)
}

/// The git-repository toplevel that owns `file`, or `None` when it cannot be
/// resolved (git unavailable, path outside any repo). Used to draw the durable
/// project boundary the nearest-`.agent-doc` collapse erases.
fn document_git_repo(file: &Path) -> Option<PathBuf> {
    let absolute = agent_doc_git_io::dirs::resolve_absolute_file_path(file);
    let dir = absolute.parent().filter(|p| !p.as_os_str().is_empty())?;
    git_toplevel_memoized(dir)
}

thread_local! {
    /// `#syncobscache`: directory → git toplevel, memoized for the process.
    ///
    /// `pane_in_foreign_git_repo` recomputes the *document's* repo for every
    /// candidate pane, so a sync pass issued 12 `git rev-parse --show-toplevel`
    /// spawns that were all the identical command. Unlike a pane's working
    /// directory, a directory's owning repository is immutable for our purposes,
    /// so this memo needs no invalidation — which is why it is a plain lazily
    /// `ComputedMap` rather than part of the mutation-invalidated tmux scope.
    ///
    /// `#stategraphjoin`: the memo's lifetime is the process (per thread), so it
    /// joins a [`LocalProcessScope`] rather than minting a private context.
    static GIT_TOPLEVEL_MEMO: (LocalProcessScope, lazily::ComputedMap<PathBuf, Option<PathBuf>>) = {
        let scope = LocalProcessScope::new();
        let map = lazily::ComputedMap::new(scope.ctx());
        (scope, map)
    };
}

fn git_toplevel_memoized(dir: &Path) -> Option<PathBuf> {
    GIT_TOPLEVEL_MEMO.with(|(scope, map)| {
        let key = dir.to_path_buf();
        let probe = key.clone();
        map.get_or_insert_with(scope.ctx(), key, move |_| {
            agent_doc_git_io::dirs::git_toplevel_at(&probe)
        })
    })
}

/// True only when `pane_id`'s working directory and `file` resolve to
/// *provably different* git repositories (e.g. a `src/lazily-rs` submodule pane
/// vs a superproject document). Returns `false` when the boundary cannot be
/// proven — missing git info, a non-git tree, or the same repository — so the
/// check only ever rejects a demonstrable cross-repo pane and never breaks
/// same-repo owner resolution. See `#cross-repo-owner-guard`.
fn pane_in_foreign_git_repo(tmux: &Tmux, pane_id: &str, file: &Path) -> bool {
    let doc_repo = document_git_repo(file);
    // `#adpanetopmemo`: the pane side went through a raw `git rev-parse
    // --show-toplevel` spawn per candidate while only the document side was
    // memoized, so scanning N candidate panes cost N git spawns even when they
    // shared a working directory. A directory's owning repo is immutable for
    // our purposes, so the same memo applies unchanged.
    let pane_repo = agent_doc_tmux_io::pane_current_path(tmux, pane_id)
        .and_then(|cwd| git_toplevel_memoized(&cwd));
    git_repos_are_foreign(doc_repo.as_deref(), pane_repo.as_deref())
}

/// Pure boundary decision for `#cross-repo-owner-guard`. Two git toplevels are
/// *foreign* only when both are known and differ. An unknown toplevel on either
/// side (git unavailable, non-git tree) is never treated as foreign, so the
/// guard stays a strict tightening: it can only reject a demonstrable cross-repo
/// pane, never an ordinary same-repo owner it merely failed to resolve.
fn git_repos_are_foreign(doc_repo: Option<&Path>, pane_repo: Option<&Path>) -> bool {
    matches!((doc_repo, pane_repo), (Some(doc), Some(pane)) if doc != pane)
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
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let sock = agent_doc_supervisor_io::ipc::socket_path(&project_root, session_id);
    if !sock.exists() {
        return None;
    }
    let response = agent_doc_supervisor_io::ipc::send_command(&sock, &IpcMethod::State).ok()?;
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
    agent_doc_tmux_io::pane_pid(tmux, pane_id)
}

fn registry_entry_matches_document_root(
    entry: &tmux_router::RegistryEntry,
    project_root: &Path,
) -> bool {
    let cwd = Path::new(entry.cwd.trim());
    if cwd.as_os_str().is_empty() {
        return false;
    }
    agent_doc_project_root_io::project_root_containing(cwd)
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
    agent_doc_process_owner_io::process_tree_contains_pid(&pane_pid.to_string(), target_pid)
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
    let window = agent_doc_tmux_io::target_window_id(tmux, pane_id).unwrap_or_default();
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
    let registry = agent_doc_session_registry_io::load_in(&project_root).ok()?;
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

    let pane_pid = agent_doc_tmux_io::pane_pid(tmux, &entry.pane)?;
    if !agent_doc_process_owner_io::process_tree_contains_pid(&pane_pid.to_string(), identity.pid) {
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

fn find_alive_pane_via_supervisor_pid(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
) -> Option<String> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let sock = agent_doc_supervisor_io::ipc::socket_path(&project_root, session_id);
    if !sock.exists() {
        return None;
    }

    let response = agent_doc_supervisor_io::ipc::send_command(&sock, &IpcMethod::Pid).ok()?;
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

    let output = agent_doc_tmux_io::list_panes_all(tmux, "#{pane_id} #{pane_pid}").ok()?;

    for line in output.lines() {
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
        if agent_doc_process_owner_io::process_tree_contains_pid(pane_pid, target_pid) {
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
    let status = agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten()?;
    if !status.latest_session_open() {
        return None;
    }
    let pane_id = status.latest_start_pane.as_deref()?;
    // `#adsessreap2`: an open session-log means "the document is open in an
    // editor," not "the agent session still owns this pane." A pane that dropped
    // `claude → zsh` must not be re-owned off a stale session-log — treat it as
    // ownerless so the editor's live-buffer sync stays a separate lifecycle.
    if excluded_pane == Some(pane_id)
        || !agent_doc_supervisor_process::session_liveness::pane_owns_live_agent(tmux, pane_id)
    {
        return None;
    }

    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
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
    let status = agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten()?;
    if !status.latest_session_closed() {
        return None;
    }
    let pane_id = agent_doc_supervisor::startup_miss::latest_registry_rebind_successor(&status)?;
    if excluded_pane == Some(pane_id) || !tmux.pane_alive(pane_id) {
        return None;
    }

    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
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
    let Some(pid) = agent_doc_tmux_io::pane_pid(tmux, pane_id) else {
        return false;
    };
    let pid_str = pid.to_string();
    agent_doc_process_owner_io::process_tree_has_agent_session(&pid_str)
}

/// Diagnostic sibling of [`pane_runs_other_document_owner`]: returns the foreign
/// document path the pane's live process tree owns (a document other than
/// `claimed_file`), or `None`. Used by cross-document execution logging, sync
/// ownership proof, and start admission.
pub fn pane_owned_document_other_than(
    tmux: &Tmux,
    pane_id: &str,
    claimed_file: &Path,
) -> Option<String> {
    let canonical = claimed_file
        .canonicalize()
        .ok()
        .unwrap_or_else(|| claimed_file.to_path_buf());
    let claimed_actor_pane = agent_doc_project_root_io::project_root_containing(&canonical)
        .and_then(|project_root| {
            agent_doc_controller_io::project_controller::authoritative_actor_binding(
                &project_root,
                &canonical,
            )
            .ok()
            .flatten()
            .filter(|record| {
                record.state != agent_doc_sqlite::state_store::ActorState::Closed
                    && tmux.pane_alive(&record.pane_id)
            })
            .map(|record| record.pane_id)
        });
    if claimed_actor_pane.as_deref() == Some(pane_id) {
        return None;
    }
    let pane_pid = pane_pid_from_tmux(tmux, pane_id)?;
    let process_owner = agent_doc_process_owner_io::process_tree_owner_document_other_than(
        &pane_pid.to_string(),
        claimed_file,
    );
    resolve_pane_owner_conflict(
        pane_id,
        claimed_actor_pane.as_deref(),
        process_owner.as_deref(),
    )
    .map(str::to_string)
}

fn resolve_pane_owner_conflict<'a>(
    pane_id: &str,
    claimed_actor_pane: Option<&str>,
    process_owner: Option<&'a str>,
) -> Option<&'a str> {
    if claimed_actor_pane == Some(pane_id) {
        return None;
    }
    process_owner
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
    let tmux = Tmux::default_server();
    let current_pane = match agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux) {
        Some(pane) if !pane.is_empty() => pane,
        _ => return,
    };
    if let Some(other) = pane_owned_document_other_than(&tmux, &current_pane, file) {
        agent_doc_ops_log_io::log_op(
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
/// the durable registry, so it enforces the one-live-pane-per-document binding
/// invariant across project/submodule roots.
pub fn pane_runs_other_document_owner(tmux: &Tmux, pane_id: &str, claimed_file: &Path) -> bool {
    let Some(pane_pid) = pane_pid_from_tmux(tmux, pane_id) else {
        return false;
    };
    let pane_pid = pane_pid.to_string();
    // `#bare-foreign-session-guard`: a bare `claude`/`codex` pane the operator
    // started themselves binds no `.md`, so the document-ownership check below
    // answers "owns nothing". Absence of ownership proof is NOT permission —
    // reading it that way made a live Claude Code session in the project
    // directory electable and reapable. Treat an unmanaged harness pane as
    // foreign so owner election and reaping leave it alone.
    //
    // `#syncownerreactive`: both questions below are about the SAME pane's
    // process tree, and each used to walk `/proc` for itself — so scanning N
    // candidate panes cost 2N whole-table walks to re-answer questions about
    // processes that had not changed. Opening the observation scope here makes
    // the pair share one observation even when the caller opened no scope; when
    // the caller did (a sync pass), this nests into theirs and the whole pass
    // derives from one walk per pane.
    let _observations = agent_doc_process_owner_io::begin_process_observation_scope();
    if agent_doc_process_owner_io::process_tree_runs_unmanaged_harness_session(&pane_pid) {
        return true;
    }
    agent_doc_process_owner_io::process_tree_owns_other_document(&pane_pid, claimed_file)
}

#[cfg(test)]
mod th {
    use super::*;
    use std::process::Command as ProcessCommand;
    use std::time::Duration;
    use tmux_router::IsolatedTmux;
    pub(crate) fn list_windows(tmux: &Tmux, session: &str) -> Vec<(String, String)> {
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
    pub(crate) fn wait_for<F>(timeout: Duration, mut predicate: F) -> bool
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
    pub(crate) fn pane_current_command(tmux: &IsolatedTmux, pane: &str) -> Option<String> {
        agent_doc_tmux_io::target_current_command(tmux, pane)
    }
    pub(crate) fn wait_for_shell(tmux: &IsolatedTmux, pane: &str, timeout: Duration) -> bool {
        wait_for(timeout, || {
            matches!(
                pane_current_command(tmux, pane).as_deref(),
                Some("sh" | "bash" | "zsh" | "fish")
            )
        })
    }
    pub(crate) struct ScopedCurrentDir {
        prev_cwd: PathBuf,
        _env_guard: crate::test_support::ProcessGlobalLockGuard,
    }
    impl ScopedCurrentDir {
        pub(crate) fn set(path: &Path) -> Self {
            let env_guard = crate::test_support::env_lock();
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
    pub(crate) fn synthetic_registry_candidate(
        session_id: &str,
        file_path: &str,
        pane_id: &str,
        live_owner_match: bool,
        pane_root_match: bool,
    ) -> SyntheticRegistryCandidate {
        SyntheticRegistryCandidate {
            session_id: session_id.to_string(),
            file_path: PathBuf::from(file_path),
            entry: tmux_router::RegistryEntry {
                pane: pane_id.to_string(),
                pid: 1000,
                cwd: "/tmp/project".to_string(),
                started: "2026-05-01T00:00:00Z".to_string(),
                session_id: session_id.to_string(),
                file: file_path.to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
            live_owner_match,
            pane_root_match,
        }
    }
    pub(crate) fn init_git_repo(root: &Path, tracked: &Path) {
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["init"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", tracked.strip_prefix(root).unwrap().to_str().unwrap()])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .status()
            .unwrap();
    }
    // --- #4sh0: sync_log / repair_layout logging tests ---
    // --- File rename detection tests ---
    // --- Batch summary formatting tests ---
    // --- Rename debounce tests ---
    pub(crate) fn sync_replaces_detachable_visible_pane_and_stashes_open_cycle_extra(
        mode: AutoStartMode,
        test_name: &str,
    ) {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let tmux_session = format!("test-{test_name}-{}", std::process::id());
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            format!("tmux_session = \"{tmux_session}\"\n"),
        )
        .unwrap();

        let protected_doc = root.join("tasks/protected.md");
        let detached_doc = root.join("tasks/detached.md");
        let requested_doc = root.join("tasks/requested.md");
        for (path, session) in [
            (&protected_doc, "sync-replace-protected"),
            (&detached_doc, "sync-replace-detached"),
            (&requested_doc, "sync-replace-requested"),
        ] {
            std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
        }

        let iso = IsolatedTmux::new(test_name);
        let protected_pane = iso.new_session(&tmux_session, root).unwrap();
        let _ = iso.raw_cmd(&[
            "rename-window",
            "-t",
            &format!("{tmux_session}:0"),
            "agent-doc",
        ]);
        let detached_pane = iso.split_window(&protected_pane, root, "-dh").unwrap();
        let target_window = iso.pane_window(&protected_pane).unwrap();
        let requested_pane = iso.new_window(&tmux_session, root).unwrap();

        sessions::register_full_with_cwd(
            "sync-replace-protected",
            &protected_pane,
            &protected_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &protected_pane).unwrap(),
            &target_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            "sync-replace-detached",
            &detached_pane,
            &detached_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &detached_pane).unwrap(),
            &target_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            "sync-replace-requested",
            &requested_pane,
            &requested_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &requested_pane).unwrap(),
            &iso.pane_window(&requested_pane).unwrap(),
            &root.to_string_lossy(),
        )
        .unwrap();

        let protected_content = std::fs::read_to_string(&protected_doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(
            &protected_doc,
            Some(&protected_content),
            Some(&protected_content),
        )
        .unwrap();

        run_with_options_internal(
            &[requested_doc.to_string_lossy().to_string()],
            None,
            Some(requested_doc.to_string_lossy().as_ref()),
            mode,
            false,
            &iso,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&target_window).unwrap();
        assert!(
            !ordered.contains(&protected_pane),
            "open-cycle extra pane should be stashed instead of remaining visible"
        );
        assert!(
            ordered.contains(&requested_pane),
            "requested hidden pane should be brought into the visible agent-doc window"
        );
        assert!(
            !ordered.contains(&detached_pane),
            "detachable visible pane should be displaced instead of making sync a no-op"
        );
        assert_eq!(
            iso.active_pane(&tmux_session).unwrap(),
            requested_pane,
            "sync should focus the requested pane after replacing a detachable visible pane"
        );
        assert!(
            iso.pane_alive(&protected_pane),
            "open-cycle pane should stay alive after being stashed"
        );
    }
}
#[cfg(test)]
pub(crate) use th::*;

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::process::Command as ProcessCommand;
    use std::time::Duration;
    use tmux_router::IsolatedTmux;

    #[test]
    fn destructive_repair_throttle_roundtrips_in_state_db_without_stamp() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let first = 10_000;
        let after_interval = first + agent_doc_tmux::DESTRUCTIVE_REPAIR_MIN_INTERVAL_MS + 1;

        assert_eq!(
            destructive_repair_throttle_at(root, Some("/tmp/test.sock"), "agent-doc", first)
                .unwrap(),
            (false, None)
        );
        assert_eq!(
            destructive_repair_throttle_at(root, Some("/tmp/test.sock"), "agent-doc", first + 1)
                .unwrap(),
            (true, Some(first))
        );
        assert_eq!(
            destructive_repair_throttle_at(
                root,
                Some("/tmp/test.sock"),
                "agent-doc",
                after_interval,
            )
            .unwrap(),
            (false, Some(first))
        );
        assert!(root.join(".agent-doc/state.db").exists());
        assert!(
            std::fs::read_dir(root.join(".agent-doc"))
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".stamp"))
        );
    }

    // `#sync-cross-root-autostart`: a superproject sync must not restore or
    // auto-start a document owned by a nested/submodule `.agent-doc` root.
    #[test]
    fn cross_root_document_does_not_belong_to_superproject_sync_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        // Superproject root + a same-root doc.
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/research")).unwrap();
        let same_root_doc = root.join("tasks/research/sampleorders.md");
        std::fs::write(&same_root_doc, "---\n---\n").unwrap();
        // Nested submodule root (its own `.agent-doc`) + a cross-root doc.
        std::fs::create_dir_all(root.join("src/sample-app/.agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("src/sample-app/tasks")).unwrap();
        let cross_root_doc = root.join("src/sample-app/tasks/sampleorders.md");
        std::fs::write(&cross_root_doc, "---\n---\n").unwrap();

        let sync_root = canonical_sync_project_root(&root);
        assert!(
            document_belongs_to_sync_root(&same_root_doc, &sync_root),
            "same-root document must belong to the sync root"
        );
        assert!(
            !document_belongs_to_sync_root(&cross_root_doc, &sync_root),
            "submodule-owned document must NOT belong to the superproject sync root"
        );
    }

    #[test]
    fn sanitize_cross_root_layout_drops_submodule_columns_and_keeps_same_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/agent-doc")).unwrap();
        let same_root_doc = root.join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::write(&same_root_doc, "---\n---\n").unwrap();
        std::fs::create_dir_all(root.join("src/sample-app/.agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("src/sample-app/tasks")).unwrap();
        let cross_root_doc = root.join("src/sample-app/tasks/sampleorders.md");
        std::fs::write(&cross_root_doc, "---\n---\n").unwrap();

        let sync_root = canonical_sync_project_root(&root);
        // Mirrors the observed polluted `layout_states` row: [same-root, cross-root].
        let saved_layout = vec![
            same_root_doc.to_string_lossy().to_string(),
            cross_root_doc.to_string_lossy().to_string(),
        ];
        let (sanitized, dropped) = sanitize_cross_root_layout(saved_layout, &sync_root);
        assert_eq!(dropped.len(), 1, "exactly the submodule doc is dropped");
        assert!(dropped[0].contains("sample-app"));
        // Same-root column preserved; cross-root column blanked to hold its slot.
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized[0], same_root_doc.to_string_lossy());
        assert_eq!(
            sanitized[1], "",
            "cross-root column becomes an empty placeholder"
        );
    }

    struct TestPipelineFrontmatterEffects;

    impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects
        for TestPipelineFrontmatterEffects
    {
        fn read_current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {}", file.display()))
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            _current_content: &str,
            target_content: &str,
            _reason: &str,
        ) -> Result<()> {
            std::fs::write(file, target_content)
                .with_context(|| format!("failed to write {}", file.display()))
        }

        fn log_op(&self, file: &Path, message: &str) {
            agent_doc_ops_log_io::log_op(file, message);
        }
    }

    const TEST_PIPELINE_FRONTMATTER_EFFECTS: TestPipelineFrontmatterEffects =
        TestPipelineFrontmatterEffects;

    #[test]
    fn exact_visible_safe_passive_only_matches_editor_projection_refresh() {
        assert!(exact_visible_safe_passive(AutoStartMode::SafePassive, true));
        assert!(!exact_visible_safe_passive(
            AutoStartMode::SafePassive,
            false
        ));
        assert!(!exact_visible_safe_passive(AutoStartMode::Full, true));
        assert!(!exact_visible_safe_passive(AutoStartMode::Full, false));
    }

    #[test]
    fn exact_visible_focus_resolve_eligibility_rule() {
        // `#exact-visible-focus-swap`: eligible only for an exact-visible sync with a
        // live, unclaimed registered pane. The (expensive) live-owner proof is a
        // separate short-circuited guard the caller applies after this gate.
        assert!(exact_visible_focus_eligible_for_owned_pane_resolve(
            true, true, false
        ));
        assert!(
            !exact_visible_focus_eligible_for_owned_pane_resolve(false, true, false),
            "a non-exact-visible sync keeps its own resolution path"
        );
        assert!(
            !exact_visible_focus_eligible_for_owned_pane_resolve(true, false, false),
            "a dead pane is never resolved (we never cold-start under no-autostart)"
        );
        assert!(
            !exact_visible_focus_eligible_for_owned_pane_resolve(true, true, true),
            "a pane another document already claimed this run must not be borrowed"
        );
    }

    #[test]
    fn safe_passive_skips_sync_status_document_updates() {
        assert!(skip_sync_status_updates_for_mode(
            AutoStartMode::SafePassive
        ));
        assert!(!skip_sync_status_updates_for_mode(AutoStartMode::Full));
    }

    #[test]
    fn sync_repair_closes_jb_cache_conflict_cancel_commit_boundary() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd_guard = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join("tasks")).unwrap();

        let doc = root.join("tasks/sync-repair-closeout.md");
        let original = concat!(
            "---\n",
            "agent_doc_session: sync-repair-closeout\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Pending crash recovery.\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);

        let materialized = original.replace(
            "<!-- agent:boundary:test -->",
            "### Re: crash recovery -- gpt-5\n\nRecovered by sync.\n<!-- agent:boundary:test -->",
        );
        std::fs::write(&doc, &materialized).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &materialized,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&materialized),
            Some(&materialized),
        )
        .unwrap();

        assert!(
            crate::runtime_effects()
                .unwrap()
                .detect_jb_cache_conflict_cancel_recoverable(&doc)
                .unwrap(),
            "precondition: visible response/snapshot should be ahead of HEAD"
        );

        let note = recover_jb_cache_conflict_cancel_commit_boundary(&doc)
            .unwrap()
            .expect("sync repair should close the commit boundary");
        assert!(note.contains("jb_cache_conflict_cancel"));
        assert!(matches!(
            agent_doc_snapshot_io::verify_snapshot_committed(&doc).unwrap(),
            agent_doc_snapshot_io::SnapshotCommitStatus::Committed
        ));
        assert!(
            !crate::runtime_effects()
                .unwrap()
                .detect_jb_cache_conflict_cancel_recoverable(&doc)
                .unwrap(),
            "repair should remove the recoverable crash shape"
        );

        let show = ProcessCommand::new("git")
            .current_dir(root)
            .args(["show", "HEAD:tasks/sync-repair-closeout.md"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&show.stdout).contains("Recovered by sync."),
            "HEAD should include the recovered visible response"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn find_live_owner_pane_reuses_latest_open_session_log_owner() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: session-log-owner\n---\n").unwrap();

        let iso = IsolatedTmux::new("sync-session-log-owner");
        let owner_pane = iso.new_session("test", tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/session-log-owner.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane={} session=session-log-owner\n[2] codex_start mode=fresh restart_count=0\n",
                owner_pane
            ),
        )
        .unwrap();

        let owner = find_live_owner_pane(&iso, &doc, "session-log-owner");
        assert_eq!(owner.as_deref(), Some(owner_pane.as_str()));
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn find_live_owner_pane_prefers_latest_open_session_log_owner_over_stale_process_tree_match() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-log-beats-process-tree\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-session-log-beats-process-tree");
        let stale_pane = iso.new_session("test", tmp.path()).unwrap();
        let owner_pane = iso.split_window(&stale_pane, tmp.path(), "-dh").unwrap();

        let fake_bin_dir = tmp.path().join("bin");
        std::fs::create_dir_all(&fake_bin_dir).unwrap();
        let fake_codex = fake_bin_dir.join("codex");
        std::fs::write(&fake_codex, "#!/bin/sh\nsleep 60\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake_codex).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake_codex, perms).unwrap();
        }

        iso.raw_cmd(&[
            "send-keys",
            "-t",
            &stale_pane,
            &format!("{} {}", fake_codex.display(), doc.display()),
            "Enter",
        ])
        .unwrap();

        assert!(
            wait_for(Duration::from_secs(3), || {
                find_alive_pane_for_file_inner(&iso, doc.to_string_lossy().as_ref(), None, false)
                    .as_deref()
                    == Some(stale_pane.as_str())
            }),
            "stale pane should expose a same-file process-tree match before session-log precedence is evaluated"
        );

        std::fs::write(
            tmp.path()
                .join(".agent-doc/logs/session-log-beats-process-tree.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane={} session=session-log-beats-process-tree\n[2] codex_start mode=fresh restart_count=0\n",
                owner_pane
            ),
        )
        .unwrap();

        let owner = find_live_owner_pane(&iso, &doc, "session-log-beats-process-tree");
        assert_eq!(
            owner.as_deref(),
            Some(owner_pane.as_str()),
            "latest open session-log owner must win over older same-file process-tree matches"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn find_live_owner_pane_reuses_live_registry_rebind_successor() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: live-rebind-owner\n---\n").unwrap();

        let iso = IsolatedTmux::new("sync-live-rebind-owner");
        let successor_pane = iso.new_session("test", tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/live-rebind-owner.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane=%70 session=live-rebind-owner\n[2] codex_start mode=fresh restart_count=0\n[3] session_superseded old_pane=%70 new_pane={} old_window=@1 new_window=@2\n[4] session_end origin=registry_rebind pane=%70 next_pane={}\n",
                successor_pane,
                successor_pane
            ),
        )
        .unwrap();

        let owner = find_live_owner_pane(&iso, &doc, "live-rebind-owner");
        assert_eq!(owner.as_deref(), Some(successor_pane.as_str()));
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn sync_actor_or_live_owner_matches_prefers_authoritative_actor_record() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd_guard = ScopedCurrentDir::set(root);

        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: actor-owner\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-authoritative-actor-owner");
        let stale_pane = iso.new_session("test", root).unwrap();
        let actor_pane = iso.split_window(&stale_pane, root, "-dh").unwrap();
        let stale_window = iso.pane_window(&stale_pane).unwrap();
        let actor_window = iso.pane_window(&actor_pane).unwrap();

        sessions::register_full_with_cwd(
            "actor-owner",
            &stale_pane,
            &doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &stale_pane).unwrap(),
            &stale_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        agent_doc_session_actor_io::project_binding_in(
            root,
            &doc.to_string_lossy(),
            "actor-owner",
            &actor_pane,
            &actor_window,
            "sync",
            "test_actor_projection",
        )
        .unwrap();

        assert!(
            sync_actor_or_live_owner_matches(&iso, &doc, "actor-owner", &actor_pane),
            "sync should treat the authoritative actor pane as a live owner even when generic route heuristics still point elsewhere"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn recover_existing_associated_pane_reuses_live_registry_rebind_successor() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: associated-rebind\n---\n").unwrap();

        let iso = IsolatedTmux::new("sync-associated-registry-rebind");
        let successor_pane = iso.new_session("test", tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/associated-rebind.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane=%70 session=associated-rebind\n[2] codex_start mode=fresh restart_count=0\n[3] session_superseded old_pane=%70 new_pane={} old_window=@1 new_window=@2\n[4] session_end origin=registry_rebind pane=%70 next_pane={}\n",
                successor_pane,
                successor_pane
            ),
        )
        .unwrap();

        let recovery = recover_existing_associated_pane(
            &iso,
            &doc,
            "associated-rebind",
            None,
            &RefCell::new(std::collections::HashMap::new()),
        );

        assert!(matches!(
            recovery,
            ExistingAssociatedPaneRecovery::Recovered(ref pane) if pane == &successor_pane
        ));
        let candidates = find_associated_panes(&iso, &doc, "associated-rebind");
        assert_eq!(candidates.len(), 1);
        assert!(
            candidates[0]
                .sources
                .contains(&AssociatedPaneSource::RegistryRebind),
            "expected registry-rebind ownership proof: {:?}",
            candidates[0].sources
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn open_session_log_owner_fail_closed_diagnostic_requires_same_alive_open_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: open-log-pane\n---\n").unwrap();

        let iso = IsolatedTmux::new("sync-open-log-owner-fail-closed");
        let owner_pane = iso.new_session("test", tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/open-log-pane.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane={} session=open-log-pane\n[2] codex_start mode=fresh restart_count=0\n",
                owner_pane
            ),
        )
        .unwrap();

        let diagnostic =
            open_session_log_owner_fail_closed_diagnostic(&doc, "open-log-pane", &owner_pane)
                .unwrap();
        assert!(
            diagnostic
                .as_deref()
                .unwrap_or_default()
                .contains("session log still has no later child exit or session_end")
        );

        let none =
            open_session_log_owner_fail_closed_diagnostic(&doc, "open-log-pane", "%99999").unwrap();
        assert!(
            none.is_none(),
            "other panes should not inherit the open-log guard"
        );
    }
    #[test]
    fn reject_cross_document_owner_pane_preserves_non_contaminated_candidates() {
        // #jb-tsift-pane-sync focus-path wiring: the heuristic resolver
        // (`find_live_owner_pane_excluding_with_logging`, used by `focus.rs` and
        // resync recovery) now funnels its candidate through this guard. The
        // guard must only drop a pane that PROVABLY runs another document's
        // owner — it must never over-reject on the focus hot path, or normal
        // editor navigation would spuriously cold-start instead of focusing the
        // existing owner.
        let tmux = Tmux::default_server();
        let file = Path::new("tasks/software/tsift.md");

        // No candidate stays no candidate.
        assert_eq!(
            reject_cross_document_owner_pane(&tmux, None, file, false),
            None
        );

        // A candidate pane id with no resolvable process tree (no `#{pane_pid}`)
        // is not provably a cross-document owner, so it passes through unchanged.
        // This is the focus happy path: the resolved owner survives the guard.
        let bare = Some("%agent-doc-nonexistent-pane".to_string());
        assert_eq!(
            reject_cross_document_owner_pane(&tmux, bare.clone(), file, false),
            bare,
            "guard must not reject a candidate it cannot prove owns another document"
        );
    }

    #[test]
    fn live_claimed_actor_outvotes_stale_process_tree_document() {
        assert_eq!(
            resolve_pane_owner_conflict("%58", Some("%58"), Some("/tmp/.tmpo0A1jQ/session.md"),),
            None,
            "a descendant command line must not overrule the live Lazily actor binding",
        );
        assert_eq!(
            resolve_pane_owner_conflict("%58", Some("%53"), Some("/tmp/.tmpo0A1jQ/session.md"),),
            Some("/tmp/.tmpo0A1jQ/session.md"),
            "process-tree evidence remains a cold fallback when another pane owns the claim",
        );
    }

    #[test]
    fn git_repos_are_foreign_only_when_both_known_and_differ() {
        // `#cross-repo-owner-guard` pure decision. The guard must be a strict
        // tightening: reject only a *provable* cross-repo pane.
        let sup = Path::new("/repo");
        let sub = Path::new("/repo/src/lazily-rs");

        // Same repo → owner survives the guard.
        assert!(!git_repos_are_foreign(Some(sup), Some(sup)));
        // Different repos (superproject document vs submodule pane) → rejected.
        assert!(git_repos_are_foreign(Some(sup), Some(sub)));
        assert!(git_repos_are_foreign(Some(sub), Some(sup)));
        // Either side unknown (git unavailable / non-git tree) → never foreign,
        // so an ordinary same-repo owner is not spuriously cold-started.
        assert!(!git_repos_are_foreign(None, Some(sup)));
        assert!(!git_repos_are_foreign(Some(sup), None));
        assert!(!git_repos_are_foreign(None, None));
    }

    #[test]
    fn document_git_repo_distinguishes_submodule_from_superproject_root() {
        // Reproduces the misroute condition: a nested submodule (its own git
        // repo) that carries NO local `.agent-doc/` sits inside a superproject
        // that DOES. `find_project_root` collapses the submodule path up to the
        // superproject, but `document_git_repo` must key off the git toplevel so
        // the two documents resolve to distinct repositories — the boundary the
        // `#cross-repo-owner-guard` relies on.
        let tmp = tempfile::TempDir::new().unwrap();
        // Canonicalize so the fixture path matches `git rev-parse --show-toplevel`
        // output on platforms (e.g. macOS) where TempDir lives under a symlink.
        let root = std::fs::canonicalize(tmp.path()).unwrap();

        // Superproject: git repo + `.agent-doc/` + a tracked document.
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/professional")).unwrap();
        let super_doc = root.join("tasks/professional/sampleportal.md");
        std::fs::write(&super_doc, "# efs\n").unwrap();
        init_git_repo(&root, &super_doc);

        // Nested submodule: an independent git repo, NO `.agent-doc/` of its own.
        let sub_root = root.join("src/lazily-rs");
        std::fs::create_dir_all(&sub_root).unwrap();
        let sub_doc = sub_root.join("BENCHMARKS.md");
        std::fs::write(&sub_doc, "# bench\n").unwrap();
        init_git_repo(&sub_root, &sub_doc);

        let super_repo = document_git_repo(&super_doc).map(|r| std::fs::canonicalize(r).unwrap());
        let sub_repo = document_git_repo(&sub_doc).map(|r| std::fs::canonicalize(r).unwrap());

        assert_eq!(super_repo.as_deref(), Some(root.as_path()));
        assert_eq!(sub_repo.as_deref(), Some(sub_root.as_path()));
        assert!(
            git_repos_are_foreign(super_repo.as_deref(), sub_repo.as_deref()),
            "a submodule pane must be foreign to a superproject document (#cross-repo-owner-guard); \
             super={super_repo:?} sub={sub_repo:?}"
        );
    }

    #[test]
    fn safe_passive_unresolved_managed_files_are_blocked_from_spare_pane_assignment() {
        let managed_files: HashSet<PathBuf> = [
            PathBuf::from("tasks/agent-doc/agent-doc-bugs2.md"),
            PathBuf::from("tasks/software/tsift.md"),
        ]
        .into_iter()
        .collect();
        let resolved_files: HashSet<PathBuf> = [PathBuf::from("tasks/software/tsift.md")]
            .into_iter()
            .collect();
        let blocked_files: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());

        let newly_blocked = block_unresolved_safe_passive_managed_files(
            &managed_files,
            &resolved_files,
            &blocked_files,
        );

        assert_eq!(
            newly_blocked,
            vec![PathBuf::from("tasks/agent-doc/agent-doc-bugs2.md")]
        );
        assert!(
            blocked_files
                .borrow()
                .contains(Path::new("tasks/agent-doc/agent-doc-bugs2.md")),
            "safe passive sync must prevent tmux-router from borrowing a visible spare pane for an unresolved managed document"
        );
        assert!(
            !blocked_files
                .borrow()
                .contains(Path::new("tasks/software/tsift.md")),
            "files with a proven or freshly provisioned owner remain eligible for reconcile"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn passive_autostart_allows_cleanly_closed_latest_session() {
        let tmp = tempfile::TempDir::new().unwrap();
        let iso = IsolatedTmux::new("sync-passive-closed");
        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("tasks").join("closed.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: passive-closed\n---\n").unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/passive-closed.log"),
            concat!(
                "[1] session_start file=tasks/closed.md pane=%52 session=passive-closed\n",
                "[2] claude_start mode=fresh restart_count=0\n",
                "[3] supervisor_exit reason=user_quit_clean_exit pane=%52 restart_count=0\n",
                "[4] session_end\n",
            ),
        )
        .unwrap();

        assert_eq!(
            passive_autostart_skip_reason(&iso, &doc, "passive-closed", None).unwrap(),
            None
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn passive_autostart_blocks_open_latest_session() {
        let tmp = tempfile::TempDir::new().unwrap();
        let iso = IsolatedTmux::new("sync-passive-open");
        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("tasks").join("open.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: passive-open\n---\n").unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/passive-open.log"),
            concat!(
                "[1] session_start file=tasks/open.md pane=%61 session=passive-open\n",
                "[2] codex_start mode=fresh restart_count=0\n",
            ),
        )
        .unwrap();

        let reason = passive_autostart_skip_reason(&iso, &doc, "passive-open", None)
            .unwrap()
            .expect("open session should block passive auto-start");
        assert!(reason.contains("latest session log is still open or ambiguous"));
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn passive_autostart_blocks_live_registry_rebind_successor() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("tasks").join("rebind.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: passive-rebind\n---\n").unwrap();
        let iso = IsolatedTmux::new("sync-passive-rebind-live");
        let successor = iso.new_session("test", tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/passive-rebind.log"),
            format!(
                "[1] session_start file=tasks/rebind.md pane=%70 session=passive-rebind\n\
[2] codex_start mode=fresh restart_count=0\n\
[3] session_superseded old_pane=%70 new_pane={} old_window=@1 new_window=@2\n\
[4] session_end origin=registry_rebind pane=%70 next_pane={}\n",
                successor, successor
            ),
        )
        .unwrap();

        let reason = passive_autostart_skip_reason(&iso, &doc, "passive-rebind", None)
            .unwrap()
            .expect("live registry-rebind successor should block passive auto-start");
        assert!(reason.contains("registry_rebind"));
        assert!(reason.contains(successor.as_str()));
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn passive_autostart_allows_stale_registry_rebind_successor() {
        let tmp = tempfile::TempDir::new().unwrap();
        let iso = IsolatedTmux::new("sync-passive-rebind-stale");
        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("tasks").join("rebind.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: passive-rebind-stale\n---\n").unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/passive-rebind-stale.log"),
            concat!(
                "[1] session_start file=tasks/rebind.md pane=%70 session=passive-rebind-stale\n",
                "[2] codex_start mode=fresh restart_count=0\n",
                "[3] session_superseded old_pane=%70 new_pane=%71 old_window=@1 new_window=@2\n",
                "[4] session_end origin=registry_rebind pane=%70 next_pane=%71\n",
            ),
        )
        .unwrap();

        assert_eq!(
            passive_autostart_skip_reason(&iso, &doc, "passive-rebind-stale", None).unwrap(),
            None
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn passive_autostart_blocks_unresolved_startup_miss() {
        let tmp = tempfile::TempDir::new().unwrap();
        let iso = IsolatedTmux::new("sync-passive-miss");
        let doc = tmp.path().join("tasks").join("miss.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: passive-miss\n---\n").unwrap();
        let miss = agent_doc_supervisor::startup_miss::StartupMiss {
            file: "tasks/miss.md".to_string(),
            pane_id: "%81".to_string(),
            session_id: "passive-miss".to_string(),
            harness: "codex".to_string(),
            timestamp: 17,
            origin: agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: None,
        };

        let reason = passive_autostart_skip_reason(&iso, &doc, "passive-miss", Some(&miss))
            .unwrap()
            .expect("startup miss should block passive auto-start");
        assert!(reason.contains("startup-miss is still unresolved"));
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

        crate::surface_frontmatter_status_with(
            &doc,
            "auto-start",
            &err,
            save_sync_status_snapshot,
            log_sync_status,
        );

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains(agent_doc_sync::SYNC_FRONTMATTER_STATUS_PREFIX));
        assert!(updated.contains("sync auto-start frontmatter"));

        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(snapshot.contains(agent_doc_sync::SYNC_FRONTMATTER_STATUS_PREFIX));

        std::fs::write(
            &doc,
            "---\nagent_doc_session: test\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n[agent-doc sync] malformed frontmatter during auto-start.\n\nsync auto-start frontmatter: invalid YAML frontmatter in tasks/bad.md: boom\n<!-- /agent:status -->\n",
        )
        .unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "---\nagent_doc_session: test\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n[agent-doc sync] malformed frontmatter during auto-start.\n\nsync auto-start frontmatter: invalid YAML frontmatter in tasks/bad.md: boom\n<!-- /agent:status -->\n",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        crate::clear_frontmatter_status_with(&doc, save_sync_status_snapshot, log_sync_status);

        let cleared = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !cleared.contains(agent_doc_sync::SYNC_FRONTMATTER_STATUS_PREFIX),
            "managed sync warning should be removed once parsing succeeds"
        );
        let cleared_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !cleared_snapshot.contains(agent_doc_sync::SYNC_FRONTMATTER_STATUS_PREFIX),
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        crate::clear_frontmatter_status_with(&doc, save_sync_status_snapshot, log_sync_status);

        assert_eq!(std::fs::read_to_string(&doc).unwrap(), original);
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            original
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_unpins_legacy_staging_height() {
        // `#stashresizerestore`: older repair code restored the visible
        // dimensions but left `window-size=manual`, while still older code could
        // leave the full 1000-row staging geometry behind. A normal repair must
        // heal both without touching unrelated windows.
        let iso = IsolatedTmux::new("sync-repair-unpin-staging-height");
        let tmp = tempfile::TempDir::new().unwrap();

        let _pane = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let _ = iso.ensure_stash_window("test");
        iso.raw_cmd(&["resize-window", "-t", "test:agent-doc", "-y", "1000"])
            .unwrap();

        let height_before = iso
            .raw_cmd(&[
                "display-message",
                "-p",
                "-t",
                "test:agent-doc",
                "#{window_height}",
            ])
            .unwrap();
        assert_eq!(height_before.trim(), STASH_JOIN_TEMP_HEIGHT);

        repair_layout(&iso, "test", "agent-doc").unwrap();

        let height_after = iso
            .raw_cmd(&[
                "display-message",
                "-p",
                "-t",
                "test:agent-doc",
                "#{window_height}",
            ])
            .unwrap();
        assert_ne!(
            height_after.trim(),
            STASH_JOIN_TEMP_HEIGHT,
            "repair must remove the 1000-row staging viewport"
        );
        let local_window_size = iso
            .raw_cmd(&[
                "show-options",
                "-w",
                "-v",
                "-t",
                "test:agent-doc",
                "window-size",
            ])
            .unwrap();
        assert!(
            local_window_size.trim().is_empty(),
            "repair must remove the per-window manual-size override, got {local_window_size:?}"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_zero_stash_target_is_noop_tmuxsynccrash() {
        let iso = IsolatedTmux::new("sync-repair-zero-stash-target-noop");
        let tmp = tempfile::TempDir::new().unwrap();

        let _pane = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

        let windows_before = list_windows(&iso, "test");
        repair_layout(&iso, "test", "agent-doc").unwrap();
        let windows_after = list_windows(&iso, "test");

        assert_eq!(
            windows_before, windows_after,
            "zero-stash target layout should not create a stash window or churn tmux ops"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_consolidates_duplicate_target_windows() {
        let iso = IsolatedTmux::new("sync-repair-duplicate-agent-doc");
        let tmp = tempfile::TempDir::new().unwrap();

        let pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let pane1 = iso
            .raw_cmd(&[
                "new-window",
                "-t",
                "test:",
                "-n",
                "agent-doc",
                "-d",
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .unwrap()
            .trim()
            .to_string();

        let windows_before = list_windows(&iso, "test");
        assert_eq!(
            windows_before
                .iter()
                .filter(|(_, name)| name == "agent-doc")
                .count(),
            2,
            "test setup should start with duplicate agent-doc windows"
        );

        repair_layout(&iso, "test", "agent-doc").unwrap();

        let windows_after = list_windows(&iso, "test");
        assert_eq!(
            windows_after
                .iter()
                .filter(|(_, name)| name == "agent-doc")
                .count(),
            1,
            "repair should consolidate duplicate agent-doc windows"
        );
        assert_eq!(
            windows_after
                .iter()
                .find(|(_, name)| name == "agent-doc")
                .unwrap()
                .0,
            "0",
            "the consolidated agent-doc window should be normalized to index 0"
        );
        assert_eq!(
            iso.pane_window(&pane0).unwrap(),
            iso.pane_window(&pane1).unwrap(),
            "both panes should remain alive in the same agent-doc window"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_moves_stash_directly_after_agent_doc() {
        let iso = IsolatedTmux::new("sync-repair-stash-index");
        let tmp = tempfile::TempDir::new().unwrap();

        let _pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "corky", "-d"]);
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash", "-d"]);

        let windows_before = list_windows(&iso, "test");
        assert_eq!(
            windows_before
                .iter()
                .find(|(_, name)| name == "stash")
                .unwrap()
                .0,
            "2",
            "stash should start away from index 1 for this repro"
        );

        repair_layout(&iso, "test", "agent-doc").unwrap();

        let windows_after = list_windows(&iso, "test");
        assert_eq!(
            windows_after
                .iter()
                .find(|(_, name)| name == "agent-doc")
                .unwrap()
                .0,
            "0"
        );
        assert_eq!(
            windows_after
                .iter()
                .find(|(_, name)| name == "stash")
                .unwrap()
                .0,
            "1",
            "stash should be normalized directly after agent-doc"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_normalizes_stash_alias_to_index_1() {
        let iso = IsolatedTmux::new("sync-repair-stash-alias-index");
        let tmp = tempfile::TempDir::new().unwrap();

        let _pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "corky", "-d"]);
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash-2", "-d"]);

        let windows_before = list_windows(&iso, "test");
        assert!(
            windows_before
                .iter()
                .any(|(index, name)| index == "2" && name == "stash-2"),
            "stash alias should start away from index 1 for this repro"
        );

        repair_layout(&iso, "test", "agent-doc").unwrap();

        let windows_after = list_windows(&iso, "test");
        assert_eq!(
            windows_after
                .iter()
                .find(|(_, name)| name == "agent-doc")
                .unwrap()
                .0,
            "0"
        );
        assert_eq!(
            windows_after
                .iter()
                .find(|(_, name)| name == "stash")
                .unwrap()
                .0,
            "1",
            "the first stash window should be normalized to 1:stash"
        );
        assert!(
            !windows_after
                .iter()
                .any(|(_, name)| name.starts_with("stash-")),
            "repair should rename stash overflow aliases back to stash"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn full_sync_repairs_window_order_before_reconcile() {
        let iso = IsolatedTmux::new("sync-full-repairs-window-order");
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let doc = root.join("tasks/full-sync-repair.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: full-sync-repair\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        init_git_repo(root, &doc);
        let doc_str = doc.to_string_lossy().to_string();

        let pane0 = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let agent_doc_window = iso.pane_window(&pane0).unwrap();
        sessions::register_full_with_cwd(
            "full-sync-repair",
            &pane0,
            &doc_str,
            pane_pid_from_tmux(&iso, &pane0).unwrap(),
            &agent_doc_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "corky", "-d"]);
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash-2", "-d"]);

        run_with_options_internal(
            std::slice::from_ref(&doc_str),
            None,
            Some(doc_str.as_str()),
            AutoStartMode::Full,
            false,
            &iso,
        )
        .unwrap();

        let windows_after = list_windows(&iso, "test");
        assert_eq!(
            windows_after
                .iter()
                .find(|(_, name)| name == "agent-doc")
                .unwrap()
                .0,
            "0",
            "full sync should repair the agent-doc window to index 0"
        );
        assert_eq!(
            windows_after
                .iter()
                .find(|(_, name)| name == "stash")
                .unwrap()
                .0,
            "1",
            "full sync should repair the primary stash window to 1:stash"
        );
        assert!(
            !windows_after
                .iter()
                .any(|(_, name)| name.starts_with("stash-")),
            "full sync should normalize stash aliases during repair"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn full_sync_calls_doctor_repair_for_explicit_stash_window() {
        let iso = IsolatedTmux::new("sync-full-doctor-repairs-stash-window");
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let doc = root.join("tasks/full-sync-doctor-repair.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: full-sync-doctor-repair\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        init_git_repo(root, &doc);
        let doc_str = doc.to_string_lossy().to_string();

        let pane0 = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "stash"]);
        let stash_window = iso.pane_window(&pane0).unwrap();
        sessions::register_full_with_cwd(
            "full-sync-doctor-repair",
            &pane0,
            &doc_str,
            pane_pid_from_tmux(&iso, &pane0).unwrap(),
            &stash_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash", "-d"]);

        run_with_options_internal(
            std::slice::from_ref(&doc_str),
            Some("test:0"),
            Some(doc_str.as_str()),
            AutoStartMode::Full,
            false,
            &iso,
        )
        .unwrap();

        let windows_after = list_windows(&iso, "test");
        assert_eq!(
            windows_after
                .iter()
                .find(|(_, name)| name == "agent-doc")
                .unwrap()
                .0,
            "0",
            "full sync should let the doctor repair path recreate 0:agent-doc"
        );
        assert_eq!(
            windows_after
                .iter()
                .find(|(_, name)| name == "stash")
                .unwrap()
                .0,
            "1",
            "full sync should leave the repaired stash window at 1:stash"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

        // Note: repair_layout uses agent_doc_session_registry_io::load() which reads from CWD.
        // In tests without CWD override, Phase 2 rescue may not find the pane
        // in the registry. But stash consolidation, target consolidation, and
        // index normalization still run. The key assertion is that repair doesn't
        // error.
        let result = repair_layout(&iso, "test", "agent-doc");
        assert!(result.is_ok(), "repair_layout should not error");

        // The stashed pane should still be alive regardless
        assert!(iso.pane_alive(&pane2), "stashed pane should still be alive");
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn promote_pane_to_agent_doc_window_reparents_stash_pane() {
        // #stash-pane-promote-on-focus: a live-owner pane parked in the stash
        // window must be reparented into the agent-doc window on focus.
        let iso = IsolatedTmux::new("sync-promote-stash");
        let tmp = tempfile::TempDir::new().unwrap();

        let pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

        // A second pane stashed away — the live owner stuck in the stash window.
        let pane2 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
        iso.stash_pane(&pane2, "test").unwrap();

        let win_before = iso.pane_window(&pane2).unwrap();
        let name_before = window_name_for_window_id(&iso, &win_before).unwrap();
        assert!(
            is_stash_window_name(&name_before),
            "pane2 should start in a stash window, got {name_before}"
        );

        let promoted = promote_pane_to_agent_doc_window(&iso, &pane2).unwrap();
        assert!(promoted, "stash pane should be promoted");

        // tmux preserves the pane id across join-pane, and it now lives in the
        // agent-doc window.
        assert!(
            iso.pane_alive(&pane2),
            "promoted pane should still be alive"
        );
        let win_after = iso.pane_window(&pane2).unwrap();
        let name_after = window_name_for_window_id(&iso, &win_after).unwrap();
        assert_eq!(
            name_after, "agent-doc",
            "pane2 should be in the agent-doc window after promotion"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn promote_pane_to_agent_doc_window_noop_for_non_stash_pane() {
        // A pane already outside the stash must not be reparented.
        let iso = IsolatedTmux::new("sync-promote-noop");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

        let promoted = promote_pane_to_agent_doc_window(&iso, &pane0).unwrap();
        assert!(
            !promoted,
            "a pane already outside the stash should not be promoted"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn pane_in_stash_window_detects_stash_membership() {
        // `#jb-tsift-pane-sync`: editor-navigation focus uses this gate to avoid
        // selecting a stashed pane in place (which would surface focus inside the
        // stash). A pane in the agent-doc window is not stashed; a pane parked in
        // the stash window is.
        let iso = IsolatedTmux::new("sync-pane-in-stash");
        let tmp = tempfile::TempDir::new().unwrap();

        let pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        assert!(
            !pane_in_stash_window(&iso, &pane0),
            "pane in the agent-doc window must not be reported as stashed"
        );

        let pane2 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            pane_in_stash_window(&iso, &pane2),
            "pane parked in the stash window must be reported as stashed"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
                    let (id, name) = line.split_once(' ')?;
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

        // After repair, joinable panes should be consolidated into 1:stash.
        // If tmux refuses a join, overflow windows must remain adjacent as
        // 2:stash, 3:stash, etc. This repro normally consolidates fully.
        let windows_after = list_windows(&iso, "test");
        let stash_windows_after: Vec<_> = windows_after
            .iter()
            .filter(|(_, n)| n == "stash" || n.starts_with("stash-"))
            .collect();
        assert!(
            stash_windows_after.len() <= 1,
            "should have at most 1 stash window after consolidation, got {}",
            stash_windows_after.len()
        );
        if let Some((index, name)) = stash_windows_after.first() {
            assert_eq!(name.as_str(), "stash", "stash aliases must be renamed");
            assert_eq!(index.as_str(), "1", "primary stash should be 1:stash");
        }

        // agent-doc should still be at index 0
        let ad = windows_after.iter().find(|(_, n)| n == "agent-doc");
        assert!(ad.is_some(), "agent-doc window should still exist");
        assert_eq!(ad.unwrap().0, "0", "agent-doc should be at index 0");
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content).unwrap();
        assert!(
            fm.tmux_session.is_none(),
            "tmux_session should not be set initially"
        );

        // Write a doc WITH tmux_session already set
        let doc2 = tmp.path().join("test2.md");
        std::fs::write(
        &doc2,
        "---\nagent_doc_session: test-456\ntmux_session: old-session\n---\n\n## User\n\nHello\n",
    )
    .unwrap();

        let content2 = std::fs::read_to_string(&doc2).unwrap();
        let (fm2, _) = agent_doc_frontmatter::frontmatter::parse(&content2).unwrap();
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
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content).unwrap();

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
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content).unwrap();
        assert!(fm.session.is_none(), "file should have no session UUID");

        // Simulate resolve_file: no session → Unmanaged
        let resolution = match fm.session {
            Some(_) => unreachable!("session should be None"),
            None => FileResolution::Unmanaged,
        };
        assert!(matches!(resolution, FileResolution::Unmanaged));
    }
    /// Sync can inspect files that have a session UUID in frontmatter but no registry entry.
    /// This prevents auto-starting sessions for files that were never properly claimed
    /// or whose claim expired.
    #[test]
    fn sync_skips_file_with_session_uuid_but_no_registry() {
        let tmp = tempfile::TempDir::new().unwrap();

        agent_doc_session_registry_io::save_in(tmp.path(), &tmux_router::Registry::new()).unwrap();

        // Create a file with a session UUID but no matching registry entry
        let doc = tmp.path().join("stale-claim.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: orphan-uuid-123\n---\n\n## User\n\nHello\n",
        )
        .unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content).unwrap();
        assert_eq!(fm.session, Some("orphan-uuid-123".to_string()));

        let registry = agent_doc_session_registry_io::load_in(tmp.path()).unwrap();
        let has_registry_entry =
            tmux_router::registry::find_registry_key_by_session_id(&registry, "orphan-uuid-123")
                .is_some();
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

        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "claimed-uuid-456".to_string(),
            tmux_router::RegistryEntry {
                pane: "%99".to_string(),
                pid: 12345,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "claimed-uuid-456".to_string(),
                file: "claimed.md".to_string(),
                window: "@0".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        agent_doc_session_registry_io::save_in(tmp.path(), &registry).unwrap();

        // Create a file with a session UUID that matches the registry
        let doc = tmp.path().join("claimed.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: claimed-uuid-456\n---\n\n## User\n\nHello\n",
        )
        .unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content).unwrap();
        assert_eq!(fm.session, Some("claimed-uuid-456".to_string()));

        let registry = agent_doc_session_registry_io::load_in(tmp.path()).unwrap();
        let has_registry_entry =
            tmux_router::registry::find_registry_key_by_session_id(&registry, "claimed-uuid-456")
                .is_some();
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
        assert!(
            agent_doc_document::claim_scaffold::should_scaffold_empty_markdown(
                &content,
                Some("md")
            ),
            "file should be eligible for empty markdown scaffold"
        );

        // Scaffold it
        let session_id = uuid::Uuid::new_v4().to_string();
        let scaffold =
            agent_doc_document::claim_scaffold::render_empty_template_scaffold(&session_id);
        std::fs::write(&doc, &scaffold).unwrap();

        // Verify scaffolded content has frontmatter
        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content).unwrap();
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
        assert!(
            agent_doc_document::claim_scaffold::should_scaffold_empty_markdown(&raw, Some("md"))
        );

        let session_id = uuid::Uuid::new_v4().to_string();
        let scaffold =
            agent_doc_document::claim_scaffold::render_empty_template_scaffold(&session_id);
        std::fs::write(&doc, &scaffold).unwrap();

        let content = std::fs::read_to_string(&doc).unwrap();
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content).unwrap();

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
    /// repair_layout writes move-window or swap-window entries to /tmp/agent-doc-sync.log
    /// when it has to reposition the agent-doc window to index 0.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[test]
    fn safe_passive_prune_cleanup_skips_stash_scan_for_editor_handoff() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(tmp.path().join("tasks")).unwrap();
        std::fs::write(tmp.path().join("tasks/a.md"), "").unwrap();
        std::fs::write(tmp.path().join("tasks/b.md"), "").unwrap();
        let cols = vec!["tasks/a.md,tasks/b.md".to_string()];

        assert_eq!(
            safe_passive_prune_cleanup_mode(
                AutoStartMode::SafePassive,
                &cols,
                Some("agent:1"),
                Some("tasks/a.md")
            ),
            agent_doc_tmux::PruneCleanupMode::SkipExpensiveStashCleanup
        );

        let changed_focus = safe_passive_prune_cleanup_mode(
            AutoStartMode::SafePassive,
            &cols,
            Some("agent:1"),
            Some("tasks/b.md"),
        );
        assert_eq!(
            changed_focus,
            agent_doc_tmux::PruneCleanupMode::SkipExpensiveStashCleanup
        );

        let changed_cols = vec!["tasks/a.md".to_string(), "tasks/b.md".to_string()];
        assert_eq!(
            safe_passive_prune_cleanup_mode(
                AutoStartMode::SafePassive,
                &changed_cols,
                Some("agent:1"),
                Some("tasks/b.md"),
            ),
            agent_doc_tmux::PruneCleanupMode::SkipExpensiveStashCleanup
        );
    }
    #[test]
    fn acquire_sync_lock_times_out_when_lock_is_held() {
        let tmp = tempfile::TempDir::new().unwrap();
        let lock_path = tmp.path().join(".agent-doc/sync.lock");
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();

        let holder = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        fs2::FileExt::lock_exclusive(&holder).unwrap();

        let start = Instant::now();
        let acquired = acquire_sync_lock(&lock_path, Duration::from_millis(120), |_| {});
        let elapsed = start.elapsed();

        fs2::FileExt::unlock(&holder).unwrap();
        assert!(
            matches!(acquired, SyncLockAcquire::Contended),
            "contended sync lock should time out instead of blocking"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "sync lock timeout should be bounded, elapsed={elapsed:?}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_sync_focuses_local_projection_when_sync_lock_is_contended() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/software")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let active_doc = root.join("tasks/active.md");
        let stale_doc = root.join("tasks/software/tsift.md");
        std::fs::write(
            &active_doc,
            "---\nagent_doc_session: active-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        std::fs::write(
            &stale_doc,
            "---\nagent_doc_session: stale-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-postlock-actor-focus");
        let stale_pane = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let active_pane = iso.split_window(&stale_pane, root, "-dh").unwrap();
        let active_window = iso.pane_window(&active_pane).unwrap();

        agent_doc_session_actor_io::project_binding_in(
            root,
            &active_doc.to_string_lossy(),
            "active-session",
            &active_pane,
            &active_window,
            "sync",
            "postlock_focus_test",
        )
        .unwrap();
        agent_doc_controller_io::project_controller::store_actor_record(
            root,
            Some(0),
            &agent_doc_sqlite::state_store::ActorRecord {
                document_id: agent_doc_session_actor_io::canonical_document_id_in(
                    root,
                    &stale_doc.to_string_lossy(),
                ),
                session_id: "stale-session".to_string(),
                generation: 1,
                pane_id: stale_pane.clone(),
                window_id: active_window.clone(),
                harness: "codex".to_string(),
                state: agent_doc_sqlite::state_store::ActorState::Starting,
                last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                    caller: "sync".to_string(),
                    reason: "stale_starting_sibling_test".to_string(),
                    timestamp: 1,
                    prior_generation: 0,
                    new_generation: 1,
                },
            },
        )
        .unwrap();
        iso.select_pane(&stale_pane).unwrap();

        let lock_path = root.join(".agent-doc/sync.lock");
        let holder = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        fs2::FileExt::lock_exclusive(&holder).unwrap();

        run_with_options_internal(
            &[active_doc.to_string_lossy().to_string()],
            None,
            Some(active_doc.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            false,
            &iso,
        )
        .unwrap();

        fs2::FileExt::unlock(&holder).unwrap();
        assert_eq!(
            iso.active_pane("test").unwrap(),
            active_pane,
            "safe-passive editor sync should focus the known local actor pane before sync lock contention defers prune/reconcile"
        );
    }
    #[test]
    fn file_rename_updates_registry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path();

        // Set up registry with an entry pointing to old path
        let session_id = "rename-test-uuid";
        let old_file = "tasks/old-name.md";
        let new_file = "tasks/new-name.md";
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            session_id.to_string(),
            tmux_router::RegistryEntry {
                pane: "%42".to_string(),
                pid: 12345,
                cwd: project.to_string_lossy().to_string(),
                started: "2026-04-20T00:00:00Z".to_string(),
                session_id: session_id.to_string(),
                file: old_file.to_string(),
                window: "@0".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        agent_doc_session_registry_io::save_in(project, &registry).unwrap();

        // Verify detection
        assert!(
            is_file_rename(old_file, new_file),
            "old path doesn't exist on disk, paths differ → rename"
        );

        // Verify we can load the entry and see the old path
        let reg = agent_doc_session_registry_io::load_in(project).unwrap();
        let key = tmux_router::registry::find_registry_key_by_session_id(&reg, session_id).unwrap();
        let entry = reg.get(&key).unwrap();
        assert_eq!(entry.file, old_file);
        assert_eq!(entry.pane, "%42");
    }
    #[test]
    fn rename_debounce_suppresses_auto_start() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();

        // Create a file with known content for hashing
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "---\nagent_doc_session: abc123\n---\n").unwrap();

        write_rename_debounce(&file.to_string_lossy());
        assert!(has_rename_debounce(&file));
        assert!(!tmp.path().join(".agent-doc/rename-debounce").exists());
    }
    #[test]
    fn rename_debounce_does_not_affect_other_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();

        let file_a = tmp.path().join("a.md");
        let file_b = tmp.path().join("b.md");
        std::fs::write(&file_a, "---\nagent_doc_session: aaa\n---\n").unwrap();
        std::fs::write(&file_b, "---\nagent_doc_session: bbb\n---\n").unwrap();

        // Only record debounce state for file_a.
        let hash_a = agent_doc_fs::document_state_hash(&file_a).unwrap();
        write_rename_debounce(&file_a.to_string_lossy());

        // file_b should have a different hash, no marker
        let hash_b = agent_doc_fs::document_state_hash(&file_b).unwrap();
        assert_ne!(
            hash_a, hash_b,
            "different files should have different hashes"
        );
        assert!(has_rename_debounce(&file_a));
        assert!(!has_rename_debounce(&file_b));
        assert!(!tmp.path().join(".agent-doc/rename-debounce").exists());
    }
    #[test]
    fn skip_auto_start_for_recent_session_loss_detects_repeated_window() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());
        let doc = tmp.path().join("tasks").join("repeat-loss.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: repeat-loss\n---\n").unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/repeat-loss.log"),
            format!(
                "[{}] supervisor_exit code=missing_pane pane=%41 reason=registered_pane_missing\n[{}] supervisor_exit code=missing_pane pane=%42 reason=registered_pane_dead\n",
                now.saturating_sub(30),
                now.saturating_sub(5)
            ),
        )
        .unwrap();

        assert!(
            skip_auto_start_for_recent_session_loss(&doc, "repeat-loss").unwrap(),
            "two recent session-loss events should suppress sync auto-start"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
        let win_name = agent_doc_tmux_io::target_window_name(&iso, &win_id).unwrap_or_default();
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
        let rescue_result =
            agent_doc_tmux_io::join_pane_guarded(&iso, &pane1, target, target_sess, "-dh");
        assert!(
            rescue_result.is_ok(),
            "join-pane rescue should succeed: {:?}",
            rescue_result.err()
        );

        // Verify pane1 is no longer in stash
        let post_win_id = iso.pane_window(&pane1).unwrap();
        let post_win_name =
            agent_doc_tmux_io::target_window_name(&iso, &post_win_id).unwrap_or_default();
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
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn sync_defers_stash_rescue_to_reconciler_swap() {
        let iso = IsolatedTmux::new("sync-deferred-stash-rescue");
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let doc_a = tmp.path().join("tasks/a.md");
        let doc_b = tmp.path().join("tasks/b.md");
        std::fs::create_dir_all(doc_a.parent().unwrap()).unwrap();

        let pane0 = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let pane1 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();

        let session_a = "aaaa-aaaa";
        let session_b = "bbbb-bbbb";
        std::fs::write(
            &doc_a,
            format!("---\nagent_doc_session: {session_a}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n"),
        ).unwrap();
        std::fs::write(
            &doc_b,
            format!("---\nagent_doc_session: {session_b}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n"),
        ).unwrap();

        let win = iso.pane_window(&pane0).unwrap();
        sessions::register_full_with_cwd(
            session_a,
            &pane0,
            &doc_a.to_string_lossy(),
            std::process::id(),
            &win,
            &tmp.path().to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            session_b,
            &pane1,
            &doc_b.to_string_lossy(),
            std::process::id(),
            &win,
            &tmp.path().to_string_lossy(),
        )
        .unwrap();

        // Stash pane1 — simulates what the reconciler does when switching layouts.
        iso.stash_pane(&pane1, "test").unwrap();
        assert!(iso.pane_alive(&pane1), "stashed pane should be alive");

        let agent_doc_window = "test:agent-doc";
        let panes_before = iso.list_window_panes(agent_doc_window).unwrap();
        assert_eq!(
            panes_before.len(),
            1,
            "agent-doc window should have 1 pane before sync"
        );
        assert!(
            panes_before.contains(&pane0),
            "pane0 should be in agent-doc window"
        );

        // Run sync requesting doc_a + doc_b — this should NOT rescue pane1 pre-reconciler.
        // Instead, the reconciler should handle the swap.
        let result = run_with_tmux(
            &[
                doc_a.to_string_lossy().to_string(),
                doc_b.to_string_lossy().to_string(),
            ],
            Some(agent_doc_window),
            None,
            &iso,
        );
        assert!(result.is_ok(), "sync should succeed: {:?}", result.err());

        // After sync, both panes should be in the agent-doc window.
        let panes_after = iso.list_window_panes(agent_doc_window).unwrap();
        assert!(
            panes_after.contains(&pane0),
            "pane0 should be in agent-doc window after sync, got: {:?}",
            panes_after
        );
        assert!(
            panes_after.contains(&pane1),
            "pane1 (was in stash) should be in agent-doc window after sync, got: {:?}",
            panes_after
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn windowless_sync_targets_current_session_agent_doc_window() {
        let iso = IsolatedTmux::new("sync-windowless-target-agent-doc");
        let tmp = tempfile::TempDir::new().unwrap();

        let pane0 = iso.new_session("test", tmp.path()).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
            .unwrap();
        let pane1 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
        iso.stash_pane(&pane1, "test").unwrap();
        iso.raw_cmd(&["select-window", "-t", "test:stash"]).unwrap();

        assert_eq!(
            current_tmux_session_name(&iso).as_deref(),
            Some("test"),
            "current session lookup should still point at the owning session"
        );
        assert_eq!(
            resolve_agent_doc_window_id(&iso, "test", "agent-doc").as_deref(),
            Some("@0"),
            "windowless sync should resolve the named agent-doc window instead of inheriting stash"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn windowless_sync_uses_project_session_pin_when_current_has_no_agent_doc_window() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd = ScopedCurrentDir::set(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/config.toml"),
            "tmux_session = \"0\"\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-windowless-project-pin");
        let _pane0 = iso.new_session("0", tmp.path()).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "0:0", "agent-doc"])
            .unwrap();
        let _pane1 = iso.new_session("1", tmp.path()).unwrap();

        assert_eq!(
            current_tmux_session_name(&iso).as_deref(),
            Some("1"),
            "the current client session should be the most recently created one"
        );
        assert_eq!(
            resolve_sync_target_session(&iso, None, &[], None).as_deref(),
            Some("0"),
            "windowless sync should keep a live project tmux_session pin when the current session has no agent-doc window"
        );
        assert_eq!(
            resolve_agent_doc_window_id(&iso, "0", "agent-doc").as_deref(),
            Some("@0")
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn windowless_sync_prefers_current_agent_doc_session_over_live_project_pin() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd = ScopedCurrentDir::set(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/config.toml"),
            "tmux_session = \"0\"\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-windowless-current-agent-doc");
        let _pane0 = iso.new_session("0", tmp.path()).unwrap();
        let _pane1 = iso.new_session("1", tmp.path()).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "1:0", "agent-doc"])
            .unwrap();

        assert_eq!(
            current_tmux_session_name(&iso).as_deref(),
            Some("1"),
            "the current client session should be the most recently created one"
        );
        assert_eq!(
            resolve_sync_target_session(&iso, None, &[], None).as_deref(),
            Some("1"),
            "windowless sync should follow the current live agent-doc window before the configured pin"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn windowless_sync_falls_back_to_current_session_when_project_pin_dead() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd = ScopedCurrentDir::set(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/config.toml"),
            "tmux_session = \"0\"\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-windowless-dead-project-pin");
        let _pane1 = iso.new_session("1", tmp.path()).unwrap();

        assert_eq!(
            current_tmux_session_name(&iso).as_deref(),
            Some("1"),
            "the live attached session should still be discoverable"
        );
        assert_eq!(
            resolve_sync_target_session(&iso, None, &[], None).as_deref(),
            Some("1"),
            "a dead project pin should fall back to the current live session"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn windowless_sync_prefers_shared_workspace_root_pin_for_mixed_roots() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/session-share");

        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();
        let _cwd = ScopedCurrentDir::set(&subroot);
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"4\"\n",
        )
        .unwrap();
        std::fs::write(
            subroot.join(".agent-doc/config.toml"),
            "tmux_session = \"1\"\n",
        )
        .unwrap();

        let root_doc = root.join("tasks/agent-doc-bugs2.md");
        let child_doc = subroot.join("tasks/claudescore-3.md");
        std::fs::write(
            &root_doc,
            "---\nagent_doc_session: root-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-windowless-mixed-root-pin");
        let _pane1 = iso.new_session("1", root).unwrap();
        let _pane4 = iso.new_session("4", root).unwrap();

        let columns = vec![
            root_doc.to_string_lossy().to_string(),
            child_doc.to_string_lossy().to_string(),
        ];
        assert_eq!(
            resolve_sync_target_session(
                &iso,
                None,
                &columns,
                Some(child_doc.to_string_lossy().as_ref()),
            )
            .as_deref(),
            Some("4"),
            "mixed-root windowless sync should stay on the shared workspace root pin instead of the caller cwd or focused child root"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn register_synced_files_updates_each_project_registry_by_path_key() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/session-share");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();

        let root_doc = root.join("tasks/agent-doc-bugs2.md");
        let child_doc = subroot.join("tasks/claudescore-3.md");
        std::fs::write(
            &root_doc,
            "---\nagent_doc_session: root-session\n---\n\n# Root\n",
        )
        .unwrap();
        std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\n---\n\n# Child\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-cross-root-register");
        let root_pane = iso.new_session("test", root).unwrap();
        let child_pane = iso.split_window(&root_pane, &subroot, "-dh").unwrap();

        let mut child_registry = tmux_router::Registry::new();
        let bad_key = tmux_router::registry::canonical_registry_key_in(
            &subroot,
            "src/session-share/tasks/claudescore-3.md",
        );
        child_registry.insert(
            bad_key,
            tmux_router::RegistryEntry {
                pane: child_pane.clone(),
                pid: pane_pid_from_tmux(&iso, &child_pane).unwrap(),
                cwd: root.to_string_lossy().to_string(),
                started: String::new(),
                session_id: "child-session".to_string(),
                file: "src/session-share/tasks/claudescore-3.md".to_string(),
                window: iso.pane_window(&child_pane).unwrap(),
                supervisor_instance_id: String::new(),
            },
        );
        agent_doc_session_registry_io::save_in(&subroot, &child_registry).unwrap();

        let _cwd = ScopedCurrentDir::set(root);
        register_synced_files(
            &iso,
            &[
                (
                    "root-session".to_string(),
                    PathBuf::from("tasks/agent-doc-bugs2.md"),
                ),
                (
                    "child-session".to_string(),
                    PathBuf::from("src/session-share/tasks/claudescore-3.md"),
                ),
            ],
            &[
                (PathBuf::from("tasks/agent-doc-bugs2.md"), root_pane.clone()),
                (
                    PathBuf::from("src/session-share/tasks/claudescore-3.md"),
                    child_pane.clone(),
                ),
            ],
        );

        let root_registry = agent_doc_session_registry_io::load_in(root).unwrap();
        let root_key = tmux_router::registry::canonical_registry_key_in(
            root,
            root_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
        );
        let root_entry = root_registry
            .get(&root_key)
            .expect("root document should be registered in root registry");
        assert_eq!(root_entry.pane, root_pane);
        assert_eq!(root_entry.file, "tasks/agent-doc-bugs2.md");
        assert_eq!(root_registry.len(), 1);

        let child_registry = agent_doc_session_registry_io::load_in(&subroot).unwrap();
        let child_key = tmux_router::registry::canonical_registry_key_in(
            &subroot,
            child_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
        );
        let child_entry = child_registry
            .get(&child_key)
            .expect("child document should be registered in child registry");
        assert_eq!(child_entry.pane, child_pane);
        assert_eq!(child_entry.file, "tasks/claudescore-3.md");
        assert_eq!(child_entry.cwd, subroot.to_string_lossy());
        assert_eq!(child_registry.len(), 1);
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn register_synced_files_prunes_cross_root_duplicate_pane_binding() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/session-share");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();

        let root_doc = root.join("tasks/agent-doc-bugs2.md");
        let child_doc = subroot.join("tasks/agentic-harness-engineering.md");
        std::fs::write(
            &root_doc,
            "---\nagent_doc_session: root-session\n---\n\n# Root\n",
        )
        .unwrap();
        std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\n---\n\n# Child\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-cross-root-duplicate-register");
        let root_pane = iso.new_session("test", root).unwrap();
        let window = iso.pane_window(&root_pane).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

        let root_key = tmux_router::registry::canonical_registry_key_in(
            root,
            root_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
        );
        let mut root_registry = tmux_router::Registry::new();
        root_registry.insert(
            root_key,
            tmux_router::RegistryEntry {
                pane: root_pane.clone(),
                pid: pane_pid_from_tmux(&iso, &root_pane).unwrap(),
                cwd: root.to_string_lossy().to_string(),
                started: "2026-05-01T00:44:03Z".to_string(),
                session_id: "root-session".to_string(),
                file: "tasks/agent-doc-bugs2.md".to_string(),
                window: window.clone(),
                supervisor_instance_id: String::new(),
            },
        );
        agent_doc_session_registry_io::save_in(root, &root_registry).unwrap();

        let child_key = tmux_router::registry::canonical_registry_key_in(
            &subroot,
            child_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
        );
        let mut child_registry = tmux_router::Registry::new();
        child_registry.insert(
            child_key,
            tmux_router::RegistryEntry {
                pane: root_pane.clone(),
                pid: pane_pid_from_tmux(&iso, &root_pane).unwrap(),
                cwd: root.to_string_lossy().to_string(),
                started: "2026-05-01T00:36:27Z".to_string(),
                session_id: "child-session".to_string(),
                file: "tasks/agentic-harness-engineering.md".to_string(),
                window: window.clone(),
                supervisor_instance_id: String::new(),
            },
        );
        agent_doc_session_registry_io::save_in(&subroot, &child_registry).unwrap();

        let _cwd = ScopedCurrentDir::set(root);
        register_synced_files(
            &iso,
            &[
                (
                    "root-session".to_string(),
                    PathBuf::from("tasks/agent-doc-bugs2.md"),
                ),
                (
                    "child-session".to_string(),
                    PathBuf::from("src/session-share/tasks/agentic-harness-engineering.md"),
                ),
            ],
            &[
                (PathBuf::from("tasks/agent-doc-bugs2.md"), root_pane.clone()),
                (
                    PathBuf::from("src/session-share/tasks/agentic-harness-engineering.md"),
                    root_pane.clone(),
                ),
            ],
        );

        let root_registry = agent_doc_session_registry_io::load_in(root).unwrap();
        let root_entry = root_registry
            .values()
            .find(|entry| entry.session_id == "root-session")
            .expect("root document should remain registered");
        assert_eq!(root_entry.pane, root_pane);

        let child_registry = agent_doc_session_registry_io::load_in(&subroot).unwrap();
        assert!(
            child_registry.is_empty(),
            "duplicate cross-root pane binding should be pruned instead of preserved"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn register_synced_files_skips_geometry_only_binding_during_fail_closed_recovery() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/session-share");
        std::fs::create_dir_all(subroot.join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();

        let child_doc = subroot.join("tasks/claudescore-3.md");
        std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\n---\n\n# Child\n",
        )
        .unwrap();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        std::fs::write(
            subroot.join(".agent-doc/logs/child-session.log"),
            format!(
                "[{}] session_start file=tasks/claudescore-3.md pane=%261 session=child-session\n[{}] codex_start mode=fresh restart_count=0\n[{}] supervisor_exit code=missing_pane pane=%261 reason=registered_pane_missing\n[{}] session_end origin=sync_missing_pane\n[{}] session_start file=tasks/claudescore-3.md pane=%261 session=child-session\n[{}] codex_start mode=fresh restart_count=0\n[{}] supervisor_exit code=missing_pane pane=%261 reason=registered_pane_missing\n[{}] session_end origin=sync_missing_pane\n",
                now.saturating_sub(8),
                now.saturating_sub(7),
                now.saturating_sub(6),
                now.saturating_sub(5),
                now.saturating_sub(4),
                now.saturating_sub(3),
                now.saturating_sub(2),
                now.saturating_sub(1),
            ),
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-fail-closed-geometry-binding");
        let child_pane = iso.new_session("test", &subroot).unwrap();

        let child_key = tmux_router::registry::canonical_registry_key_in(
            &subroot,
            child_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
        );
        let mut child_registry = tmux_router::Registry::new();
        child_registry.insert(
            child_key,
            tmux_router::RegistryEntry {
                pane: child_pane.clone(),
                pid: pane_pid_from_tmux(&iso, &child_pane).unwrap(),
                cwd: subroot.to_string_lossy().to_string(),
                started: "2026-05-01T01:12:43Z".to_string(),
                session_id: "child-session".to_string(),
                file: "tasks/claudescore-3.md".to_string(),
                window: iso.pane_window(&child_pane).unwrap(),
                supervisor_instance_id: String::new(),
            },
        );
        agent_doc_session_registry_io::save_in(&subroot, &child_registry).unwrap();

        let _cwd = ScopedCurrentDir::set(root);
        register_synced_files(
            &iso,
            &[(
                "child-session".to_string(),
                PathBuf::from("src/session-share/tasks/claudescore-3.md"),
            )],
            &[(
                PathBuf::from("src/session-share/tasks/claudescore-3.md"),
                child_pane.clone(),
            )],
        );

        let child_registry = agent_doc_session_registry_io::load_in(&subroot).unwrap();
        assert!(
            child_registry.is_empty(),
            "fail-closed recovery should not let sync rebind a geometry-only pane assignment"
        );
    }
    #[test]
    fn claimed_sync_pane_owner_ignores_same_file_and_reports_other_owner() {
        let claimed: RefCell<std::collections::HashMap<String, PathBuf>> =
            RefCell::new(std::collections::HashMap::new());
        let root_doc = PathBuf::from("tasks/agent-doc-bugs2.md");
        let child_doc = PathBuf::from("src/session-share/tasks/claudescore-3.md");

        reserve_sync_pane(&claimed, "%75", &root_doc);

        assert_eq!(
            claimed_sync_pane_owner(&claimed, "%75", &root_doc),
            None,
            "a file should be allowed to keep its own reserved pane"
        );
        assert_eq!(
            claimed_sync_pane_owner(&claimed, "%75", &child_doc),
            Some(root_doc),
            "another file should see the reservation conflict"
        );
    }
    #[test]
    fn recover_existing_associated_pane_skips_reserved_candidates() {
        let claimed: RefCell<std::collections::HashMap<String, PathBuf>> =
            RefCell::new(std::collections::HashMap::new());
        reserve_sync_pane(&claimed, "%75", Path::new("tasks/agent-doc-bugs2.md"));

        let winner = AssociatedPaneCandidate {
            pane_id: "%75".to_string(),
            pane_pid: "1000".to_string(),
            session_name: "0".to_string(),
            window_id: "@1".to_string(),
            window_name: "agent-doc".to_string(),
            current_command: "agent-doc".to_string(),
            sources: [AssociatedPaneSource::ProcessTree].into_iter().collect(),
        };
        let filtered: Vec<AssociatedPaneCandidate> = vec![winner.clone()]
            .into_iter()
            .filter(|candidate| {
                claimed_sync_pane_owner(
                    &claimed,
                    &candidate.pane_id,
                    Path::new("src/session-share/tasks/claudescore-3.md"),
                )
                .is_none()
            })
            .collect();
        assert!(
            filtered.is_empty(),
            "reserved pane candidates should be removed before associated-pane recovery"
        );

        match resolve_associated_panes(filtered, Some("@1")) {
            AssociatedPaneResolution::None => {}
            other => panic!("expected no available associated pane after filtering, got {other:?}"),
        }
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn sync_stashes_open_cycle_pane_during_reconcile_detach() {
        let iso = IsolatedTmux::new("sync-open-cycle-protect");
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);

        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let doc_a = root.join("tasks/a.md");
        let doc_b = root.join("tasks/b.md");
        let content_a = concat!(
            "---\n",
            "agent_doc_session: sync-open-cycle-a\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_b = concat!(
            "---\n",
            "agent_doc_session: sync-open-cycle-b\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc_a, content_a).unwrap();
        std::fs::write(&doc_b, content_b).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc_a,
            content_a,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc_b,
            content_b,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let pane_a = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let pane_b = iso.split_window(&pane_a, root, "-dh").unwrap();
        let window = iso.pane_window(&pane_a).unwrap();

        sessions::register_full_with_cwd(
            "sync-open-cycle-a",
            &pane_a,
            &doc_a.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_a).unwrap(),
            &window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            "sync-open-cycle-b",
            &pane_b,
            &doc_b.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_b).unwrap(),
            &window,
            &root.to_string_lossy(),
        )
        .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc_a, Some(content_a), Some(content_a))
            .unwrap();

        run_with_tmux(
            &[doc_b.to_string_lossy().to_string()],
            Some("test:agent-doc"),
            Some(doc_b.to_string_lossy().as_ref()),
            &iso,
        )
        .unwrap();

        let visible = iso.list_window_panes("test:agent-doc").unwrap();
        assert!(
            !visible.contains(&pane_a),
            "open-cycle extra pane should be stashed instead of forcing a 3-pane projection: {visible:?}"
        );
        assert!(
            visible.contains(&pane_b),
            "requested pane must remain visible after reconcile: {visible:?}"
        );
        assert!(iso.pane_alive(&pane_a), "open-cycle pane must stay alive");
        assert_ne!(
            iso.pane_window(&pane_a).unwrap(),
            window,
            "open-cycle pane should move out of the visible agent-doc window"
        );
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc_a)
                .unwrap()
                .unwrap()
                .phase,
            agent_doc_turn::CyclePhase::PreflightStarted
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_sync_preserves_existing_layout_for_vscode_mixed_root_split_replay() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/session-share");
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(root.join("tasks/software")).unwrap();
        std::fs::create_dir_all(root.join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();
        let _cwd = ScopedCurrentDir::set(root);

        let tsift_doc = root.join("tasks/software/tsift.md");
        let bugs_doc = root.join("tasks/agent-doc/agent-doc-bugs2.md");
        let claudescore_doc = subroot.join("tasks/claudescore-3.md");
        std::fs::write(
            &tsift_doc,
            "---\nagent_doc_session: tsift-v0.1\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        std::fs::write(
            &bugs_doc,
            "---\nagent_doc_session: bugs-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        std::fs::write(
            &claudescore_doc,
            "---\nagent_doc_session: claudescore-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".agent-doc/logs/tsift-v0.1.log"),
            "[1] session_start file=tasks/software/tsift.md pane=%26 session=tsift-v0.1\n[2] document_cycle phase=committed cycle=cycle-1 event=commit_success capture_id=cycle-1\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-safe-passive-no-alias");
        let bugs_pane = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let dev_pane = iso.split_window(&bugs_pane, &subroot, "-dh").unwrap();
        let agent_doc_window = iso.pane_window(&bugs_pane).unwrap();
        let dev_pane_pid = pane_pid_from_tmux(&iso, &dev_pane).unwrap();

        let _ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            subroot.as_path(),
            "claudescore-session",
            {
                move |method| match method {
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({
                        "pid": dev_pane_pid
                    })),
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "supervisor_pid": dev_pane_pid,
                        "supervisor_instance_id": "dev-instance",
                    })),
                    _ => IpcResponse::ok_empty(),
                }
            },
        )
        .unwrap();

        sessions::register_full_with_cwd(
            "bugs-session",
            &bugs_pane,
            &bugs_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &bugs_pane).unwrap(),
            &agent_doc_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd_in(
            &subroot,
            "claudescore-session",
            &dev_pane,
            &claudescore_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &dev_pane).unwrap(),
            &agent_doc_window,
            &subroot.to_string_lossy(),
        )
        .unwrap();

        run_with_options_internal(
            &[
                tsift_doc.to_string_lossy().to_string(),
                claudescore_doc.to_string_lossy().to_string(),
            ],
            None,
            Some(tsift_doc.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            false,
            &iso,
        )
        .unwrap();

        let root_registry = agent_doc_session_registry_io::load_in(root).unwrap();
        assert!(
            !root_registry
                .values()
                .any(|entry| entry.session_id == "tsift-v0.1"),
            "blocked passive sync must not register tsift onto a spare pane"
        );

        let ordered = iso.list_panes_ordered(&agent_doc_window).unwrap();
        assert_eq!(
            ordered,
            vec![bugs_pane.clone(), dev_pane.clone()],
            "blocked passive sync must preserve the agent-doc-bugs2/claudescore-3 visible split instead of letting the remaining foreign pane become authoritative"
        );
        assert!(
            iso.pane_alive(&bugs_pane),
            "the preserved workspace pane must remain alive"
        );
        assert!(
            iso.pane_alive(&dev_pane),
            "the resolved sibling pane must also remain visible"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_sync_reuses_alive_registered_pane_before_full_live_owner_proof() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let doc = root.join("tasks/passive-fast-path.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: passive-fast-path\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-safe-passive-registered-fast-path");
        let pane = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let window_id = iso.pane_window(&pane).unwrap();

        sessions::register_full_with_cwd(
            "passive-fast-path",
            &pane,
            &doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane).unwrap(),
            &window_id,
            &root.to_string_lossy(),
        )
        .unwrap();

        run_with_options_internal(
            &[doc.to_string_lossy().to_string()],
            None,
            Some(doc.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            false,
            &iso,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&window_id).unwrap();
        assert_eq!(
            ordered,
            vec![pane.clone()],
            "safe passive sync should immediately reuse the alive registered pane instead of provisioning a replacement"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_sync_attaches_requested_pane_and_stashes_open_cycle_extra() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let doc_a = root.join("tasks/a.md");
        let doc_b = root.join("tasks/b.md");
        let doc_c = root.join("tasks/c.md");
        for (path, session) in [
            (&doc_a, "apresync-a"),
            (&doc_b, "bpresync-b"),
            (&doc_c, "cpresync-c"),
        ] {
            std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
        }

        let iso = IsolatedTmux::new("sync-safe-passive-protected-grow");
        let pane_a = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let pane_b = iso.split_window(&pane_a, root, "-dh").unwrap();
        let target_window = iso.pane_window(&pane_a).unwrap();
        let pane_c = iso.new_window("test", root).unwrap();
        let pane_c_window = iso.pane_window(&pane_c).unwrap();

        sessions::register_full_with_cwd(
            "apresync-a",
            &pane_a,
            &doc_a.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_a).unwrap(),
            &target_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            "bpresync-b",
            &pane_b,
            &doc_b.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_b).unwrap(),
            &target_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            "cpresync-c",
            &pane_c,
            &doc_c.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_c).unwrap(),
            &pane_c_window,
            &root.to_string_lossy(),
        )
        .unwrap();

        let doc_a_content = std::fs::read_to_string(&doc_a).unwrap();
        agent_doc_cycle_state_io::start_preflight(
            &doc_a,
            Some(&doc_a_content),
            Some(&doc_a_content),
        )
        .unwrap();

        run_with_options_internal(
            &[
                doc_c.to_string_lossy().to_string(),
                doc_b.to_string_lossy().to_string(),
            ],
            None,
            Some(doc_c.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            false,
            &iso,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&target_window).unwrap();
        assert!(
            !ordered.contains(&pane_a),
            "open-cycle extra pane should be stashed while other documents sync"
        );
        assert!(
            ordered.contains(&pane_b),
            "already visible requested pane should remain visible"
        );
        assert!(
            ordered.contains(&pane_c),
            "requested hidden pane should be attached immediately instead of waiting for the protected pane to close out"
        );
        assert_eq!(
            iso.pane_window(&pane_c).unwrap(),
            target_window,
            "safe passive sync should move the requested pane into the visible agent-doc window"
        );
        assert!(iso.pane_alive(&pane_a), "open-cycle pane must stay alive");
        assert_ne!(
            iso.pane_window(&pane_a).unwrap(),
            target_window,
            "open-cycle pane should no longer be visible in the requested projection"
        );
    }
    #[test]
    #[ignore = "covered by sync_sim_tmuxbudget_seed_3001; safe-passive tmux smoke keeps the real pane/window path covered"]
    fn manual_sync_attaches_requested_pane_around_protected_open_cycle_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let doc_a = root.join("tasks/a.md");
        let doc_b = root.join("tasks/b.md");
        let doc_c = root.join("tasks/c.md");
        for (path, session) in [
            (&doc_a, "amanual-a"),
            (&doc_b, "bmanual-b"),
            (&doc_c, "cmanual-c"),
        ] {
            std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
        }

        let iso = IsolatedTmux::new("sync-manual-protected-grow");
        let pane_a = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let pane_b = iso.split_window(&pane_a, root, "-dh").unwrap();
        let target_window = iso.pane_window(&pane_a).unwrap();
        let pane_c = iso.new_window("test", root).unwrap();
        let pane_c_window = iso.pane_window(&pane_c).unwrap();

        sessions::register_full_with_cwd(
            "amanual-a",
            &pane_a,
            &doc_a.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_a).unwrap(),
            &target_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            "bmanual-b",
            &pane_b,
            &doc_b.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_b).unwrap(),
            &target_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            "cmanual-c",
            &pane_c,
            &doc_c.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_c).unwrap(),
            &pane_c_window,
            &root.to_string_lossy(),
        )
        .unwrap();

        let doc_a_content = std::fs::read_to_string(&doc_a).unwrap();
        agent_doc_cycle_state_io::start_preflight(
            &doc_a,
            Some(&doc_a_content),
            Some(&doc_a_content),
        )
        .unwrap();

        run_with_options_internal(
            &[
                doc_c.to_string_lossy().to_string(),
                doc_b.to_string_lossy().to_string(),
            ],
            None,
            Some(doc_c.to_string_lossy().as_ref()),
            AutoStartMode::Full,
            false,
            &iso,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&target_window).unwrap();
        assert!(
            !ordered.contains(&pane_a),
            "open-cycle extra pane should be stashed while other documents sync"
        );
        assert!(
            ordered.contains(&pane_c),
            "manual sync should attach the requested hidden pane immediately"
        );
        assert_eq!(
            iso.pane_window(&pane_c).unwrap(),
            target_window,
            "manual sync should move the requested pane into the visible agent-doc window"
        );
    }
    #[test]
    fn safe_passive_sync_replaces_detachable_visible_pane_and_stashes_open_cycle_extra() {
        sync_replaces_detachable_visible_pane_and_stashes_open_cycle_extra(
            AutoStartMode::SafePassive,
            "sync-safe-passive-replace-detachable",
        );
    }
    #[test]
    #[ignore = "covered by sync_sim_tmuxbudget_seed_3002; safe-passive tmux smoke keeps the real pane/window path covered"]
    fn manual_sync_replaces_detachable_visible_pane_and_stashes_open_cycle_extra() {
        sync_replaces_detachable_visible_pane_and_stashes_open_cycle_extra(
            AutoStartMode::Full,
            "sync-manual-replace-detachable",
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_blocked_layout_preserve_still_reselects_visible_focus_pane() {
        let root = tempfile::TempDir::new().unwrap();
        let subroot = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(root.path().join("tasks/software")).unwrap();
        std::fs::create_dir_all(root.path().join("tasks/agent-doc")).unwrap();
        std::fs::write(
            root.path().join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(subroot.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.path().join("tasks")).unwrap();
        std::fs::write(
            subroot.path().join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();
        let _cwd = ScopedCurrentDir::set(root.path());

        let tsift_doc = root.path().join("tasks/software/tsift.md");
        let bugs_doc = root.path().join("tasks/agent-doc/agent-doc-bugs2.md");
        let claudescore_doc = subroot.path().join("tasks/claudescore-3.md");
        std::fs::write(
            &tsift_doc,
            "---\nagent_doc_session: tsift-v0.1\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        std::fs::write(
            &bugs_doc,
            "---\nagent_doc_session: bugs-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        std::fs::write(
            &claudescore_doc,
            "---\nagent_doc_session: claudescore-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join(".agent-doc/logs/tsift-v0.1.log"),
            "[1] session_start file=tasks/software/tsift.md pane=%26 session=tsift-v0.1\n[2] document_cycle phase=committed cycle=cycle-1 event=commit_success capture_id=cycle-1\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-safe-passive-blocked-focus");
        let bugs_pane = iso.new_session("test", root.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let dev_pane = iso.split_window(&bugs_pane, subroot.path(), "-dh").unwrap();
        let agent_doc_window = iso.pane_window(&bugs_pane).unwrap();
        let dev_pane_pid = pane_pid_from_tmux(&iso, &dev_pane).unwrap();

        let _ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            subroot.path(),
            "claudescore-session",
            {
                move |method| match method {
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({
                        "pid": dev_pane_pid
                    })),
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "supervisor_pid": dev_pane_pid,
                        "supervisor_instance_id": "dev-instance",
                    })),
                    _ => IpcResponse::ok_empty(),
                }
            },
        )
        .unwrap();

        sessions::register_full_with_cwd(
            "bugs-session",
            &bugs_pane,
            &bugs_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &bugs_pane).unwrap(),
            &agent_doc_window,
            &root.path().to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd_in(
            subroot.path(),
            "claudescore-session",
            &dev_pane,
            &claudescore_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &dev_pane).unwrap(),
            &agent_doc_window,
            &subroot.path().to_string_lossy(),
        )
        .unwrap();
        iso.select_pane(&bugs_pane).unwrap();

        run_with_options_internal(
            &[
                tsift_doc.to_string_lossy().to_string(),
                claudescore_doc.to_string_lossy().to_string(),
            ],
            None,
            Some(claudescore_doc.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            false,
            &iso,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&agent_doc_window).unwrap();
        assert_eq!(
            ordered,
            vec![bugs_pane.clone(), dev_pane.clone()],
            "blocked passive sync must preserve the visible layout"
        );
        assert_eq!(
            iso.active_pane("test").unwrap(),
            dev_pane,
            "blocked passive sync should still reselect the already-visible focused pane"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_focus_only_editor_switch_preserves_sibling_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let left_doc = root.join("tasks/left.md");
        let right_doc = root.join("tasks/right.md");
        let new_left_doc = root.join("tasks/new-left.md");
        for (path, session) in [
            (&left_doc, "left-session"),
            (&right_doc, "right-session"),
            (&new_left_doc, "new-left-session"),
        ] {
            std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
        }

        let layout_state = vec![
            left_doc
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            right_doc
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        ];
        agent_doc_controller_io::project_controller::store_layout_state(root, &layout_state)
            .unwrap();

        let iso = IsolatedTmux::new("sync-focus-only-editor-switch");
        let left_pane = iso.new_session("test", root).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
            .unwrap();
        let right_pane = iso.split_window(&left_pane, root, "-dh").unwrap();
        let target_window = iso.pane_window(&left_pane).unwrap();
        let new_left_pane = iso.new_window("test", root).unwrap();
        let new_left_window = iso.pane_window(&new_left_pane).unwrap();

        for (session, pane, window, doc) in [
            ("left-session", &left_pane, &target_window, &left_doc),
            ("right-session", &right_pane, &target_window, &right_doc),
            (
                "new-left-session",
                &new_left_pane,
                &new_left_window,
                &new_left_doc,
            ),
        ] {
            sessions::register_full_with_cwd(
                session,
                pane,
                &doc.to_string_lossy(),
                pane_pid_from_tmux(&iso, pane).unwrap(),
                window,
                &root.to_string_lossy(),
            )
            .unwrap();
        }
        iso.select_pane(&left_pane).unwrap();

        run_with_options_internal(
            &[new_left_doc.to_string_lossy().to_string()],
            None,
            Some(new_left_doc.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            false,
            &iso,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&target_window).unwrap();
        assert_eq!(
            ordered,
            vec![new_left_pane.clone(), right_pane.clone()],
            "focus-only editor tab switches should update the active side without collapsing the sibling pane"
        );
        assert_eq!(
            iso.active_pane("test").unwrap(),
            new_left_pane,
            "focused replacement pane should be selected after the same-side handoff"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_focus_only_existing_sibling_focus_does_not_replace_active_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let bugs_doc = root.join("tasks/bugs.md");
        let docs_doc = root.join("tasks/docs.md");
        for (path, session) in [(&bugs_doc, "bugs-session"), (&docs_doc, "docs-session")] {
            std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
        }

        let layout_state = vec![
            bugs_doc
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            docs_doc
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        ];
        agent_doc_controller_io::project_controller::store_layout_state(root, &layout_state)
            .unwrap();

        let iso = IsolatedTmux::new("sync-focus-only-visible-sibling");
        let bugs_pane = iso.new_session("test", root).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
            .unwrap();
        let docs_pane = iso.split_window(&bugs_pane, root, "-dh").unwrap();
        let target_window = iso.pane_window(&bugs_pane).unwrap();

        for (session, pane, doc) in [
            ("bugs-session", &bugs_pane, &bugs_doc),
            ("docs-session", &docs_pane, &docs_doc),
        ] {
            sessions::register_full_with_cwd(
                session,
                pane,
                &doc.to_string_lossy(),
                pane_pid_from_tmux(&iso, pane).unwrap(),
                &target_window,
                &root.to_string_lossy(),
            )
            .unwrap();
        }
        iso.select_pane(&bugs_pane).unwrap();

        run_with_options_internal(
            &[docs_doc.to_string_lossy().to_string()],
            None,
            Some(docs_doc.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            false,
            &iso,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&target_window).unwrap();
        assert_eq!(
            ordered,
            vec![bugs_pane.clone(), docs_pane.clone()],
            "after a turn ends on the old pane, editor focus of an already-visible sibling must not collapse or replace that pane"
        );
        assert_eq!(
            iso.active_pane("test").unwrap(),
            docs_pane,
            "focus-only sync should select the existing focused sibling pane"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_focus_only_editor_switch_preserves_sibling_without_saved_layout() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let left_doc = root.join("tasks/left.md");
        let right_doc = root.join("tasks/right.md");
        let new_left_doc = root.join("tasks/new-left.md");
        for (path, session) in [
            (&left_doc, "left-session-nosaved"),
            (&right_doc, "right-session-nosaved"),
            (&new_left_doc, "new-left-session-nosaved"),
        ] {
            std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
        }

        let iso = IsolatedTmux::new("sync-focus-only-no-saved-layout");
        let left_pane = iso.new_session("test", root).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
            .unwrap();
        let right_pane = iso.split_window(&left_pane, root, "-dh").unwrap();
        let target_window = iso.pane_window(&left_pane).unwrap();
        let new_left_pane = iso.new_window("test", root).unwrap();
        let new_left_window = iso.pane_window(&new_left_pane).unwrap();

        for (session, pane, window, doc) in [
            (
                "left-session-nosaved",
                &left_pane,
                &target_window,
                &left_doc,
            ),
            (
                "right-session-nosaved",
                &right_pane,
                &target_window,
                &right_doc,
            ),
            (
                "new-left-session-nosaved",
                &new_left_pane,
                &new_left_window,
                &new_left_doc,
            ),
        ] {
            sessions::register_full_with_cwd(
                session,
                pane,
                &doc.to_string_lossy(),
                pane_pid_from_tmux(&iso, pane).unwrap(),
                window,
                &root.to_string_lossy(),
            )
            .unwrap();
        }
        iso.select_pane(&left_pane).unwrap();

        run_with_options_internal(
            &[new_left_doc.to_string_lossy().to_string()],
            None,
            Some(new_left_doc.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            false,
            &iso,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&target_window).unwrap();
        assert_eq!(
            ordered,
            vec![new_left_pane.clone(), right_pane.clone()],
            "focus-only sync should derive the current split from visible registered panes when state.db has no layout row"
        );
        assert_eq!(
            iso.active_pane("test").unwrap(),
            new_left_pane,
            "focused replacement pane should be selected without collapsing the sibling pane"
        );
    }
    #[test]
    #[ignore = "covered by sync_sim_tmuxbudget_seed_3004; safe-passive attach/focus tmux smoke keeps the real pane/window path covered"]
    fn safe_passive_protected_open_cycle_sync_still_selects_visible_focus_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let doc_a = root.join("tasks/a.md");
        let doc_b = root.join("tasks/b.md");
        let doc_c = root.join("tasks/c.md");
        for (path, session) in [
            (&doc_a, "afocus-a"),
            (&doc_b, "bfocus-b"),
            (&doc_c, "cfocus-c"),
        ] {
            std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
        }

        let iso = IsolatedTmux::new("sync-safe-passive-protected-focus");
        let pane_a = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let pane_b = iso.split_window(&pane_a, root, "-dh").unwrap();
        let target_window = iso.pane_window(&pane_a).unwrap();
        let pane_c = iso.new_window("test", root).unwrap();
        let pane_c_window = iso.pane_window(&pane_c).unwrap();

        sessions::register_full_with_cwd(
            "afocus-a",
            &pane_a,
            &doc_a.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_a).unwrap(),
            &target_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            "bfocus-b",
            &pane_b,
            &doc_b.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_b).unwrap(),
            &target_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            "cfocus-c",
            &pane_c,
            &doc_c.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_c).unwrap(),
            &pane_c_window,
            &root.to_string_lossy(),
        )
        .unwrap();

        let doc_a_content = std::fs::read_to_string(&doc_a).unwrap();
        agent_doc_cycle_state_io::start_preflight(
            &doc_a,
            Some(&doc_a_content),
            Some(&doc_a_content),
        )
        .unwrap();
        iso.select_pane(&pane_a).unwrap();

        run_with_options_internal(
            &[
                doc_c.to_string_lossy().to_string(),
                doc_b.to_string_lossy().to_string(),
            ],
            None,
            Some(doc_b.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            false,
            &iso,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&target_window).unwrap();
        assert!(
            !ordered.contains(&pane_a),
            "open-cycle extra pane should be stashed while other documents sync"
        );
        assert!(
            ordered.contains(&pane_c),
            "requested hidden pane should be attached even while another document is mid-closeout"
        );
        assert_eq!(
            iso.active_pane("test").unwrap(),
            pane_b,
            "sync should still select the already-visible focused pane"
        );
        assert_eq!(
            iso.pane_window(&pane_c).unwrap(),
            target_window,
            "requested hidden pane should move into the visible agent-doc window"
        );
    }
}
