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
//! - **File rename detection:** The editor path-transition projection is primary.
//!   Compatibility sync can recover a missed event by locating exactly one registry
//!   entry with the same session UUID after the new path-key lookup misses. It reuses
//!   and re-registers that pane only when the old path is absent and the pane still
//!   proves ownership against that old path. Live old paths, duplicate UUID rows, and
//!   weakened ownership fail closed. Editor plugins trigger the primary transition on
//!   `FileRenameEvent` (JB) or `onDidRenameFiles` (VS Code).
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
//! - Exact-visible automatic projections are atomic when ownership is blocked.
//!   They may provision an owner in full mode, but if sync finds a legacy associated
//!   pane that requires explicit repair, it preserves the current tmux layout rather
//!   than reconciling only the resolved subset and collapsing the visible window.
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

use agent_doc_controller::pane_layout::MainLayoutEligibility;
use anyhow::{Context, Result};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
#[cfg(test)]
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
#[cfg(test)]
use tempfile::NamedTempFile;

use crate::CrossRootDocumentPaneObservation;
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
    AutoStartMode, GatedLayoutDecision, RENAME_DEBOUNCE_TTL_SECS,
    SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET, SYNC_DOCTOR_REPAIR_BUDGET, SYNC_LOCK_WAIT_LATENCY_BUDGET,
    SYNC_OWNERSHIP_PROOF_BUDGET, SYNC_PROJECTION_REFRESH_BUDGET, SYNC_PRUNE_BUDGET,
    SYNC_PRUNE_SUBPHASE_BUDGET, SYNC_ROUTER_BUDGET, SYNC_SAFE_PASSIVE_TOTAL_BUDGET,
    SYNC_WINDOW_RESOLUTION_BUDGET, WindowIndexNormalizationPlan, auto_started_panes_summary,
    blocked_owner_layout_decision, destructive_repair_throttle_state_key, effective_sync_columns,
    epoch_millis_now, gated_layout_decision, last_visible_excerpt, latency_budget_status,
    plan_window_index_normalization, planned_stash_window_indices, registry_relative_file_path,
    rename_debounce_expired, sanitize_excerpt, sync_latency_message,
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
pub(crate) mod pane_repair;
pub(crate) use pane_repair::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncRunReport {
    pub applied: bool,
    pub reason: String,
    pub file_panes: Vec<(PathBuf, String)>,
    /// GH #136: column documents the stale-supervisor gate deliberately left
    /// unrealised this pass (see `layout_column_audit::gate_stale_column_panes`).
    pub gated_documents: Vec<PathBuf>,
}

impl Default for SyncRunReport {
    fn default() -> Self {
        Self {
            applied: true,
            reason: "applied".to_string(),
            file_panes: Vec::new(),
            gated_documents: Vec::new(),
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
        let gated_documents = std::mem::take(&mut report.borrow_mut().gated_documents);
        *report.borrow_mut() = SyncRunReport {
            applied: false,
            reason: reason.to_string(),
            file_panes: Vec::new(),
            gated_documents,
        };
    });
}

fn record_sync_gated_documents(gated: &[PathBuf]) {
    LAST_SYNC_RUN_REPORT.with(|report| {
        report.borrow_mut().gated_documents = gated.to_vec();
    });
}

pub fn last_sync_run_report() -> SyncRunReport {
    LAST_SYNC_RUN_REPORT.with(|report| report.borrow().clone())
}

fn log_sync_latency(
    _focus: Option<&str>,
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
    // `#tmuxautosyncreactive`: do NOT mirror per-phase sync latency into the
    // project `ops.log` from the structural-sync hot path. `ops.log` is on disk
    // and contended by every controller thread (CRDT, RPC, idle-watch, editor
    // traffic), so each `log_op` here cost ~100ms under load — the sync body's
    // wall time was dominated by this logging, not by real sync work (measured:
    // a 798ms `prune` phase whose actual prune work was 1ms). The latency data
    // is still recorded by `sync_log` to the tmpfs `/tmp/agent-doc-sync.log`.
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
    let tmux = agent_doc_tmux_io::configured_tmux();
    repair_file_state_with_tmux(&tmux, file)
}

/// Rewrite a document's recorded pane→window binding from where the pane
/// actually is (`#panewindowbindingrebind`).
///
/// `session doctor` reports recorded-vs-live window drift, and the pane-layout
/// focus effect refuses to mirror onto a pane whose binding drifted — both
/// correct, and together they mean a bad binding silently and permanently
/// disables editor→tmux focus sync for that document. Nothing rewrote the
/// binding: `project_binding_in` runs at bind time and never again, so
/// `window_id` freezes at whatever was captured then.
///
/// Measured 2026-08-13: `tasks/haiven-dev.md` was bound `window=@0` on both the
/// actor record and the registry entry. `@0` is not a live window here (`@6` is
/// `agent-doc`, `@9` is `stash`), so the drift predicate was true wherever the
/// pane sat — moving the pane back into the visible window did not help, and
/// `session doctor --repair`, `resync --fix`, and `agent-doc sync` all left it.
/// The sibling `tasks/sdk.md`, bound `window=@6`, worked throughout.
///
/// The live window is the only authority for where a pane IS; that is already
/// why `pane_window_binding_drifted` exists, and this applies the same rule in
/// the repair direction. Re-binding through `project_binding_in` also re-derives
/// the recorded harness, which freezes at bind time for the same reason.
///
/// Conservative in the same way as the predicate it reuses: an unreadable record
/// or a window tmux cannot report is not drift and is never repaired. The
/// generation is preserved because the pane is unchanged — this corrects a
/// stale FACT about a binding, it does not re-elect one.
/// Public entry point, so the doctor can run this BEFORE the document-repair
/// pass. The two are unrelated concerns, and document repair can legitimately
/// fail (an attached editor with no replica, say) — ordering the binding repair
/// behind it made a defect that permanently breaks focus sync unfixable exactly
/// when the document was also unhealthy.
pub fn repair_pane_window_binding(file: &Path) -> Result<Option<String>> {
    let tmux = agent_doc_tmux_io::configured_tmux();
    refresh_drifted_pane_window_binding(&tmux, file)
}

fn refresh_drifted_pane_window_binding(tmux: &Tmux, file: &Path) -> Result<Option<String>> {
    refresh_drifted_pane_window_binding_with_origin(
        tmux,
        file,
        "session-doctor",
        "live_window_rebind",
    )
}

fn refresh_drifted_pane_window_binding_with_origin(
    tmux: &Tmux,
    file: &Path,
    caller: &str,
    reason: &str,
) -> Result<Option<String>> {
    refresh_drifted_pane_window_binding_with_hook(
        file,
        caller,
        reason,
        |pane| agent_doc_tmux_io::target_window_id(tmux, pane),
        &mut || {},
    )
}

/// [`refresh_drifted_pane_window_binding_with_origin`] with the live-window
/// observation injected and a hook between the decision read and the fenced
/// actor write, so tests drive the race without a tmux server.
///
/// `#syncbindingfence`: the window correction keeps the observed pane, session,
/// generation, and lifecycle state, and is compare-and-swapped on the observed
/// record ([`agent_doc_session_actor_io::correct_binding_window_fenced_in`]).
/// Re-binding through `project_binding_in` stamped an owner's `Busy` as `Ready`,
/// and, because it re-reads the generation at write time, bound the observed
/// pane back over an owner that moved the document between the read and the
/// write. A record that changed after the read is left alone; the next pass
/// re-observes it.
fn refresh_drifted_pane_window_binding_with_hook(
    file: &Path,
    caller: &str,
    reason: &str,
    live_window_of: impl Fn(&str) -> Option<String>,
    between_decision_and_write: &mut dyn FnMut(),
) -> Result<Option<String>> {
    let base_dir = match agent_doc_fs::find_project_root_canonical(file) {
        Some(root) => root,
        None => return Ok(None),
    };
    let file_key = file.to_string_lossy().to_string();
    let (Some(record), revision) =
        agent_doc_session_actor_io::load_record_with_revision_in(&base_dir, &file_key)?
    else {
        return Ok(None);
    };
    if record.pane_id.trim().is_empty() {
        return Ok(None);
    }
    let Some(live_window) = live_window_of(&record.pane_id) else {
        return Ok(None);
    };
    let live_window = live_window.trim().to_string();
    if live_window.is_empty() {
        return Ok(None);
    }

    let registry_window = registry_window_for_pane(&base_dir, &record.pane_id)?;
    let actor_drifted = agent_doc_controller::pane_layout::pane_window_binding_drifted(
        &record.window_id,
        Some(&live_window),
    );
    let registry_drifted = registry_window.as_deref().is_some_and(|window| {
        agent_doc_controller::pane_layout::pane_window_binding_drifted(window, Some(&live_window))
    });
    if !actor_drifted && !registry_drifted {
        return Ok(None);
    }

    let recorded = record.window_id.clone();
    between_decision_and_write();
    let mut actor_rebound = false;
    if actor_drifted {
        match agent_doc_session_actor_io::correct_binding_window_fenced_in(
            &base_dir,
            &file_key,
            &record,
            revision,
            &live_window,
            caller,
            reason,
        ) {
            Ok(_) => actor_rebound = true,
            Err(err) if agent_doc_session_actor_io::binding_fence_lost(&err) => {
                sync_log(&format!(
                    "pane_window_binding_rebind_superseded file={} pane={} generation={} err={}",
                    file.display(),
                    record.pane_id,
                    record.generation,
                    err
                ));
            }
            Err(err) => return Err(err),
        }
    }
    // The registry half is a physical observation about the pane, not a
    // binding decision: tmux says where the pane IS regardless of the record.
    let registry_rebound =
        registry_drifted && rebind_registry_pane_window(&base_dir, &record.pane_id, &live_window)?;
    if !actor_rebound && !registry_rebound {
        return Ok(None);
    }

    Ok(Some(format!(
        "Rebound pane `{}` for `{}` from stale window `{}` to its live window `{}`.",
        record.pane_id,
        file.display(),
        recorded,
        live_window,
    )))
}

/// Publish the physical pane window observed after layout reconciliation.
///
/// tmux preserves a pane id while `join-pane`, `move-pane`, and `break-pane`
/// change its window. The structural effect therefore has to refresh the actor
/// and registry projections before returning success; otherwise focus sync sees
/// a permanently stale window even though the pane is visibly in place.
fn refresh_synced_pane_window_bindings(
    tmux: &Tmux,
    file_panes: &[(PathBuf, String)],
) -> Result<Vec<String>> {
    let mut seen = std::collections::HashSet::new();
    let mut notes = Vec::new();
    for (file, _) in file_panes {
        if !seen.insert(file.clone()) {
            continue;
        }
        if let Some(note) = refresh_drifted_pane_window_binding_with_origin(
            tmux,
            file,
            "sync",
            "post_layout_live_window_rebind",
        )? {
            sync_log(&format!(
                "post_sync_window_binding_rebound file={} note={}",
                file.display(),
                note
            ));
            notes.push(note);
        }
    }
    Ok(notes)
}

/// The window the durable registry records for `pane`, if it has an entry.
fn registry_window_for_pane(base_dir: &Path, pane: &str) -> Result<Option<String>> {
    let registry = agent_doc_session_registry_io::load_in(base_dir)?;
    Ok(registry
        .values()
        .find(|entry| entry.pane == pane)
        .map(|entry| entry.window.clone()))
}

/// Point the registry entry for `pane` at `live_window`, under the registry lock.
/// Returns whether an entry changed.
fn rebind_registry_pane_window(base_dir: &Path, pane: &str, live_window: &str) -> Result<bool> {
    let registry_path = agent_doc_session_registry_io::registry_path_in(base_dir);
    let _lock = tmux_router::RegistryLock::acquire(&registry_path)?;
    let mut registry = agent_doc_session_registry_io::load_in(base_dir)?;
    let Some(key) = registry
        .iter()
        .find(|(_, entry)| entry.pane == pane)
        .map(|(key, _)| key.clone())
    else {
        return Ok(false);
    };
    let Some(entry) = registry.get_mut(&key) else {
        return Ok(false);
    };
    if entry.window == live_window {
        return Ok(false);
    }
    entry.window = live_window.to_string();
    agent_doc_session_registry_io::save_in(base_dir, &registry)?;
    Ok(true)
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
    if let Some(session_name) = resolve_sync_target_session(tmux, None, &columns, None, &[])?
        && repair_layout(tmux, &session_name, "agent-doc")?.repaired()
    {
        // `#syncdoctorconverged`: report the action only when the pass actually
        // moved something. An unconditional note made the sync log claim a
        // repair on every sync, which is indistinguishable from a repair that
        // never converges.
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
    run_with_options_internal(
        col_args,
        window,
        focus,
        AutoStartMode::Full,
        false,
        true,
        &agent_doc_tmux_io::configured_tmux(),
    )
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
    run_with_options_internal_at_root(
        project_root,
        col_args,
        window,
        focus,
        AutoStartMode::Full,
        false,
        true,
        &[],
        &MainLayoutEligibility::default(),
        &agent_doc_tmux_io::configured_tmux(),
    )
}

pub fn run_provision_only_in_project_root(
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
        AutoStartMode::Full,
        false,
        false,
        &[],
        &MainLayoutEligibility::default(),
        &agent_doc_tmux_io::configured_tmux(),
    )
}

pub fn run_provision_only_exact_visible_with_actor_bindings_in_project_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
) -> Result<()> {
    run_with_options_internal_at_root(
        project_root,
        col_args,
        window,
        focus,
        AutoStartMode::Full,
        true,
        false,
        actor_bindings,
        &MainLayoutEligibility::default(),
        &agent_doc_tmux_io::configured_tmux(),
    )
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
        false,
        &[],
        &MainLayoutEligibility::default(),
        &agent_doc_tmux_io::configured_tmux(),
    )
}

/// Record a debounce lease for a file that was just renamed.
/// Subsequent syncs within 5s will skip auto-start for this file.
pub fn write_rename_debounce(file_path: &str) {
    if let Err(err) =
        write_rename_debounce_at(file_path, agent_doc_sqlite::state_store::timestamp_secs())
    {
        eprintln!("[sync] warning: rename debounce state not recorded for {file_path}: {err:#}");
        sync_log(&format!(
            "rename-debounce: record failed for {file_path} error={err:#}"
        ));
    }
}

/// Clock-injected core of [`write_rename_debounce`]: records the lease with
/// `now_secs` as its heartbeat so tests can pin the debounce window instead of
/// racing the wall clock. `Ok(false)` when the file has no state hash or no
/// project root (nothing to debounce); a state.db failure is an error rather
/// than a silently missing lease.
fn write_rename_debounce_at(file_path: &str, now_secs: u64) -> anyhow::Result<bool> {
    let file = Path::new(file_path);
    let hash = agent_doc_fs::document_state_hash(file).unwrap_or_default();
    if hash.is_empty() {
        return Ok(false);
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Ok(false);
    };
    agent_doc_controller_io::project_controller::upsert_coordination_lease(
        &project_root,
        &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
            scope_kind: "rename_debounce".to_string(),
            scope_id: hash.clone(),
            holder: file_path.to_string(),
            holder_pid: Some(std::process::id()),
            heartbeat_secs: now_secs,
        },
    )?;
    eprintln!(
        "[sync] rename debounce state recorded for {} ({})",
        file_path, hash
    );
    sync_log(&format!(
        "rename-debounce: recorded lease for {} hash={}",
        file_path, hash
    ));
    Ok(true)
}

/// Check if a file has an active rename debounce lease (within TTL).
fn has_rename_debounce(file_path: &Path) -> bool {
    has_rename_debounce_at(file_path, agent_doc_sqlite::state_store::timestamp_secs())
}

/// Clock-injected core of [`has_rename_debounce`]: the lease is active while
/// `now_secs - heartbeat_secs < RENAME_DEBOUNCE_TTL_SECS`.
fn has_rename_debounce_at(file_path: &Path, now_secs: u64) -> bool {
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
    let age = Duration::from_secs(now_secs.saturating_sub(lease.heartbeat_secs));
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
    run_layout_only_exact_visible_with_tmux_in_project_root(
        project_root,
        col_args,
        window,
        focus,
        &agent_doc_tmux_io::configured_tmux(),
    )
}

pub fn run_layout_only_exact_visible_with_tmux_in_project_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    run_layout_only_exact_visible_with_actor_bindings_and_tmux_in_project_root(
        project_root,
        col_args,
        window,
        focus,
        &[],
        tmux,
    )
}

pub fn run_layout_only_exact_visible_with_actor_bindings_and_tmux_in_project_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
    tmux: &Tmux,
) -> Result<()> {
    run_with_options_internal_at_root(
        project_root,
        col_args,
        window,
        focus,
        AutoStartMode::SafePassive,
        true,
        false,
        actor_bindings,
        &MainLayoutEligibility::default(),
        tmux,
    )
}

pub fn run_layout_only_exact_visible_with_actor_bindings_in_project_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
) -> Result<()> {
    run_layout_only_exact_visible_with_actor_bindings_and_tmux_in_project_root(
        project_root,
        col_args,
        window,
        focus,
        actor_bindings,
        &agent_doc_tmux_io::configured_tmux(),
    )
}

/// Controller effect entry point carrying the retained main-layout exclusion
/// projection into the sync process. This is deliberately one typed boundary:
/// standalone/older callers retain all-eligible behavior, while the controller
/// cannot accidentally drop the projection when changing sync mode.
#[allow(clippy::too_many_arguments)]
pub fn run_controller_layout_with_main_layout_eligibility_in_project_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    no_autostart: bool,
    exact_visible_target: bool,
    route_created_panes: bool,
    actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
    main_layout_eligibility: &MainLayoutEligibility,
) -> Result<()> {
    run_with_options_internal_at_root(
        project_root,
        col_args,
        window,
        focus,
        if no_autostart {
            AutoStartMode::SafePassive
        } else {
            AutoStartMode::Full
        },
        exact_visible_target,
        route_created_panes,
        actor_bindings,
        main_layout_eligibility,
        &agent_doc_tmux_io::configured_tmux(),
    )
}

pub fn run_with_tmux(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    run_with_options_internal(
        col_args,
        window,
        focus,
        AutoStartMode::Full,
        false,
        false,
        tmux,
    )
}

fn load_live_authoritative_actor_record_uncached(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
) -> Option<agent_doc_controller::actor::ActorRecord> {
    let record = load_live_authoritative_actor_record_for_file_uncached(tmux, file)?;
    (record.session_id == session_id).then_some(record)
}

/// Resolve the controller's hot file→actor projection without first reading
/// document frontmatter.
///
/// The actor binding is keyed by the canonical document path and already
/// carries the session id. Requiring a caller to rediscover that id from disk
/// before consulting the actor would invert editor authority for unsaved
/// buffers and add an avoidable content RPC to every exact-visible tab switch.
fn load_live_authoritative_actor_record_for_file_uncached(
    tmux: &Tmux,
    file: &Path,
) -> Option<agent_doc_controller::actor::ActorRecord> {
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
    if !pane_projection_is_reusable_for_document(tmux, &record.pane_id, file) {
        return None;
    }
    Some(record)
}

fn pane_projection_reuse_allowed(pane_alive: bool, pane_runs_other_document: bool) -> bool {
    pane_alive && !pane_runs_other_document
}

/// Decide whether a controller/registry projection may be reused as physical
/// pane ownership for `file`.
///
/// A live tmux id is not sufficient: pane ids are server-global and a stale
/// actor or registry projection can outlive the document session that used to
/// own it. Reusing that projection would relabel another document's live pane,
/// make the layout observer report false convergence, and then make editor
/// routing fail at the final foreign-owner guard. Physical process ownership is
/// therefore the freshness witness at every exact-visible reuse boundary.
fn pane_projection_is_reusable_for_document(tmux: &Tmux, pane_id: &str, file: &Path) -> bool {
    let pane_alive = tmux.pane_alive(pane_id);
    let pane_runs_other_document =
        pane_alive && pane_runs_other_document_owner(tmux, pane_id, file);
    if pane_runs_other_document {
        sync_log(&format!(
            "stale_pane_projection_rejected file={} pane={} reason=foreign_document_owner",
            file.display(),
            pane_id,
        ));
    }
    pane_projection_reuse_allowed(pane_alive, pane_runs_other_document)
}

/// `#ownershipbindingfirst`: resolve the actor record from the document path
/// alone, memoized for the run.
///
/// The by-session variant below cannot serve the ownership hot path, because
/// obtaining its `session_id` argument is exactly what costs the ~540-620ms
/// `resolve_current_document` round-trip. The binding already carries the
/// session id, so keying the cache by file removes that ordering constraint.
fn load_live_authoritative_actor_record_for_file_cached(
    tmux: &Tmux,
    file: &Path,
    proof_cache: &SyncProofCache,
) -> Option<agent_doc_controller::actor::ActorRecord> {
    if proof_cache.skip_authoritative_actor_lookup {
        return None;
    }
    let key = sync_proof_file_key(file);
    if let Some(record) = proof_cache.actor_records_by_file.borrow().get(&key) {
        return record.clone();
    }

    let record = load_live_authoritative_actor_record_for_file_uncached(tmux, file);
    proof_cache
        .actor_records_by_file
        .borrow_mut()
        .insert(key, record.clone());
    record
}

fn load_live_authoritative_actor_record_cached(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    proof_cache: &SyncProofCache,
) -> Option<agent_doc_controller::actor::ActorRecord> {
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

/// A stale supervisor pane must not become recycle-overdue while the owning
/// controller proves that it is still interacting with the operator. The
/// short-lived turn marker is the fast path, but turns and permission prompts
/// can outlive that marker. The foreground blocker used by safe-boundary
/// replacement and the generation-fenced actor's live lease are the fallbacks.
fn pane_interaction_is_active(
    tmux: &Tmux,
    caller_project_root: &Path,
    file: &Path,
    pane: &str,
    proof_cache: &SyncProofCache,
) -> bool {
    let owner_root = agent_doc_project_root_io::project_root_containing(file)
        .unwrap_or_else(|| caller_project_root.to_path_buf());
    if agent_doc_turn_status_io::turn_active_for_pane(&owner_root, pane) {
        return true;
    }

    // The safe-boundary replacement path also refuses to interrupt harness
    // blockers. Observe the same foreground truth here so a long permission
    // prompt or active turn cannot become "overdue" merely because its short
    // coordination marker and transition heartbeat aged out.
    if agent_doc_tmux_io::capture_pane(tmux, pane)
        .ok()
        .is_some_and(|capture| {
            let harness = resolve_harness_for_sync(file);
            let blocker = harness.dispatch_blocker_reason(&capture);
            if let Some(reason) = blocker.as_deref() {
                sync_log(&format!(
                    "stale_column_interaction_proven file={} pane={} source=harness_blocker reason={}",
                    file.display(),
                    pane,
                    reason.replace(' ', "_")
                ));
            }
            blocker.is_some()
        })
    {
        return true;
    }

    let Some(record) =
        load_live_authoritative_actor_record_for_file_cached(tmux, file, proof_cache)
    else {
        return false;
    };
    if record.pane_id != pane
        || !matches!(
            record.state,
            agent_doc_controller::actor::ActorState::Busy
                | agent_doc_controller::actor::ActorState::WaitingInput
        )
    {
        return false;
    }

    let Ok(conn) = agent_doc_sqlite::state_store::open_state_db(&owner_root) else {
        return false;
    };
    let Ok(Some(lease)) = agent_doc_sqlite::state_store::load_supervisor_lease_from_db(
        &conn,
        &record.document_id,
        record.generation,
    ) else {
        return false;
    };
    interaction_actor_lease_is_live(&record, pane, &lease)
}

fn interaction_actor_lease_is_live(
    record: &agent_doc_controller::actor::ActorRecord,
    pane: &str,
    lease: &agent_doc_sqlite::state_store::SupervisorLeaseStatus,
) -> bool {
    interaction_actor_lease_is_live_at(
        record,
        pane,
        lease,
        agent_doc_sqlite::state_store::timestamp_secs(),
        lease
            .supervisor_pid
            .is_some_and(agent_doc_controller_io::process::process_is_alive),
    )
}

fn interaction_actor_lease_is_live_at(
    record: &agent_doc_controller::actor::ActorRecord,
    pane: &str,
    lease: &agent_doc_sqlite::state_store::SupervisorLeaseStatus,
    now: u64,
    supervisor_process_alive: bool,
) -> bool {
    let actor_interactive = record.pane_id == pane
        && matches!(
            record.state,
            agent_doc_controller::actor::ActorState::Busy
                | agent_doc_controller::actor::ActorState::WaitingInput
        );
    let lease_interactive = matches!(
        lease.runtime_state.as_deref(),
        Some("busy" | "waiting_input")
    );
    actor_interactive
        && lease.generation == record.generation
        && lease_interactive
        && agent_doc_controller::status::supervisor_lease_is_fresh_and_alive(
            lease.last_heartbeat,
            supervisor_process_alive,
            now,
            agent_doc_controller_io::project_controller::SUPERVISOR_LEASE_GUARD_STALE_AFTER,
        )
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

/// Bring the durable registry back in line with the authoritative actor record.
fn refresh_durable_registry_for_actor_record(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    record: &agent_doc_controller::actor::ActorRecord,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
) {
    let actor_pane = record.pane_id.as_str();
    if lookup_registry_entry_for_file_session(file, session_id)
        .as_ref()
        .map(|entry| entry.pane.as_str())
        == Some(actor_pane)
    {
        return;
    }
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
    if let Err(err) = refresh_registry_for_actor_pane(tmux, file, session_id, actor_pane) {
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

/// Resolve the actor pane on the paths that still need document content first.
///
/// `#ownershipbindingfirst` moved the by-session actor lookup ahead of the
/// content RPC, so by the time this runs the binding has already been consulted
/// and missed (or was deliberately skipped). What remains are the two
/// exact-visible shapes, which are keyed off the editor projection rather than
/// the actor store.
fn resolve_actor_pane_after_content(
    file_path: &Path,
    exact_visible_target: bool,
    is_cross_root: bool,
) -> Result<Option<String>> {
    if !exact_visible_target {
        // `#ownershipbindingfirst`: the binding-first lookup above already
        // asked the controller for this document and got nothing back. Asking
        // again would repeat the same RPC — including its 5s timeout — for a
        // miss we have already observed this run.
        sync_log(&format!(
            "ownership_binding_first_miss file={} source=controller_actor_lookup",
            file_path.display(),
        ));
        return Ok(None);
    }
    if !is_cross_root {
        // `#tmuxautosyncreactive` / `#lazily-hot-path`: the editor projection is
        // the controller's domain. The reactive bindings (actor store +
        // structural receipt) already named every pane the controller created;
        // if none matched, no controller-managed pane exists. Skip the
        // exhaustive `/proc` ownership walk (measured at ~4.4s for a single
        // cold-start file) and go straight to provisioning. The walk only
        // prevents duplicate panes — but the controller knows it hasn't created
        // one.
        sync_log(&format!(
            "exact_visible_ownership_walk_skipped file={} reason=no_reactive_binding",
            file_path.display(),
        ));
        return Ok(None);
    }
    // The exact-visible pre-IPC branch already made the one permitted bounded
    // owning-controller observation for this run. Repeating it here would turn
    // a miss or deadline into a second serial round trip.
    sync_log(&format!(
        "exact_visible_cross_root_actor_lookup_not_repeated file={}",
        file_path.display(),
    ));
    Ok(None)
}

fn observe_cross_root_document_pane(
    owner_root: &Path,
    file_path: &Path,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
    source: &str,
) -> Result<Option<agent_doc_controller_io::project_controller::ControllerTmuxActorBinding>> {
    let started = Instant::now();
    let observation = crate::runtime_effects()?.resolve_cross_root_document_pane(
        owner_root,
        file_path,
        agent_doc_sync::SYNC_CROSS_ROOT_BINDING_DEADLINE,
    )?;
    log_sync_latency(
        focus,
        "cross_root_actor_binding_rpc",
        started.elapsed(),
        agent_doc_sync::SYNC_CROSS_ROOT_BINDING_DEADLINE,
        auto_start_mode,
    );
    Ok(match observation {
        CrossRootDocumentPaneObservation::Found(binding) => Some(binding),
        CrossRootDocumentPaneObservation::Missing => {
            sync_log(&format!(
                "{source}_cross_root_actor_projection_missing file={}",
                file_path.display(),
            ));
            None
        }
        CrossRootDocumentPaneObservation::DeadlineExceeded { deadline } => {
            sync_log(&format!(
                "{source}_cross_root_actor_projection_deadline_exceeded file={} deadline_ms={}",
                file_path.display(),
                deadline.as_millis(),
            ));
            None
        }
    })
}

/// `#ownershipbindingfirst`: resolve the authoritative actor binding for a
/// document *before* its content has been fetched.
///
/// Identical to [`project_authoritative_actor_binding`] except that it does not
/// take a `session_id` — it returns the whole record, whose `session_id` is the
/// value the caller would otherwise have paid an editor content RPC to
/// rediscover from frontmatter. `rpc.rs::authoritative_actor_binding` states the
/// rule this implements: the binding is keyed by canonical document path and
/// already carries the session id, and making a caller rediscover that id first
/// "would invert editor authority for unsaved buffers and add an avoidable
/// content RPC".
///
/// Ownership remains fail-closed: the underlying loader returns `None` unless
/// the bound pane is alive, so a stale binding can never route a document.
fn project_authoritative_actor_binding_by_file(
    tmux: &Tmux,
    file: &Path,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
    proof_cache: &SyncProofCache,
) -> Option<agent_doc_controller::actor::ActorRecord> {
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
    // `specs/07-session-tmux-commands.md`: safe-passive document binding must
    // use the live local actor projection instead of a controller RPC on the
    // editor fast path, logging `source=local_projection` plus an immediate
    // `controller_actor_lookup` success sample. Like the pre-reorder path, this
    // shortcut returns before the durable registry refresh.
    if matches!(auto_start_mode, AutoStartMode::SafePassive)
        && let Some(record) =
            load_live_authoritative_actor_record_for_file_cached(tmux, file, proof_cache)
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
            record.pane_id
        ));
        return Some(record);
    }

    let lookup_start = Instant::now();
    let record = load_live_authoritative_actor_record_for_file_cached(tmux, file, proof_cache);
    log_sync_latency(
        focus,
        "controller_actor_lookup",
        lookup_start.elapsed(),
        SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET,
        auto_start_mode,
    );
    let record = record?;
    refresh_durable_registry_for_actor_record(
        tmux,
        file,
        &record.session_id.clone(),
        &record,
        focus,
        auto_start_mode,
    );
    Some(record)
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

/// Whether a layout repair pass actually issued mutating tmux commands.
///
/// `#syncdoctorconverged`: the pre- and post-reconcile doctor passes used to
/// report `Repaired ... layout` unconditionally, so the sync log claimed a
/// repair on every single sync and the message could never be used to tell a
/// real repair from a no-op. Callers now report only what happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutRepairOutcome {
    AlreadyConverged,
    Repaired,
}

impl LayoutRepairOutcome {
    pub fn repaired(self) -> bool {
        matches!(self, Self::Repaired)
    }
}

pub fn repair_layout(
    tmux: &Tmux,
    session_name: &str,
    target_window_name: &str,
) -> Result<LayoutRepairOutcome> {
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
        "#{window_id} #{window_panes} #{window_height} #{automatic-rename} #{allow-rename} #{window_width} #{window-size} #{window_name}",
    );
    let window_list = match output {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "[repair] failed to list windows for session {}: {}",
                session_name, e
            );
            return Ok(LayoutRepairOutcome::AlreadyConverged);
        }
    };
    let mut mutated = false;

    // Parse window identity, sizing, and effective role-name options.
    struct WinInfo {
        id: String,
        name: String,
        _pane_count: usize,
        height: String,
        automatic_rename: bool,
        allow_rename: bool,
        width: String,
        /// Effective (inherited) `window-size`; GH #244 only probes the local
        /// option when this is `manual`, so the converged path costs nothing.
        effective_window_size: String,
    }
    let windows: Vec<WinInfo> = window_list
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(8, ' ');
            let id = parts.next()?.to_string();
            let pane_count: usize = parts.next()?.parse().ok()?;
            let height = parts.next()?.to_string();
            let automatic_rename = parts.next()? == "1";
            let allow_rename = parts.next()? == "1";
            let width = parts.next()?.to_string();
            let effective_window_size = parts.next()?.to_string();
            let name = parts.next()?.to_string();
            Some(WinInfo {
                id,
                name,
                _pane_count: pane_count,
                height,
                automatic_rename,
                allow_rename,
                width,
                effective_window_size,
            })
        })
        .collect();

    // Attached clients are listed at most once, and only when an owned window
    // carries an effective `manual` window-size (GH #244).
    let mut attached_clients: Option<Vec<agent_doc_tmux::AttachedClientViewport>> = None;
    // `#stashresizerestore`: heal windows pinned by binaries that used the
    // 1000-row join workaround without fully removing tmux's manual-size
    // override. Restrict recovery to the exact staging height and agent-doc's
    // owned target/stash windows so an operator's unrelated manual layout is
    // never normalized.
    for window in &windows {
        let agent_doc_owned =
            window.name == target_window_name || is_stash_window_name(&window.name);
        // Pin role names before resize wakes children or consolidation surveys
        // windows again. Otherwise tmux can rename a stash after its foreground
        // `agent-doc` process and the next phase bulk-promotes its sessions.
        if agent_doc_owned {
            for (option, enabled) in [
                ("automatic-rename", window.automatic_rename),
                ("allow-rename", window.allow_rename),
            ] {
                if enabled {
                    tmux.raw_cmd(&["set-window-option", "-t", &window.id, option, "off"])
                        .with_context(|| {
                            format!("pin managed window {} option {option}", window.id)
                        })?;
                    mutated = true;
                    sync_log(&format!(
                        "layout_repair_pinned_window_name window={} name={} option={option}",
                        window.id, window.name
                    ));
                }
            }
        }
        if agent_doc_owned && window.height == STASH_JOIN_TEMP_HEIGHT {
            match agent_doc_tmux_io::resize_window_to_clients(tmux, &window.id) {
                Ok(()) => {
                    mutated = true;
                    sync_log(&format!(
                        "layout_repair_unpinned_window window={} name={} stale_height={}",
                        window.id, window.name, window.height
                    ))
                }
                Err(err) => eprintln!(
                    "[repair] warning: could not unpin stale 1000-row window {} ({}): {}",
                    window.id, window.name, err
                ),
            }
        } else if agent_doc_owned && window.effective_window_size == "manual" {
            // GH #244: a per-window `window-size manual` pin (left by tmux's
            // `resize-window -A`, e.g. an operator's "fit window" key) holds
            // the window at an old client size: dot-padded in a larger client,
            // cropped in a smaller one, or both when `-A` ran with two
            // differently sized clients attached (GH #268). Release the pin
            // unless some client attached to this session shows the window at
            // exactly the pinned size.
            let clients = attached_clients.get_or_insert_with(|| {
                agent_doc_tmux_io::list_attached_client_viewports(tmux, session_name)
            });
            let local_window_size = agent_doc_tmux_io::local_window_size_option(tmux, &window.id);
            let (Ok(width), Ok(height)) = (window.width.parse(), window.height.parse()) else {
                continue;
            };
            if agent_doc_tmux::manual_window_pin_needs_refit(
                local_window_size.as_deref(),
                width,
                height,
                clients,
            ) {
                match agent_doc_tmux_io::resize_window_to_clients(tmux, &window.id) {
                    Ok(()) => {
                        mutated = true;
                        sync_log(&format!(
                            "layout_repair_window_size_pin_released session={} window={} name={} pinned={}x{} clients=[{}]",
                            session_name,
                            window.id,
                            window.name,
                            width,
                            height,
                            clients
                                .iter()
                                .map(|client| {
                                    let (width, height) = client.window_area();
                                    format!("{width}x{height}")
                                })
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    }
                    Err(err) => eprintln!(
                        "[repair] warning: could not release window-size pin on {} ({}): {}",
                        window.id, window.name, err
                    ),
                }
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
        // Target exists and stash is consolidated. Skip Phases 1+2, and — when
        // one window observation proves the always-run phases below would not
        // move anything either — return without issuing any tmux command at
        // all (`#syncdoctorconverged`). The steady state is by far the common
        // case: this pass runs twice per full sync, so an editor tab switch was
        // paying a dozen redundant tmux round-trips per gesture to rediscover a
        // layout that was already correct.
        if !mutated
            && agent_doc_sync::session_layout_normalization_converged(
                &list_session_windows(tmux, session_name),
                target_window_name,
                is_stash_window_name,
            )
        {
            sync_log(&format!(
                "layout_repair_already_converged session={session_name} target={target_window_name} #syncdoctorconverged"
            ));
            return Ok(LayoutRepairOutcome::AlreadyConverged);
        }
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
                    return Ok(LayoutRepairOutcome::Repaired);
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
                        return Ok(LayoutRepairOutcome::Repaired);
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

    Ok(LayoutRepairOutcome::Repaired)
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

/// Give displaced open-cycle owners a stable hidden role before tmux can
/// auto-rename their new window from the foreground `agent-doc` process.
/// Otherwise the post-sync doctor sees a false duplicate target and joins the
/// protected pane straight back into the exact-visible editor projection.
fn pin_displaced_open_cycle_windows_as_stash(
    tmux: &Tmux,
    target_window: Option<&str>,
    pane_ids: &HashSet<String>,
) -> Result<()> {
    let target_window_id =
        target_window.and_then(|target| agent_doc_tmux_io::target_window_id(tmux, target));
    let mut pinned_windows = HashSet::new();
    for pane_id in pane_ids {
        let Some(window_id) = agent_doc_tmux_io::target_window_id(tmux, pane_id) else {
            continue;
        };
        if target_window_id.as_deref() == Some(window_id.as_str())
            || !pinned_windows.insert(window_id.clone())
        {
            continue;
        }
        for option in ["automatic-rename", "allow-rename"] {
            tmux.raw_cmd(&["set-window-option", "-t", &window_id, option, "off"])
                .with_context(|| {
                    format!("failed to pin displaced open-cycle window {window_id} option {option}")
                })?;
        }
        agent_doc_tmux_io::rename_window(tmux, &window_id, "stash").with_context(|| {
            format!("failed to classify displaced open-cycle window {window_id} as stash")
        })?;
        sync_log(&format!(
            "reconcile_open_cycle_window_stashed window={} reason=protected_owner_displaced",
            window_id
        ));
    }
    Ok(())
}

fn sync_log(msg: &str) {
    crate::append_sync_log(msg);
}

/// `#tmuxpanesreversed`: the post-operation check of the pane-order
/// invariant (SPEC: after every layout operation the wanted panes, left to
/// right by `pane_left`, are the wanted column order). Re-reads geometry,
/// repairs a complete-but-misordered window with `swap-pane`, and writes one
/// `tmux_pane_order` line per layout pass to ops.log with the host's tmux
/// version, base indices, wanted order, and positions before and after. A
/// violation that survives the repair is logged as an error, never silently
/// accepted.
fn enforce_synced_pane_order(
    tmux: &Tmux,
    project_root: &Path,
    col_args: &[String],
    result: &tmux_router::SyncResult,
    requested_window: Option<&str>,
    before: Option<agent_doc_tmux::pane_order::PaneOrderSnapshot>,
) {
    use agent_doc_tmux_io::pane_order::PaneOrderOutcome;
    let wanted = agent_doc_tmux::pane_order::wanted_pane_order(col_args, &result.file_panes);
    // A `before` read of a different window than the one tmux-router chose
    // describes nothing about this pass.
    let before = before.filter(|_| requested_window == Some(result.target_window.as_str()));
    let pass = agent_doc_tmux_io::pane_order::enforce_pane_order(
        tmux,
        &result.target_window,
        &wanted,
        before,
    );
    let line = pass.log_line();
    sync_log(&line);
    let _ = agent_doc_ops_log_io::append_ops_log_at_project(
        project_root,
        &line,
        agent_doc_ops_log_io::OpsLogTracking::default(),
    );
    match pass.outcome {
        PaneOrderOutcome::Violated => {
            eprintln!(
                "[sync] ERROR: tmux pane order still violates the wanted column order after repair: {line}"
            );
            tracing::error!(line = %line, "tmux pane order invariant violated");
        }
        PaneOrderOutcome::Repaired => {
            eprintln!("[sync] repaired tmux pane order: {line}");
        }
        _ => {}
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

/// The caller pane's session, or tmux's ambient session preferring one a
/// client is attached to (GH #243; see `agent_doc_tmux_io::client_visibility`).
fn current_tmux_session_name(tmux: &Tmux) -> Option<String> {
    agent_doc_tmux_io::ambient_current_session_for_process(tmux).map(|ambient| ambient.session)
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

fn exact_visible_safe_passive(auto_start_mode: AutoStartMode, exact_visible_target: bool) -> bool {
    matches!(auto_start_mode, AutoStartMode::SafePassive) && exact_visible_target
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

/// Load the multi-session policy (`tmux_sessions`, GH #17) for a project root.
/// A missing or empty list is the single-session default.
pub fn tmux_session_policy_for_root(root: &Path) -> agent_doc_sync::TmuxSessionPolicy {
    let config_path = root.join(".agent-doc").join("config.toml");
    let config = agent_doc_project_config_io::load_project_from(&config_path);
    let navigation_lanes = agent_doc_project_config_io::editor_navigation_lanes(&config);
    agent_doc_sync::TmuxSessionPolicy::new(config.tmux_sessions)
        .with_navigation_lanes(navigation_lanes)
}

/// Load the multi-session policy that governs a sync/route candidate set: the
/// `.agent-doc` root shared by the documents, else the focused document's
/// project, else the cwd project.
pub fn tmux_session_policy_for_scope(
    col_args: &[String],
    focus: Option<&str>,
) -> agent_doc_sync::TmuxSessionPolicy {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    agent_doc_sync::sync_scope_root(col_args, focus, &cwd)
        .map(|root| tmux_session_policy_for_root(&root))
        .unwrap_or_default()
}

/// A document's own `tmux_session` frontmatter binding, if any.
pub fn document_tmux_session_binding(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let (fm, _) = frontmatter::parse(&content).ok()?;
    fm.tmux_session
        .map(|session| session.trim().to_string())
        .filter(|session| !session.is_empty())
}

/// The document binding that participates in session selection: the focused
/// document's, else the first bound column document's. Always `None` in
/// single-session mode so default routing is unchanged.
pub fn scoped_document_tmux_session_binding(
    policy: &agent_doc_sync::TmuxSessionPolicy,
    col_args: &[String],
    focus: Option<&str>,
) -> Option<String> {
    if !policy.is_multi_session() {
        return None;
    }
    agent_doc_sync::sync_candidate_files(col_args, focus)
        .iter()
        .find_map(|path| document_tmux_session_binding(path))
}

/// Multi-session sync scope (GH #17): drop layout columns whose documents are
/// bound to a different allowed session than the one this sync arranges. Each
/// such document keeps its pane in its own session. No-op in single-session
/// mode.
pub fn drop_columns_bound_to_other_sessions(
    policy: &agent_doc_sync::TmuxSessionPolicy,
    col_args: Vec<String>,
    target_session: &str,
) -> (Vec<String>, Vec<(String, String)>) {
    if !policy.is_multi_session() {
        return (col_args, Vec::new());
    }
    let mut dropped = Vec::new();
    let kept = col_args
        .into_iter()
        .filter_map(|column| {
            let files: Vec<&str> = column
                .split(',')
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .collect();
            let mut kept_files = Vec::new();
            for file in files {
                let binding = document_tmux_session_binding(Path::new(file));
                if policy.column_bound_to_other_session(binding.as_deref(), target_session) {
                    dropped.push((file.to_string(), binding.unwrap_or_default()));
                } else {
                    kept_files.push(file);
                }
            }
            (!kept_files.is_empty()).then(|| kept_files.join(","))
        })
        .collect();
    (kept, dropped)
}

/// `#ridernavp6switchback`: in `shared` navigation mode, retarget a sync whose
/// resolved session is an installation lane session agent-doc created (marked
/// for this root) to the project session.
///
/// After `editor_navigation` switches back to `shared`, lane sessions survive
/// until `agent-doc gc` folds them. Without this, a sync with no explicit
/// window follows the focused document's pane (or the tmux client's current
/// session) into the stale lane and joins the project session's panes into it.
/// Returns the project session when the target was retargeted, else `None`.
/// The decision is the pure [`agent_doc_editor_surface::shared_sync_target_session`].
pub fn shared_mode_stale_lane_target(
    tmux: &Tmux,
    project_root: &Path,
    resolved_session: &str,
) -> Option<String> {
    let config = agent_doc_project_config_io::load_project_from(
        &project_root.join(".agent-doc").join("config.toml"),
    );
    let (mode, _) = agent_doc_project_config_io::editor_navigation_mode(&config);
    let project_session = config
        .tmux_session
        .as_deref()
        .map(str::trim)
        .filter(|session| !session.is_empty())?;
    if mode != agent_doc_editor_surface::NavigationMode::Shared
        || resolved_session == project_session
        || !agent_doc_editor_surface::is_lane_session_of(project_session, resolved_session)
    {
        return None;
    }
    let marked = agent_doc_controller_io::project_controller::navigation_lane_session_marked(
        tmux,
        project_root,
        resolved_session,
    );
    let target = agent_doc_editor_surface::shared_sync_target_session(
        mode,
        project_session,
        resolved_session,
        marked,
        tmux.session_alive(project_session),
    );
    (target != resolved_session).then(|| target.to_string())
}

/// Per-installation navigation lanes (`#ideriderindependentnav` P2b): drop
/// layout columns whose document already has a live pane in another lane of
/// this project (the project session or a sibling installation lane session)
/// than `target_session`. A pane never changes lane implicitly; P4's explicit
/// handoff is the only path that may move it. Identity unless the project opts
/// into `editor_navigation = "per_installation"`, has a `tmux_session` pin, and
/// `target_session` is one of that pin's lanes.
pub fn drop_columns_owned_by_other_navigation_lanes(
    tmux: &Tmux,
    project_root: &Path,
    col_args: Vec<String>,
    target_session: &str,
    actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
) -> (Vec<String>, Vec<(String, String)>) {
    let config = agent_doc_project_config_io::load_project_from(
        &project_root.join(".agent-doc").join("config.toml"),
    );
    if !agent_doc_project_config_io::editor_navigation_lanes(&config) {
        return (col_args, Vec::new());
    }
    let Some(project_session) = config
        .tmux_session
        .as_deref()
        .map(str::trim)
        .filter(|session| !session.is_empty())
    else {
        return (col_args, Vec::new());
    };
    if target_session != project_session
        && !agent_doc_editor_surface::is_lane_session_of(project_session, target_session)
    {
        return (col_args, Vec::new());
    }
    let partition = agent_doc_editor_surface::partition_lane_layout_columns(
        &col_args,
        project_session,
        target_session,
        |file| {
            let pane = layout_document_live_pane(tmux, project_root, file, actor_bindings)?;
            tmux.pane_session(&pane).ok()
        },
    );
    (partition.kept, partition.dropped)
}

/// The live pane of one layout document: its actor binding, else its session
/// registry entry.
fn layout_document_live_pane(
    tmux: &Tmux,
    project_root: &Path,
    file: &str,
    actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
) -> Option<String> {
    let canonical = |file: &str| {
        let path = Path::new(file.trim());
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            project_root.join(path)
        };
        path.canonicalize().unwrap_or(path)
    };
    let document = canonical(file);
    actor_bindings
        .iter()
        .find(|binding| canonical(&binding.document_path) == document)
        .map(|binding| binding.pane_id.clone())
        .or_else(|| {
            agent_doc_session_registry_io::lookup_file_entry_in(project_root, &document)
                .ok()
                .flatten()
                .map(|entry| entry.pane)
        })
        .filter(|pane| !pane.is_empty() && tmux.pane_alive(pane))
}

/// One layout document a shared-mode sync adopted from a stale navigation lane
/// (`#ridernavp6switchback`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleLaneAdoption {
    pub document: String,
    pub pane: String,
    pub from_session: String,
    /// `Ok` when the pane now lives in the project session's `stash` window
    /// (or already lived in the project session), else the refusal reason.
    pub outcome: std::result::Result<(), String>,
}

/// `#ridernavp6switchback`: in `shared` navigation mode, move the pane of each
/// layout document that still lives in an installation lane session agent-doc
/// created (marked for this root) into the project session's `stash` window
/// before the reconcile, with the P4 handoff mechanics
/// (`hand_over_navigation_lane_pane`: same pane id and actor binding, an empty
/// shell left behind when it was the lane window's last pane). The reconcile
/// then places it from the project session like any stashed pane, so a lane
/// pane is never swapped against a project pane across sessions (which would
/// push the project pane into the lane) and never started a second time.
///
/// Identity unless the project is in shared mode, has a `tmux_session` pin,
/// and `target_session` is that pin. A session that only carries a lane's name
/// is never touched (`#bare-foreign-session-guard`).
pub fn adopt_stale_navigation_lane_panes(
    tmux: &Tmux,
    project_root: &Path,
    col_args: &[String],
    target_session: &str,
    actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
) -> Vec<StaleLaneAdoption> {
    let config = agent_doc_project_config_io::load_project_from(
        &project_root.join(".agent-doc").join("config.toml"),
    );
    let (mode, _) = agent_doc_project_config_io::editor_navigation_mode(&config);
    if mode != agent_doc_editor_surface::NavigationMode::Shared {
        return Vec::new();
    }
    let Some(project_session) = config
        .tmux_session
        .as_deref()
        .map(str::trim)
        .filter(|session| !session.is_empty())
    else {
        return Vec::new();
    };
    if target_session != project_session {
        return Vec::new();
    }
    // Liveness and session are read fresh: an earlier phase of this pass may
    // have cached a pane snapshot of another tmux server.
    tmux_router::invalidate_pane_snapshot();
    let mut adoptions = Vec::new();
    for file in col_args
        .iter()
        .flat_map(|column| column.split(','))
        .map(str::trim)
        .filter(|file| !file.is_empty())
    {
        let Some(pane) = layout_document_live_pane(tmux, project_root, file, actor_bindings) else {
            continue;
        };
        let Ok(from_session) = tmux.pane_session(&pane) else {
            continue;
        };
        if from_session == project_session
            || !agent_doc_editor_surface::is_lane_session_of(project_session, &from_session)
            || !agent_doc_controller_io::project_controller::navigation_lane_session_marked(
                tmux,
                project_root,
                &from_session,
            )
        {
            continue;
        }
        let outcome = agent_doc_controller_io::project_controller::hand_over_navigation_lane_pane(
            tmux,
            project_root,
            project_session,
            project_session,
            &agent_doc_controller_io::project_controller::NavigationLaneHandoff {
                document: file.to_string(),
                pane: pane.clone(),
                from_session: from_session.clone(),
            },
        )
        .map(|_| ());
        adoptions.push(StaleLaneAdoption {
            document: file.to_string(),
            pane,
            from_session,
            outcome,
        });
    }
    adoptions
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActorPanePlacement {
    session: String,
    in_agent_doc_window: bool,
}

fn focused_actor_session_from_bindings(
    focus: Option<&str>,
    actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
    mut pane_placement: impl FnMut(&str) -> Option<ActorPanePlacement>,
) -> Option<String> {
    let focus = agent_doc_sync::normalize_scope_arg(focus)?;
    let mut focused = None;
    let mut visible_layout = None;
    for binding in actor_bindings {
        let Some(placement) = pane_placement(&binding.pane_id) else {
            continue;
        };
        if placement.session.trim().is_empty() {
            continue;
        }
        if binding.document_path == focus {
            focused = Some(placement.clone());
        }
        if placement.in_agent_doc_window && visible_layout.is_none() {
            visible_layout = Some(placement);
        }
    }
    focused
        .as_ref()
        .filter(|placement| placement.in_agent_doc_window)
        .or(visible_layout.as_ref())
        .or(focused.as_ref())
        .map(|placement| placement.session.clone())
}

fn resolve_sync_target_session(
    tmux: &Tmux,
    window: Option<&str>,
    col_args: &[String],
    focus: Option<&str>,
    actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
) -> Result<Option<String>> {
    let policy = tmux_session_policy_for_scope(col_args, focus);
    resolve_sync_target_session_with_policy(tmux, window, col_args, focus, actor_bindings, &policy)
}

fn resolve_sync_target_session_with_policy(
    tmux: &Tmux,
    window: Option<&str>,
    col_args: &[String],
    focus: Option<&str>,
    actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
    policy: &agent_doc_sync::TmuxSessionPolicy,
) -> Result<Option<String>> {
    let explicit = window.and_then(|target| session_name_for_target_window(tmux, target));
    let focused_actor = focused_actor_session_from_bindings(focus, actor_bindings, |pane_id| {
        let session = tmux.pane_session(pane_id).ok()?;
        let window = tmux.pane_window(pane_id).ok()?;
        let in_agent_doc_window =
            agent_doc_tmux_io::target_window_name(tmux, &window).as_deref() == Some("agent-doc");
        Some(ActorPanePlacement {
            session,
            in_agent_doc_window,
        })
    });
    let current_agent_doc = current_agent_doc_session_name(tmux);
    let scoped_project = agent_doc_sync::shared_sync_scope_root(col_args, focus)
        .and_then(|scope_root| configured_session_for_root(tmux, &scope_root));
    let fallback = resolve_preferred_session(tmux, None, "[sync]");
    let document_binding = scoped_document_tmux_session_binding(policy, col_args, focus);
    let decision = match agent_doc_sync::select_layout_session_with_policy(
        agent_doc_sync::LayoutSessionCandidates {
            explicit,
            focused_actor,
            document_binding,
            current_agent_doc,
            scoped_project,
            fallback,
        },
        policy,
    ) {
        Ok(Some(decision)) => decision,
        Ok(None) => return Ok(None),
        Err(refused) => {
            sync_log(&format!(
                "layout_session_refused session={} authority={:?} allowed={}",
                refused.session,
                refused.authority,
                refused.allowed.join(",")
            ));
            return Err(anyhow::anyhow!("[sync] {refused}"));
        }
    };
    sync_log(&format!(
        "layout_session_selected session={} authority={:?}",
        decision.session, decision.authority
    ));
    Ok(Some(decision.session))
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

/// True when `pane_id`'s own live process tree still runs an agent-doc owner
/// session for `file`.
///
/// `#fpeselectstashpane`: this is the *bound-pane guard* the focus paths use in
/// place of resolving a pane live. It asks one question about one named pane —
/// it never scans for an alternative — so its answer can only invalidate a
/// binding, never nominate a different pane.
pub fn pane_process_tree_owns_document(tmux: &Tmux, pane_id: &str, file: &Path) -> bool {
    let Some(pane_pid) = agent_doc_tmux_io::pane_pid(tmux, pane_id) else {
        return false;
    };
    agent_doc_process_owner_io::process_tree_has_agent_doc_owner_for_file(
        &pane_pid.to_string(),
        &file.to_string_lossy(),
    )
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
/// logged and reported as `Ok(false)`; focus then re-observes the pane and
/// refuses to select it while it is still in the stash, because a pane in a
/// non-active window is never a focus target (`layoutpublisherarbiter`).
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
    exact_visible_target: bool,
    route_created_panes: bool,
    tmux: &Tmux,
) -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_with_options_internal_at_root(
        &cwd,
        col_args,
        window,
        focus,
        auto_start_mode,
        exact_visible_target,
        route_created_panes,
        &[],
        &MainLayoutEligibility::default(),
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

fn canonical_main_layout_document_id(project_root: &Path, document: &str) -> String {
    let path = Path::new(document.trim());
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };
    path.canonicalize()
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// Apply the controller's durable document↔view exclusion before pane
/// resolution, auto-start, column memory publication, or tmux reconciliation.
fn filter_main_layout_columns(
    project_root: &Path,
    eligibility: &MainLayoutEligibility,
    columns: Vec<String>,
) -> (Vec<String>, Vec<String>) {
    let mut dropped = Vec::new();
    let columns = columns
        .into_iter()
        .filter_map(|column| {
            let kept = column
                .split(',')
                .map(str::trim)
                .filter(|document| !document.is_empty())
                .filter_map(|document| {
                    if eligibility
                        .permits(&canonical_main_layout_document_id(project_root, document))
                    {
                        Some(document.to_string())
                    } else {
                        dropped.push(document.to_string());
                        None
                    }
                })
                .collect::<Vec<_>>();
            (!kept.is_empty()).then(|| kept.join(","))
        })
        .collect();
    (columns, dropped)
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
    preserve_cross_root: bool,
) -> (Vec<String>, Vec<String>) {
    if preserve_cross_root {
        return (saved_layout, Vec::new());
    }
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

/// `#netadv5` R7: acquire the sync lock and apply
/// [`agent_doc_sync::sync_lock_disposition`]. `Ok(Some(guard))` proceeds,
/// `Ok(None)` is the quiet safe-passive skip, and `Err` is a retryable abort:
/// a full sync never reconciles stash/join-pane state without exclusion just
/// because its wait budget elapsed.
pub(crate) fn acquire_sync_lock_for_mode(
    lock_path: &Path,
    auto_start_mode: AutoStartMode,
    wait_budget: Duration,
    mut log: impl FnMut(String),
) -> Result<Option<crate::SyncLockAcquire>> {
    let start = Instant::now();
    let guard = acquire_sync_lock(lock_path, wait_budget, &mut log);
    let elapsed = start.elapsed();
    let contended = matches!(guard, crate::SyncLockAcquire::Contended);
    match agent_doc_sync::sync_lock_disposition(auto_start_mode, guard.is_acquired(), contended) {
        agent_doc_sync::SyncLockDisposition::Proceed => Ok(Some(guard)),
        agent_doc_sync::SyncLockDisposition::SkipPassive => {
            let message =
                agent_doc_sync::safe_passive_lock_contention_message(elapsed, wait_budget);
            eprintln!("{}", message);
            log(message);
            Ok(None)
        }
        agent_doc_sync::SyncLockDisposition::AbortRetryable => {
            let message = format!(
                "{} lock={} elapsed_ms={} budget_ms={} action=retry (a full sync never runs without its lock)",
                agent_doc_sync::SYNC_LOCK_CONTENDED_RETRY_MARKER,
                lock_path.display(),
                elapsed.as_millis(),
                wait_budget.as_millis(),
            );
            log(message.clone());
            anyhow::bail!(message)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_with_options_internal_at_root(
    project_root: &Path,
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    auto_start_mode: AutoStartMode,
    exact_visible_target: bool,
    route_created_panes: bool,
    reactive_actor_bindings: &[agent_doc_controller_io::project_controller::ControllerTmuxActorBinding],
    main_layout_eligibility: &MainLayoutEligibility,
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
        agent_doc_sync::sync_lock_wait_budget_from_env_value(
            std::env::var(agent_doc_sync::SYNC_LOCK_WAIT_BUDGET_ENV)
                .ok()
                .as_deref(),
        )
    };
    let sync_lock_start = Instant::now();
    let Some(lock_guard) = acquire_sync_lock_for_mode(
        lock_path,
        auto_start_mode,
        sync_lock_wait_budget,
        |message| sync_log(&message),
    )?
    else {
        return Ok(());
    };
    let sync_lock_elapsed = sync_lock_start.elapsed();
    log_sync_latency(
        focus,
        "sync_lock_wait",
        sync_lock_elapsed,
        SYNC_LOCK_WAIT_LATENCY_BUDGET,
        auto_start_mode,
    );

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
    // `#ledgerscope`: the dominant hot-path cost.  `load_startup_miss` (and
    // `take_superseded_startup_miss`, which calls it again) loads the ENTIRE
    // `state.db` event ledger and JSON-replays every event just to project one
    // document out of it — per document. Measured here: 995ms of a 1024ms
    // per-document ownership loop, for two documents. The ledger is one shared
    // input per project, so this scope loads it once and lets every projection
    // derive from that; `append_event` invalidates it.
    let _state_ledger = agent_doc_supervisor_io::begin_state_ledger_scope();
    // `#syncownerreactive` / `#stategraphjoin`: the per-document ownership loop
    // resolves each candidate pane with `process_tree_contains_pid` /
    // `process_tree_owner_document_other_than` (sync.rs ~4881/4963/5020/5155),
    // which without a scope re-walks `/proc` for every pane, every caller, every
    // pass — the exact "re-observe on every question" shape `owner_graph` exists
    // to stop. Opening the scope here routes those reads through the reactive
    // `owner_graph`: each pane root PID is observed once and its derived owner
    // cells are reused for the rest of the run. Moving/selecting a tmux pane
    // does not change its process tree, so the `LocalReadScope` observation
    // stays valid across the run's tmux mutations; a genuinely new pane is a new
    // root PID and is observed on first request. Nested with the local scope
    // opened by the unmanaged-harness check below (ref-counted, not replaced).
    let _process_observation = agent_doc_process_owner_io::begin_process_observation_scope();
    let _lock_guard = lock_guard;

    // `#tmuxautosyncreactive`: if a newer layout superseded this generation
    // while we waited for the lock or opened scopes, bail before any sync body
    // work — the worker reloops to the newest generation instead of applying a
    // stale one. No-op for the standalone CLI (no binding set).
    if agent_doc_process_owner_io::structural_effect_superseded() {
        sync_log("structural_effect_cancelled_superseded phase=after_sync_lock");
        return Ok(());
    }

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
            match agent_doc_controller_io::project_controller::close_stale_dead_pane_actors_for_caller(
                &scope_root,
                |pane| {
                    agent_doc_supervisor_process::session_liveness::pane_owns_live_agent(
                        tmux, pane,
                    )
                },
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

    // Cross-root guard (`#sync-cross-root-autostart`): generic remembered layout
    // must not cause a parent controller to provision a nested-root document.
    // Exact-visible editor snapshots are different: they are an authoritative
    // workspace layout observation and run with no autostart. Preserve those
    // columns so the pane-layout effect can join the nested controller's already
    // owned actor binding without corrupting mixed-root layout memory.
    let sync_project_root = canonical_sync_project_root(project_root);
    let (saved_layout, dropped) =
        sanitize_cross_root_layout(saved_layout, &sync_project_root, exact_visible_target);
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
    let (eligible_columns, view_bound_documents) =
        filter_main_layout_columns(&sync_project_root, main_layout_eligibility, col_args);
    col_args = eligible_columns;
    for document in &view_bound_documents {
        sync_log(&format!(
            "main_layout_document_excluded file={document} reason=bound_to_editor_view"
        ));
    }
    let focus = focus.filter(|document| {
        main_layout_eligibility.permits(&canonical_main_layout_document_id(
            &sync_project_root,
            document,
        ))
    });
    let skip_autostart_diagnostics =
        exact_visible_safe_passive(auto_start_mode, exact_visible_target);
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
    let target_session_start = Instant::now();
    let tmux_session_policy = tmux_session_policy_for_scope(&col_args, focus);
    let target_session = resolve_sync_target_session_with_policy(
        tmux,
        window,
        &col_args,
        focus,
        reactive_actor_bindings,
        &tmux_session_policy,
    )?;
    // `#ridernavp6switchback`: a stale lane session (shared mode, before gc)
    // is never a shared sync's target; arrange the project session instead.
    let (target_session, window) = match target_session.as_deref().and_then(|resolved| {
        shared_mode_stale_lane_target(tmux, &sync_project_root, resolved)
            .map(|project| (resolved.to_string(), project))
    }) {
        Some((stale, project)) => {
            sync_log(&format!(
                "navigation_lane_stale_target_retargeted stale_session={stale} target_session={project} explicit_window={}",
                window.unwrap_or("-")
            ));
            (Some(project), None)
        }
        None => (target_session, window),
    };
    if let Some(session) = target_session.as_deref() {
        let (kept, dropped) =
            drop_columns_bound_to_other_sessions(&tmux_session_policy, col_args, session);
        col_args = kept;
        for (file, bound_session) in &dropped {
            sync_log(&format!(
                "cross_session_layout_dropped file={file} bound_session={bound_session} target_session={session}"
            ));
        }
        // `#ideriderindependentnav` P2b, defense in depth: the controller
        // already drops other lanes' documents before this effect runs, but
        // column memory above can substitute a remembered document back in.
        let (kept, dropped) = drop_columns_owned_by_other_navigation_lanes(
            tmux,
            &sync_project_root,
            col_args,
            session,
            reactive_actor_bindings,
        );
        col_args = kept;
        for (file, owner_session) in &dropped {
            sync_log(&format!(
                "cross_lane_layout_dropped document={file} owner_session={owner_session} target_session={session}"
            ));
        }
        for adoption in adopt_stale_navigation_lane_panes(
            tmux,
            &sync_project_root,
            &col_args,
            session,
            reactive_actor_bindings,
        ) {
            let line = match &adoption.outcome {
                Ok(()) => format!(
                    "navigation_lane_stale_pane_adopted document={} pane={} from_session={} target_session={session}",
                    adoption.document, adoption.pane, adoption.from_session
                ),
                Err(reason) => format!(
                    "navigation_lane_stale_pane_adopt_refused document={} pane={} from_session={} target_session={session} reason={reason}",
                    adoption.document, adoption.pane, adoption.from_session
                ),
            };
            sync_log(&line);
            agent_doc_ops_log_io::log_op(&sync_project_root, &line);
        }
    }
    log_sync_latency(
        focus,
        "window_target_session",
        target_session_start.elapsed(),
        SYNC_WINDOW_RESOLUTION_BUDGET,
        auto_start_mode,
    );
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
        // `#syncdoctorconverged`: the pre-reconcile doctor pass sat between two
        // measured phases and was invisible to the phase meter, which is how a
        // pair of unconditional full layout-repair passes hid inside every
        // editor-triggered sync. Meter it like every other phase so its cost
        // stays attributable.
        let doctor_repair_start = Instant::now();
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
        log_sync_latency(
            focus,
            "doctor_repair_pre",
            doctor_repair_start.elapsed(),
            SYNC_DOCTOR_REPAIR_BUDGET,
            auto_start_mode,
        );
    }
    let mut effective_window = match (window, target_session.as_deref()) {
        (Some(w), _) => Some(w.to_string()),
        (None, Some(session_name)) => Some(format!("{session_name}:agent-doc")),
        (None, None) => None,
    };
    let explicit_window_is_agent_doc =
        window.is_some_and(|target| target_is_agent_doc_window(tmux, target));
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
    if window.is_none()
        && let Some(ref session_name) = target_session
    {
        let agent_doc_lookup_start = Instant::now();
        let resolved_agent_doc_window =
            resolve_agent_doc_window_id(tmux, session_name, "agent-doc");
        log_sync_latency(
            focus,
            "window_agent_doc_lookup",
            agent_doc_lookup_start.elapsed(),
            SYNC_WINDOW_RESOLUTION_BUDGET,
            auto_start_mode,
        );
        if let Some(resolved_window_id) = resolved_agent_doc_window
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
        }
    }
    let window = effective_window.as_deref();
    let layout_observation_start = Instant::now();
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
        "window_layout_observation",
        layout_observation_start.elapsed(),
        SYNC_WINDOW_RESOLUTION_BUDGET,
        auto_start_mode,
    );
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
        exact_visible_target,
    );
    match &focus_only_expansion.event {
        Some(agent_doc_tmux::TmuxFocusOnlyExpansionEvent::ExactVisibleProjection) => {
            sync_log(&format!(
                "safe_passive_exact_visible_target columns={:?}",
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
    // `#ideriderindependentnav` P4: a passive focus-only expansion restores
    // columns from the project's remembered layout, which can name a document
    // whose pane lives in another navigation lane (for example one an explicit
    // claim just handed over). Re-apply the lane guard so the expansion can
    // never join that pane back across sessions.
    if let Some(session) = target_session.as_deref() {
        let (kept, dropped) = drop_columns_owned_by_other_navigation_lanes(
            tmux,
            &sync_project_root,
            col_args,
            session,
            reactive_actor_bindings,
        );
        col_args = kept;
        for (file, owner_session) in &dropped {
            sync_log(&format!(
                "cross_lane_layout_dropped document={file} owner_session={owner_session} target_session={session} stage=focus_only_expansion"
            ));
        }
    }
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
    let mut prune_cleanup_mode = safe_passive_prune_cleanup_mode_at_root(
        auto_start_mode,
        col_args,
        window,
        focus,
        &layout_state_root,
    );
    // `#tmuxautosyncreactive`: an exact-visible editor projection already knows
    // the desired pane set — the stash-pane purge is housekeeping (removing
    // idle shells), not layout convergence, and its per-pane kill-pane spawns
    // (~20ms each) dominate the prune. Defer it to the periodic housekeeping
    // pass so the editor-triggered sync is faster.
    if exact_visible_target
        && !matches!(
            prune_cleanup_mode,
            agent_doc_tmux::PruneCleanupMode::SkipExpensiveStashCleanup
        )
    {
        prune_cleanup_mode = agent_doc_tmux::PruneCleanupMode::SkipExpensiveStashCleanup;
        sync_log("exact_visible_prune_cleanup_skipped reason=frontend_snapshot_known_layout");
    }
    if matches!(
        prune_cleanup_mode,
        agent_doc_tmux::PruneCleanupMode::SkipExpensiveStashCleanup
    ) {
        sync_log("safe_passive_prune_cleanup_skipped reason=unchanged_recent_layout");
    }
    // `#tmuxautosyncreactive` diagnostic: the `prune` phase wall (~300ms) does
    // not appear in any `prune_*` sub-phase (they sum to ~1ms), so split the
    // window to localize it. `prune_pre` is mode-resolve + sync_log; the rest is
    // the timed call itself (`prune_with_tmux_wall`).
    let prune_pre_elapsed = prune_start.elapsed();
    let prune_with_tmux_start = Instant::now();
    let prune_timings = match resync::prune_with_tmux_timed_in_mode(tmux, prune_cleanup_mode) {
        Ok((_removed, timings)) => timings,
        Err(err) => {
            eprintln!("[sync] warning: prune failed: {}", err);
            sync_log(&format!("warning: prune failed: {}", err));
            Vec::new()
        }
    }; // Clean stale entries before layout calculation
    let prune_with_tmux_wall = prune_with_tmux_start.elapsed();
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
        "prune_pre",
        prune_pre_elapsed,
        SYNC_PRUNE_BUDGET,
        auto_start_mode,
    );
    log_sync_latency(
        hot_latency_focus,
        "prune_with_tmux_wall",
        prune_with_tmux_wall,
        SYNC_PRUNE_BUDGET,
        auto_start_mode,
    );
    log_sync_latency(
        hot_latency_focus,
        "prune",
        prune_start.elapsed(),
        SYNC_PRUNE_BUDGET,
        auto_start_mode,
    );

    // Bail before the ownership loop + tmux-router reconciliation if a newer
    // layout superseded this generation during the prune.
    if agent_doc_process_owner_io::structural_effect_superseded() {
        sync_log("structural_effect_cancelled_superseded phase=after_prune");
        return Ok(());
    }

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
    // Cross-root pane ownership stays with the nested controller. The parent
    // sync carries only controller-proven, in-memory bindings into tmux-router.
    let reactive_actor_bindings: HashMap<PathBuf, _> = reactive_actor_bindings
        .iter()
        .map(|binding| (PathBuf::from(&binding.document_path), binding))
        .collect();
    // `#tmuxautosyncreactive` diagnostic: the ownership fast path
    // (`exact_visible_actor_projection_reused`) only hits when bindings are
    // populated for the desired files. Log the count so a slow ownership loop
    // is attributable to empty bindings (actor store miss) vs. stale panes.
    sync_log(&format!(
        "reactive_actor_bindings count={} exact_visible_target={}",
        reactive_actor_bindings.len(),
        exact_visible_target,
    ));
    let mut pre_resolved_panes: HashMap<PathBuf, String> = HashMap::new();

    let resolve_file = |path: &Path| -> Option<FileResolution> {
        // Resolve once through the editor-authority boundary. Every computed
        // below (scaffold, initialization, template validation, session
        // ownership, and pane routing) derives from this exact projection.
        let effects = match crate::runtime_effects() {
            Ok(effects) => effects,
            Err(error) => {
                eprintln!(
                    "[sync] warning: current document authority unavailable for {}: {}",
                    path.display(),
                    error
                );
                return None;
            }
        };
        let mut content = match effects
            .resolve_current_document(path, "sync_resolve_file_authority")
        {
            Ok(content) => content,
            Err(error) => {
                eprintln!(
                    "[sync] warning: could not resolve current editor-authoritative content for {}: {}",
                    path.display(),
                    error
                );
                return None;
            }
        };

        // Step 1: Auto-scaffold empty .md projections before initialization.
        // The edit is delivered through the authority plane; the operator is
        // never required to save and disk is never used as a competing source.
        if path.extension() == Some(std::ffi::OsStr::new("md"))
            && agent_doc_document::claim_scaffold::should_scaffold_empty_markdown(
                &content,
                path.extension().and_then(|extension| extension.to_str()),
            )
        {
            eprintln!("[sync] auto-scaffolding empty file: {}", path.display());
            let session_id = uuid::Uuid::new_v4().to_string();
            let scaffold =
                agent_doc_document::claim_scaffold::render_empty_template_scaffold(&session_id);
            if let Err(e) = effects.write_current_document(path, &scaffold) {
                eprintln!(
                    "[sync] warning: failed to scaffold {}: {}",
                    path.display(),
                    e
                );
                return Some(FileResolution::Unmanaged);
            }
            content = scaffold;
        }

        // Step 2: Ensure UUID/snapshot initialization from that same value.
        // The commit effect owns native-save convergence and exact staging.
        match agent_doc_workflow_io::document_init::ensure_initialized_with_content(
            path,
            &content,
            |doc, updated| effects.write_current_document(doc, updated),
            |doc| effects.commit(doc),
            agent_doc_ops_log_io::log_op,
        ) {
            Ok((resolved, _changed)) => content = resolved,
            Err(e) => {
                eprintln!(
                    "[sync] warning: authority-based initialization failed for {}: {}",
                    path.display(),
                    e
                );
            }
        }

        // Step 3: Resolve the reactive template/session projection.
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

            // Exact editor-visible navigation has a generation-fenced actor
            // projection in this controller for the healthy/common case. That
            // projection is the hot-path ownership fact: publish it directly
            // into tmux-router and avoid re-reading frontmatter, the startup
            // ledger, and the durable registry merely to reconstruct the same
            // file→pane relationship. Missing/ambiguous actors still fall
            // through to the full fail-closed proof path below.
            if exact_visible_target
                && let Some(binding) = reactive_actor_bindings.get(file_path)
                && pane_projection_is_reusable_for_document(tmux, &binding.pane_id, file_path)
            {
                sync_log(&format!(
                    "exact_visible_actor_projection_reused file={} pane={} generation={}",
                    file_path.display(),
                    binding.pane_id,
                    binding.generation
                ));
                safe_passive_managed_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
                safe_passive_resolved_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
                session_files
                    .borrow_mut()
                    .push((binding.session_id.clone(), file_path.to_path_buf()));
                pre_resolved_panes.insert(file_path.to_path_buf(), binding.pane_id.clone());
                reserve_sync_pane(&claimed_sync_panes, &binding.pane_id, file_path);
                sa += seg_mark.elapsed();
                continue;
            }

            let is_cross_root = !document_belongs_to_sync_root(file_path, &sync_project_root);
            // `#tmuxautosyncreactive` / `#lazily-hot-path`: an exact-visible
            // editor sync is a pane-only projection — arrange existing panes,
            // don't re-derive ownership from editor content. The fallback below
            // calls `resolve_current_document`, a per-file editor IPC
            // round-trip measured at 300-860ms each, only to parse frontmatter
            // for a session_id. For a file whose pane the controller already
            // reconciled, reuse that binding and skip the editor IPC entirely.
            // The reactive fast path above covers the populated-binding case;
            // this covers the registry (same-root) / owning controller
            // (cross-root) when the in-memory binding was evicted but the pane
            // is still registered. `skip_autostart_diagnostics` keeps its own
            // dedicated branch below.
            if exact_visible_target && !skip_autostart_diagnostics {
                let reused: Option<(String, String, u64, &'static str)> = if is_cross_root {
                    agent_doc_project_root_io::project_root_containing(file_path)
                        .and_then(|owner_root| {
                            observe_cross_root_document_pane(
                                &owner_root,
                                file_path,
                                hot_latency_focus,
                                auto_start_mode,
                                "exact_visible",
                            )
                            .unwrap_or_else(|error| {
                                eprintln!(
                                    "[sync] warning: owning-controller actor projection unavailable for {}: {}",
                                    file_path.display(),
                                    error,
                                );
                                sync_log(&format!(
                                    "exact_visible_cross_root_actor_projection_failed file={} error={}",
                                    file_path.display(),
                                    error,
                                ));
                                None
                            })
                        })
                        .filter(|binding| {
                            pane_projection_is_reusable_for_document(
                                tmux,
                                &binding.pane_id,
                                file_path,
                            )
                        })
                        .map(|binding| {
                            (
                                binding.session_id,
                                binding.pane_id,
                                binding.generation,
                                "cross_root_controller",
                            )
                        })
                } else {
                    agent_doc_session_registry_io::lookup_file_entry_in(
                        &sync_project_root,
                        file_path,
                    )
                    .ok()
                    .flatten()
                    .filter(|entry| {
                        pane_projection_is_reusable_for_document(tmux, &entry.pane, file_path)
                    })
                    .map(|entry| (entry.session_id, entry.pane, 0, "registry"))
                };
                if let Some((session_id, pane_id, generation, source)) = reused {
                    sync_log(&format!(
                        "exact_visible_pane_reused_before_ipc file={} pane={} generation={} source={}",
                        file_path.display(),
                        pane_id,
                        generation,
                        source,
                    ));
                    safe_passive_managed_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
                    safe_passive_resolved_files
                        .borrow_mut()
                        .insert(file_path.to_path_buf());
                    session_files
                        .borrow_mut()
                        .push((session_id, file_path.to_path_buf()));
                    pre_resolved_panes.insert(file_path.to_path_buf(), pane_id.clone());
                    reserve_sync_pane(&claimed_sync_panes, &pane_id, file_path);
                    sa += seg_mark.elapsed();
                    continue;
                }
            }
            if skip_autostart_diagnostics && is_cross_root {
                // A mixed-root exact-visible layout is a pane-only tmux target.
                // Ask the owning controller for its existing actor binding before
                // touching editor content. This observation never focuses,
                // resumes, provisions, or copies the foreign actor into the
                // parent registry.
                let owner_root = agent_doc_project_root_io::project_root_containing(file_path);
                let observed = match owner_root {
                    Some(owner_root) => observe_cross_root_document_pane(
                        &owner_root,
                        file_path,
                        hot_latency_focus,
                        auto_start_mode,
                        "safe_passive",
                    ),
                    None => Ok(None),
                };
                match observed {
                    Ok(Some(binding))
                        if pane_projection_is_reusable_for_document(
                            tmux,
                            &binding.pane_id,
                            file_path,
                        ) =>
                    {
                        sync_log(&format!(
                            "safe_passive_cross_root_actor_projection_reused file={} pane={} generation={}",
                            file_path.display(),
                            binding.pane_id,
                            binding.generation,
                        ));
                        safe_passive_managed_files
                            .borrow_mut()
                            .insert(file_path.to_path_buf());
                        safe_passive_resolved_files
                            .borrow_mut()
                            .insert(file_path.to_path_buf());
                        session_files
                            .borrow_mut()
                            .push((binding.session_id.clone(), file_path.to_path_buf()));
                        pre_resolved_panes.insert(file_path.to_path_buf(), binding.pane_id.clone());
                        reserve_sync_pane(&claimed_sync_panes, &binding.pane_id, file_path);
                        sa += seg_mark.elapsed();
                        continue;
                    }
                    Ok(Some(binding)) => {
                        sync_log(&format!(
                            "safe_passive_cross_root_actor_projection_dead file={} pane={} generation={}",
                            file_path.display(),
                            binding.pane_id,
                            binding.generation,
                        ));
                    }
                    Ok(None) => {
                        sync_log(&format!(
                            "safe_passive_cross_root_actor_projection_missing file={}",
                            file_path.display(),
                        ));
                    }
                    Err(error) => {
                        eprintln!(
                            "[sync] warning: owning-controller actor projection unavailable for {}: {}",
                            file_path.display(),
                            error,
                        );
                        sync_log(&format!(
                            "safe_passive_cross_root_actor_projection_failed file={} error={}",
                            file_path.display(),
                            error,
                        ));
                    }
                }
                blocked_unresolved_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
                continue;
            }
            // `#ownershipbindingfirst`: consult the controller's file→actor
            // binding BEFORE fetching document content.
            //
            // The slow ownership path used to fetch the whole document over
            // editor IPC (`resolve_current_document`, measured at ~540-620ms per
            // file on this path — 2 files x ~600ms is the bulk of every
            // 1330-1474ms `ownership_proof` in the field log) purely to read one
            // frontmatter field, `session:`, and then hand that value to the
            // actor lookup. The binding is keyed by canonical document path and
            // already carries the session id, so the content RPC bought nothing
            // that the ~95ms lookup does not already return. Ask the binding
            // first; only a binding miss pays for content.
            //
            // This narrows, never widens, what may own a document: the loader
            // still requires a live bound pane, so ownership stays fail-closed.
            // It does change which side wins a session-id disagreement — the
            // live actor now beats the document's declared `session:` instead of
            // being filtered out by it. That is the direction `#lazily-hot-path`
            // and `rpc.rs::authoritative_actor_binding` both prescribe: the
            // actor is the hot-path authority and frontmatter may lag a reroute.
            let binding_first = if exact_visible_target {
                // Neither exact-visible branch below wants this lookup. The
                // same-root case deliberately goes straight to provisioning, and
                // the cross-root case already asked the owning controller at the
                // pre-IPC reuse branch above — repeating it here would just buy a
                // second round-trip to the same miss.
                None
            } else {
                project_authoritative_actor_binding_by_file(
                    tmux,
                    file_path,
                    hot_latency_focus,
                    auto_start_mode,
                    &proof_cache,
                )
            };

            let (session_id, authoritative_actor_pane) = if let Some(record) = binding_first {
                sync_log(&format!(
                    "ownership_binding_first_hit file={} pane={} generation={} skipped=content_rpc",
                    file_path.display(),
                    record.pane_id,
                    record.generation,
                ));
                // The proven binding is the resolution tmux-router needs, so
                // publish it as a pre-resolved pane. Otherwise the router calls
                // `resolve_file`, which crosses the same editor-authority
                // boundary this reorder just avoided — a SECOND content RPC per
                // file, timed under `tmux_router` (539-825ms in the same slow
                // runs whose `ownership_proof` was 1330-1474ms). tmux-router's
                // own comment gives the identical rationale for the short
                // circuit: a caller-proven live binding is already the
                // resolution.
                pre_resolved_panes.insert(file_path.to_path_buf(), record.pane_id.clone());
                (record.session_id.clone(), Some(record.pane_id.clone()))
            } else {
                let content = match crate::runtime_effects().and_then(|effects| {
                    effects.resolve_current_document(file_path, "sync_ownership_authority")
                }) {
                    Ok(content) => content,
                    Err(error) => {
                        eprintln!(
                            "[sync] warning: current editor-authoritative content unavailable for {}: {}",
                            file_path.display(),
                            error
                        );
                        continue;
                    }
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
                let pane = resolve_actor_pane_after_content(
                    file_path,
                    exact_visible_target,
                    is_cross_root,
                )?;
                (session_id, pane)
            };
            if matches!(auto_start_mode, AutoStartMode::SafePassive) {
                safe_passive_managed_files
                    .borrow_mut()
                    .insert(file_path.to_path_buf());
            }
            sa += seg_mark.elapsed();
            seg_mark = Instant::now();
            let registered_match = lookup_registry_match_for_file_session(file_path, &session_id);
            let registered_entry = registered_match.as_ref().map(|matched| &matched.entry);
            let registered_pane = authoritative_actor_pane
                .clone()
                .or_else(|| registered_entry.as_ref().map(|entry| entry.pane.clone()));

            if is_cross_root {
                // The nested controller owns provisioning and durable registry
                // state. Reuse a proven live owner when one exists; otherwise
                // delegate the resume effect to that controller. The resulting
                // pane binding is ephemeral input to tmux-router, never a parent
                // registry write (`#sync-cross-root-autostart`).
                let proven_live_pane = registered_pane.as_ref().filter(|pane| {
                    tmux.pane_alive(pane)
                        && registered_pane_proves_live_owner(
                            tmux,
                            file_path,
                            &session_id,
                            pane,
                            &proof_cache,
                        )
                });
                let delegated_pane = if let Some(pane) = proven_live_pane {
                    Ok(Some((*pane).clone()))
                } else if skip_autostart_diagnostics {
                    Ok(None)
                } else if let Some(owner_root) =
                    agent_doc_project_root_io::project_root_containing(file_path)
                {
                    crate::runtime_effects()?
                        .ensure_cross_root_document_pane(&owner_root, file_path)
                        .map(|pane| {
                            pane.filter(|pane| {
                                pane_projection_is_reusable_for_document(tmux, pane, file_path)
                            })
                        })
                } else {
                    Ok(None)
                };
                match delegated_pane {
                    Ok(Some(pane)) => {
                        eprintln!(
                            "[sync] using owning-controller pane {} for cross-root document {}",
                            pane,
                            file_path.display()
                        );
                        sync_log(&format!(
                            "cross_root_controller_pane_resolved file={} pane={} sync_root={}",
                            file_path.display(),
                            pane,
                            sync_project_root.display()
                        ));
                        pre_resolved_panes.insert(file_path.to_path_buf(), pane.clone());
                        reserve_sync_pane(&claimed_sync_panes, &pane, file_path);
                        if matches!(auto_start_mode, AutoStartMode::SafePassive) {
                            safe_passive_resolved_files
                                .borrow_mut()
                                .insert(file_path.to_path_buf());
                        }
                    }
                    Ok(None) => {
                        eprintln!(
                            "[sync] owning controller did not provide a live pane for cross-root document {}",
                            file_path.display()
                        );
                        sync_log(&format!(
                            "cross_root_controller_pane_unresolved file={} sync_root={}",
                            file_path.display(),
                            sync_project_root.display()
                        ));
                        blocked_unresolved_files
                            .borrow_mut()
                            .insert(file_path.to_path_buf());
                    }
                    Err(error) => {
                        eprintln!(
                            "[sync] owning-controller pane effect failed for cross-root document {}: {}",
                            file_path.display(),
                            error
                        );
                        sync_log(&format!(
                            "cross_root_controller_pane_failed file={} sync_root={} error={}",
                            file_path.display(),
                            sync_project_root.display(),
                            error
                        ));
                        blocked_unresolved_files
                            .borrow_mut()
                            .insert(file_path.to_path_buf());
                    }
                }
                continue;
            }
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
            let ownership_file = registered_match
                .as_ref()
                .map(|matched| matched.registered_file.as_path())
                .unwrap_or(file_path);
            let rename_candidate = registered_match
                .as_ref()
                .is_some_and(|matched| matched.renamed);
            let registered_live_owner = (!skip_autostart_diagnostics || rename_candidate)
                && registered_pane.as_ref().is_some_and(|pane| {
                    authoritative_actor_pane.as_deref() == Some(pane.as_str())
                        || registered_pane_proves_live_owner(
                            tmux,
                            ownership_file,
                            &session_id,
                            pane,
                            &proof_cache,
                        )
                });
            if rename_candidate
                && registered_live_owner
                && claimed_owner.is_none()
                && let Some(pane) = registered_pane.as_ref()
                && let Some(entry) = registered_entry
            {
                eprintln!(
                    "[sync] file renamed: {} → {} — reusing pane {} (session {})",
                    entry.file,
                    file_path.display(),
                    pane,
                    session_id
                );
                sync_log(&format!(
                    "file_rename_owner_reused old_file={} new_file={} pane={} session={}",
                    ownership_file.display(),
                    file_path.display(),
                    pane,
                    session_id
                ));
                if let Err(error) = reregister_recovered_owner(tmux, file_path, &session_id, pane) {
                    eprintln!("[sync] warning: re-register failed: {}", error);
                }
            }
            if (!skip_autostart_diagnostics || rename_candidate)
                && matches!(auto_start_mode, AutoStartMode::SafePassive)
                && let Some(pane_id) = registered_pane.as_ref()
                && claimed_owner.is_none()
                && registered_live_owner
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
                    false,
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
                route_created_panes,
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
                    if route_created_panes {
                        return Err(e).with_context(|| {
                            format!(
                                "manual sync failed to create and route pane for {}",
                                file_path.display()
                            )
                        });
                    }
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

    let mut blocked_files: Vec<String> = blocked_unresolved_files
        .borrow()
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    blocked_files.sort();
    if blocked_owner_layout_decision(
        auto_start_mode,
        exact_visible_target,
        route_created_panes,
        blocked_files.len(),
    ) == GatedLayoutDecision::PreserveCurrent
    {
        let blocked_summary = blocked_files.join(", ");
        let projection_kind = if matches!(auto_start_mode, AutoStartMode::SafePassive) {
            "safe passive"
        } else {
            "automatic exact-visible"
        };
        let message = format!(
            "[sync] {projection_kind} sync preserved the current tmux layout because unresolved files remain blocked: {blocked_summary}"
        );
        eprintln!("{message}");
        mark_sync_layout_preserved(&message);
        // Autonomous reconciliation is atomic and focus-neutral. In
        // particular, unresolved ownership must never turn a durable registry
        // entry in the stash into a `select-pane` target or collapse the
        // requested editor surface to its resolved subset.
        sync_log(&format!(
            "blocked_owner_layout_preserved blocked_files={} mode={} exact_visible={} route_created_panes={}",
            blocked_summary,
            auto_start_mode.log_label(),
            exact_visible_target,
            route_created_panes,
        ));
        return Ok(());
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
    // `#stalecontrollerhotupdate` (c): tmux-router consults `protect_pane` only
    // for panes this layout does not want. A pane whose live owner document
    // belongs to a FOREIGN project root (neither this root nor nested beneath
    // it) is another controller's column: never stash or swap it out. Observed
    // live (2026-10-15): haiven-dev's controller stashed agent-loop's
    // agent-doc-bugs.md pane on every one of 292 retries while agent-loop's
    // controller kept restoring it.
    //
    // "Another project's controller owns it" means that controller holds the
    // window's layout claim. When THIS root holds it (the editor's newest frame
    // is this root's, e.g. the operator went from 2 panes to 1), the foreign
    // pane is an ordinary extra and may be stashed.
    let window_driven_by_other_root = window.is_some_and(|window| {
        agent_doc_controller_io::project_controller::window_layout_driven_by_other_root(
            tmux,
            window,
            project_root,
        )
    });
    let logged_foreign_panes: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    let protect_foreign_pane = |pane_id: &str| -> bool {
        if !window_driven_by_other_root {
            return false;
        }
        let Some(document) =
            agent_doc_controller_io::project_controller::foreign_pane_owner_document(
                tmux,
                pane_id,
                project_root,
            )
        else {
            return false;
        };
        if logged_foreign_panes
            .borrow_mut()
            .insert(pane_id.to_string())
        {
            sync_log(&format!(
                "layout_foreign_pane_protected pane={} document={} project_root={} (#stalecontrollerhotupdate)",
                pane_id,
                document.display(),
                project_root.display(),
            ));
        }
        true
    };
    let allow_open_cycle_detach = |pane_id: &str| {
        let _ = log_open_cycle_detach(pane_id);
        protect_foreign_pane(pane_id)
    };
    let tmux_router_options = tmux_router::SyncOptions {
        protect_pane: Some(&allow_open_cycle_detach),
        allow_unresolved_pane_assignment: Some(&allow_unresolved_pane_assignment),
        pre_resolved_panes: Some(&pre_resolved_panes),
        // SafePassive = editor-driven poll / tab-selection: reconcile layout but
        // don't steal the operator's active tmux pane (#panefocussteal).
        passive: matches!(auto_start_mode, AutoStartMode::SafePassive),
    };

    // GH #109: capture every pane's window/title before tmux-router moves
    // anything, so the post-router audit can say which column pane left the
    // stash and why it was the one chosen.
    let pane_windows_before_router = crate::layout_column_audit::snapshot_pane_windows(tmux);

    // GH #121: staleness is a precondition of selection, not a postscript to
    // it. A column whose own pane runs a replaced supervisor binary is removed
    // from what tmux-router realises (the focused document excepted) and its
    // safe-boundary recycle is requested; the next sync after the recycle
    // admits the now-fresh pane.
    let requested_column_count = col_args.len();
    let (router_col_args, excluded_column_count) = {
        let gate_session_keys: HashMap<PathBuf, String> = session_files
            .borrow()
            .iter()
            .map(|(key, file)| (file.clone(), key.clone()))
            .collect();
        let gate_registry_pane = |file: &Path| -> Option<String> {
            let key = gate_session_keys.get(file)?;
            tmux_router::registry::lookup(tmux_router_registry_path, key)
                .ok()
                .flatten()
        };
        // GH #124: a live agent pane is never stashed in favour of a stale
        // supervisor pane, so the focused-document exception needs to know
        // which panes in the target window are mid-turn.
        let gate_live_turn_window_panes = || -> Vec<String> {
            let Some(window) = window else {
                return Vec::new();
            };
            tmux.list_window_panes(window)
                .unwrap_or_default()
                .into_iter()
                .filter(|pane| agent_doc_turn_status_io::turn_active_for_pane(project_root, pane))
                .collect()
        };
        // GH #136: a stale stash pane may replace a column, never add one, so
        // the bounded focus exception needs the target window's pane count.
        let gate_window_pane_count = || -> Option<usize> {
            let window = window?;
            tmux.list_window_panes(window).ok().map(|panes| panes.len())
        };
        let gate_pane_interaction_active = |file: &Path, pane: &str| {
            pane_interaction_is_active(tmux, project_root, file, pane, &proof_cache)
        };
        let gate = crate::layout_column_audit::gate_stale_column_panes(
            tmux,
            &crate::layout_column_audit::StaleColumnGateInput {
                col_args,
                focus,
                target_window: window,
                pre_resolved: &pre_resolved_panes,
                registry_pane: &gate_registry_pane,
                before: &pane_windows_before_router,
                live_turn_window_panes: &gate_live_turn_window_panes,
                window_pane_count: &gate_window_pane_count,
                pane_interaction_active: &gate_pane_interaction_active,
            },
        );
        // GH #136: acknowledge what this pass deliberately does not build, so
        // the controller converges on it instead of retrying it.
        record_sync_gated_documents(&gate.excluded);
        let excluded_column_count = requested_column_count.saturating_sub(gate.col_args.len());
        (gate.col_args, excluded_column_count)
    };
    if gated_layout_decision(
        auto_start_mode,
        requested_column_count,
        excluded_column_count,
    ) == GatedLayoutDecision::PreserveCurrent
    {
        // Safe-passive exact-visible observations are atomic surface
        // projections: a temporary stale-supervisor exclusion must not be
        // realised as a narrower layout. Full/manual repair may still realise
        // a non-empty remainder, while every mode preserves when all columns
        // were gated out.
        let message = if excluded_column_count == requested_column_count {
            "[sync] every layout column was excluded as stale; preserving the current tmux layout (GH #124)".to_string()
        } else {
            format!(
                "[sync] safe passive sync preserved the current tmux layout because {} of {} requested columns were excluded as stale",
                excluded_column_count, requested_column_count
            )
        };
        eprintln!("{message}");
        mark_sync_layout_preserved(&message);
        sync_log(&format!(
            "layout_preserved_stale_columns_excluded excluded={} requested={} mode={} (GH #124)",
            excluded_column_count,
            requested_column_count,
            auto_start_mode.log_label(),
        ));
        return Ok(());
    }
    let col_args: &[String] = &router_col_args;

    // Open-cycle panes are logged during DETACH, but they are not kept visible.
    // Stashing preserves the process and avoids recurring 3-pane projections.
    // `#tmuxpanesreversed`: pane geometry before the layout operation, read
    // past the observation cache, for the post-operation order check's log.
    let pane_order_before =
        window.and_then(|window| agent_doc_tmux_io::pane_order::observe_pane_order(tmux, window));
    let router_start = Instant::now();
    let result = tmux_router::sync_with_options(
        col_args,
        window,
        focus,
        tmux,
        tmux_router_registry_path,
        &resolve_file,
        &tmux_router_options,
    );
    // tmux-router mutates tmux through its own command builder, which the
    // pass-wide observation cache cannot see: without this, every cached
    // read below (the GH #109 "after" snapshot among them) would replay the
    // layout from BEFORE the router ran.
    agent_doc_tmux_io::observation_cache::observe_external_mutation();
    let result = result?;
    pin_displaced_open_cycle_windows_as_stash(tmux, window, &logged_open_cycle_panes.borrow())?;
    agent_doc_tmux_io::observation_cache::observe_external_mutation();
    enforce_synced_pane_order(
        tmux,
        &sync_project_root,
        col_args,
        &result,
        window,
        pane_order_before,
    );
    LAST_SYNC_RUN_REPORT.with(|report| {
        report.borrow_mut().file_panes = result.file_panes.clone();
    });
    log_sync_latency(
        focus,
        "tmux_router",
        router_start.elapsed(),
        SYNC_ROUTER_BUDGET,
        auto_start_mode,
    );
    let post_router_start = Instant::now();

    // GH #109: validate and attribute the pane realising each column. A stash
    // promotion, a pane bound to another document, or a stale supervisor each
    // get a `layout_column_pane_selected` line; a stale supervisor in the
    // column's own pane also gets its safe-boundary recycle requested. Never
    // moves, kills, or reaps a pane.
    {
        let pane_windows_after_router = crate::layout_column_audit::snapshot_pane_windows(tmux);
        let session_keys: HashMap<PathBuf, String> = session_files
            .borrow()
            .iter()
            .map(|(key, file)| (file.clone(), key.clone()))
            .collect();
        let registry_pane = |file: &Path| -> Option<String> {
            let key = session_keys.get(file)?;
            match tmux_router::registry::lookup(tmux_router_registry_path, key) {
                Ok(pane) => pane,
                Err(error) => {
                    eprintln!(
                        "[sync] layout column audit: registry lookup failed for {}: {error:#}",
                        file.display()
                    );
                    None
                }
            }
        };
        crate::layout_column_audit::audit_layout_column_panes(
            tmux,
            &crate::layout_column_audit::LayoutColumnAuditInput {
                file_panes: &result.file_panes,
                pre_resolved: &pre_resolved_panes,
                registry_pane: &registry_pane,
                before: &pane_windows_before_router,
                after: &pane_windows_after_router,
                focus,
            },
        );
    }

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
                    "tmux_layout_target_exceeded_by_open_cycles window={} desired_panes={} actual_visible_panes={} extras={:?}",
                    w, projected, pane_count, extra_panes
                ));
            } else {
                let warning = format!(
                    "[sync] warning: visible pane count {} exceeds requested editor projection {} after sync for window {}",
                    pane_count, projected, w
                );
                eprintln!("{}", warning);
                sync_log(&format!(
                    "tmux_layout_target_exceeded window={} desired_panes={} actual_visible_panes={} unprotected_extras={:?}",
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
    for note in refresh_synced_pane_window_bindings(tmux, &result.file_panes)? {
        eprintln!("[sync] {}", note);
    }

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
        let doctor_repair_start = Instant::now();
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
        log_sync_latency(
            focus,
            "doctor_repair_post",
            doctor_repair_start.elapsed(),
            SYNC_DOCTOR_REPAIR_BUDGET,
            auto_start_mode,
        );
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
        map.get_or_insert_with(scope.ctx(), key, move |_, _| {
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

/// Re-register `pane_id` as `file`'s owner after a sync or resync recovery
/// proved the pane hosts it.
///
/// `#syncbindingfence`: the actor write is decided from one read of the actor
/// record and compare-and-swapped on it
/// ([`sessions::rebind_recovered_owner_in`]). A record that already binds the
/// session to the pane is left to its owner (only the registry is refreshed),
/// and a newer owner or operator commit for another pane is never bound over.
pub fn reregister_recovered_owner(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
) -> anyhow::Result<()> {
    rebind_recovered_owner_pane(tmux, file, session_id, pane_id, true)
}

/// Refresh the durable registry entry for the pane the authoritative actor
/// record already names, without writing the actor (`#syncbindingfence`).
fn refresh_registry_for_actor_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
) -> anyhow::Result<()> {
    rebind_recovered_owner_pane(tmux, file, session_id, pane_id, false)
}

fn rebind_recovered_owner_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
    may_write_actor: bool,
) -> anyhow::Result<()> {
    let Some((canonical_file, project_root, _registry_key)) = registry_location_for_file(file)
    else {
        return sessions::register(session_id, pane_id, &file.to_string_lossy());
    };
    let file_str = registry_relative_file_path(&project_root, &canonical_file);

    let supervisor = lookup_registry_entry_for_file_session(file, session_id)
        .filter(|entry| {
            entry.pane == pane_id
                && entry.pid != 0
                && !entry.supervisor_instance_id.is_empty()
                && pane_contains_supervisor_pid(tmux, pane_id, entry.pid)
        })
        .map(|entry| SupervisorIdentity {
            pid: entry.pid,
            instance_id: entry.supervisor_instance_id,
        })
        .or_else(|| {
            query_supervisor_identity(file, session_id)
                .filter(|identity| pane_contains_supervisor_pid(tmux, pane_id, identity.pid))
        });
    let pid = supervisor
        .as_ref()
        .map(|identity| identity.pid)
        .unwrap_or_else(|| pane_pid_from_tmux(tmux, pane_id).unwrap_or(std::process::id()));
    let window = agent_doc_tmux_io::target_window_id(tmux, pane_id).unwrap_or_default();
    let cwd = project_root.to_string_lossy();
    let outcome = sessions::rebind_recovered_owner_in(
        &project_root,
        sessions::RecoveredOwnerBinding {
            session_id,
            pane_id,
            file: &file_str,
            pid,
            window: &window,
            cwd: &cwd,
            supervisor_instance_id: supervisor
                .as_ref()
                .map(|identity| identity.instance_id.as_str()),
            may_write_actor,
        },
    )?;
    match outcome {
        sessions::RecoveredOwnerBind::OwnerSuperseded {
            pane_id: owner_pane,
            generation,
            caller,
            reason,
        } => sync_log(&format!(
            "recovered_owner_bind_superseded file={} pane={} owner_pane={} generation={} caller={} reason={}",
            file.display(),
            pane_id,
            owner_pane,
            generation,
            caller,
            reason
        )),
        sessions::RecoveredOwnerBind::ActorNotBound => sync_log(&format!(
            "registry_refresh_skipped_actor_not_bound file={} pane={}",
            file.display(),
            pane_id
        )),
        sessions::RecoveredOwnerBind::RegistryRefreshed
        | sessions::RecoveredOwnerBind::Bound { .. } => {}
    }
    Ok(())
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
                record.state != agent_doc_controller::actor::ActorState::Closed
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
    let tmux = agent_doc_tmux_io::configured_tmux();
    let current_pane = match agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux) {
        Ok(pane) if !pane.is_empty() => pane,
        Ok(_) => return,
        Err(error) => {
            eprintln!("[sync] current tmux pane unavailable: {error}");
            return;
        }
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
    pane_occupant_for_document(tmux, pane_id, claimed_file) != PaneOccupant::Free
}

/// Who occupies a pane, relative to the document a caller wants to put there (GH #82).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneOccupant {
    /// Nothing in the pane binds a document other than the claimed one.
    Free,
    /// An agent-doc/codex owner session for this other document runs there.
    OtherDocument(String),
    /// A bare harness session agent-doc did not launch (`#bare-foreign-session-guard`):
    /// it owns NO document, and it is not ours to claim, dispatch into, or reap.
    ForeignHarness,
}

/// [`pane_runs_other_document_owner`], keeping the answer's evidence. "Runs another
/// document" and "runs the operator's own harness" lead to different routes: the first is
/// a real conflict, the second means the binding that pointed here is stale.
pub fn pane_occupant_for_document(tmux: &Tmux, pane_id: &str, claimed_file: &Path) -> PaneOccupant {
    let Some(pane_pid) = pane_pid_from_tmux(tmux, pane_id) else {
        return PaneOccupant::Free;
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
        return PaneOccupant::ForeignHarness;
    }
    match agent_doc_process_owner_io::process_tree_owner_document_other_than(
        &pane_pid,
        claimed_file,
    ) {
        Some(other) if argv_owner_is_stale_for_claimed_binding(pane_id, claimed_file, &other) => {
            agent_doc_ops_log_io::log_op(
                claimed_file,
                &format!(
                    "pane_occupant_stale_argv_identity file={} pane={} argv_document={} \
                     registry_binds_claimed=true action=treat_as_free (GH #89)",
                    claimed_file.display(),
                    pane_id,
                    other,
                ),
            );
            PaneOccupant::Free
        }
        Some(other) => PaneOccupant::OtherDocument(other),
        None => PaneOccupant::Free,
    }
}

/// GH #89: the registry binding for `claimed_file` outranks a pane's frozen argv.
fn argv_owner_is_stale_for_claimed_binding(
    pane_id: &str,
    claimed_file: &Path,
    argv_document: &str,
) -> bool {
    let base_dir = agent_doc_session_registry_io::dispatch_registry::registry_base_dir_for_dispatch(
        &claimed_file.to_string_lossy(),
    );
    match agent_doc_session_registry_io::dispatch_registry::load_registry_in(&base_dir) {
        Ok(registry) => argv_owner_is_stale_spawn_identity(
            &registry,
            pane_id,
            claimed_file,
            argv_document,
            &base_dir,
        ),
        Err(err) => {
            eprintln!(
                "[sync] could not load the registry to check pane {pane_id}'s argv identity \
                 for {}: {err:#}",
                claimed_file.display()
            );
            false
        }
    }
}

/// GH #89: a pane's process argv is fixed when its supervisor starts, so a
/// document rename leaves it naming the OLD path forever, while rename
/// reconciliation moves the registry binding to the new one. Comparing the
/// mutable binding against the immutable argv refused, permanently, the one
/// pane the registry binds the renamed document to.
///
/// The argv document is a stale spawn-time identity only when the registry
/// binds `claimed_file` to `pane` AND no registry entry still binds the argv
/// document to that pane. A pane genuinely running another bound document keeps
/// the cross-document guard.
pub fn argv_owner_is_stale_spawn_identity(
    registry: &tmux_router::Registry,
    pane: &str,
    claimed_file: &Path,
    argv_document: &str,
    root: &Path,
) -> bool {
    use agent_doc_session_registry_io::dispatch_registry::{
        canonical_dispatch_file, canonical_registered_file,
    };
    let claimed = canonical_dispatch_file(claimed_file);
    let argv_path = Path::new(argv_document);
    let argv_abs = if argv_path.is_absolute() {
        argv_path.to_path_buf()
    } else {
        root.join(argv_path)
    };
    let argv = std::fs::canonicalize(&argv_abs).unwrap_or(argv_abs);
    let mut binds_claimed = false;
    for entry in registry.values().filter(|entry| entry.pane == pane) {
        let bound = canonical_registered_file(entry);
        if bound == argv || entry.file == argv_document {
            return false;
        }
        if bound == claimed {
            binds_claimed = true;
        }
    }
    binds_claimed
}

#[cfg(test)]
mod stale_argv_identity_tests {
    use super::argv_owner_is_stale_spawn_identity;

    fn entry(pane: &str, cwd: &std::path::Path, file: &str) -> tmux_router::RegistryEntry {
        tmux_router::RegistryEntry {
            pane: pane.to_string(),
            pid: 1,
            cwd: cwd.display().to_string(),
            started: String::new(),
            session_id: format!("s-{file}"),
            file: file.to_string(),
            window: String::new(),
            supervisor_instance_id: String::new(),
        }
    }

    /// GH #89: after `agent-doc.md` -> `agent-doc.ad.md`, the registry binds the
    /// new path to %31 while %31's supervisor argv still names the old path.
    #[test]
    fn renamed_document_registry_binding_outranks_frozen_argv() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(root.join("tasks/agent-doc.ad.md"), "x").unwrap();
        let claimed = root.join("tasks/agent-doc.ad.md");
        let registry = std::collections::HashMap::from([(
            "s".to_string(),
            entry("%31", root, "tasks/agent-doc.ad.md"),
        )]);

        assert!(argv_owner_is_stale_spawn_identity(
            &registry,
            "%31",
            &claimed,
            "tasks/agent-doc.md",
            root,
        ));
    }

    /// The cross-document guard survives: a pane still bound to the argv
    /// document is that document's pane, whatever else claims it.
    #[test]
    fn pane_still_bound_to_argv_document_is_not_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(root.join("tasks/a.md"), "x").unwrap();
        std::fs::write(root.join("tasks/b.md"), "x").unwrap();
        let claimed = root.join("tasks/b.md");
        let both = std::collections::HashMap::from([
            ("a".to_string(), entry("%31", root, "tasks/a.md")),
            ("b".to_string(), entry("%31", root, "tasks/b.md")),
        ]);
        assert!(!argv_owner_is_stale_spawn_identity(
            &both,
            "%31",
            &claimed,
            "tasks/a.md",
            root,
        ));

        // No binding of the claimed document to the pane: the argv owner stands.
        let other_only =
            std::collections::HashMap::from([("a".to_string(), entry("%31", root, "tasks/a.md"))]);
        assert!(!argv_owner_is_stale_spawn_identity(
            &other_only,
            "%31",
            &claimed,
            "tasks/a.md",
            root,
        ));
        let elsewhere =
            std::collections::HashMap::from([("b".to_string(), entry("%7", root, "tasks/b.md"))]);
        assert!(!argv_owner_is_stale_spawn_identity(
            &elsewhere,
            "%31",
            &claimed,
            "tasks/a.md",
            root,
        ));
    }
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
    /// `#syncioredlivetmux`: make `pane` the live owner of `doc` the way a
    /// route-owned `agent-doc start` would. Since `#adsessreap2` an open
    /// session log only names an owner while that pane runs a non-shell
    /// foreground process, and since `#bare-foreign-session-guard` a harness
    /// that binds no document is foreign, so a bare shell pane is no longer
    /// enough. The pane's shell is replaced by a long-lived process exec'd
    /// directly (no shell startup to race) whose command line binds `doc`; it
    /// dies with the isolated tmux server.
    pub(crate) fn respawn_pane_as_document_owner(
        tmux: &IsolatedTmux,
        pane: &str,
        cwd: &Path,
        doc: &Path,
    ) {
        let command = format!(
            "bash -c \"exec -a agent-doc perl -e 'sleep 600' start --route-owned -- {}\"",
            doc.display()
        );
        let cwd = cwd.to_string_lossy();
        tmux.raw_cmd(&[
            "respawn-pane",
            "-k",
            "-c",
            cwd.as_ref(),
            "-t",
            pane,
            &command,
        ])
        .unwrap();
        assert!(
            wait_for(Duration::from_secs(15), || {
                agent_doc_supervisor_process::session_liveness::pane_owns_live_agent(tmux, pane)
                    && !pane_runs_other_document_owner(tmux, pane, doc)
            }),
            "pane {pane} should run a live owner of {}",
            doc.display()
        );
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

        let iso = agent_doc_tmux_io::isolated_test_tmux(test_name);
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
        let requested_window = iso.pane_window(&requested_pane).unwrap();

        for (pane, doc) in [
            (&protected_pane, &protected_doc),
            (&detached_pane, &detached_doc),
            (&requested_pane, &requested_doc),
        ] {
            respawn_pane_as_document_owner(&iso, pane, root, doc);
        }

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
            &requested_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        for (session, pane, window, doc) in [
            (
                "sync-replace-protected",
                &protected_pane,
                &target_window,
                &protected_doc,
            ),
            (
                "sync-replace-detached",
                &detached_pane,
                &target_window,
                &detached_doc,
            ),
            (
                "sync-replace-requested",
                &requested_pane,
                &requested_window,
                &requested_doc,
            ),
        ] {
            agent_doc_session_actor_io::project_binding_in(
                root,
                &doc.to_string_lossy(),
                session,
                pane,
                window,
                "sync",
                "replace_detachable_fixture",
            )
            .unwrap();
        }

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
    fn exact_visible_cross_root_after_content_does_not_repeat_actor_lookup() {
        let file = Path::new("/tmp/example-project/tasks/example.md");
        assert_eq!(
            resolve_actor_pane_after_content(file, true, true).unwrap(),
            None,
            "the one bounded pre-content observation owns this transition; a miss must fall through without another RPC",
        );
    }

    #[test]
    fn exact_visible_target_rejects_live_pane_owned_by_another_document() {
        assert!(pane_projection_reuse_allowed(true, false));
        assert!(!pane_projection_reuse_allowed(false, false));
        assert!(
            !pane_projection_reuse_allowed(true, true),
            "pane liveness must not let a stale actor or registry projection relabel a foreign document owner"
        );
    }

    #[test]
    fn stale_stash_permission_prompt_remains_interaction_active_after_turn_marker_expires() {
        let now = 10_000;
        let record = agent_doc_controller::actor::ActorRecord {
            document_id: "/nested/tasks/devops.md".to_string(),
            session_id: "session-devops".to_string(),
            generation: 16,
            pane_id: "%125".to_string(),
            window_id: "@2".to_string(),
            harness: "codex".to_string(),
            state: agent_doc_controller::actor::ActorState::WaitingInput,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
                caller: "supervisor".to_string(),
                reason: "harness_permission_prompt".to_string(),
                timestamp: now - 90,
                prior_generation: 16,
                new_generation: 16,
            },
        };
        let lease = agent_doc_sqlite::state_store::SupervisorLeaseStatus {
            generation: 16,
            supervisor_pid: Some(4242),
            supervisor_socket: Some("/tmp/devops.sock".to_string()),
            last_heartbeat: Some(now - 1),
            runtime_state: Some("waiting_input".to_string()),
        };

        assert!(interaction_actor_lease_is_live_at(
            &record, "%125", &lease, now, true
        ));

        let mut idle = record.clone();
        idle.state = agent_doc_controller::actor::ActorState::Ready;
        assert!(!interaction_actor_lease_is_live_at(
            &idle, "%125", &lease, now, true
        ));

        let mut stale_lease = lease.clone();
        stale_lease.last_heartbeat = Some(now - 61);
        assert!(!interaction_actor_lease_is_live_at(
            &record,
            "%125",
            &stale_lease,
            now,
            true
        ));
        assert!(!interaction_actor_lease_is_live_at(
            &record, "%125", &lease, now, false
        ));
        assert!(!interaction_actor_lease_is_live_at(
            &record, "%999", &lease, now, true
        ));

        let active_capture = "\
• Working (19m 44s • esc to interrupt)

› Ask Codex to do anything
  GPT-5.6-Sol high · ~/work · Context 24% used
";
        assert_eq!(
            agent_doc_harness::HarnessConfig::codex()
                .dispatch_blocker_reason(active_capture)
                .as_deref(),
            Some("active codex turn"),
            "foreground harness evidence keeps a long turn protected even after actor/lease timers age out"
        );
    }

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
        let (sanitized, dropped) = sanitize_cross_root_layout(saved_layout, &sync_root, false);
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

    #[test]
    fn exact_visible_layout_memory_preserves_cross_root_columns() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(root.join("src/haiven-dev/.agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("src/haiven-dev/tasks")).unwrap();
        let root_doc = root.join("tasks/agent-doc-bugs2.md");
        let child_doc = root.join("src/haiven-dev/tasks/haiven-dev.md");
        std::fs::write(&root_doc, "---\n---\n").unwrap();
        std::fs::write(&child_doc, "---\n---\n").unwrap();

        let saved_layout = vec![
            root_doc.to_string_lossy().to_string(),
            child_doc.to_string_lossy().to_string(),
        ];
        let (preserved, dropped) = sanitize_cross_root_layout(saved_layout.clone(), &root, true);

        assert_eq!(preserved, saved_layout);
        assert!(dropped.is_empty());
    }

    /// `#ownershipbindingfirst`: the ownership proof consults the controller's
    /// file→actor binding before fetching document content, so a document whose
    /// content is unavailable still routes when a live binding names its pane.
    ///
    /// This is the discriminating shape for the reorder. Before it, the loop
    /// fetched content first purely to read `session:` out of frontmatter, so an
    /// unreadable document made the whole file `continue` and its pane stayed
    /// stashed — even though the binding already knew the session and the pane.
    /// The content RPC costs ~540-620ms per file on this path; a binding hit
    /// costs ~95ms and now short-circuits it.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn ownership_binding_first_routes_without_content_authority() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let doc = root.join("tasks/binding-first.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: bound-session\n---\n\n# Doc\n",
        )
        .unwrap();

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-ownership-binding-first");
        let anchor_pane = iso.new_session("test", &root).unwrap();
        let target_window = iso.pane_window(&anchor_pane).unwrap();
        iso.raw_cmd(&["rename-window", "-t", &target_window, "agent-doc"])
            .unwrap();
        let doc_pane = iso.new_window("test", &root).unwrap();
        let doc_window = iso.pane_window(&doc_pane).unwrap();
        iso.raw_cmd(&["rename-window", "-t", &doc_window, "stash"])
            .unwrap();

        sessions::register_full_with_cwd_in(
            &root,
            "bound-session",
            &doc_pane,
            &doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &doc_pane).unwrap(),
            &doc_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        // The live actor binding is the ownership fact under test: it carries
        // both the pane and the session id, which is exactly what the removed
        // content round-trip used to rediscover.
        agent_doc_session_actor_io::project_binding_in(
            &root,
            &doc.to_string_lossy(),
            "bound-session",
            &doc_pane,
            &doc_window,
            "sync",
            "ownership_binding_first",
        )
        .unwrap();

        // Make document content unavailable. Any implementation that reads
        // content before consulting the binding cannot resolve ownership here.
        std::fs::remove_file(&doc).unwrap();
        std::fs::create_dir(&doc).unwrap();

        let columns = vec![doc.to_string_lossy().to_string()];
        let _cwd = ScopedCurrentDir::set(&root);

        run_with_options_internal_at_root(
            &root,
            &columns,
            Some(&target_window),
            Some(doc.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            // `exact_visible_target=false` is the measured slow shape: no
            // reactive bindings are carried in, so every editor fast path above
            // the ownership loop is disabled and the loop must resolve ownership
            // itself.
            false,
            false,
            &[],
            &MainLayoutEligibility::default(),
            &iso,
        )
        .unwrap();

        // The anchor pane holds no requested document, so sync stashes it and
        // the single requested column is the bound pane. Before the reorder the
        // unreadable document made ownership unresolvable, the file was skipped
        // (`resolved=0`), and `%1` stayed in the stash window.
        assert_eq!(
            iso.list_panes_ordered(&target_window).unwrap(),
            vec![doc_pane],
            "a live actor binding should resolve ownership and route the pane without any document content read"
        );
        assert!(
            !anchor_pane.is_empty(),
            "anchor pane id should have been captured"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn exact_visible_cross_root_actor_binding_routes_without_content_authority() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let child_root = root.join("src/haiven-dev");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(child_root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(child_root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let root_doc = root.join("tasks/agent-doc-bugs2.md");
        let child_doc = child_root.join("tasks/haiven-dev.md");
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-cross-root-pane-only");
        let decoy_pane = iso.new_session("test", &root).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
            .unwrap();
        let decoy_window = iso.pane_window(&decoy_pane).unwrap();
        let root_pane = iso.new_window("test", &root).unwrap();
        let root_window = iso.pane_window(&root_pane).unwrap();
        iso.raw_cmd(&["rename-window", "-t", &root_window, "agent-doc"])
            .unwrap();
        let child_pane = iso.new_window("test", &child_root).unwrap();
        let child_window = iso.pane_window(&child_pane).unwrap();
        iso.raw_cmd(&["rename-window", "-t", &child_window, "stash"])
            .unwrap();

        sessions::register_full_with_cwd_in(
            &child_root,
            "child-session",
            &child_pane,
            &child_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &child_pane).unwrap(),
            &child_window,
            &child_root.to_string_lossy(),
        )
        .unwrap();

        // Reproduce temporary editor-content authority loss. The owning
        // controller/registry observation remains sufficient for pane layout.
        std::fs::remove_file(&child_doc).unwrap();
        std::fs::create_dir(&child_doc).unwrap();
        let root_binding =
            agent_doc_controller_io::project_controller::ControllerTmuxActorBinding {
                document_path: root_doc.to_string_lossy().to_string(),
                session_id: "root-session".to_string(),
                pane_id: root_pane.clone(),
                generation: 7,
            };
        let columns = vec![
            root_doc.to_string_lossy().to_string(),
            child_doc.to_string_lossy().to_string(),
        ];
        let _cwd = ScopedCurrentDir::set(&root);

        run_with_options_internal_at_root(
            &root,
            &columns,
            Some(&root_window),
            Some(root_doc.to_string_lossy().as_ref()),
            AutoStartMode::SafePassive,
            true,
            false,
            &[root_binding],
            &MainLayoutEligibility::default(),
            &iso,
        )
        .unwrap();

        assert_eq!(
            iso.list_panes_ordered(&root_window).unwrap(),
            vec![root_pane, child_pane],
            "the existing nested-root pane should move from stash into the explicit agent-doc window"
        );
        assert_eq!(
            iso.list_panes_ordered(&decoy_window).unwrap(),
            vec![decoy_pane],
            "a different same-named window must not replace the explicit target"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn exact_visible_cross_root_full_mode_resolves_via_owning_controller() {
        // The editor tab-switch path runs `AutoStartMode::Full` with
        // `exact_visible_target=true` (`skip_autostart_diagnostics` is
        // false), so the cross-root reactive branch at the top of the per-file
        // loop does not fire. The cross-root child pane must still resolve
        // through the owning controller's reactive binding
        // (`resolve_cross_root_document_pane`) instead of the exhaustive
        // `/proc` ownership walk, which measured 670ms-1.3s per cross-root file
        // on this path.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let child_root = root.join("src/haiven-dev");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(child_root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(child_root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();

        let root_doc = root.join("tasks/agent-doc-bugs2.md");
        let child_doc = child_root.join("tasks/haiven-dev.md");
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-cross-root-full");
        let root_pane = iso.new_session("test", &root).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
            .unwrap();
        let root_window = iso.pane_window(&root_pane).unwrap();
        let child_pane = iso.new_window("test", &child_root).unwrap();
        let child_window = iso.pane_window(&child_pane).unwrap();
        iso.raw_cmd(&["rename-window", "-t", &child_window, "stash"])
            .unwrap();

        sessions::register_full_with_cwd_in(
            &child_root,
            "child-session",
            &child_pane,
            &child_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &child_pane).unwrap(),
            &child_window,
            &child_root.to_string_lossy(),
        )
        .unwrap();

        let root_binding =
            agent_doc_controller_io::project_controller::ControllerTmuxActorBinding {
                document_path: root_doc.to_string_lossy().to_string(),
                session_id: "root-session".to_string(),
                pane_id: root_pane.clone(),
                generation: 7,
            };
        let columns = vec![
            root_doc.to_string_lossy().to_string(),
            child_doc.to_string_lossy().to_string(),
        ];
        let _cwd = ScopedCurrentDir::set(&root);

        run_with_options_internal_at_root(
            &root,
            &columns,
            Some(&root_window),
            Some(root_doc.to_string_lossy().as_ref()),
            AutoStartMode::Full,
            true,
            false,
            &[root_binding],
            &MainLayoutEligibility::default(),
            &iso,
        )
        .unwrap();

        assert_eq!(
            iso.list_panes_ordered(&root_window).unwrap(),
            vec![root_pane, child_pane],
            "the cross-root child pane must resolve through the owning controller and move into the agent-doc window without the /proc ownership walk",
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-session-log-owner");
        let owner_pane = iso.new_session("test", tmp.path()).unwrap();
        respawn_pane_as_document_owner(&iso, &owner_pane, tmp.path(), &doc);
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-session-log-beats-process-tree");
        let stale_pane = iso.new_session("test", tmp.path()).unwrap();
        let owner_pane = iso.split_window(&stale_pane, tmp.path(), "-dh").unwrap();
        respawn_pane_as_document_owner(&iso, &owner_pane, tmp.path(), &doc);

        // `#ownerpaneflake`: the stale pane's same-file `codex <doc>` process
        // used to be typed into the pane's interactive shell with `send-keys`
        // and then polled for against a 3s deadline. That raced the operator's
        // shell startup (~2s for a zsh profile on an idle machine, unbounded
        // under load), so the precondition failed whenever the shell had not
        // read the keystrokes in time. The process is now exec'd directly as
        // the pane's root (multi-argument `respawn-pane` runs without a shell),
        // and the fixture waits for the readiness file the process itself
        // writes. Once that file exists the process is necessarily in `/proc`
        // with `<doc>` on its command line, so the process-tree precondition
        // is checked once, not polled against a clock.
        let fake_bin_dir = tmp.path().join("bin");
        std::fs::create_dir_all(&fake_bin_dir).unwrap();
        let fake_codex = fake_bin_dir.join("codex");
        let stale_ready = tmp.path().join("stale-codex.ready");
        // `sleep` runs as a child, not via `exec`, so the `codex <doc>` command
        // line that binds the document stays alive as the pane root.
        std::fs::write(
            &fake_codex,
            format!("#!/bin/sh\n: > '{}'\nsleep 60\n", stale_ready.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake_codex).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake_codex, perms).unwrap();
        }

        let fake_codex_arg = fake_codex.to_string_lossy();
        let doc_arg = doc.to_string_lossy();
        let cwd_arg = tmp.path().to_string_lossy();
        iso.raw_cmd(&[
            "respawn-pane",
            "-k",
            "-c",
            cwd_arg.as_ref(),
            "-t",
            &stale_pane,
            fake_codex_arg.as_ref(),
            doc_arg.as_ref(),
        ])
        .unwrap();

        // The bound only turns a process that never starts into a failure
        // message; success is the event (the readiness file), not elapsed time.
        assert!(
            wait_for(Duration::from_secs(120), || stale_ready.exists()),
            "stale pane's same-file codex process should start and write {}",
            stale_ready.display()
        );
        assert_eq!(
            find_alive_pane_for_file_inner(&iso, doc_arg.as_ref(), None, false).as_deref(),
            Some(stale_pane.as_str()),
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
    fn pane_occupant_tells_a_bare_harness_from_another_documents_owner() {
        // GH #82: both used to answer `pane_runs_other_document_owner == true`, and the
        // route then refused "it now runs another document" for a pane running only the
        // operator's own `claude`, which owns no document.
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("tasks").join("1109.md");
        let other = tmp.path().join("tasks").join("agent-doc.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: occupant-doc\n---\n").unwrap();
        std::fs::write(&other, "---\nagent_doc_session: occupant-other\n---\n").unwrap();

        let fake_bin_dir = tmp.path().join("bin");
        std::fs::create_dir_all(&fake_bin_dir).unwrap();
        for name in ["claude", "agent-doc"] {
            let fake = fake_bin_dir.join(name);
            std::fs::write(&fake, "#!/bin/sh\nsleep 60\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&fake).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&fake, perms).unwrap();
            }
        }

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-pane-occupant-classes");
        let shell_pane = iso.new_session("test", tmp.path()).unwrap();
        let bare_pane = iso.split_window(&shell_pane, tmp.path(), "-dh").unwrap();
        let owner_pane = iso.split_window(&shell_pane, tmp.path(), "-dv").unwrap();
        assert!(wait_for_shell(&iso, &shell_pane, Duration::from_secs(15)));
        for (pane, command) in [
            (
                &bare_pane,
                fake_bin_dir.join("claude").display().to_string(),
            ),
            (
                &owner_pane,
                format!(
                    "{} start {}",
                    fake_bin_dir.join("agent-doc").display(),
                    other.display()
                ),
            ),
        ] {
            iso.raw_cmd(&["send-keys", "-t", pane, &command, "Enter"])
                .unwrap();
        }

        assert!(
            wait_for(Duration::from_secs(15), || {
                pane_occupant_for_document(&iso, &bare_pane, &doc) == PaneOccupant::ForeignHarness
            }),
            "a bare harness owns no document and must read as foreign, not as another document"
        );
        assert!(
            wait_for(Duration::from_secs(15), || {
                matches!(
                    pane_occupant_for_document(&iso, &owner_pane, &doc),
                    PaneOccupant::OtherDocument(ref owned) if owned.ends_with("agent-doc.md")
                )
            }),
            "an agent-doc owner of a different document is the real conflict, and names it"
        );
        assert_eq!(
            pane_occupant_for_document(&iso, &shell_pane, &doc),
            PaneOccupant::Free
        );
        // The boolean predicate keeps its old meaning for every other caller.
        assert!(pane_runs_other_document_owner(&iso, &bare_pane, &doc));
        assert!(pane_runs_other_document_owner(&iso, &owner_pane, &doc));
        assert!(!pane_runs_other_document_owner(&iso, &shell_pane, &doc));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn foreign_root_pane_is_protected_from_a_nested_controller_only() {
        // `#stalecontrollerhotupdate` (c): agent-loop's agent-doc-bugs.md pane
        // shares a window with haiven-dev's panes. haiven-dev (nested) must
        // treat it as foreign and never stash it; agent-loop (enclosing) owns
        // the window and treats haiven-dev panes as its own.
        let tmp = tempfile::TempDir::new().unwrap();
        let outer = tmp.path().canonicalize().unwrap();
        let inner = outer.join("src").join("haiven-dev");
        std::fs::create_dir_all(outer.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(inner.join(".agent-doc")).unwrap();
        let bugs = outer.join("tasks").join("agent-doc-bugs.md");
        let sdk = inner.join("tasks").join("sdk.md");
        for doc in [&bugs, &sdk] {
            std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
            std::fs::write(doc, "---\nagent_doc_session: s\n---\n").unwrap();
        }
        let iso = agent_doc_tmux_io::isolated_test_tmux("stalecontroller-foreign-pane");
        let shell_pane = iso.new_session("test", &outer).unwrap();
        let bugs_pane = iso.split_window(&shell_pane, &outer, "-dh").unwrap();
        let sdk_pane = iso.split_window(&shell_pane, &inner, "-dh").unwrap();
        // Run each fake supervisor as the pane's own process (no interactive
        // shell startup to race), named `agent-doc` so tmux reports a live
        // agent rather than a bare shell, with a route-owned start command line.
        for (pane, doc) in [(&bugs_pane, &bugs), (&sdk_pane, &sdk)] {
            let command = format!(
                "bash -c \"exec -a agent-doc perl -e 'sleep 60' start --route-owned -- {}\"",
                doc.display()
            );
            iso.raw_cmd(&["respawn-pane", "-k", "-t", pane, &command])
                .unwrap();
        }
        let foreign = |pane: &str, root: &Path| {
            agent_doc_controller_io::project_controller::foreign_pane_owner_document(
                &iso, pane, root,
            )
        };
        assert!(
            wait_for(Duration::from_secs(15), || foreign(&bugs_pane, &inner)
                .is_some_and(|document| document == bugs)),
            "the superproject document's pane is foreign to the nested controller"
        );
        assert!(
            wait_for(Duration::from_secs(15), || {
                agent_doc_supervisor_process::session_liveness::pane_owns_live_agent(
                    &iso, &sdk_pane,
                )
            }),
            "the nested document's fake supervisor is live"
        );
        assert_eq!(
            foreign(&bugs_pane, &outer),
            None,
            "agent-loop owns its own pane"
        );
        assert_eq!(
            foreign(&sdk_pane, &outer),
            None,
            "agent-loop owns the main window"
        );
        assert_eq!(foreign(&sdk_pane, &inner), None, "haiven-dev's own pane");
        assert_eq!(
            foreign(&shell_pane, &inner),
            None,
            "a shell owns no document"
        );

        // The haiven-dev controller realises its own `[sdk]` in the shared
        // window: the foreign pane is protected from the stash, the unmanaged
        // shell is not.
        let window = agent_doc_tmux_io::target_window_id(&*iso, &sdk_pane).unwrap();
        let registry = tmp.path().join("registry.json");
        let pre_resolved: HashMap<PathBuf, String> =
            [(sdk.clone(), sdk_pane.clone())].into_iter().collect();
        let resolve = |_: &Path| {
            Some(tmux_router::FileResolution::Registered {
                key: "sdk".to_string(),
                tmux_session: None,
            })
        };
        let realise = |protect: &dyn Fn(&str) -> bool| {
            tmux_router::sync_with_options(
                &[sdk.display().to_string()],
                Some(&window),
                None,
                &iso,
                &registry,
                &resolve,
                &tmux_router::SyncOptions {
                    protect_pane: Some(protect),
                    pre_resolved_panes: Some(&pre_resolved),
                    ..tmux_router::SyncOptions::default()
                },
            )
            .unwrap();
            iso.list_window_panes(&window).unwrap()
        };
        let protect = |pane: &str| foreign(pane, &inner).is_some();
        let panes = realise(&protect);
        assert!(panes.contains(&bugs_pane), "foreign pane kept: {panes:?}");
        assert!(panes.contains(&sdk_pane), "own column realised: {panes:?}");
        assert!(
            !panes.contains(&shell_pane),
            "unmanaged shell stashed: {panes:?}"
        );
        // Sensitivity: without the guard the same pass stashes the foreign pane.
        let panes = realise(&|_: &str| false);
        assert!(
            !panes.contains(&bugs_pane),
            "unguarded pass stashes it: {panes:?}"
        );

        // The guard is armed only while another root holds the window claim:
        // a nested controller that holds it (the editor went 2 -> 1 pane onto
        // its document) may drop the foreign pane.
        use agent_doc_controller::layout_owner::{WINDOW_LAYOUT_CLAIM_OPTION, WindowLayoutClaim};
        let driven_by_other = |root: &Path| {
            agent_doc_controller_io::project_controller::window_layout_driven_by_other_root(
                &iso, &window, root,
            )
        };
        assert!(!driven_by_other(&inner), "no claim: unarmed");
        let claim = |root: &Path| WindowLayoutClaim {
            project_root: root.to_path_buf(),
            stamp_ms: 1,
            generation: 1,
            publication: None,
        };
        agent_doc_tmux_io::set_window_user_option(
            &*iso,
            &window,
            WINDOW_LAYOUT_CLAIM_OPTION,
            &claim(&outer).encode(),
        )
        .unwrap();
        assert!(driven_by_other(&inner));
        assert!(!driven_by_other(&outer));
        agent_doc_tmux_io::set_window_user_option(
            &*iso,
            &window,
            WINDOW_LAYOUT_CLAIM_OPTION,
            &claim(&inner).encode(),
        )
        .unwrap();
        assert!(!driven_by_other(&inner));
        assert!(driven_by_other(&outer));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn layout_column_never_reuses_a_pane_bound_to_another_document() {
        // GH #109 ask 1: a pane whose agent-doc supervisor is bound to document A
        // must never realise document B's column, however live it is; it stays
        // eligible for A's own column. Also pins the reusable freshness predicate
        // on real panes: the column's own supervisor is found in the pane tree,
        // and agent-doc's `⚠ STALE SUPERVISOR` title alone never reads as stale
        // (GH #121).
        let tmp = tempfile::TempDir::new().unwrap();
        let doc_a = tmp.path().join("tasks").join("1102.md");
        let doc_b = tmp.path().join("tasks").join("agent-doc.ad.md");
        std::fs::create_dir_all(doc_a.parent().unwrap()).unwrap();
        std::fs::write(&doc_a, "---\nagent_doc_session: column-a\n---\n").unwrap();
        std::fs::write(&doc_b, "---\nagent_doc_session: column-b\n---\n").unwrap();

        let fake_bin_dir = tmp.path().join("bin");
        std::fs::create_dir_all(&fake_bin_dir).unwrap();
        let fake = fake_bin_dir.join("agent-doc");
        std::fs::write(&fake, "#!/bin/sh\nsleep 60\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-column-foreign-document");
        let shell_pane = iso.new_session("test", tmp.path()).unwrap();
        let owner_a = iso.split_window(&shell_pane, tmp.path(), "-dh").unwrap();
        assert!(wait_for_shell(&iso, &shell_pane, Duration::from_secs(15)));
        let command = format!(
            "{} start --route-owned -- {}",
            fake.display(),
            doc_a.display()
        );
        iso.raw_cmd(&["send-keys", "-t", &owner_a, &command, "Enter"])
            .unwrap();
        assert!(
            wait_for(Duration::from_secs(15), || {
                matches!(
                    pane_occupant_for_document(&iso, &owner_a, &doc_b),
                    PaneOccupant::OtherDocument(ref owned) if owned.ends_with("1102.md")
                )
            }),
            "the supervisor bound to A must read as another document's owner for B"
        );
        assert!(
            !pane_projection_is_reusable_for_document(&iso, &owner_a, &doc_b),
            "A's live pane must never satisfy B's column"
        );
        assert!(
            pane_projection_is_reusable_for_document(&iso, &owner_a, &doc_a),
            "A's own pane stays eligible for A's column"
        );

        let own =
            crate::layout_column_audit::pane_supervisor_freshness(&iso, &owner_a, &doc_a, None);
        assert_ne!(
            own,
            crate::layout_column_audit::PaneSupervisorFreshness::Unknown {
                reason: "no_supervisor_process"
            },
            "the column's own supervisor must be found in the pane's process tree"
        );

        assert_eq!(
            crate::layout_column_audit::pane_supervisor_freshness(&iso, &shell_pane, &doc_a, None),
            crate::layout_column_audit::PaneSupervisorFreshness::Unknown {
                reason: "no_supervisor_process"
            }
        );
        iso.raw_cmd(&[
            "select-pane",
            "-t",
            &shell_pane,
            "-T",
            agent_doc_turn::turn_status::STALE_SUPERVISOR_PANE_MARKER,
        ])
        .unwrap();
        assert_eq!(
            crate::layout_column_audit::pane_supervisor_freshness(&iso, &shell_pane, &doc_a, None),
            crate::layout_column_audit::PaneSupervisorFreshness::Unknown {
                reason: "no_supervisor_process"
            },
            "GH #121: agent-doc's own stale-supervisor title is never evidence of staleness"
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-live-rebind-owner");
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-authoritative-actor-owner");
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-associated-registry-rebind");
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-open-log-owner-fail-closed");
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
        let tmux = agent_doc_tmux_io::configured_tmux();
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-passive-closed");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-passive-open");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-passive-rebind-live");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-passive-rebind-stale");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-passive-miss");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-skip-correct");
        let tmp = tempfile::TempDir::new().unwrap();

        // Create session with agent-doc window at index 0 + one stash window
        let _pane = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let _ = iso.ensure_stash_window("test");

        let windows_before = list_windows(&iso, "test");

        // repair_layout should succeed and not change anything
        let outcome = repair_layout(&iso, "test", "agent-doc").unwrap();

        let windows_after = list_windows(&iso, "test");
        assert_eq!(
            windows_before, windows_after,
            "layout was already correct — nothing should change"
        );
        // `#syncdoctorconverged`: a correct layout must also *report* that it
        // changed nothing, so the doctor pass stops claiming a repair on every
        // sync and the caller can skip the note.
        assert_eq!(
            outcome,
            LayoutRepairOutcome::AlreadyConverged,
            "an already-correct layout must report AlreadyConverged, not a repair"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_pins_managed_names_without_promoting_stash() {
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-stable-role-names");
        let tmp = tempfile::TempDir::new().unwrap();
        let first = iso.new_session("test", tmp.path()).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
            .unwrap();
        iso.raw_cmd(&["split-window", "-dh", "-t", &first]).unwrap();
        let visible = iso.pane_window(&first).unwrap();
        let stash = iso.ensure_stash_window("test").unwrap();
        for _ in 0..3 {
            iso.raw_cmd(&["split-window", "-dv", "-t", &stash]).unwrap();
        }
        let other = iso
            .raw_cmd(&[
                "new-window",
                "-d",
                "-t",
                "test:",
                "-n",
                "operator",
                "-P",
                "-F",
                "#{window_id}",
            ])
            .unwrap()
            .trim()
            .to_string();
        let visible_before = iso.list_window_panes(&visible).unwrap();
        let stash_before = iso.list_window_panes(&stash).unwrap();
        assert_eq!(visible_before.len(), 2);
        assert_eq!(stash_before.len(), 4);
        for (window, name) in [
            (&visible, "agent-doc"),
            (&stash, "stash"),
            (&other, "operator"),
        ] {
            // Hold the observed name deterministic while automatic renaming is
            // enabled; repair must disable the option, not rely on this format.
            iso.raw_cmd(&[
                "set-window-option",
                "-t",
                window,
                "automatic-rename-format",
                name,
            ])
            .unwrap();
            for option in ["automatic-rename", "allow-rename"] {
                iso.raw_cmd(&["set-window-option", "-t", window, option, "on"])
                    .unwrap();
            }
        }
        assert_eq!(
            repair_layout(&iso, "test", "agent-doc").unwrap(),
            LayoutRepairOutcome::Repaired
        );
        for window in [&visible, &stash, &other] {
            for option in ["automatic-rename", "allow-rename"] {
                let value = iso
                    .raw_cmd(&["show-window-options", "-v", "-t", window, option])
                    .unwrap();
                assert_eq!(value.trim(), if window == &other { "on" } else { "off" });
            }
        }
        // A subsequent child command name cannot rename this stash to agent-doc.
        iso.raw_cmd(&[
            "set-window-option",
            "-t",
            &stash,
            "automatic-rename-format",
            "agent-doc",
        ])
        .unwrap();
        assert_eq!(
            repair_layout(&iso, "test", "agent-doc").unwrap(),
            LayoutRepairOutcome::AlreadyConverged
        );
        assert_eq!(iso.list_window_panes(&visible).unwrap(), visible_before);
        assert_eq!(iso.list_window_panes(&stash).unwrap(), stash_before);
        assert_eq!(
            window_name_for_window_id(&iso, &stash).as_deref(),
            Some("stash")
        );
    }

    /// `#syncdoctorconverged`: the converged fast path must skip the always-run
    /// consolidation/index-normalization phases entirely — a drifted layout
    /// still has to report a real repair, so the outcome distinguishes the two.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_reports_repair_only_when_it_moves_windows() {
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-outcome-honest");
        let tmp = tempfile::TempDir::new().unwrap();

        let _pane = iso.new_session("test", tmp.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let _ = iso.ensure_stash_window("test");

        assert_eq!(
            repair_layout(&iso, "test", "agent-doc").unwrap(),
            LayoutRepairOutcome::AlreadyConverged
        );

        // A stash alias is exactly the drift Phase 4 renames back to `stash`.
        let stash_window = list_session_windows(&iso, "test")
            .into_iter()
            .find(|(_, _, name)| is_stash_window_name(name))
            .map(|(_, id, _)| id)
            .expect("stash window");
        iso.raw_cmd(&["rename-window", "-t", &stash_window, "stash-2"])
            .unwrap();

        assert_eq!(
            repair_layout(&iso, "test", "agent-doc").unwrap(),
            LayoutRepairOutcome::Repaired,
            "a stash alias awaiting rename must still draw a real repair"
        );
        assert_eq!(
            repair_layout(&iso, "test", "agent-doc").unwrap(),
            LayoutRepairOutcome::AlreadyConverged,
            "the repair must converge — a second pass has nothing left to do"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_unpins_legacy_staging_height() {
        // `#stashresizerestore`: older repair code restored the visible
        // dimensions but left `window-size=manual`, while still older code could
        // leave the full 1000-row staging geometry behind. A normal repair must
        // heal both without touching unrelated windows.
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-unpin-staging-height");
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

    fn gh244_tmux_value(iso: &tmux_router::Tmux, args: &[&str]) -> String {
        iso.raw_cmd(args).unwrap().trim().to_string()
    }

    fn gh244_window_size(iso: &tmux_router::Tmux, target: &str) -> String {
        gh244_tmux_value(
            iso,
            &[
                "display-message",
                "-p",
                "-t",
                target,
                "#{window_width}x#{window_height}",
            ],
        )
    }

    fn gh244_local_window_size(iso: &tmux_router::Tmux, target: &str) -> String {
        gh244_tmux_value(
            iso,
            &["show-options", "-w", "-v", "-t", target, "window-size"],
        )
    }

    /// A real terminal client (GH #244): control-mode clients report no
    /// `#{client_height}`, so the "smaller than every client" decision needs a
    /// pty-backed `tmux attach` whose size the test can grow.
    struct Gh244PtyClient {
        master: Box<dyn portable_pty::MasterPty + Send>,
        child: Box<dyn portable_pty::Child + Send + Sync>,
    }

    impl Gh244PtyClient {
        fn attach(
            iso: &agent_doc_tmux_io::IsolatedTestTmux,
            session: &str,
            cols: u16,
            rows: u16,
        ) -> Self {
            let pair = portable_pty::native_pty_system()
                .openpty(Self::size(cols, rows))
                .expect("openpty");
            let mut command = portable_pty::CommandBuilder::new("tmux");
            command.arg("-S");
            command.arg(iso.socket_path());
            command.args(["attach-session", "-t", session]);
            command.env_remove("TMUX");
            command.env("TERM", "xterm-256color");
            let child = pair
                .slave
                .spawn_command(command)
                .expect("spawn tmux client");
            drop(pair.slave);
            // Drain the client's output so it never blocks on a full pty.
            let mut reader = pair.master.try_clone_reader().expect("pty reader");
            std::thread::spawn(move || {
                let mut sink = [0u8; 4096];
                while matches!(std::io::Read::read(&mut reader, &mut sink), Ok(n) if n > 0) {}
            });
            Self {
                master: pair.master,
                child,
            }
        }

        fn size(cols: u16, rows: u16) -> portable_pty::PtySize {
            portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            }
        }

        fn resize(&self, cols: u16, rows: u16) {
            self.master
                .resize(Self::size(cols, rows))
                .expect("resize pty");
        }
    }

    impl Drop for Gh244PtyClient {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn gh244_wait_until(what: &str, mut ready: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !ready() {
            assert!(std::time::Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_refits_agent_doc_window_pinned_below_its_client_gh244() {
        // GH #244: a client attaches, the operator's "fit window" key runs
        // `resize-window -A` (which leaves a per-window `window-size manual`),
        // then the terminal grows. The owned window stays small and dot-padded
        // until repair releases the pin.
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-gh244-manual-pin-refit");
        let tmp = tempfile::TempDir::new().unwrap();
        let _pane = iso.new_session("test", tmp.path()).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
            .unwrap();
        let _ = iso.ensure_stash_window("test");
        iso.raw_cmd(&["set-option", "-g", "window-size", "latest"])
            .unwrap();
        iso.raw_cmd(&["select-window", "-t", "test:agent-doc"])
            .unwrap();

        let client = Gh244PtyClient::attach(&iso, "test", 100, 30);
        gh244_wait_until("window follows the 100x30 client", || {
            gh244_window_size(&iso, "test:agent-doc") == "100x29"
        });

        iso.raw_cmd(&["resize-window", "-A", "-t", "test:agent-doc"])
            .unwrap();
        assert_eq!(gh244_local_window_size(&iso, "test:agent-doc"), "manual");

        client.resize(160, 40);
        gh244_wait_until("client grows to 160x40", || {
            gh244_tmux_value(
                &iso,
                &[
                    "list-clients",
                    "-t",
                    "test",
                    "-F",
                    "#{client_width}x#{client_height}",
                ],
            ) == "160x40"
        });
        assert_eq!(
            gh244_window_size(&iso, "test:agent-doc"),
            "100x29",
            "the manual pin holds the window at the old client size"
        );

        assert_eq!(
            repair_layout(&iso, "test", "agent-doc").unwrap(),
            LayoutRepairOutcome::Repaired,
            "a pinned, undersized owned window is not converged"
        );
        assert_eq!(
            gh244_local_window_size(&iso, "test:agent-doc"),
            "",
            "repair must release the per-window pin"
        );
        gh244_wait_until("window re-fits the 160x40 client", || {
            gh244_window_size(&iso, "test:agent-doc") == "160x39"
        });
        assert_eq!(
            repair_layout(&iso, "test", "agent-doc").unwrap(),
            LayoutRepairOutcome::AlreadyConverged,
            "the re-fit converges"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_releases_a_pin_taken_across_two_clients_gh268() {
        // GH #268: `resize-window -A` with a narrow-tall and a wide-short
        // client attached pins the owned window at the larger width and the
        // larger height. That pin is larger than each client in one dimension,
        // so the #244 "smaller than every client" guard never released it, not
        // even after the extra client detached.
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-gh268-two-client-pin");
        let tmp = tempfile::TempDir::new().unwrap();
        let _pane = iso.new_session("test", tmp.path()).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
            .unwrap();
        let _ = iso.ensure_stash_window("test");
        iso.raw_cmd(&["set-option", "-g", "window-size", "latest"])
            .unwrap();
        iso.raw_cmd(&["select-window", "-t", "test:agent-doc"])
            .unwrap();

        let attached = || {
            gh244_tmux_value(
                &iso,
                &[
                    "list-clients",
                    "-t",
                    "test",
                    "-F",
                    "#{client_width}x#{client_height}",
                ],
            )
            .lines()
            .count()
        };
        let pin_across_both = || {
            iso.raw_cmd(&["resize-window", "-A", "-t", "test:agent-doc"])
                .unwrap();
            assert_eq!(gh244_local_window_size(&iso, "test:agent-doc"), "manual");
            assert_eq!(
                gh244_window_size(&iso, "test:agent-doc"),
                "202x29",
                "-A takes the larger width and the larger height"
            );
        };

        let ide = Gh244PtyClient::attach(&iso, "test", 202, 15);
        gh244_wait_until("the IDE client attaches", || attached() == 1);
        let powershell = Gh244PtyClient::attach(&iso, "test", 120, 30);
        gh244_wait_until("the second client attaches", || attached() == 2);

        // Both clients attached: the pin crops one and pads the other.
        pin_across_both();
        assert_eq!(
            repair_layout(&iso, "test", "agent-doc").unwrap(),
            LayoutRepairOutcome::Repaired,
            "a pin no attached client is shown correctly at is not converged"
        );
        assert_eq!(
            gh244_local_window_size(&iso, "test:agent-doc"),
            "",
            "repair must release the pin while both clients are attached"
        );
        assert_eq!(
            repair_layout(&iso, "test", "agent-doc").unwrap(),
            LayoutRepairOutcome::AlreadyConverged,
            "the release converges"
        );

        // Re-pinned, then the extra client goes away: the window is still
        // taller than the IDE client's 14 rows and must be released.
        pin_across_both();
        drop(powershell);
        gh244_wait_until("the second client detaches", || attached() == 1);
        assert_eq!(gh244_window_size(&iso, "test:agent-doc"), "202x29");
        assert_eq!(
            repair_layout(&iso, "test", "agent-doc").unwrap(),
            LayoutRepairOutcome::Repaired
        );
        assert_eq!(gh244_local_window_size(&iso, "test:agent-doc"), "");
        gh244_wait_until("window re-fits the 202x15 client", || {
            gh244_window_size(&iso, "test:agent-doc") == "202x14"
        });
        drop(ide);
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_keeps_manual_pins_it_does_not_own_gh244() {
        // `#bare-foreign-session-guard`: an operator's own window keeps its
        // pin, and an owned window with no attached client has nothing to fit.
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-gh244-foreign-pin-kept");
        let tmp = tempfile::TempDir::new().unwrap();
        let _pane = iso.new_session("test", tmp.path()).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
            .unwrap();
        let _ = iso.ensure_stash_window("test");
        iso.raw_cmd(&["new-window", "-d", "-t", "test:", "-n", "editor"])
            .unwrap();
        iso.raw_cmd(&["set-option", "-g", "window-size", "latest"])
            .unwrap();
        iso.raw_cmd(&["select-window", "-t", "test:agent-doc"])
            .unwrap();
        let _idle = iso.new_session("idle", tmp.path()).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "idle:0", "agent-doc"])
            .unwrap();
        let _ = iso.ensure_stash_window("idle");

        let _client = Gh244PtyClient::attach(&iso, "test", 160, 40);
        gh244_wait_until("window follows the 160x40 client", || {
            gh244_window_size(&iso, "test:agent-doc") == "160x39"
        });

        // `resize-window -x/-y` pins exactly like `-A` does.
        for target in ["test:editor", "idle:agent-doc"] {
            iso.raw_cmd(&["resize-window", "-t", target, "-x", "100", "-y", "30"])
                .unwrap();
            assert_eq!(gh244_local_window_size(&iso, target), "manual");
        }

        repair_layout(&iso, "test", "agent-doc").unwrap();
        repair_layout(&iso, "idle", "agent-doc").unwrap();

        for target in ["test:editor", "idle:agent-doc"] {
            assert_eq!(
                gh244_local_window_size(&iso, target),
                "manual",
                "{target} must keep its pin"
            );
            assert_eq!(gh244_window_size(&iso, target), "100x30", "{target}");
        }
        assert_eq!(gh244_window_size(&iso, "test:agent-doc"), "160x39");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_zero_stash_target_is_noop_tmuxsynccrash() {
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-zero-stash-target-noop");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-move-idx0");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-duplicate-agent-doc");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-stash-index");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-stash-alias-index");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-full-repairs-window-order");
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
        respawn_pane_as_document_owner(&iso, &pane0, root, &doc);
        sessions::register_full_with_cwd(
            "full-sync-repair",
            &pane0,
            &doc_str,
            pane_pid_from_tmux(&iso, &pane0).unwrap(),
            &agent_doc_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        agent_doc_session_actor_io::project_binding_in(
            root,
            &doc_str,
            "full-sync-repair",
            &pane0,
            &agent_doc_window,
            "sync",
            "full_sync_window_order_fixture",
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
        if let Some((index, _)) = windows_after.iter().find(|(_, name)| name == "stash") {
            assert_eq!(
                index, "1",
                "a surviving primary stash window must be repaired to 1:stash"
            );
        }
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-full-doctor-repairs-stash-window");
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
        respawn_pane_as_document_owner(&iso, &pane0, root, &doc);
        sessions::register_full_with_cwd(
            "full-sync-doctor-repair",
            &pane0,
            &doc_str,
            pane_pid_from_tmux(&iso, &pane0).unwrap(),
            &stash_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        agent_doc_session_actor_io::project_binding_in(
            root,
            &doc_str,
            "full-sync-doctor-repair",
            &pane0,
            &stash_window,
            "sync",
            "full_sync_doctor_fixture",
        )
        .unwrap();
        let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash", "-d"]);

        run_with_options_internal(
            std::slice::from_ref(&doc_str),
            Some("test:0"),
            Some(doc_str.as_str()),
            AutoStartMode::Full,
            false,
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
        if let Some((index, _)) = windows_after.iter().find(|(_, name)| name == "stash") {
            assert_eq!(
                index, "1",
                "a surviving repaired stash window must remain at 1:stash"
            );
        }
        assert!(
            !windows_after
                .iter()
                .any(|(_, name)| name.starts_with("stash-")),
            "doctor repair must not leave legacy stash aliases"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_layout_rescues_pane_from_stash() {
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-rescue-stash");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-promote-stash");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-promote-noop");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-pane-in-stash");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-consolidate");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-repair-swap-idx0");
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
        assert!(
            content.contains(
                "## Run <!-- agent:run priority subagents clear -->\n<!-- /agent:run -->"
            ),
            "sync must use claim's canonical agent:run scaffold"
        );
        assert!(!content.contains("## Queue"));
        assert!(
            !content.contains("<!-- agent:queue"),
            "new sync scaffolds must not emit the legacy agent:queue alias"
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
        assert!(content.contains("<!-- agent:run priority subagents clear -->"));
        assert!(!content.contains("<!-- agent:queue"));

        // Sync and Claim for Tmux Pane share this generator. Parse the rendered
        // document so parity covers structure, balance, and canonical ordering.
        let components = agent_doc_element::element::parse(&content)
            .expect("sync scaffold must form a valid component AST");
        let component_names = components
            .iter()
            .map(|component| component.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            component_names,
            [
                "status", "exchange", "queue", "todo", "backlog", "review", "icebox", "done",
                "notes",
            ],
            "sync must render the same complete scaffold as claim"
        );
        assert_eq!(components[0].patch_mode(), Some("replace"));
        assert_eq!(components[1].patch_mode(), Some("append"));
        assert_eq!(
            components[2].attrs.get("priority").map(String::as_str),
            Some("")
        );
        assert_eq!(
            components[2].attrs.get("subagents").map(String::as_str),
            Some("")
        );
        assert_eq!(
            components[2].attrs.get("clear").map(String::as_str),
            Some("")
        );
        assert_eq!(components[2].attrs.len(), 3);
        let todo = components
            .iter()
            .find(|component| component.name == "todo")
            .expect("sync scaffold includes todo");
        assert_eq!(todo.attrs.get("priority").map(String::as_str), Some(""));
        let backlog = components
            .iter()
            .find(|component| component.name == "backlog")
            .expect("sync scaffold includes backlog");
        assert_eq!(backlog.attrs.get("priority").map(String::as_str), Some(""));
        assert_eq!(backlog.attrs.get("queue").map(String::as_str), Some(""));
        assert!(
            components[3..]
                .iter()
                .filter(|component| { component.name != "todo" && component.name != "backlog" })
                .all(|component| component.attrs.is_empty())
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-log-move-window");
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
    /// `#netadv5` R7: a slow prior sync holds the lock past the wait budget.
    /// The full sync must abort retryably (no unlocked reconcile), and once
    /// the holder releases, a retry acquires and proceeds.
    #[test]
    fn contended_full_sync_aborts_then_progresses_after_release() {
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

        let err = acquire_sync_lock_for_mode(
            &lock_path,
            AutoStartMode::Full,
            Duration::from_millis(80),
            |_| {},
        )
        .unwrap_err();
        assert!(
            format!("{err:#}").contains(agent_doc_sync::SYNC_LOCK_CONTENDED_RETRY_MARKER),
            "{err:#}"
        );
        let passive = acquire_sync_lock_for_mode(
            &lock_path,
            AutoStartMode::SafePassive,
            Duration::from_millis(20),
            |_| {},
        )
        .unwrap();
        assert!(passive.is_none(), "safe-passive contention skips quietly");

        fs2::FileExt::unlock(&holder).unwrap();
        let retried = acquire_sync_lock_for_mode(
            &lock_path,
            AutoStartMode::Full,
            Duration::from_millis(80),
            |_| {},
        )
        .unwrap()
        .expect("the retry after release proceeds");
        assert!(retried.is_acquired());
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
    fn safe_passive_sync_preserves_operator_focus_when_sync_lock_is_contended() {
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-postlock-actor-focus");
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
            &agent_doc_controller::actor::ActorRecord {
                document_id: agent_doc_session_actor_io::canonical_document_id_in(
                    root,
                    &stale_doc.to_string_lossy(),
                ),
                session_id: "stale-session".to_string(),
                generation: 1,
                pane_id: stale_pane.clone(),
                window_id: active_window.clone(),
                harness: "codex".to_string(),
                state: agent_doc_controller::actor::ActorState::Starting,
                last_transition: agent_doc_controller::actor::ActorLastTransition {
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
            false,
            &iso,
        )
        .unwrap();

        fs2::FileExt::unlock(&holder).unwrap();
        assert_eq!(
            iso.active_pane("test").unwrap(),
            stale_pane,
            "safe-passive editor sync must not steal operator focus before sync lock contention defers prune/reconcile"
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
            agent_doc_sync::is_file_rename(old_file, new_file),
            "old path doesn't exist on disk, paths differ → rename"
        );

        // Verify we can load the entry and see the old path
        let reg = agent_doc_session_registry_io::load_in(project).unwrap();
        let key = tmux_router::registry::find_registry_key_by_session_id(&reg, session_id).unwrap();
        let entry = reg.get(&key).unwrap();
        assert_eq!(entry.file, old_file);
        assert_eq!(entry.pane, "%42");
    }
    /// Fixed virtual "now" for the rename-debounce tests (2026-01-01T00:00:00Z).
    const RENAME_DEBOUNCE_TEST_EPOCH_SECS: u64 = 1_767_225_600;
    #[test]
    fn rename_debounce_suppresses_auto_start() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();

        // Create a file with known content for hashing
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "---\nagent_doc_session: abc123\n---\n").unwrap();

        // Virtual clock: the window is pinned to the recorded heartbeat, so the
        // assertion does not depend on how long the state.db write and read take
        // on a loaded host.
        let recorded_at = RENAME_DEBOUNCE_TEST_EPOCH_SECS;
        assert!(write_rename_debounce_at(&file.to_string_lossy(), recorded_at).unwrap());
        assert!(has_rename_debounce_at(&file, recorded_at));
        assert!(has_rename_debounce_at(
            &file,
            recorded_at + RENAME_DEBOUNCE_TTL_SECS - 1
        ));
        assert!(!tmp.path().join(".agent-doc/rename-debounce").exists());
    }
    #[test]
    fn rename_debounce_expires_after_ttl() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("test.md");
        std::fs::write(&file, "---\nagent_doc_session: expire123\n---\n").unwrap();

        let recorded_at = RENAME_DEBOUNCE_TEST_EPOCH_SECS;
        assert!(write_rename_debounce_at(&file.to_string_lossy(), recorded_at).unwrap());
        assert!(has_rename_debounce_at(&file, recorded_at));
        assert!(
            !has_rename_debounce_at(&file, recorded_at + RENAME_DEBOUNCE_TTL_SECS),
            "a lease aged exactly the TTL is expired"
        );
        // The expired check clears the lease: even a check back inside the
        // original window no longer finds it.
        assert!(!has_rename_debounce_at(&file, recorded_at));
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
        let recorded_at = RENAME_DEBOUNCE_TEST_EPOCH_SECS;
        assert!(write_rename_debounce_at(&file_a.to_string_lossy(), recorded_at).unwrap());

        // file_b should have a different hash, no marker
        let hash_b = agent_doc_fs::document_state_hash(&file_b).unwrap();
        assert_ne!(
            hash_a, hash_b,
            "different files should have different hashes"
        );
        assert!(has_rename_debounce_at(&file_a, recorded_at));
        assert!(!has_rename_debounce_at(&file_b, recorded_at));
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-stash-discover-window");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-deferred-stash-rescue");
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
        respawn_pane_as_document_owner(&iso, &pane0, tmp.path(), &doc_a);
        respawn_pane_as_document_owner(&iso, &pane1, tmp.path(), &doc_b);
        sessions::register_full_with_cwd(
            session_a,
            &pane0,
            &doc_a.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane0).unwrap(),
            &win,
            &tmp.path().to_string_lossy(),
        )
        .unwrap();
        agent_doc_session_actor_io::project_binding_in(
            tmp.path(),
            &doc_a.to_string_lossy(),
            session_a,
            &pane0,
            &win,
            "sync",
            "deferred_stash_rescue_fixture",
        )
        .unwrap();
        sessions::register_full_with_cwd(
            session_b,
            &pane1,
            &doc_b.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane1).unwrap(),
            &win,
            &tmp.path().to_string_lossy(),
        )
        .unwrap();
        agent_doc_session_actor_io::project_binding_in(
            tmp.path(),
            &doc_b.to_string_lossy(),
            session_b,
            &pane1,
            &win,
            "sync",
            "deferred_stash_rescue_fixture",
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-windowless-target-agent-doc");
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-windowless-project-pin");
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
            resolve_sync_target_session(&iso, None, &[], None, &[])
                .unwrap()
                .as_deref(),
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-windowless-current-agent-doc");
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
            resolve_sync_target_session(&iso, None, &[], None, &[])
                .unwrap()
                .as_deref(),
            Some("1"),
            "windowless sync should follow the current live agent-doc window before the configured pin"
        );
    }

    /// `#ideriderindependentnav` P2b: the sync body's defense-in-depth lane
    /// guard is the identity unless the project opts into per-installation
    /// navigation and the target is one of the pin's lanes.
    #[test]
    fn navigation_lane_guard_is_identity_without_per_installation_lanes() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        // Never started: any tmux read would fail, proving none is attempted.
        let tmux = Tmux::default_server()
            .with_server_socket(Some("agent-doc-test-lane-guard-never-started".to_string()));
        let columns = vec!["/p/a.md".to_string(), "/p/b.md,/p/c.md".to_string()];
        for config in [
            "",
            "tmux_session = \"proj\"\n",
            "tmux_session = \"proj\"\neditor_navigation = \"shared\"\n",
            "editor_navigation = \"per_installation\"\n",
        ] {
            std::fs::write(tmp.path().join(".agent-doc/config.toml"), config).unwrap();
            let (kept, dropped) = drop_columns_owned_by_other_navigation_lanes(
                &tmux,
                tmp.path(),
                columns.clone(),
                "proj-IU-0a1b2c3d",
                &[],
            );
            assert_eq!(kept, columns, "{config:?}");
            assert!(dropped.is_empty(), "{config:?}");
        }
        std::fs::write(
            tmp.path().join(".agent-doc/config.toml"),
            "tmux_session = \"proj\"\neditor_navigation = \"per_installation\"\n",
        )
        .unwrap();
        let (kept, dropped) = drop_columns_owned_by_other_navigation_lanes(
            &tmux,
            tmp.path(),
            columns.clone(),
            "research",
            &[],
        );
        assert_eq!(kept, columns, "a foreign target session is not a lane");
        assert!(dropped.is_empty());
    }

    /// `#ideriderindependentnav` P2b: a column whose document's live pane is in
    /// another lane of the project never reaches the lane's sync body. Isolated
    /// tmux server only.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn navigation_lane_guard_drops_documents_whose_pane_lives_in_another_lane() {
        let tmux = agent_doc_tmux_io::isolated_test_tmux("lane-guard");
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/config.toml"),
            "tmux_session = \"proj\"\neditor_navigation = \"per_installation\"\n",
        )
        .unwrap();
        let shared_pane = tmux.new_session("proj", tmp.path()).unwrap();
        let idea_pane = tmux.new_session("proj-IU-0a1b2c3d", tmp.path()).unwrap();
        let rider_pane = tmux.new_session("proj-RD-9f8e7d6c", tmp.path()).unwrap();
        let doc = |name: &str| {
            let path = tmp.path().join(name);
            std::fs::write(&path, "# doc\n").unwrap();
            path.to_string_lossy().into_owned()
        };
        let (shared_doc, idea_doc, rider_doc, new_doc) = (
            doc("shared.md"),
            doc("idea.md"),
            doc("rider.md"),
            doc("new.md"),
        );
        let binding = |document: &str, pane: &str| {
            agent_doc_controller_io::project_controller::ControllerTmuxActorBinding {
                document_path: document.to_string(),
                session_id: String::new(),
                pane_id: pane.to_string(),
                generation: 1,
            }
        };
        let bindings = vec![
            binding(&shared_doc, &shared_pane),
            binding(&idea_doc, &idea_pane),
            binding(&rider_doc, &rider_pane),
        ];
        let columns = vec![
            idea_doc.clone(),
            format!("{shared_doc},{new_doc}"),
            rider_doc.clone(),
        ];
        let (kept, dropped) = drop_columns_owned_by_other_navigation_lanes(
            &tmux,
            tmp.path(),
            columns,
            "proj-IU-0a1b2c3d",
            &bindings,
        );
        assert_eq!(kept, vec![idea_doc, new_doc]);
        assert_eq!(
            dropped,
            vec![
                (shared_doc, "proj".to_string()),
                (rider_doc, "proj-RD-9f8e7d6c".to_string()),
            ]
        );
    }

    /// `#ideriderindependentnav` P3: the end-to-end two-lane case through the
    /// real sync body. IDEA and Rider each get a freshly provisioned lane
    /// session (exactly as the controller and the IDE terminal attach create
    /// it: an `agent-doc` window holding one empty shell pane, plus `stash`).
    /// Under alternating syncs with two different layouts, where Rider's layout
    /// also names a document whose pane lives in IDEA's lane:
    /// - each lane's `agent-doc` window shows exactly its own layout, and the
    ///   tmux layout of each window is stable from one round to the next;
    /// - the contended document is dropped from Rider's layout, never moved;
    /// - no pane ever changes session, and the shared session is untouched;
    /// - the lane session's initial empty shell pane is stashed inside its own
    ///   lane session on the first sync, never left as a visible extra column.
    ///
    /// Isolated tmux server only.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn navigation_lane_guard_two_lanes_alternate_without_crossing_sessions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"proj\"\neditor_navigation = \"per_installation\"\n",
        )
        .unwrap();
        let _cwd = ScopedCurrentDir::set(&root);
        let iso = agent_doc_tmux_io::isolated_test_tmux("lane-guard-e2e");

        // The operator's shared session, which neither lane may touch.
        let shared_pane = iso.new_session("proj", &root).unwrap();
        let shared_window = iso.pane_window(&shared_pane).unwrap();
        iso.raw_cmd(&["rename-window", "-t", &shared_window, "agent-doc"])
            .unwrap();
        let shared_panes_before = iso.list_window_panes(&shared_window).unwrap();

        let lane_session = |client: &str| {
            agent_doc_controller_io::project_controller::navigation_lane_session_for_client(
                &iso, &root, client,
            )
            .unwrap()
            .expect("an installation lane")
        };
        let idea = lane_session("jetbrains@IU.0a1b2c3d-pid:41");
        let rider = lane_session("jetbrains@RD.9f8e7d6c-pid:42");
        assert!(idea.provisioned && rider.provisioned);
        let agent_doc_window = |session: &str| {
            iso.raw_cmd(&[
                "list-windows",
                "-t",
                &format!("{session}:"),
                "-F",
                "#{window_id} #{window_name}",
            ])
            .unwrap()
            .lines()
            .find_map(|line| {
                let (id, name) = line.split_once(' ')?;
                (name == "agent-doc").then(|| id.to_string())
            })
            .expect("lane agent-doc window")
        };
        let idea_window = agent_doc_window(&idea.session);
        let rider_window = agent_doc_window(&rider.session);
        // A new lane session starts with one empty shell pane in `agent-doc`.
        let idea_initial = iso.list_window_panes(&idea_window).unwrap();
        let rider_initial = iso.list_window_panes(&rider_window).unwrap();
        assert_eq!(idea_initial.len(), 1);
        assert_eq!(rider_initial.len(), 1);

        // Each document's pane starts outside its lane's agent-doc window, in
        // its own lane session.
        let document = |name: &str, session: &str| {
            let path = root.join(format!("tasks/{name}.md"));
            std::fs::write(
                &path,
                format!("---\nagent_doc_session: {name}\n---\n\n# {name}\n"),
            )
            .unwrap();
            let pane = iso.new_window(session, &root).unwrap();
            let window = iso.pane_window(&pane).unwrap();
            let file = path.to_string_lossy().into_owned();
            sessions::register_full_with_cwd_in(
                &root,
                name,
                &pane,
                &file,
                pane_pid_from_tmux(&iso, &pane).unwrap(),
                &window,
                &root.to_string_lossy(),
            )
            .unwrap();
            (file, pane)
        };
        let (idea_a, idea_a_pane) = document("idea-a", &idea.session);
        let (idea_b, idea_b_pane) = document("idea-b", &idea.session);
        let (rider_a, rider_a_pane) = document("rider-a", &rider.session);
        let (rider_b, rider_b_pane) = document("rider-b", &rider.session);
        let pane_sessions = [
            (&idea_a_pane, &idea.session),
            (&idea_b_pane, &idea.session),
            (&rider_a_pane, &rider.session),
            (&rider_b_pane, &rider.session),
            (&idea_initial[0], &idea.session),
            (&rider_initial[0], &rider.session),
        ];

        let sync = |columns: Vec<String>, window: &str, focus: &str| {
            run_with_options_internal_at_root(
                &root,
                &columns,
                Some(window),
                Some(focus),
                AutoStartMode::SafePassive,
                false,
                false,
                &[],
                &MainLayoutEligibility::default(),
                &iso,
            )
            .unwrap();
        };
        let window_layout = |window: &str| {
            iso.raw_cmd(&["display-message", "-p", "-t", window, "#{window_layout}"])
                .unwrap()
                .trim()
                .to_string()
        };
        let mut previous: Option<(String, String)> = None;
        for round in 0..3 {
            sync(vec![idea_a.clone(), idea_b.clone()], &idea_window, &idea_a);
            // Rider's editor also shows IDEA's `idea-a` (a contended document).
            sync(
                vec![rider_a.clone(), idea_a.clone(), rider_b.clone()],
                &rider_window,
                &rider_a,
            );

            assert_eq!(
                iso.list_panes_ordered(&idea_window).unwrap(),
                vec![idea_a_pane.clone(), idea_b_pane.clone()],
                "round {round}: IDEA's lane shows exactly IDEA's layout"
            );
            assert_eq!(
                iso.list_panes_ordered(&rider_window).unwrap(),
                vec![rider_a_pane.clone(), rider_b_pane.clone()],
                "round {round}: Rider's lane shows Rider's layout without the contended pane"
            );
            for (pane, session) in pane_sessions {
                assert!(iso.pane_alive(pane), "round {round}: {pane} stays alive");
                assert_eq!(
                    &iso.pane_session(pane).unwrap(),
                    session,
                    "round {round}: {pane} never changes session"
                );
            }
            assert_eq!(
                iso.list_window_panes(&shared_window).unwrap(),
                shared_panes_before,
                "round {round}: the shared session is untouched"
            );
            let layouts = (window_layout(&idea_window), window_layout(&rider_window));
            if let Some(previous) = &previous {
                assert_eq!(
                    &layouts, previous,
                    "round {round}: each lane's layout is stable under alternation"
                );
            }
            previous = Some(layouts);
        }
        // The empty initial shell panes were stashed inside their own lanes.
        for (pane, session) in [
            (&idea_initial[0], &idea.session),
            (&rider_initial[0], &rider.session),
        ] {
            let window = iso.pane_window(pane).unwrap();
            assert_eq!(
                window_name_for_window_id(&iso, &window).as_deref(),
                Some("stash"),
                "{pane}: a new lane's empty pane is stashed in {session}"
            );
        }
    }

    /// `#ideriderindependentnav` P4: the explicit handoff of a contended
    /// document through the real sync body. IDEA's lane shows `a` and `b`;
    /// Rider's editor also shows `a`. Passive syncs leave `a` in IDEA's lane.
    /// An explicit claim from Rider (the controller's handoff, then Rider's
    /// sync, then IDEA's passive republish) moves `a`'s pane into Rider's lane
    /// session; afterwards alternating passive syncs never move it back, an
    /// explicit claim from IDEA hands it back, and at every step:
    /// - each document has exactly one live pane, visible in exactly one lane
    ///   window (no pane duplicated or lost);
    /// - each lane window shows exactly the documents its lane owns;
    /// - the shared session is untouched.
    ///
    /// Isolated tmux server only.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn navigation_lane_guard_explicit_handoff_moves_a_contended_pane_once() {
        use agent_doc_controller_io::project_controller::{
            NavigationLaneHandoff, NavigationLaneHandoffOutcome, hand_over_navigation_lane_pane,
            navigation_lane_session_for_client,
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"proj\"\neditor_navigation = \"per_installation\"\n",
        )
        .unwrap();
        let _cwd = ScopedCurrentDir::set(&root);
        let iso = agent_doc_tmux_io::isolated_test_tmux("lane-handoff-e2e");

        let shared_pane = iso.new_session("proj", &root).unwrap();
        let shared_window = iso.pane_window(&shared_pane).unwrap();
        iso.raw_cmd(&["rename-window", "-t", &shared_window, "agent-doc"])
            .unwrap();
        let shared_panes_before = iso.list_window_panes(&shared_window).unwrap();

        let lane_session = |client: &str| {
            navigation_lane_session_for_client(&iso, &root, client)
                .unwrap()
                .expect("an installation lane")
                .session
        };
        let idea = lane_session("jetbrains@IU.0a1b2c3d-pid:41");
        let rider = lane_session("jetbrains@RD.9f8e7d6c-pid:42");
        let agent_doc_window = |session: &str| {
            iso.raw_cmd(&[
                "list-windows",
                "-t",
                &format!("{session}:"),
                "-F",
                "#{window_id} #{window_name}",
            ])
            .unwrap()
            .lines()
            .find_map(|line| {
                let (id, name) = line.split_once(' ')?;
                (name == "agent-doc").then(|| id.to_string())
            })
            .expect("lane agent-doc window")
        };
        let idea_window = agent_doc_window(&idea);
        let rider_window = agent_doc_window(&rider);

        let document = |name: &str, session: &str| {
            let path = root.join(format!("tasks/{name}.md"));
            std::fs::write(
                &path,
                format!("---\nagent_doc_session: {name}\n---\n\n# {name}\n"),
            )
            .unwrap();
            let pane = iso.new_window(session, &root).unwrap();
            let window = iso.pane_window(&pane).unwrap();
            let file = path.to_string_lossy().into_owned();
            sessions::register_full_with_cwd_in(
                &root,
                name,
                &pane,
                &file,
                pane_pid_from_tmux(&iso, &pane).unwrap(),
                &window,
                &root.to_string_lossy(),
            )
            .unwrap();
            (file, pane)
        };
        let (doc_a, pane_a) = document("contended-a", &idea);
        let (doc_b, pane_b) = document("idea-b", &idea);
        let (doc_r, pane_r) = document("rider-r", &rider);

        let sync = |columns: Vec<String>, window: &str, focus: &str| {
            run_with_options_internal_at_root(
                &root,
                &columns,
                Some(window),
                Some(focus),
                AutoStartMode::SafePassive,
                false,
                false,
                &[],
                &MainLayoutEligibility::default(),
                &iso,
            )
            .unwrap();
        };
        let idea_layout = || vec![doc_a.clone(), doc_b.clone()];
        let rider_layout = || vec![doc_r.clone(), doc_a.clone()];
        let hand_over = |from: &str, to: &str| {
            hand_over_navigation_lane_pane(
                &iso,
                &root,
                "proj",
                to,
                &NavigationLaneHandoff {
                    document: doc_a.clone(),
                    pane: pane_a.clone(),
                    from_session: from.to_string(),
                },
            )
            .unwrap()
        };
        let check = |step: &str, idea_panes: Vec<&String>, rider_panes: Vec<&String>| {
            let idea_now = iso.list_panes_ordered(&idea_window).unwrap();
            let rider_now = iso.list_panes_ordered(&rider_window).unwrap();
            assert_eq!(
                idea_now,
                idea_panes.into_iter().cloned().collect::<Vec<_>>(),
                "{step}: IDEA's lane window"
            );
            assert_eq!(
                rider_now,
                rider_panes.into_iter().cloned().collect::<Vec<_>>(),
                "{step}: Rider's lane window"
            );
            for pane in [&pane_a, &pane_b, &pane_r] {
                assert!(iso.pane_alive(pane), "{step}: {pane} is never lost");
                let visible = [&idea_now, &rider_now]
                    .iter()
                    .filter(|window| window.contains(pane))
                    .count();
                assert!(visible <= 1, "{step}: {pane} visible in two lanes");
            }
            assert_eq!(
                iso.list_window_panes(&shared_window).unwrap(),
                shared_panes_before,
                "{step}: the shared session is untouched"
            );
        };

        // Passive syncs: `a` stays in IDEA's lane.
        for round in 0..2 {
            sync(idea_layout(), &idea_window, &doc_a);
            sync(rider_layout(), &rider_window, &doc_r);
            check(
                &format!("passive round {round}"),
                vec![&pane_a, &pane_b],
                vec![&pane_r],
            );
            assert_eq!(iso.pane_session(&pane_a).unwrap(), idea);
        }

        // Rider's explicit claim: the controller hands the pane over, Rider's
        // sync places it, and IDEA's passive republish drops it.
        assert!(matches!(
            hand_over(&idea, &rider),
            NavigationLaneHandoffOutcome::Moved { placeholder: None }
        ));
        sync(rider_layout(), &rider_window, &doc_a);
        sync(idea_layout(), &idea_window, &doc_b);
        check("after Rider's claim", vec![&pane_b], vec![&pane_r, &pane_a]);
        assert_eq!(iso.pane_session(&pane_a).unwrap(), rider);

        // Passive alternation never moves it back.
        for round in 0..2 {
            sync(idea_layout(), &idea_window, &doc_a);
            sync(rider_layout(), &rider_window, &doc_r);
            check(
                &format!("passive round {round} after Rider's claim"),
                vec![&pane_b],
                vec![&pane_r, &pane_a],
            );
            assert_eq!(iso.pane_session(&pane_a).unwrap(), rider);
        }

        // IDEA's explicit claim hands it back.
        assert!(matches!(
            hand_over(&rider, &idea),
            NavigationLaneHandoffOutcome::Moved { placeholder: None }
        ));
        sync(idea_layout(), &idea_window, &doc_a);
        sync(rider_layout(), &rider_window, &doc_r);
        check("after IDEA's claim", vec![&pane_a, &pane_b], vec![&pane_r]);
        assert_eq!(iso.pane_session(&pane_a).unwrap(), idea);
    }

    /// `#ridernavp6switchback`: `editor_navigation` switches from
    /// `per_installation` back to `shared` while an installation lane session
    /// still holds document panes, and the shared sync runs before
    /// `agent-doc gc` migrates them. Through the real sync body:
    /// - a shared sync never arranges the stale lane session, even when the
    ///   focused document's pane (or the tmux client) is still there or the
    ///   caller passes the lane's own `agent-doc` window: it arranges the
    ///   project session (`navigation_lane_stale_target_retargeted`), so no
    ///   shared pane is ever joined into the lane;
    /// - a lane document in the shared layout is adopted into the project
    ///   session: the same pane is joined across sessions, passive or manual,
    ///   never started a second time (no autostart duplicate) and never lost;
    /// - a lane document outside the shared layout stays in its lane, and
    ///   `agent-doc gc` folds it and closes the lane afterwards.
    ///
    /// Isolated tmux server only.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn navigation_lane_guard_switch_back_to_shared_adopts_lane_panes_before_gc() {
        use agent_doc_controller_io::project_controller::{
            navigation_lane_session_for_client, reconcile_navigation_lane_sessions,
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let config = root.join(".agent-doc/config.toml");
        std::fs::write(
            &config,
            "tmux_session = \"proj\"\neditor_navigation = \"per_installation\"\n",
        )
        .unwrap();
        let _cwd = ScopedCurrentDir::set(&root);
        let iso = agent_doc_tmux_io::isolated_test_tmux("lane-switch-back-e2e");

        let shared_pane = iso.new_session("proj", &root).unwrap();
        let shared_window = iso.pane_window(&shared_pane).unwrap();
        iso.raw_cmd(&["rename-window", "-t", &shared_window, "agent-doc"])
            .unwrap();
        let idea = navigation_lane_session_for_client(&iso, &root, "jetbrains@IU.0a1b2c3d-pid:41")
            .unwrap()
            .expect("an installation lane")
            .session;
        let agent_doc_window = |session: &str| {
            iso.raw_cmd(&[
                "list-windows",
                "-t",
                &format!("{session}:"),
                "-F",
                "#{window_id} #{window_name}",
            ])
            .unwrap()
            .lines()
            .find_map(|line| {
                let (id, name) = line.split_once(' ')?;
                (name == "agent-doc").then(|| id.to_string())
            })
            .expect("agent-doc window")
        };
        let idea_window = agent_doc_window(&idea);
        let document = |name: &str, session: &str| {
            let path = root.join(format!("tasks/{name}.md"));
            std::fs::write(
                &path,
                format!("---\nagent_doc_session: {name}\n---\n\n# {name}\n"),
            )
            .unwrap();
            let pane = iso.new_window(session, &root).unwrap();
            let window = iso.pane_window(&pane).unwrap();
            let file = path.to_string_lossy().into_owned();
            sessions::register_full_with_cwd_in(
                &root,
                name,
                &pane,
                &file,
                pane_pid_from_tmux(&iso, &pane).unwrap(),
                &window,
                &root.to_string_lossy(),
            )
            .unwrap();
            (file, pane)
        };
        let (doc_s, pane_s) = document("shared-s", "proj");
        let (doc_a, pane_a) = document("lane-a", &idea);
        let (doc_b, pane_b) = document("lane-b", &idea);
        // The controller passes its actor bindings; a standalone sync has
        // only the session registry.
        let bindings = std::cell::RefCell::new(Vec::new());
        let sync =
            |columns: Vec<String>, window: Option<&str>, focus: &str, mode: AutoStartMode| {
                run_with_options_internal_at_root(
                    &root,
                    &columns,
                    window,
                    Some(focus),
                    mode,
                    false,
                    false,
                    &bindings.borrow(),
                    &MainLayoutEligibility::default(),
                    &iso,
                )
                .unwrap();
            };
        let session_of = |pane: &str| iso.pane_session(pane).unwrap();
        // Every document keeps its one pane: alive, and no other pane of the
        // server runs in the document's place (no autostart duplicate).
        let all_panes = || {
            iso.raw_cmd(&["list-panes", "-a", "-F", "#{pane_id}"])
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };

        // Per-installation mode: `s` in the shared session, `a` and `b` in
        // IDEA's lane window.
        sync(
            vec![doc_s.clone()],
            Some(&shared_window),
            &doc_s,
            AutoStartMode::SafePassive,
        );
        sync(
            vec![doc_a.clone(), doc_b.clone()],
            Some(&idea_window),
            &doc_a,
            AutoStartMode::SafePassive,
        );
        assert_eq!(
            iso.list_panes_ordered(&idea_window).unwrap(),
            vec![pane_a.clone(), pane_b.clone()]
        );
        assert_eq!(session_of(&pane_s), "proj");
        let panes_before_switch = all_panes();

        // Switch back to shared (the controller reads it on its next start);
        // no gc has run, so IDEA's lane still holds `a` and `b`.
        std::fs::write(
            &config,
            "tmux_session = \"proj\"\neditor_navigation = \"shared\"\n",
        )
        .unwrap();
        // The tmux client's current session is the lane too.
        iso.raw_cmd(&["switch-client", "-t", &format!("{idea}:")])
            .ok();

        // A passive shared heartbeat with no explicit window, focused on `a`
        // whose pane is in the lane: it arranges the project session and
        // adopts `a`'s pane into it; `s` never enters the lane.
        sync(
            vec![doc_s.clone(), doc_a.clone()],
            None,
            &doc_a,
            AutoStartMode::SafePassive,
        );
        assert_eq!(
            iso.list_panes_ordered(&shared_window).unwrap(),
            vec![pane_s.clone(), pane_a.clone()],
            "the shared window adopts a's pane next to s"
        );
        assert_eq!(session_of(&pane_s), "proj", "s never moves into the lane");
        assert_eq!(session_of(&pane_a), "proj", "a is adopted, same pane id");
        assert_eq!(
            session_of(&pane_b),
            idea,
            "b is not in the layout: it stays"
        );
        assert!(iso.session_alive(&idea), "only gc closes the lane");

        // A passive heartbeat focused on `b` (still in the lane) that does
        // not show `b`: nothing crosses into or out of the lane.
        sync(
            vec![doc_s.clone(), doc_a.clone()],
            None,
            &doc_b,
            AutoStartMode::SafePassive,
        );
        assert_eq!(
            iso.list_panes_ordered(&shared_window).unwrap(),
            vec![pane_s.clone(), pane_a.clone()]
        );
        assert_eq!(session_of(&pane_b), idea);

        // A sync carrying the controller's actor bindings and naming the
        // stale lane's own `agent-doc` window (a caller that still holds it):
        // it is retargeted to the project session and adopts `b` there. The
        // retarget and the adoption run before any mode-specific phase, so a
        // manual (full) sync takes the same path; full mode is not driven
        // here because its dead-pane actor sweep reads the default tmux
        // server, not this isolated one.
        *bindings.borrow_mut() = [(&doc_s, &pane_s), (&doc_a, &pane_a), (&doc_b, &pane_b)]
            .into_iter()
            .map(|(document, pane)| {
                agent_doc_controller_io::project_controller::ControllerTmuxActorBinding {
                    document_path: document.clone(),
                    session_id: String::new(),
                    pane_id: pane.clone(),
                    generation: 1,
                }
            })
            .collect();
        sync(
            vec![doc_s.clone(), doc_b.clone()],
            Some(&idea_window),
            &doc_b,
            AutoStartMode::SafePassive,
        );
        assert_eq!(
            iso.list_panes_ordered(&shared_window).unwrap(),
            vec![pane_s.clone(), pane_b.clone()],
            "the explicit stale lane window is replaced by the project window"
        );
        for pane in [&pane_s, &pane_a, &pane_b] {
            assert!(iso.pane_alive(pane), "{pane} is never lost");
            assert_eq!(
                session_of(pane),
                "proj",
                "{pane} lives in the project session"
            );
        }
        // No document was started a second time: each registry entry still
        // names its original pane, and every pane that appeared since the
        // switch is a bare shell outside the project's layout window (the
        // project's new `stash` window and the shell a handoff leaves in the
        // lane's emptied `agent-doc` window).
        for (file, pane) in [(&doc_s, &pane_s), (&doc_a, &pane_a), (&doc_b, &pane_b)] {
            let entry = agent_doc_session_registry_io::lookup_file_entry_in(&root, Path::new(file))
                .unwrap()
                .expect("registered document");
            assert_eq!(&entry.pane, pane, "{file} keeps its one pane");
        }
        let shared_layout = iso.list_window_panes(&shared_window).unwrap();
        for pane in all_panes()
            .into_iter()
            .filter(|pane| !panes_before_switch.contains(pane))
        {
            assert!(!shared_layout.contains(&pane), "{pane} entered the layout");
            let window = iso.pane_window(&pane).unwrap();
            let name = window_name_for_window_id(&iso, &window).unwrap_or_default();
            let session = session_of(&pane);
            assert!(
                (session == "proj" && name == "stash") || (session == idea && name == "agent-doc"),
                "{pane} is a bare shell in {session}:{name}"
            );
        }

        // gc: the lane holds no document pane any more, so it closes.
        let report = reconcile_navigation_lane_sessions(&iso, &root, &Default::default(), false);
        assert!(report.folded.is_empty(), "{report:?}");
        assert_eq!(report.closed, vec![idea.clone()], "{report:?}");
        assert!(!iso.session_alive(&idea));
        for pane in [&pane_s, &pane_a, &pane_b] {
            assert!(iso.pane_alive(pane), "{pane} survives the gc");
        }
    }

    #[test]
    fn multi_session_sync_drops_columns_bound_to_other_sessions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let write = |name: &str, binding: Option<&str>| {
            let path = tmp.path().join(name);
            let binding = binding
                .map(|session| format!("tmux_session: {session}\n"))
                .unwrap_or_default();
            std::fs::write(
                &path,
                format!("---\nagent_doc_session: {name}\n{binding}---\n"),
            )
            .unwrap();
            path.to_string_lossy().into_owned()
        };
        let main_doc = write("ops.md", Some("main"));
        let research_doc = write("notes.md", Some("research"));
        let unbound = write("plan.md", None);
        let columns = vec![main_doc.clone(), format!("{research_doc},{unbound}")];

        let multi = agent_doc_sync::TmuxSessionPolicy::new(["main", "research"]);
        let (kept, dropped) =
            drop_columns_bound_to_other_sessions(&multi, columns.clone(), "research");
        assert_eq!(kept, vec![format!("{research_doc},{unbound}")]);
        assert_eq!(dropped, vec![(main_doc.clone(), "main".to_string())]);
        assert_eq!(
            scoped_document_tmux_session_binding(&multi, &columns, Some(&research_doc)),
            Some("research".to_string())
        );

        let single = agent_doc_sync::TmuxSessionPolicy::single_session();
        let (kept, dropped) =
            drop_columns_bound_to_other_sessions(&single, columns.clone(), "research");
        assert_eq!(kept, columns, "single-session sync keeps every column");
        assert!(dropped.is_empty());
        assert_eq!(
            scoped_document_tmux_session_binding(&single, &columns, Some(&research_doc)),
            None
        );
    }

    #[test]
    fn focused_actor_session_uses_joined_binding_instead_of_ambient_context() {
        let bindings = vec![
            agent_doc_controller_io::project_controller::ControllerTmuxActorBinding {
                document_path: "/workspace/tasks/focused.md".to_string(),
                session_id: "document-session".to_string(),
                pane_id: "%23".to_string(),
                generation: 13,
            },
        ];

        let session = focused_actor_session_from_bindings(
            Some("/workspace/tasks/focused.md"),
            &bindings,
            |pane_id| {
                (pane_id == "%23").then(|| ActorPanePlacement {
                    session: "terminal-owner".to_string(),
                    in_agent_doc_window: true,
                })
            },
        );

        assert_eq!(session.as_deref(), Some("terminal-owner"));
    }

    #[test]
    fn stashed_focus_uses_visible_sibling_actor_session() {
        let bindings = vec![
            agent_doc_controller_io::project_controller::ControllerTmuxActorBinding {
                document_path: "/workspace/tasks/agent-doc-bugs.md".to_string(),
                session_id: "root-session".to_string(),
                pane_id: "%0".to_string(),
                generation: 1,
            },
            agent_doc_controller_io::project_controller::ControllerTmuxActorBinding {
                document_path: "/workspace/tasks/focused.md".to_string(),
                session_id: "document-session".to_string(),
                pane_id: "%23".to_string(),
                generation: 13,
            },
        ];

        let session = focused_actor_session_from_bindings(
            Some("/workspace/tasks/focused.md"),
            &bindings,
            |pane_id| match pane_id {
                "%0" => Some(ActorPanePlacement {
                    session: "terminal-owner".to_string(),
                    in_agent_doc_window: true,
                }),
                "%23" => Some(ActorPanePlacement {
                    session: "replacement-controller".to_string(),
                    in_agent_doc_window: false,
                }),
                _ => None,
            },
        );

        assert_eq!(session.as_deref(), Some("terminal-owner"));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn windowless_sync_preserves_focused_actor_session_across_controller_recycle_context() {
        let tmp = tempfile::TempDir::new().unwrap();
        let focus = tmp.path().join("focused.md");
        let focus = focus.to_string_lossy().to_string();
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-windowless-focused-actor-session");
        let actor_pane = iso.new_session("terminal-owner", tmp.path()).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "terminal-owner:0", "agent-doc"])
            .unwrap();
        let _controller_pane = iso
            .new_session("replacement-controller", tmp.path())
            .unwrap();
        iso.raw_cmd(&[
            "rename-window",
            "-t",
            "replacement-controller:0",
            "agent-doc",
        ])
        .unwrap();
        let bindings = vec![
            agent_doc_controller_io::project_controller::ControllerTmuxActorBinding {
                document_path: focus.clone(),
                session_id: "document-session".to_string(),
                pane_id: actor_pane,
                generation: 13,
            },
        ];

        assert_eq!(
            current_tmux_session_name(&iso).as_deref(),
            Some("replacement-controller"),
            "the recycled controller context should differ from the actor owner"
        );
        assert_eq!(
            resolve_sync_target_session(
                &iso,
                None,
                std::slice::from_ref(&focus),
                Some(&focus),
                &bindings,
            )
            .unwrap()
            .as_deref(),
            Some("terminal-owner"),
            "the focused live actor must outrank the recycled controller's ambient tmux session"
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-windowless-dead-project-pin");
        let _pane1 = iso.new_session("1", tmp.path()).unwrap();

        assert_eq!(
            current_tmux_session_name(&iso).as_deref(),
            Some("1"),
            "the live attached session should still be discoverable"
        );
        assert_eq!(
            resolve_sync_target_session(&iso, None, &[], None, &[])
                .unwrap()
                .as_deref(),
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-windowless-mixed-root-pin");
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
                &[],
            )
            .unwrap()
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-cross-root-register");
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-cross-root-duplicate-register");
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-fail-closed-geometry-binding");
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
        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-open-cycle-protect");
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

        respawn_pane_as_document_owner(&iso, &pane_a, root, &doc_a);
        respawn_pane_as_document_owner(&iso, &pane_b, root, &doc_b);

        sessions::register_full_with_cwd(
            "sync-open-cycle-a",
            &pane_a,
            &doc_a.to_string_lossy(),
            pane_pid_from_tmux(&iso, &pane_a).unwrap(),
            &window,
            &root.to_string_lossy(),
        )
        .unwrap();
        agent_doc_session_actor_io::project_binding_in(
            root,
            &doc_a.to_string_lossy(),
            "sync-open-cycle-a",
            &pane_a,
            &window,
            "sync",
            "open_cycle_detach_fixture",
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
        agent_doc_session_actor_io::project_binding_in(
            root,
            &doc_b.to_string_lossy(),
            "sync-open-cycle-b",
            &pane_b,
            &window,
            "sync",
            "open_cycle_detach_fixture",
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-safe-passive-no-alias");
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-safe-passive-registered-fast-path");
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-safe-passive-protected-grow");
        let pane_a = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let pane_b = iso.split_window(&pane_a, root, "-dh").unwrap();
        let target_window = iso.pane_window(&pane_a).unwrap();
        let pane_c = iso.new_window("test", root).unwrap();
        let pane_c_window = iso.pane_window(&pane_c).unwrap();

        for (pane, doc) in [(&pane_a, &doc_a), (&pane_b, &doc_b), (&pane_c, &doc_c)] {
            respawn_pane_as_document_owner(&iso, pane, root, doc);
        }

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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-manual-protected-grow");
        let pane_a = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let pane_b = iso.split_window(&pane_a, root, "-dh").unwrap();
        let target_window = iso.pane_window(&pane_a).unwrap();
        let pane_c = iso.new_window("test", root).unwrap();
        let pane_c_window = iso.pane_window(&pane_c).unwrap();

        for (pane, doc) in [(&pane_a, &doc_a), (&pane_b, &doc_b), (&pane_c, &doc_c)] {
            respawn_pane_as_document_owner(&iso, pane, root, doc);
        }

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
        for (session, pane, window, doc) in [
            ("amanual-a", &pane_a, &target_window, &doc_a),
            ("bmanual-b", &pane_b, &target_window, &doc_b),
            ("cmanual-c", &pane_c, &pane_c_window, &doc_c),
        ] {
            agent_doc_session_actor_io::project_binding_in(
                root,
                &doc.to_string_lossy(),
                session,
                pane,
                window,
                "sync",
                "manual_protected_grow_fixture",
            )
            .unwrap();
        }

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
            false,
            &iso,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&target_window).unwrap();
        assert!(
            !ordered.contains(&pane_a),
            "open-cycle extra pane should be stashed while other documents sync"
        );
        let pane_a_window = iso.pane_window(&pane_a).unwrap();
        assert!(
            window_name_for_window_id(&iso, &pane_a_window)
                .as_deref()
                .is_some_and(is_stash_window_name),
            "post-sync doctor should classify the displaced open-cycle pane as stash"
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
    fn safe_passive_blocked_layout_preserve_keeps_operator_focus() {
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-safe-passive-blocked-focus");
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
            bugs_pane,
            "blocked passive sync must not move operator focus"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_editor_switch_preserves_sibling_and_operator_focus() {
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-focus-only-editor-switch");
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
            left_pane,
            "safe-passive same-side handoff must not steal operator focus"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_existing_sibling_focus_does_not_replace_or_select_it() {
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-focus-only-visible-sibling");
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
            bugs_pane,
            "safe-passive sync must preserve operator focus instead of selecting the editor-focused sibling"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_editor_switch_without_saved_layout_preserves_operator_focus() {
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-focus-only-no-saved-layout");
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
            left_pane,
            "safe-passive replacement must preserve operator focus without collapsing the sibling pane"
        );
    }
    /// A window id no live tmux window in these fixtures can hold.
    const STALE_WINDOW: &str = "@9999";

    /// `#panewindowbindingrebind` — the 2026-08-13 defect: a document bound with
    /// a window id that is not a live window (`@0`) drifts against EVERY live
    /// window, so the focus effect refuses forever and editor->tmux sync is
    /// permanently dead for that document. Nothing rewrote the binding, because
    /// `project_binding_in` runs once at bind time and never again.
    #[test]
    #[ignore = "live tmux server; runs in the tmux CI leg (`make tmux-ci`), not the default development suite"]
    fn drifted_pane_window_binding_is_rebound_from_the_live_window() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let doc = root.join("tasks/bound.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: bound-session\nagent_doc_format: template\n---\n",
        )
        .unwrap();
        let _cwd = ScopedCurrentDir::set(root);

        let iso = agent_doc_tmux_io::isolated_test_tmux("pane-window-binding-rebind");
        let pane = iso.new_session("test", root).unwrap();
        let live_window = iso.pane_window(&pane).unwrap();
        let file_key = doc.to_string_lossy().to_string();

        // Bind with a window that is not the pane's live window — the observed
        // shape, where the recorded value never named a real window at all.
        agent_doc_session_actor_io::project_binding_in(
            root,
            &file_key,
            "bound-session",
            &pane,
            // NOT `@0`: a fresh tmux server really does hand out `@0`, which is
            // why the production defect could not be fixed by treating `@0` as a
            // sentinel — it is a legitimate window id, just not this pane's.
            STALE_WINDOW,
            "test",
            "seed_stale_binding",
        )
        .unwrap();
        sessions::register_full_with_cwd_in(
            root,
            "bound-session",
            &pane,
            &file_key,
            pane_pid_from_tmux(&iso, &pane).unwrap(),
            STALE_WINDOW,
            &root.to_string_lossy(),
        )
        .unwrap();
        assert_ne!(
            live_window, STALE_WINDOW,
            "fixture must actually be drifted"
        );

        let notes = refresh_synced_pane_window_bindings(&iso, &[(doc.clone(), pane.clone())])
            .expect("post-sync receipt must repair a drifted binding");
        let note = notes.first().expect("a drifted binding must be repaired");
        assert!(
            note.contains(STALE_WINDOW),
            "the note names the stale window: {note}"
        );
        assert!(note.contains(&live_window), "and the live one: {note}");

        let record = agent_doc_session_actor_io::load_record_in(root, &file_key)
            .unwrap()
            .expect("record survives the rebind");
        assert_eq!(record.window_id, live_window);
        assert_eq!(record.pane_id, pane, "the pane binding itself is unchanged");
        assert_eq!(
            registry_window_for_pane(root, &pane).unwrap().as_deref(),
            Some(live_window.as_str()),
            "the durable registry follows the same physical observation"
        );

        // Idempotent: a converged binding is not a repair, so a second pass
        // reports nothing rather than rewriting on every doctor run.
        assert!(
            refresh_drifted_pane_window_binding(&iso, &doc)
                .unwrap()
                .is_none()
        );
    }

    /// A document bound on `%5` with a stale window, plus its key.
    fn window_drift_fixture(root: &Path) -> (PathBuf, String) {
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let doc = root.join("tasks/bound.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: bound-session\nagent_doc_format: template\n---\n",
        )
        .unwrap();
        let file_key = doc.to_string_lossy().to_string();
        agent_doc_session_actor_io::project_binding_in(
            root,
            &file_key,
            "bound-session",
            "%5",
            STALE_WINDOW,
            "test",
            "seed_stale_binding",
        )
        .unwrap();
        (doc, file_key)
    }

    fn owner_transition(
        root: &Path,
        file_key: &str,
        state: agent_doc_controller::actor::ActorState,
    ) {
        agent_doc_session_actor_io::transition_state_in(
            root,
            file_key,
            "bound-session",
            "%5",
            Some(1),
            state,
            "supervisor",
            "turn_start",
        )
        .unwrap();
    }

    fn refresh_window(doc: &Path, hook: &mut dyn FnMut()) -> Option<String> {
        refresh_drifted_pane_window_binding_with_hook(
            doc,
            "sync",
            "post_layout_live_window_rebind",
            |_| Some("@5".to_string()),
            hook,
        )
        .unwrap()
    }

    /// `#syncbindingfence`: a window correction is a fact about where the pane
    /// is, not a re-election. Routing it through `project_binding_in` stamped
    /// the running owner's `Busy` record `Ready`.
    #[test]
    fn window_binding_refresh_keeps_the_owners_state_and_generation() {
        use agent_doc_controller::actor::ActorState;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        let (doc, file_key) = window_drift_fixture(root);
        owner_transition(root, &file_key, ActorState::Busy);

        let note = refresh_window(&doc, &mut || {}).expect("a drifted window is corrected");
        assert!(note.contains(STALE_WINDOW), "{note}");

        let record = agent_doc_session_actor_io::load_record_in(root, &file_key)
            .unwrap()
            .unwrap();
        assert_eq!(record.window_id, "@5");
        assert_eq!(record.pane_id, "%5");
        assert_eq!(record.generation, 1);
        assert_eq!(
            record.state,
            ActorState::Busy,
            "the correction must not re-project the owner as Ready"
        );
    }

    /// The owner moves the document to another pane between the refresh's
    /// read and its write. The old write re-read the generation and bound the
    /// observed pane back over the new owner at generation + 1.
    #[test]
    fn window_binding_refresh_is_fenced_against_an_owner_rebind_after_its_read() {
        use agent_doc_controller::actor::ActorState;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        let (doc, file_key) = window_drift_fixture(root);
        let mut moved = false;
        let mut owner_moves = || {
            if !moved {
                moved = true;
                agent_doc_session_actor_io::record_session_start_direct(
                    &doc,
                    "bound-session",
                    "%9",
                    "@7",
                    2,
                )
                .unwrap();
            }
        };

        assert_eq!(refresh_window(&doc, &mut owner_moves), None);

        let record = agent_doc_session_actor_io::load_record_in(root, &file_key)
            .unwrap()
            .unwrap();
        assert_eq!(record.pane_id, "%9");
        assert_eq!(record.generation, 2);
        assert_eq!(record.state, ActorState::Starting);
        assert_eq!(record.window_id, "@7");
    }

    /// A same-generation owner transition after the read: only the revision
    /// fence sees it. A generation-only fence lands the correction with the
    /// observed `Ready` state over the owner's `Busy`.
    #[test]
    fn window_binding_refresh_is_fenced_against_a_same_generation_owner_transition() {
        use agent_doc_controller::actor::ActorState;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        let (doc, file_key) = window_drift_fixture(root);
        let mut transitioned = false;
        let mut owner_goes_busy = || {
            if !transitioned {
                transitioned = true;
                owner_transition(root, &file_key, ActorState::Busy);
            }
        };

        // The registry half still records where the pane physically is.
        let _ = refresh_window(&doc, &mut owner_goes_busy);

        let record = agent_doc_session_actor_io::load_record_in(root, &file_key)
            .unwrap()
            .unwrap();
        assert_eq!(record.state, ActorState::Busy);
        assert_eq!(record.last_transition.caller, "supervisor");
        assert_eq!(
            record.window_id, STALE_WINDOW,
            "the next pass re-observes; this one wrote nothing"
        );
    }

    /// The conservative half, mirroring `pane_window_binding_drifted`: a pane
    /// tmux cannot locate is never "misplaced", so a transient read failure must
    /// not rewrite a binding.
    #[test]
    #[ignore = "live tmux server; runs in the tmux CI leg (`make tmux-ci`), not the default development suite"]
    fn an_unlocatable_pane_is_never_rebound() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let doc = root.join("tasks/gone.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: gone-session\nagent_doc_format: template\n---\n",
        )
        .unwrap();
        let _cwd = ScopedCurrentDir::set(root);

        let iso = agent_doc_tmux_io::isolated_test_tmux("pane-window-binding-unlocatable");
        let file_key = doc.to_string_lossy().to_string();
        agent_doc_session_actor_io::project_binding_in(
            root,
            &file_key,
            "gone-session",
            "%99999",
            STALE_WINDOW,
            "test",
            "seed_stale_binding",
        )
        .unwrap();

        assert!(
            refresh_drifted_pane_window_binding(&iso, &doc)
                .unwrap()
                .is_none()
        );
        let record = agent_doc_session_actor_io::load_record_in(root, &file_key)
            .unwrap()
            .unwrap();
        assert_eq!(
            record.window_id, STALE_WINDOW,
            "an unknown live window is not drift"
        );
    }

    #[test]
    #[ignore = "covered by sync_sim_tmuxbudget_seed_3004; safe-passive attach/focus tmux smoke keeps the real pane/window path covered"]
    fn safe_passive_protected_open_cycle_sync_preserves_operator_focus() {
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

        let iso = agent_doc_tmux_io::isolated_test_tmux("sync-safe-passive-protected-focus");
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
            pane_a,
            "safe-passive sync must preserve operator focus even when its pane moves to stash"
        );
        assert_eq!(
            iso.pane_window(&pane_c).unwrap(),
            target_window,
            "requested hidden pane should move into the visible agent-doc window"
        );
    }

    #[test]
    fn main_layout_filter_removes_bound_documents_before_sync_resolution() {
        let root = PathBuf::from("/project");
        let eligibility = MainLayoutEligibility::new(["/project/tasks/detached.md".to_string()]);
        let (columns, dropped) = filter_main_layout_columns(
            &root,
            &eligibility,
            vec![
                "tasks/main.md,tasks/detached.md".to_string(),
                "tasks/detached.md".to_string(),
            ],
        );

        assert_eq!(columns, vec!["tasks/main.md"]);
        assert_eq!(dropped, vec!["tasks/detached.md", "tasks/detached.md"]);
    }
}
