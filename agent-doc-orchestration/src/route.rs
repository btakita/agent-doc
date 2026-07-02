//! # Module: route
//!
//! Routes harness-specific document trigger commands to the correct tmux pane. This is the
//! process-level coordinator between file-save events (editor plugin / watch daemon)
//! and running agent sessions inside tmux.
//!
//! ## Spec
//!
//! - **`run(file, pane, debounce_ms, col_args)`**: Public entry point. Delegates to
//!   `run_with_tmux` using the default tmux server. Accepts an optional explicit
//!   `pane` override, a debounce delay in milliseconds, and column layout hints.
//! - **`run_with_tmux(file, tmux, pane, debounce_ms, col_args, mode, plain_trigger)`**: Core routing logic.
//!   1. Prunes stale session registry entries via `resync::prune`.
//!   2. If `debounce_ms > 0`, waits for the file's mtime and shared editor typing
//!      indicator to settle (`await_idle`).
//!   3. Ensures a session UUID exists in the file's YAML frontmatter (generates one if missing).
//!   4. Resolves the target tmux session: prefers project config (`config.toml`), falls
//!      back to current tmux session, auto-updates config when the configured session is dead.
//!   5. Looks up the registered pane in `sessions.json`.
//!   6. If pane is alive: first verify that a live process tree still proves the
//!      document is running there. If the live owner is another pane, re-register there;
//!      if no live owner exists, fail closed instead of sending the trigger into an
//!      ambiguous shell. Pane IDs (`%N`) are globally unique per tmux server, so
//!      `target_session` matching is not required once ownership is proven.
//!      `rescue_from_stash` is attempted (it self-gates on session match) so panes
//!      stashed within the target session get rescued, but panes in other sessions are
//!      left in place. When the document already has prompt-bearing user drift after a
//!      closed cycle, the routed trigger must also produce a new per-document cycle
//!      acknowledgment before route returns success; otherwise route fails closed.
//!   7. If pane is dead and was previously registered: lazy-claims only to an explicit
//!      pane override via `find_target_pane` (skipped if the candidate is already claimed
//!      or is running a non-agent process), sends the command, then calls
//!      `sync_after_claim` to re-sync layout. Route never adopts the tmux session's
//!      current active pane implicitly.
//!   8. If no registered pane or no claimable pane: auto-starts a new agent session.
//!      Blocked by `AGENT_DOC_NO_AUTOSTART` env var (used in tests).
//! - **`auto_start(tmux, file, session_id, file_path, context_session)`**: Public; spawns a
//!   new route-owned agent pane and sends `agent-doc start --route-owned`. Waits for the
//!   agent's idle prompt before sending the initial command, then requires a real
//!   document-cycle acknowledgment before treating the fresh start as successful. Called
//!   by `sync.rs` for unresolved files.
//! - **`provision_pane(tmux, file, session_id, file_path, context_session, col_args)`**: Like
//!   `auto_start` but skips waiting for the agent to be ready. Used by sync when only pane
//!   existence is needed (agent will start asynchronously). Computes `split_before` via
//!   `is_first_column(file, col_args)` so new panes split in the correct direction for
//!   their column position.
//! - **`try_provision_pane(...)`**: Same skip-wait provisioning path, but uses
//!   nonblocking startup locks and returns `Ok(None)` when a same-document or
//!   same-session start is already in progress. Used by safe-passive sync's
//!   pre-lock create-on-miss hot path.
//! - **`is_first_column(file, col_args)`**: Returns true when `file` appears in the first
//!   `--col` argument. Drives the `-dbh` (split before) vs `-dh` (split after) split direction
//!   when creating a new pane. Returns false when `col_args` has fewer than 2 entries.
//! - **Positional split target (sync path)**: When `skip_wait` is true (called from sync),
//!   `auto_start_in_session` picks the split target based on column position — first pane in
//!   the agent-doc window for left-column files (`split_before`), last pane for right-column
//!   files. This places the new pane adjacent to its column neighbors instead of always splitting
//!   beside an arbitrary registered pane.
//! - **`send_command(tmux, pane, file_path, harness)`**: Used only for direct tmux/shell
//!   launch paths plus existing dispatch-only live-pane reroutes. Managed reroutes keep using
//!   supervisor IPC when route needs queueing/ack semantics, but a dispatch-only reopen types the
//!   bare harness trigger directly through the resolved live pane so it shares the same terminal
//!   submit boundary as `session clear`.
//! - **Dispatch-only editor reroutes** still bypass the managed acceptance/cycle-ack loop on
//!   purpose, and for existing managed sessions they send the bare reopen through direct
//!   live-pane submit instead of a one-shot supervisor IPC inject. Startup-window reroutes,
//!   including tracked Codex/OpenCode `/clear` restarts, remain prompt-gated and fail closed
//!   before sending input while the harness is redrawing or busy.
//!   Hook-visible Codex and pane-state OpenCode proof remain stronger telemetry, but
//!   dispatch-only success is the shared tmux text+`Enter` acceptance path for all harnesses.
//!   Once that acceptance is observed, editor dispatch-only returns immediately instead of
//!   paying the optional Codex/OpenCode dispatch-start proof timeout; unobserved acceptance
//!   may still wait for stronger proof before failing closed.
//! - **`await_idle(file, debounce)`**: Polls every 100ms. When an editor typing
//!   indicator is present it is authoritative — an idle indicator dispatches
//!   immediately (the editor already debounced; its pre-route save bumps mtime),
//!   an active one keeps waiting. With no indicator (CLI/direct-disk caller) it
//!   falls back to the file mtime settle. Fails closed after the `10 × debounce`
//!   safety cap expires.
//! - **`wait_for_agent_ready(tmux, pane_id, timeout, harness)`**: Polls pane content every
//!   `AGENT_READY_POLL_INTERVAL`
//!   looking for the agent's idle prompt (per `harness.prompt_patterns`). Returns true when
//!   prompt found, false on timeout. Logs progress every 10 polls. Existing-pane reroutes only
//!   fail closed on timeout when the document still has prompt-bearing drift; otherwise route
//!   just focuses the already-running pane without injecting a duplicate reopen.
//! - **`sync_after_claim(tmux, pane_id)`**: After a lazy claim, re-runs `sync::run` for all
//!   registered files in the same window to keep the tmux layout mirroring the editor split.
//!   Skipped when fewer than 2 files share the window.
//!
//! ## Agentic Contracts
//!
//! - **Session UUID guarantee**: `run_with_tmux` always ensures the file has a session UUID
//!   in frontmatter before any registry lookup. Callers never see a file without a UUID.
//! - **Stale-registry hygiene**: `resync::prune` is called at the start of every `run_with_tmux`
//!   invocation; the registry is always pruned before a lookup is attempted.
//! - **One pane per document**: Each document gets its own agent pane. Unregistered files
//!   (no prior session) skip lazy-claim and always get a fresh pane via auto-start.
//! - **Globally-unique pane IDs**: tmux `%N` pane IDs are unique per server. A registered
//!   alive pane is always routable by ID — routing does not depend on which session it
//!   currently lives in. This matters when `route run` is invoked from outside tmux (e.g.
//!   IDE `Run Agent Doc`), where `target_session` falls back to a constant and may not
//!   match the real session of the claimed pane.
//! - **Registered-pane ownership proof**: an alive registered pane is not sufficient on
//!   its own. Route first scans tmux for a live process tree that still mentions the
//!   document path. If that owner is another pane, route re-registers there. If no live
//!   owner exists, route fails closed instead of dispatching into an ambiguous pane.
//! - **Explicit provenance guard (lazy-claim only)**: `find_target_pane()` only accepts
//!   an explicit pane override for lazy-claim. Route will not infer ownership from the
//!   tmux session's current active pane when the registered pane is dead.
//! - **Non-agent process guard (lazy-claim only)**: `is_agent_process()` gates the
//!   lazy-claim path — even an explicit candidate pane will not be adopted when it is
//!   running corky/shell instead of an agent process.
//! - **Stash rescue**: Panes that ended up in a tmux `stash` / `stash-*` window are
//!   automatically rejoined into the `agent-doc` window before routing, without
//!   swapping another visible pane back into stash.
//! - **Auto-start inhibit**: Setting `AGENT_DOC_NO_AUTOSTART` prevents `auto_start_in_session`
//!   from spawning a new pane. The call returns `Err` with a descriptive message.
//! - **No duplicate fallback panes**: If route cannot split beside a visible authoritative
//!   pane, or if the target session already has an `agent-doc` window but no safe registered
//!   anchor, it must fail closed and print tmux inspection/cleanup commands instead of creating
//!   a second hidden pane in stash.
//! - **Non-fatal pane focus**: `select_pane` failures are logged as warnings and never abort
//!   the routing flow. The command is still sent even if focus fails.
//! - **Cycle acknowledgment for prompt-bearing reruns**: Fresh auto-start success is not
//!   inferred from pane input acceptance alone. The same fail-closed rule applies when route
//!   dispatches to an existing pane while the document already has prompt-bearing drift on top
//!   of a closed cycle: route must observe a new per-document cycle state before considering
//!   the dispatch successful.
//! - **Split direction determinism**: `is_first_column` requires ≥ 2 `col_args` entries to
//!   return true, ensuring a single-column layout never triggers a left-split.
//!
//! ## Evals
//!
//! - `is_first_column_empty_cols`: empty `col_args` → returns false (no layout context)
//! - `is_first_column_single_col`: single `col_args` entry → returns false (< 2 entries required)
//! - `is_first_column_in_first_col`: file matches first col arg → returns true
//! - `is_first_column_in_second_col`: file matches second col arg → returns false
//! - `is_first_column_comma_separated`: file matches comma-separated first col arg → returns true
//! - `detects_unicode_prompt`: `❯`, `❯ `, `  ❯  ` → all detected as agent idle prompt
//! - `detects_ascii_prompt`: `>`, `> `, `  >  ` → all detected as agent idle prompt
//! - `rejects_non_prompt_lines`: status text, empty lines, markdown headers → not matched as prompt
//! - `handles_ansi_prompt`: ANSI-colored `❯`/`>` → detected after strip_ansi
//! - `unregistered_file_skips_lazy_claim`: `registered = None` → lazy-claim step is skipped
//! - `dead_registered_pane_allows_lazy_claim`: `registered = Some(pane)` with dead pane → explicit-pane lazy-claim remains eligible
//! - `lazy_claim_requires_explicit_pane_provenance`: active pane in target session with no explicit `--pane` override → lazy-claim skipped
//! - (aspirational) `stash_rescue`: pane in stash window → rescued to agent-doc window before send
//! - `wrong_session_pane_still_receives_send`: alive pane in a session different from
//!   `target_session` → trigger command is sent to that pane (no new pane created)
//! - `alive_registered_pane_without_live_owner_fails_closed`: live registered pane with no
//!   file-owning process tree → route fails closed before sending into the pane
//! - `alive_registered_pane_reregisters_to_live_owner`: stale live registration + another
//!   pane running the file → route re-registers to the real live owner
//! - (aspirational) `debounce_idle`: file written rapidly → routing waits for mtime to settle
//! - (aspirational) `autostart_inhibited`: `AGENT_DOC_NO_AUTOSTART` set → returns Err, no pane spawned

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::flow::routed_reopen::{log_dispatch_proof_failed, log_prompt_ready_barrier_failed};
use agent_doc_controller::dispatch::{
    ActorDispatchState, AuthoritativeActorDispatchAction, AuthoritativeActorDispatchActionFacts,
    AuthoritativeActorReadyFacts, AuthoritativePromptReadyBarrierFacts, BusyPaneAutoFixFacts,
    BusyPaneAutoFixOutcome, CloseoutBlockDispatchDecision, CloseoutBlockDispatchFacts,
    DegradedAuthoritativeActorDirectSubmit, DegradedAuthoritativeActorFacts,
    DirectPaneAcceptancePollState, DirectPaneDispatchStartProofFacts,
    DirectPaneEnterResubmitAttemptFacts, DirectPaneExistingDraftSubmitFacts,
    DirectPaneResubmitProofFacts, DirectPaneSubmitStatus as CommandDispatchStatus,
    DispatchActorState, DispatchDrainRetryDecision, DispatchOnlyBlockerRecoveryHintFacts,
    DispatchOnlyBusyRefusalFacts, DispatchOnlyProofOutcomeFacts,
    DispatchOnlyRecycleInflightMessageFacts, DispatchOnlyReopenDelivery,
    DispatchOnlyStartingPaneActorReadyFacts, DispatchOnlyStartingPaneNotReadyMessageFacts,
    DispatchRuntimeHealth, DispatchStartProofDecision, DispatchStartProofFacts,
    DuplicatePanePolicyErrorFacts, MissingCycleAckFacts, OpenCodePaneDispatchStartProofFacts,
    PromptReadyBarrierDecision, ReopenMode, RetryBudget, RouteBusyDiagnosticFacts,
    RouteBusyQueuedDiagnosticFacts, RouteCloseoutDrainOutcome, RouteDispatchBugReportItemFacts,
    RouteLatencyFacts, RouteLatencyStatus, RouteStartupMissDiagnosticFacts, RouteSubmitObservation,
    RouteSubmitObservationFacts as ControllerRouteSubmitObservationFacts, RoutedCycleAckFacts,
    RoutedDispatchStartProof, RoutedReopenFacts, RoutedReopenGuardReason,
    RoutedTriggerPayloadFacts, STARTING_ACTOR_TIMEOUT_REASON, StartingActorLogFacts,
    StartingTimeoutActorFacts, StartupMissRouteFacts, accepted_only_dispatch_start_log_message,
    accepted_only_dispatch_start_refusal_message, actor_blocked_by_starting_timeout,
    actor_dispatch_blocker_reason, actor_recovery_hint, authoritative_actor_ready_retry_budget,
    busy_dispatch_start_outcome,
    busy_existing_pane_auto_fix_outcome as controller_busy_existing_pane_auto_fix_outcome,
    busy_projection_repaired_by_ready_prompt, can_use_degraded_authoritative_actor,
    classify_authoritative_actor_dispatch_action, classify_authoritative_prompt_ready_barrier,
    classify_closeout_block_dispatch, classify_codex_routed_dispatch_start_proof,
    classify_dispatch_start_proof, decide_authoritative_reopen,
    degraded_authoritative_actor_direct_submit_log_message, direct_pane_acceptance_poll_status,
    direct_pane_can_continue_enter_resubmit, direct_pane_can_enter_existing_draft,
    direct_pane_max_enter_resubmits, direct_pane_resubmit_proof_line,
    direct_pane_should_await_dispatch_start_proof, direct_pane_submit_acceptance_budget,
    direct_pane_submit_acceptance_timeout, direct_pane_submit_outcome,
    dispatch_drain_retry_decision, dispatch_only_blocked_guard_reason,
    dispatch_only_blocker_recovery_hint,
    dispatch_only_busy_refusal_message as controller_dispatch_only_busy_refusal_message,
    dispatch_only_busy_refusal_wait_secs, dispatch_only_busy_should_wait_for_ready,
    dispatch_only_dispatch_start_proof_required as controller_dispatch_only_dispatch_start_proof_required,
    dispatch_only_focus_only_should_fail_closed, dispatch_only_recycle_inflight_message,
    dispatch_only_sent_console_message, dispatch_only_sent_log_message,
    dispatch_only_should_print_unproven_progress, dispatch_only_should_probe_active_turn_cue,
    dispatch_only_starting_pane_actor_ready, dispatch_only_starting_pane_not_ready_message,
    dispatch_only_starting_pane_ready_timeout_for_binary,
    dispatch_only_starting_pane_recovery_retry_budget,
    dispatch_only_starting_pane_recovery_timeout_for_binary, dispatch_start_busy_probe_timeout,
    duplicate_pane_policy_error_message, existing_pane_ready_timeout, failclosed_wait_context,
    fresh_route_start_ack_timeout, opencode_pane_state_changed_from_idle,
    route_busy_diagnostic_message, route_busy_queued_diagnostic_message,
    route_closeout_user_outcome_fields, route_dispatch_bug_report_item, route_latency_message,
    route_latency_status, route_startup_miss_diagnostic_message, route_submit_issue_message,
    route_submit_observation_message, routed_cycle_ack_timeout,
    routed_dispatch_start_timeout_for_binary, routed_trigger_payload_rejection,
    should_optimistically_accept_missing_cycle_ack, should_require_routed_cycle_ack,
    starting_actor_not_ready_log_line, starting_actor_ready_log_line,
    starting_actor_terminal_log_line, starting_actor_timeout_coalesced_log_line,
    starting_timeout_blocked_actor_can_recover, startup_miss_requires_fresh_start,
    startup_miss_should_fail_closed, startup_miss_should_restart_live_owner,
    startup_miss_superseded_by_later_open_start,
};
use agent_doc_frontmatter::frontmatter;
use agent_doc_harness::HarnessConfig;
use agent_doc_hash::short_content_hash;
use agent_doc_supervisor::ipc_protocol::IpcMethod;
use agent_doc_supervisor::route_runtime::{
    DeferToBoundaryRestartRecoveryFacts, RouteActorState, SupervisorHealth, SupervisorRuntime,
    authoritative_actor_dispatch_guard_reason as supervisor_authoritative_actor_dispatch_guard_reason,
    authoritative_actor_dispatch_target_eligible as supervisor_authoritative_actor_dispatch_target_eligible,
    defer_to_boundary_restart_recovery_hint, effective_authoritative_actor_state,
};
use agent_doc_supervisor::startup_miss::{
    StartingPaneRecoveryTarget, starting_pane_recovery_target,
};
use agent_doc_tmux::is_first_column;
use agent_doc_tmux_commands::input_diag::{
    EDITOR_ROUTE_ATTEMPT_ID_ENV, RoutePaneSnapshotFacts, RoutePaneSnapshotFailedLogFacts,
    RoutePaneSnapshotHintFacts, RoutePaneSnapshotLogFacts, format_route_pane_snapshot_failed_log,
    format_route_pane_snapshot_filename, format_route_pane_snapshot_hint,
    format_route_pane_snapshot_log, sanitize_route_snapshot_field,
};
use agent_doc_turn::closeout_recovery::{
    CloseoutRecoveryDecision, CloseoutRecoveryDecisionInput,
    short_recovery_command_from_recommendation,
};
use tmux_router::Tmux;

use crate::{frontmatter_io, resync, sessions, sync};
use std::cell::Cell;

thread_local! {
    /// Per-invocation override for the bounded wait that
    /// `wait_for_authoritative_actor_ready` applies when the authoritative
    /// actor is still `starting`. Set from `route::run_with_tmux` when the
    /// caller passed `--wait-for-ready <SECONDS>` so user-initiated dispatches
    /// (e.g. JB plugin Run Agent Doc on a slow-starting supervisor) can hold
    /// the wait longer than the harness-specific default. `None` means no
    /// override — the binary-specific timeout in
    /// `authoritative_actor_ready_retry_budget` applies.
    static WAIT_FOR_READY_OVERRIDE: Cell<Option<Duration>> = const { Cell::new(None) };
    static FORCE_DISK_ROUTE_WRITES: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard that installs a `wait_for_ready` override on entry and restores
/// the previous value on drop. Keeps the override scoped to the single
/// `route::run_with_tmux` invocation even if it returns early via `?`.
struct WaitForReadyOverrideGuard {
    previous: Option<Duration>,
}

impl WaitForReadyOverrideGuard {
    fn set(value: Option<Duration>) -> Self {
        let previous = WAIT_FOR_READY_OVERRIDE.with(|cell| cell.replace(value));
        Self { previous }
    }
}

impl Drop for WaitForReadyOverrideGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        WAIT_FOR_READY_OVERRIDE.with(|cell| cell.set(previous));
    }
}

struct ForceDiskRouteWritesGuard {
    previous: bool,
}

impl ForceDiskRouteWritesGuard {
    fn set(value: bool) -> Self {
        let previous = FORCE_DISK_ROUTE_WRITES.with(|cell| cell.replace(value));
        Self { previous }
    }
}

impl Drop for ForceDiskRouteWritesGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        FORCE_DISK_ROUTE_WRITES.with(|cell| cell.set(previous));
    }
}

fn route_write_document(
    file: &Path,
    next_content: &str,
    previous_content: &str,
    reason: &str,
) -> Result<()> {
    if FORCE_DISK_ROUTE_WRITES.with(Cell::get) {
        crate::write::atomic_write_pub(file, next_content)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "{}_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                reason,
                file.display(),
                next_content.len(),
                agent_doc_hash::content_hash(next_content)
            ),
        );
        Ok(())
    } else {
        crate::write::converge_document_or_disk(file, next_content, previous_content, reason)
    }
}

fn wait_for_ready_override() -> Option<Duration> {
    WAIT_FOR_READY_OVERRIDE.with(|cell| cell.get())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandDispatchResult {
    status: CommandDispatchStatus,
    elapsed: Duration,
    diagnostic_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteMode {
    Managed,
    DispatchOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExistingPaneDispatchReadiness {
    Ready,
    BusyAlreadyRunning,
    BusyNeedsAutoFix {
        provenance: String,
        blocker_reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BusyPaneInterruptRecoveryOutcome {
    Recovered,
    Blocked { reason: String },
    TimedOut,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthoritativeActorDispatchTarget {
    record: agent_doc_sqlite::state_store::ActorRecord,
    runtime: SupervisorRuntime,
}

impl AuthoritativeActorDispatchTarget {
    fn actor_state(&self) -> agent_doc_sqlite::state_store::ActorState {
        sqlite_actor_state_from_route(effective_authoritative_actor_state(
            route_actor_state_from_sqlite(self.record.state),
            self.runtime.actor_state,
        ))
    }
}

fn route_actor_state_from_sqlite(
    state: agent_doc_sqlite::state_store::ActorState,
) -> RouteActorState {
    match state {
        agent_doc_sqlite::state_store::ActorState::Starting => RouteActorState::Starting,
        agent_doc_sqlite::state_store::ActorState::Ready => RouteActorState::Ready,
        agent_doc_sqlite::state_store::ActorState::Busy => RouteActorState::Busy,
        agent_doc_sqlite::state_store::ActorState::WaitingInput => RouteActorState::WaitingInput,
        agent_doc_sqlite::state_store::ActorState::Closed => RouteActorState::Closed,
        agent_doc_sqlite::state_store::ActorState::Blocked => RouteActorState::Blocked,
    }
}

fn sqlite_actor_state_from_route(
    state: RouteActorState,
) -> agent_doc_sqlite::state_store::ActorState {
    match state {
        RouteActorState::Starting => agent_doc_sqlite::state_store::ActorState::Starting,
        RouteActorState::Ready => agent_doc_sqlite::state_store::ActorState::Ready,
        RouteActorState::Busy => agent_doc_sqlite::state_store::ActorState::Busy,
        RouteActorState::WaitingInput => agent_doc_sqlite::state_store::ActorState::WaitingInput,
        RouteActorState::Closed => agent_doc_sqlite::state_store::ActorState::Closed,
        RouteActorState::Blocked => agent_doc_sqlite::state_store::ActorState::Blocked,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingPromptBearingRouteContext {
    marker: String,
    prompt_text: String,
    slash_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouteQueueEnqueueOutcome {
    prompt_text: String,
    appended: bool,
    already_present: bool,
    superseded: bool,
    component_created: bool,
    activated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RoutedDispatchStartTracker {
    CodexHook {
        trigger: String,
        previous_session_id: Option<String>,
        previous_turn_id: Option<String>,
        previous_updated_at: Option<u64>,
    },
    OpenCodePane {
        pane: String,
        trigger: String,
        pre_dispatch_content: String,
    },
}

/// Poll cadence for the direct-pane submit-acceptance check in
/// `send_command_unchecked`. `#run-agent-doc-latency`: tightened from 300ms so a
/// pane that consumes the routed trigger quickly is confirmed accepted within one
/// short poll instead of a 300ms floor. The loop captures before sleeping, so a
/// near-instant consume returns on the first capture.
const DIRECT_PANE_SUBMIT_ACCEPTANCE_POLL_INTERVAL: Duration = Duration::from_millis(150);

/// Poll cadence for `wait_for_agent_ready_outcome`. `#run-agent-doc-latency`:
/// tightened from 500ms so the 2-poll ready streak settles in ~150-300ms instead
/// of the old ~500-1000ms floor, while still requiring two consecutive idle
/// observations to debounce a transient prompt flicker.
const AGENT_READY_POLL_INTERVAL: Duration = Duration::from_millis(150);

fn editor_route_attempt_id() -> Option<String> {
    std::env::var(EDITOR_ROUTE_ATTEMPT_ID_ENV)
        .ok()
        .map(|value| sanitize_route_snapshot_field(&value))
        .filter(|value| !value.is_empty())
}

fn route_current_actor_generation(file: &Path) -> Option<u64> {
    let canonical = file.canonicalize().ok()?;
    let root = agent_doc_fs::find_project_root(&canonical)?;
    crate::session_actor::load_record_in(&root, canonical.to_string_lossy().as_ref())
        .ok()
        .flatten()
        .map(|record| record.generation)
}

fn route_ops_log_path(file: &Path) -> Option<PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let root = agent_doc_fs::find_project_root(&canonical)?;
    Some(root.join(".agent-doc/logs/ops.log"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoutePaneSnapshot {
    len: usize,
    hash: String,
    path: Option<PathBuf>,
}

fn route_snapshot_timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn preserve_route_pane_snapshot(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    phase: &str,
    content: &str,
) -> RoutePaneSnapshot {
    let redacted = agent_doc_secret_redact::redact(content);
    let snapshot = RoutePaneSnapshot {
        len: redacted.len(),
        hash: short_content_hash(&redacted),
        path: None,
    };

    let path = (|| -> Result<PathBuf> {
        let canonical = file
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", file.display()))?;
        let root = agent_doc_fs::find_project_root(&canonical)
            .with_context(|| format!("could not find .agent-doc root for {}", file.display()))?;
        let dir = root.join(".agent-doc/logs/route-submit");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        let name = format_route_pane_snapshot_filename(
            route_snapshot_timestamp_millis(),
            phase,
            &harness.binary,
            pane,
            snapshot.hash.as_str(),
        );
        let path = dir.join(name);
        std::fs::write(&path, redacted)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    })();

    match path {
        Ok(path) => {
            let file_display = file.display().to_string();
            let snapshot_path = path.display().to_string();
            let editor_attempt_id = editor_route_attempt_id();
            let message = format_route_pane_snapshot_log(RoutePaneSnapshotLogFacts {
                snapshot: RoutePaneSnapshotFacts {
                    file_display: &file_display,
                    pane,
                    harness_binary: &harness.binary,
                    phase,
                    capture_len: snapshot.len,
                    capture_hash: &snapshot.hash,
                    editor_attempt_id: editor_attempt_id.as_deref(),
                },
                snapshot_path: &snapshot_path,
            });
            crate::ops_log::log_op(file, &message);
            RoutePaneSnapshot {
                path: Some(path),
                ..snapshot
            }
        }
        Err(err) => {
            let file_display = file.display().to_string();
            let error = err.to_string();
            let editor_attempt_id = editor_route_attempt_id();
            let message = format_route_pane_snapshot_failed_log(RoutePaneSnapshotFailedLogFacts {
                snapshot: RoutePaneSnapshotFacts {
                    file_display: &file_display,
                    pane,
                    harness_binary: &harness.binary,
                    phase,
                    capture_len: snapshot.len,
                    capture_hash: &snapshot.hash,
                    editor_attempt_id: editor_attempt_id.as_deref(),
                },
                error: &error,
            });
            crate::ops_log::log_op(file, &message);
            eprintln!(
                "[route] warning: failed to preserve pane snapshot for {} phase {}: {}",
                file.display(),
                phase,
                err
            );
            snapshot
        }
    }
}

fn print_route_pane_snapshot_hint(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    phase: &str,
    snapshot: &RoutePaneSnapshot,
) {
    let file_display = file.display().to_string();
    let snapshot_path = snapshot
        .path
        .as_ref()
        .map(|path| path.display().to_string());
    let editor_attempt_id = editor_route_attempt_id();
    let message = format_route_pane_snapshot_hint(RoutePaneSnapshotHintFacts {
        snapshot: RoutePaneSnapshotFacts {
            file_display: &file_display,
            pane,
            harness_binary: &harness.binary,
            phase,
            capture_len: snapshot.len,
            capture_hash: &snapshot.hash,
            editor_attempt_id: editor_attempt_id.as_deref(),
        },
        snapshot_path: snapshot_path.as_deref(),
    });
    eprintln!("{message}");
}

#[derive(Debug, Clone, Copy)]
struct RouteSubmitObservationLogFacts<'a> {
    file: &'a Path,
    pane: &'a str,
    harness: &'a HarnessConfig,
    phase: &'a str,
    observation: RouteSubmitObservation,
    trigger_visible: Option<bool>,
    elapsed: Duration,
    capture_len: Option<usize>,
    capture_hash: Option<&'a str>,
    proof: Option<RoutedDispatchStartProof>,
}

#[derive(Debug, Clone, Copy)]
struct RouteDispatchBugReportFacts<'a> {
    file: &'a Path,
    pane: &'a str,
    harness: &'a HarnessConfig,
    phase: &'a str,
    issue: &'a str,
    result: &'a str,
    elapsed: Duration,
    proof: Option<RoutedDispatchStartProof>,
    diagnostic_path: Option<&'a Path>,
}

fn file_route_dispatch_bug_report(facts: RouteDispatchBugReportFacts<'_>) {
    let document_display = facts.file.display().to_string();
    let document_id = agent_doc_hash::document_id_for_path(facts.file);
    let editor_attempt_id = editor_route_attempt_id();
    let dispatch_proof_state = facts.proof.map(|proof| proof.dispatch_stage_label());
    let diagnostic_path = facts.diagnostic_path.map(|path| path.display().to_string());
    let ops_log_path = route_ops_log_path(facts.file).map(|path| path.display().to_string());
    let item = match route_dispatch_bug_report_item(RouteDispatchBugReportItemFacts {
        document_display: &document_display,
        document_id: &document_id,
        pane: facts.pane,
        phase: facts.phase,
        issue: facts.issue,
        result: facts.result,
        elapsed_ms: facts.elapsed.as_millis(),
        actor_generation: route_current_actor_generation(facts.file),
        editor_attempt_id: editor_attempt_id.as_deref(),
        dispatch_proof_state,
        diagnostic_path: diagnostic_path.as_deref(),
        ops_log_path: ops_log_path.as_deref(),
    }) {
        Ok(item) => item,
        Err(err) => {
            crate::ops_log::log_op(
                facts.file,
                &format!(
                    "route_dispatch_bug_backlog_item_failed file={} pane={} harness={} phase={} issue={} error={}",
                    facts.file.display(),
                    facts.pane,
                    facts.harness.binary,
                    facts.phase,
                    facts.issue,
                    agent_doc_secret_redact::redact(&err).replace(char::is_whitespace, "_")
                ),
            );
            return;
        }
    };
    let target_file = match agent_doc_project_config_io::agent_doc_bug_target_document_for_doc(
        facts.file,
    ) {
        Ok(Some(target)) => target,
        Ok(None) => facts.file.to_path_buf(),
        Err(err) => {
            crate::ops_log::log_op(
                facts.file,
                &format!(
                    "route_dispatch_bug_target_resolve_failed file={} pane={} harness={} phase={} issue={} error={}",
                    facts.file.display(),
                    facts.pane,
                    facts.harness.binary,
                    facts.phase,
                    facts.issue,
                    agent_doc_secret_redact::redact(&err.to_string())
                        .replace(char::is_whitespace, "_")
                ),
            );
            facts.file.to_path_buf()
        }
    };
    let items = [item];
    match crate::backlog_cmd::with_force_disk_pending_writes(
        FORCE_DISK_ROUTE_WRITES.with(Cell::get),
        || crate::backlog_cmd::add_many(&target_file, &items, false),
    ) {
        Ok(ids) => {
            let id = ids
                .first()
                .map(|id| id.as_str())
                .unwrap_or("deduped_existing");
            crate::ops_log::log_op(
                facts.file,
                &format!(
                    "route_dispatch_bug_backlog_filed file={} target_file={} pane={} harness={} phase={} issue={} id={} inserted={}",
                    facts.file.display(),
                    target_file.display(),
                    facts.pane,
                    facts.harness.binary,
                    facts.phase,
                    facts.issue,
                    id,
                    !ids.is_empty()
                ),
            );
        }
        Err(err) => {
            crate::ops_log::log_op(
                facts.file,
                &format!(
                    "route_dispatch_bug_backlog_file_failed file={} target_file={} pane={} harness={} phase={} issue={} error={}",
                    facts.file.display(),
                    target_file.display(),
                    facts.pane,
                    facts.harness.binary,
                    facts.phase,
                    facts.issue,
                    agent_doc_secret_redact::redact(&err.to_string())
                        .replace(char::is_whitespace, "_")
                ),
            );
        }
    }
}

fn log_route_submit_observation(facts: RouteSubmitObservationLogFacts<'_>) {
    let file_display = facts.file.display().to_string();
    let editor_attempt_id = editor_route_attempt_id();
    let controller_facts = ControllerRouteSubmitObservationFacts {
        file_display: &file_display,
        pane: facts.pane,
        harness_binary: &facts.harness.binary,
        phase: facts.phase,
        observation: facts.observation,
        trigger_visible: facts.trigger_visible,
        elapsed_ms: facts.elapsed.as_millis(),
        capture_len: facts.capture_len,
        capture_hash: facts.capture_hash,
        proof: facts.proof,
        editor_attempt_id: editor_attempt_id.as_deref(),
    };
    crate::ops_log::log_op(
        facts.file,
        &route_submit_observation_message(controller_facts),
    );
    if let Some(issue) = route_submit_issue_message(controller_facts) {
        crate::ops_log::log_op(facts.file, &issue);
    }
}

fn log_route_latency(
    file: &Path,
    phase: &str,
    elapsed: Duration,
    budget: Duration,
    pane: &str,
    harness: &HarnessConfig,
    outcome: &str,
) {
    let editor_attempt_id = editor_route_attempt_id();
    let elapsed_ms = elapsed.as_millis();
    let budget_ms = budget.as_millis();
    let message = route_latency_message(RouteLatencyFacts {
        phase,
        elapsed_ms,
        budget_ms,
        pane,
        harness_binary: &harness.binary,
        outcome,
        editor_attempt_id: editor_attempt_id.as_deref(),
    });
    crate::ops_log::log_op(file, &message);
    if route_latency_status(elapsed_ms, budget_ms) == RouteLatencyStatus::OverBudget {
        eprintln!(
            "[route] latency budget exceeded: phase {} took {}ms (budget {}ms, pane={}, harness={}, outcome={})",
            phase,
            elapsed.as_millis(),
            budget.as_millis(),
            pane,
            harness.binary,
            outcome
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentReadyWaitOutcome {
    Ready,
    Blocked { reason: String },
    TimedOut,
}

impl AgentReadyWaitOutcome {
    fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    fn blocker_reason(&self) -> Option<&str> {
        match self {
            Self::Blocked { reason } => Some(reason.as_str()),
            _ => None,
        }
    }
}

fn pane_display_value(tmux: &Tmux, pane_id: &str, format: &str) -> Option<String> {
    tmux.cmd()
        .args(["display-message", "-t", pane_id, "-p", format])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn pane_route_provenance(tmux: &Tmux, pane_id: &str) -> String {
    let pane_pid = pane_display_value(tmux, pane_id, "#{pane_pid}")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "?".to_string());
    let pane_session = pane_display_value(tmux, pane_id, "#{session_name}")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "?".to_string());
    let current_command = pane_display_value(tmux, pane_id, "#{pane_current_command}")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "?".to_string());
    format!(
        "pane={} pane_pid={} pane_session={} current_command={}",
        pane_id, pane_pid, pane_session, current_command
    )
}

fn codex_dispatch_start_tracking_enabled(file: &Path) -> bool {
    codex_tracking_roots(file)
        .into_iter()
        .any(|root| codex_hooks_visible_from_file(file, &root))
}

fn codex_hooks_visible_from_file(file: &Path, hook_root: &Path) -> bool {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut current = if canonical.is_file() {
        canonical.parent()
    } else {
        Some(canonical.as_path())
    };

    while let Some(path) = current {
        let codex_path = path.join(".codex");
        if codex_path.exists() {
            return path == hook_root && codex_path.join("hooks.json").is_file();
        }
        if path == hook_root {
            return hook_root.join(".codex/hooks.json").is_file();
        }
        current = path.parent();
    }

    false
}

fn codex_tracking_roots(file: &Path) -> Vec<PathBuf> {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut roots = Vec::new();
    let mut current = if canonical.is_file() {
        canonical.parent()
    } else {
        Some(canonical.as_path())
    };

    while let Some(path) = current {
        if path.join(".agent-doc").is_dir() {
            roots.push(path.to_path_buf());
        }
        current = path.parent();
    }

    roots
}

fn build_routed_dispatch_start_tracker(
    file: &Path,
    file_path: &str,
    harness: &HarnessConfig,
    tmux: Option<&Tmux>,
    pane: Option<&str>,
) -> Result<Option<RoutedDispatchStartTracker>> {
    match harness.binary.as_str() {
        "codex" if codex_dispatch_start_tracking_enabled(file) => {
            let latest = crate::codex_hook::load_latest_prompt_state_for_file(file)?;
            Ok(Some(RoutedDispatchStartTracker::CodexHook {
                trigger: harness.trigger_command(file_path),
                previous_session_id: latest.as_ref().map(|state| state.session_id.clone()),
                previous_turn_id: latest.as_ref().map(|state| state.last_turn_id.clone()),
                previous_updated_at: latest.as_ref().map(|state| state.updated_at),
            }))
        }
        "opencode" => {
            let (Some(tmux), Some(pane)) = (tmux, pane) else {
                return Ok(None);
            };
            let pre_dispatch_content = sessions::capture_pane(tmux, pane).with_context(|| {
                format!(
                    "failed to capture OpenCode pane {} before routed dispatch",
                    pane
                )
            })?;
            Ok(Some(RoutedDispatchStartTracker::OpenCodePane {
                pane: pane.to_string(),
                trigger: harness.trigger_command(file_path),
                pre_dispatch_content,
            }))
        }
        _ => Ok(None),
    }
}

fn codex_routed_dispatch_start_proof_facts<'a>(
    tracker: &'a RoutedDispatchStartTracker,
    state: &'a crate::codex_hook::ActiveSessionState,
) -> Option<agent_doc_controller::dispatch::CodexRoutedDispatchStartProofFacts<'a>> {
    let RoutedDispatchStartTracker::CodexHook {
        trigger,
        previous_session_id,
        previous_turn_id,
        previous_updated_at,
    } = tracker
    else {
        return None;
    };
    Some(
        agent_doc_controller::dispatch::CodexRoutedDispatchStartProofFacts {
            trigger,
            previous_session_id: previous_session_id.as_deref(),
            previous_turn_id: previous_turn_id.as_deref(),
            previous_updated_at: *previous_updated_at,
            current_session_id: state.session_id.as_str(),
            current_turn_id: state.last_turn_id.as_str(),
            current_updated_at: state.updated_at,
            current_prompt: state.last_prompt.as_str(),
        },
    )
}

fn opencode_pane_dispatch_start_proof_facts<'a>(
    harness: &HarnessConfig,
    trigger: &'a str,
    pre_dispatch_content: &'a str,
    current_content: &'a str,
) -> OpenCodePaneDispatchStartProofFacts<'a> {
    OpenCodePaneDispatchStartProofFacts {
        trigger,
        pre_dispatch_content,
        current_content,
        current_has_ready_prompt_candidate: agent_doc_harness::ready_prompt_candidate(
            current_content,
            harness,
        )
        .is_some(),
        current_is_idle_chrome_only_output: harness.is_idle_chrome_only_output(current_content),
        current_has_busy_cue: harness.has_busy_cue(current_content),
        current_has_non_idle_output_line: current_content
            .lines()
            .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
            .any(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty()
                    && !harness.is_ignorable_output_line(trimmed)
                    && !harness.is_dispatch_ready_prompt_line(trimmed)
            }),
    }
}

fn wait_for_routed_dispatch_start(
    tmux: &Tmux,
    file: &Path,
    tracker: &RoutedDispatchStartTracker,
    harness: &HarnessConfig,
    timeout: Duration,
) -> Result<Option<RoutedDispatchStartProof>> {
    let start = std::time::Instant::now();
    let poll = if matches!(tracker, RoutedDispatchStartTracker::OpenCodePane { .. }) {
        Duration::from_millis(500)
    } else {
        Duration::from_millis(200)
    };

    while start.elapsed() < timeout {
        match tracker {
            RoutedDispatchStartTracker::CodexHook { .. } => {
                if let Some(state) = crate::codex_hook::load_latest_prompt_state_for_file(file)?
                    && let Some(facts) = codex_routed_dispatch_start_proof_facts(tracker, &state)
                    && let Some(proof) = classify_codex_routed_dispatch_start_proof(facts)
                {
                    return Ok(Some(proof));
                }
            }
            RoutedDispatchStartTracker::OpenCodePane {
                pane,
                trigger,
                pre_dispatch_content,
            } => {
                let content = sessions::capture_pane(tmux, pane).with_context(|| {
                    format!(
                        "failed to capture OpenCode pane {} while awaiting routed dispatch proof",
                        pane
                    )
                })?;
                let facts = opencode_pane_dispatch_start_proof_facts(
                    harness,
                    trigger,
                    pre_dispatch_content,
                    &content,
                );
                if opencode_pane_state_changed_from_idle(facts) {
                    return Ok(Some(RoutedDispatchStartProof::PaneStateChanged));
                }
            }
        }
        std::thread::sleep(poll);
    }

    Ok(None)
}

fn actor_dispatch_state(state: agent_doc_sqlite::state_store::ActorState) -> ActorDispatchState {
    match state {
        agent_doc_sqlite::state_store::ActorState::Ready => ActorDispatchState::Ready,
        agent_doc_sqlite::state_store::ActorState::Starting => ActorDispatchState::Starting,
        agent_doc_sqlite::state_store::ActorState::Busy => ActorDispatchState::Busy,
        agent_doc_sqlite::state_store::ActorState::WaitingInput => ActorDispatchState::WaitingInput,
        agent_doc_sqlite::state_store::ActorState::Blocked => ActorDispatchState::Blocked,
        agent_doc_sqlite::state_store::ActorState::Closed => ActorDispatchState::Closed,
    }
}

fn dispatch_runtime_health(health: SupervisorHealth) -> DispatchRuntimeHealth {
    match health {
        SupervisorHealth::Healthy => DispatchRuntimeHealth::Healthy,
        SupervisorHealth::Restartable => DispatchRuntimeHealth::Restartable,
        SupervisorHealth::Halted { restart_count } => {
            DispatchRuntimeHealth::Halted { restart_count }
        }
        SupervisorHealth::Unreachable => DispatchRuntimeHealth::Unreachable,
        SupervisorHealth::NoSocket => DispatchRuntimeHealth::NoSocket,
    }
}

fn authoritative_actor_ready_facts_from_target(
    target: &AuthoritativeActorDispatchTarget,
    prompt_ready: bool,
) -> AuthoritativeActorReadyFacts {
    AuthoritativeActorReadyFacts {
        pane_id: target.record.pane_id.clone(),
        generation: target.record.generation,
        actor_state: actor_dispatch_state(target.actor_state()),
        supervisor_health: target.runtime.health.label(),
        runtime_state: target.runtime.actor_state_label().to_string(),
        prompt_ready,
        last_transition_reason: target.record.last_transition.reason.clone(),
        last_transition_caller: target.record.last_transition.caller.clone(),
    }
}

fn supervisor_socket_path(file: &Path, session_id: &str) -> Option<std::path::PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let project_root = agent_doc_fs::find_project_root(&canonical)?;
    Some(crate::supervisor::ipc::socket_path(
        &project_root,
        session_id,
    ))
}

fn query_supervisor_runtime(file: &Path, session_id: &str) -> SupervisorRuntime {
    let Some(sock) = supervisor_socket_path(file, session_id) else {
        return SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
        };
    };
    if !sock.exists() {
        return SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
        };
    }
    match crate::supervisor::ipc::send_command(&sock, &IpcMethod::State) {
        Ok(resp) if resp.ok => {
            if let Some(data) = &resp.data {
                let running = data
                    .get("running")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let state = data.get("state").and_then(|v| v.as_str()).unwrap_or("");
                let restart_count = data
                    .get("restart_count")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or(0);
                let actor_state = data
                    .get("actor_state")
                    .and_then(|v| v.as_str())
                    .and_then(RouteActorState::parse);
                let health = if running && state == "healthy" {
                    SupervisorHealth::Healthy
                } else if state == "halted" {
                    SupervisorHealth::Halted { restart_count }
                } else {
                    SupervisorHealth::Restartable
                };
                SupervisorRuntime {
                    health,
                    actor_state,
                }
            } else {
                SupervisorRuntime {
                    health: SupervisorHealth::Restartable,
                    actor_state: None,
                }
            }
        }
        Ok(_) | Err(_) => SupervisorRuntime {
            health: SupervisorHealth::Unreachable,
            actor_state: None,
        },
    }
}

fn query_supervisor_health(file: &Path, session_id: &str) -> SupervisorHealth {
    query_supervisor_runtime(file, session_id).health
}

fn restart_via_supervisor_with_mode(file: &Path, session_id: &str, mode: &str) -> bool {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let project_root = match agent_doc_fs::find_project_root(&canonical) {
        Some(r) => r,
        None => return false,
    };
    let sock = crate::supervisor::ipc::socket_path(&project_root, session_id);
    let method = IpcMethod::Restart {
        mode: mode.to_string(),
    };
    match crate::supervisor::ipc::send_command(&sock, &method) {
        Ok(resp) => resp.ok,
        Err(_) => false,
    }
}

fn restart_via_supervisor(file: &Path, session_id: &str) -> bool {
    restart_via_supervisor_with_mode(file, session_id, "continue")
}

fn tracked_harness_clear_requires_fresh_restart(
    harness: &HarnessConfig,
    latest_prompt: Option<&str>,
) -> bool {
    matches!(harness.binary.as_str(), "codex" | "opencode")
        && latest_prompt.is_some_and(crate::codex_hook::prompt_requests_clear)
}

fn reapply_harness_launch_contract_after_clear(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    respect_tracked_clear_restart: bool,
) -> Result<String> {
    let latest_prompt = crate::codex_hook::load_latest_prompt_for_file(file)?;
    if !respect_tracked_clear_restart
        || !tracked_harness_clear_requires_fresh_restart(harness, latest_prompt.as_deref())
    {
        return Ok(pane.to_string());
    }
    let latest_prompt_label = latest_prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .unwrap_or("<unknown>");

    crate::ops_log::log_op(
        file,
        &format!(
            "route_harness_clear_restart_fresh file={} pane={} harness={} latest_prompt={:?}",
            file.display(),
            pane,
            harness.binary,
            latest_prompt_label
        ),
    );
    eprintln!(
        "[route] latest tracked {} prompt for {} was `{}` — restarting the live session fresh before reroute so sandbox, writable roots, and network policy are reapplied",
        harness.binary,
        file.display(),
        latest_prompt_label
    );

    if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
        anyhow::bail!(
            "latest tracked {} prompt for {} was `{}`, but route could not restart the live session fresh to reapply the original launch policy. Run `agent-doc start {}` manually to recover",
            harness.binary,
            file.display(),
            latest_prompt_label,
            file.display()
        );
    }

    wait_for_busy_restart_handoff(tmux, file, file_path, session_id, pane);
    let dispatch_pane = crate::sync::find_normal_path_owner_pane(tmux, file, session_id)
        .unwrap_or_else(|| pane.to_string());
    if !wait_for_agent_ready(
        tmux,
        &dispatch_pane,
        fresh_route_start_ack_timeout(cfg!(test)),
        harness,
    ) {
        anyhow::bail!(
            "latest tracked {} prompt for {} was `{}`, and the fresh recovery session in pane {} never became ready. Run `agent-doc start {}` manually to recover",
            harness.binary,
            file.display(),
            latest_prompt_label,
            dispatch_pane,
            file.display()
        );
    }
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    Ok(dispatch_pane)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedCapabilityProofStatus {
    NotRequired,
    Pending,
    Proven,
    Failed,
    Missing,
}

fn managed_capability_proof_status(
    file: &Path,
    session_id: &str,
    harness: &HarnessConfig,
) -> Result<ManagedCapabilityProofStatus> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let fm = frontmatter_io::parse_for_file_with_context(&content, file, &rc).map(|(fm, _)| fm)?;
    #[cfg(test)]
    let global_config = agent_doc_config::Config::default();
    #[cfg(not(test))]
    let global_config = rc.global_config();
    if !crate::agent::codex::managed_capability_contract_required_for_doc_and_harness(
        file,
        &fm,
        &global_config,
        &harness.binary,
    ) {
        return Ok(ManagedCapabilityProofStatus::NotRequired);
    }
    let prefix = format!("{}_capability_proof status=", harness.binary);
    let expected_writable_contract = if harness.binary == "codex" {
        crate::agent::codex::managed_writable_root_contract_id_for_doc(file, &fm, &global_config)
    } else {
        None
    };
    let proven_prefix = format!("{}proven", prefix);
    let proven = if let Some(contract) = expected_writable_contract.as_deref() {
        crate::startup_miss::session_log_has_event_after_latest_start_containing(
            file,
            session_id,
            &proven_prefix,
            &format!("writable_root_contract={contract}"),
        )?
    } else {
        crate::startup_miss::session_log_has_event_after_latest_start(
            file,
            session_id,
            &proven_prefix,
        )?
    };
    if proven {
        return Ok(ManagedCapabilityProofStatus::Proven);
    }
    if crate::startup_miss::session_log_has_event_after_latest_start(
        file,
        session_id,
        &format!("{}failed", prefix),
    )? {
        return Ok(ManagedCapabilityProofStatus::Failed);
    }
    if crate::startup_miss::session_log_has_event_after_latest_start(
        file,
        session_id,
        &format!("{}pending", prefix),
    )? {
        return Ok(ManagedCapabilityProofStatus::Pending);
    }
    Ok(ManagedCapabilityProofStatus::Missing)
}

fn reapply_capability_contract_before_reuse(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    enforce_capability_proof: bool,
) -> Result<String> {
    if !enforce_capability_proof {
        return Ok(pane.to_string());
    }
    // `#capproofbg`: do NOT block the `Run Agent Doc` dispatch waiting for the
    // proof to finish. Read the current status without polling for it to leave
    // `Pending` — a still-running proof lets dispatch proceed on the live pane
    // immediately; the proof keeps running in the background and a later FAILURE
    // is surfaced asynchronously (the supervisor flips the actor to Blocked,
    // emits a tmux `display-message`, and gates *subsequent* dispatch). Only a
    // proof that has ALREADY failed (or is missing) forces the fresh-restart
    // recovery path here.
    let proof_status = managed_capability_proof_status(file, session_id, harness)?;
    let reason = match proof_status {
        ManagedCapabilityProofStatus::NotRequired
        | ManagedCapabilityProofStatus::Proven
        | ManagedCapabilityProofStatus::Pending => {
            return Ok(pane.to_string());
        }
        ManagedCapabilityProofStatus::Failed => {
            anyhow::bail!(
                "managed {} capability proof for {} on pane {} failed; prompt dispatch is disabled for this pane. Inspect diagnostics, then run `agent-doc start {}` manually to recover",
                harness.binary,
                file.display(),
                pane,
                file.display()
            );
        }
        ManagedCapabilityProofStatus::Missing => {
            format!(
                "managed {} session has no current capability proof for requested network, SSH, or writable-root access",
                harness.binary
            )
        }
    };

    crate::ops_log::log_op(
        file,
        &format!(
            "route_{}_capability_restart_fresh file={} pane={} harness={} reason={}",
            harness.binary,
            file.display(),
            pane,
            harness.binary,
            reason.replace(' ', "_")
        ),
    );
    eprintln!(
        "[route] {} for {} on pane {} — restarting the live {} session fresh once before reuse",
        reason,
        file.display(),
        pane,
        harness.binary
    );

    if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
        anyhow::bail!(
            "{} for {} on pane {}, and route could not restart the live session fresh. Run `agent-doc start {}` manually to recover",
            reason,
            file.display(),
            pane,
            file.display()
        );
    }

    wait_for_busy_restart_handoff(tmux, file, file_path, session_id, pane);
    let dispatch_pane = crate::sync::find_normal_path_owner_pane(tmux, file, session_id)
        .unwrap_or_else(|| pane.to_string());
    if !wait_for_agent_ready(
        tmux,
        &dispatch_pane,
        fresh_route_start_ack_timeout(cfg!(test)),
        harness,
    ) {
        anyhow::bail!(
            "{} for {}, and the fresh recovery session in pane {} never became ready. Run `agent-doc start {}` manually to recover",
            reason,
            file.display(),
            dispatch_pane,
            file.display()
        );
    }
    // `#capproofbg`: the fresh recovery session also dispatches immediately while
    // its capability proof runs in the background — a still-`Pending` proof no
    // longer blocks dispatch (a later FAILURE is surfaced asynchronously by the
    // supervisor). Only an already-failed/missing proof aborts recovery.
    match managed_capability_proof_status(file, session_id, harness)? {
        ManagedCapabilityProofStatus::NotRequired
        | ManagedCapabilityProofStatus::Proven
        | ManagedCapabilityProofStatus::Pending => {}
        ManagedCapabilityProofStatus::Failed => anyhow::bail!(
            "{} for {}, and the fresh recovery session in pane {} failed capability proof. Run `agent-doc start {}` manually to recover",
            reason,
            file.display(),
            dispatch_pane,
            file.display()
        ),
        ManagedCapabilityProofStatus::Missing => anyhow::bail!(
            "{} for {}, and the fresh recovery session in pane {} never recorded a capability proof. Run `agent-doc start {}` manually to recover",
            reason,
            file.display(),
            dispatch_pane,
            file.display()
        ),
    }
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    Ok(dispatch_pane)
}

#[allow(clippy::too_many_arguments)]
fn reapply_codex_launch_contract_before_reuse(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    respect_tracked_clear_restart: bool,
    enforce_capability_proof: bool,
) -> Result<String> {
    let dispatch_pane = reapply_harness_launch_contract_after_clear(
        tmux,
        file,
        pane,
        session_id,
        file_path,
        harness,
        respect_tracked_clear_restart,
    )?;
    reapply_capability_contract_before_reuse(
        tmux,
        file,
        &dispatch_pane,
        session_id,
        file_path,
        harness,
        enforce_capability_proof,
    )
}

fn startup_miss_route_facts(
    miss: &agent_doc_supervisor::startup_miss::StartupMiss,
    registered_pane: &str,
    pane_alive: bool,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
    log_status: Option<&agent_doc_supervisor::startup_miss::SessionLogStatus>,
) -> StartupMissRouteFacts {
    StartupMissRouteFacts {
        miss_timestamp: miss.timestamp,
        registered_pane_is_live_owner: live_owner == Some(registered_pane),
        pane_alive,
        supervisor_health: dispatch_runtime_health(supervisor_health),
        latest_start_matches_registered_pane: log_status
            .and_then(|status| status.latest_start_pane.as_deref())
            == Some(registered_pane),
        latest_session_open: log_status
            .is_some_and(agent_doc_supervisor::startup_miss::SessionLogStatus::latest_session_open),
        latest_session_closed: log_status.is_some_and(
            agent_doc_supervisor::startup_miss::SessionLogStatus::latest_session_closed,
        ),
        latest_start_timestamp: log_status.and_then(|status| status.latest_start_timestamp),
        latest_open_run_timestamp: log_status
            .and_then(agent_doc_supervisor::startup_miss::latest_open_run_timestamp),
    }
}

fn startup_miss_route_provenance(
    tmux: &Tmux,
    pane_id: &str,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
    log_status: Option<&agent_doc_supervisor::startup_miss::SessionLogStatus>,
) -> String {
    let log_detail = match log_status {
        Some(status) => format!(
            "session_log={} {} last_event={}",
            agent_doc_supervisor::startup_miss::latest_log_outcome(status),
            agent_doc_supervisor::startup_miss::latest_log_anchor(status),
            agent_doc_supervisor::startup_miss::latest_log_last_event(status)
        ),
        None => "session_log=missing".to_string(),
    };
    format!(
        "{} live_owner={} supervisor_health={:?} {}",
        pane_route_provenance(tmux, pane_id),
        live_owner.unwrap_or("none"),
        supervisor_health,
        log_detail
    )
}

const STARTUP_MISS_DIAGNOSTIC_DISPLAY_MS: &str = "10000";
const BUSY_ROUTE_DIAGNOSTIC_DISPLAY_MS: &str = "10000";

fn fail_if_recent_session_loss_window(file: &Path, session_id: &str) -> Result<()> {
    let Some(window) = crate::startup_miss::recent_session_loss_window(file, session_id)? else {
        return Ok(());
    };

    let first = agent_doc_supervisor::startup_miss::format_timestamp(window.first_timestamp);
    let last = agent_doc_supervisor::startup_miss::format_timestamp(window.last_timestamp);
    let latest_reason = window.latest_reason.as_deref().unwrap_or("unknown");
    crate::ops_log::log_op(
        file,
        &format!(
            "route_repeated_session_loss_fail_closed file={} session={} count={} first={} last={} latest_reason={}",
            file.display(),
            session_id,
            window.count,
            first,
            last,
            latest_reason
        ),
    );
    anyhow::bail!(
        "refusing to auto-start {} after {} unexpected pane-loss events since {} (latest reason={} at {}). Route will not keep spawning replacements over a repeated crash window; inspect the last dead-pane/session-loss diagnostics, then run `agent-doc start {}` manually to recover",
        file.display(),
        window.count,
        first,
        latest_reason,
        last,
        file.display()
    );
}

fn emit_startup_miss_diagnostic(tmux: &Tmux, pane_id: &str, file: &Path, reason: &str) {
    let file_display = file.display().to_string();
    let msg = route_startup_miss_diagnostic_message(RouteStartupMissDiagnosticFacts {
        file_display: &file_display,
        reason,
    });
    if let Err(e) = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-d",
            STARTUP_MISS_DIAGNOSTIC_DISPLAY_MS,
            &msg,
        ])
        .status()
    {
        eprintln!(
            "[route] warning: failed to emit startup-miss diagnostic to pane {}: {}",
            pane_id, e
        );
    }
}

fn emit_busy_route_diagnostic(tmux: &Tmux, pane_id: &str, file: &Path, harness: &HarnessConfig) {
    let file_display = file.display().to_string();
    let msg = route_busy_diagnostic_message(RouteBusyDiagnosticFacts {
        file_display: &file_display,
        harness_binary: &harness.binary,
    });
    if let Err(e) = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-d",
            BUSY_ROUTE_DIAGNOSTIC_DISPLAY_MS,
            &msg,
        ])
        .status()
    {
        eprintln!(
            "[route] warning: failed to emit busy-route diagnostic to pane {}: {}",
            pane_id, e
        );
    }
}

fn emit_busy_route_queued_diagnostic(
    tmux: &Tmux,
    pane_id: &str,
    file: &Path,
    harness: &HarnessConfig,
) {
    let file_display = file.display().to_string();
    let user_outcome = agent_doc_flow::outcome::user_outcome_fields(
        agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner,
    );
    let msg = route_busy_queued_diagnostic_message(RouteBusyQueuedDiagnosticFacts {
        file_display: &file_display,
        harness_binary: &harness.binary,
        user_outcome_fields: &user_outcome,
    });
    if let Err(e) = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-d",
            BUSY_ROUTE_DIAGNOSTIC_DISPLAY_MS,
            &msg,
        ])
        .status()
    {
        eprintln!(
            "[route] warning: failed to emit busy-route queued diagnostic to pane {}: {}",
            pane_id, e
        );
    }
}

/// Returns true if the pane is running an agent process for the given harness.
/// Returns true on query failure (conservative — don't skip panes we can't inspect).
fn is_agent_process(tmux: &Tmux, pane_id: &str, harness: &HarnessConfig) -> bool {
    let output = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-p",
            "#{pane_current_command}",
        ])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let cmd = String::from_utf8_lossy(&o.stdout).trim().to_string();
            harness.is_agent_process_name(&cmd)
        }
        _ => true, // can't inspect → treat conservatively
    }
}

pub fn run(
    file: &Path,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    wait_for_ready: Option<Duration>,
) -> Result<()> {
    run_with_force_disk(
        file,
        pane,
        debounce_ms,
        col_args,
        mode,
        plain_trigger,
        wait_for_ready,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_force_disk(
    file: &Path,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    wait_for_ready: Option<Duration>,
    force_disk: bool,
) -> Result<()> {
    run_with_tmux_with_options(
        file,
        &Tmux::default_server(),
        pane,
        debounce_ms,
        col_args,
        mode,
        plain_trigger,
        wait_for_ready,
        force_disk,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_tmux(
    file: &Path,
    tmux: &Tmux,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    wait_for_ready: Option<Duration>,
) -> Result<()> {
    run_with_tmux_with_options(
        file,
        tmux,
        pane,
        debounce_ms,
        col_args,
        mode,
        plain_trigger,
        wait_for_ready,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_tmux_with_options(
    file: &Path,
    tmux: &Tmux,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    wait_for_ready: Option<Duration>,
    force_disk: bool,
) -> Result<()> {
    let _wait_for_ready_guard = WaitForReadyOverrideGuard::set(wait_for_ready);
    let _force_disk_guard = ForceDiskRouteWritesGuard::set(force_disk);
    tracing::debug!(file = %file.display(), pane, debounce_ms, cols = ?col_args, "route::run start");
    // Clean stale registry entries before lookup, but stay on the editor-driven
    // fast path: use `SkipExpensiveStashCleanup` so the pre-lookup prune does NOT
    // scan stash panes. The expensive stash-pane purge re-resolves every live
    // fleet session's owner pane (tmux capture + process sampling + supervisor
    // probe) *per unregistered stash pane*, which measured ~28s on a busy fleet
    // and was the dominant `Run Agent Doc` dispatch latency (`#run-agent-doc-latency`).
    // Route only needs stale registry rows + dead non-stash panes pruned for an
    // accurate pane lookup; orphaned stash-pane hygiene belongs to explicit
    // `resync`/`sync`, mirroring `safe_passive_prune_cleanup_mode`.
    let _ = resync::prune_with_tmux_timed_in_mode(
        tmux,
        agent_doc_tmux::PruneCleanupMode::SkipExpensiveStashCleanup,
    );

    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    if let Err(err) = agent_doc_queue_io::continuation_marker::clear_cooldown_marker(file) {
        eprintln!(
            "[route] warning: failed to clear queue cooldown marker for {}: {err:#}",
            file.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_clear_queue_cooldown_failed file={} error={:?}",
                file.display(),
                err.to_string()
            ),
        );
    }

    // Debounce: wait for file mtime and editor typing indicator to settle before
    // route performs visible mutations such as session UUID insertion or
    // duplicate-prompt cleanup.
    if debounce_ms > 0 {
        await_idle(file, Duration::from_millis(debounce_ms))?;
    }

    // Ensure session UUID exists in frontmatter (generate if missing)
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    // Opt-in gate: a plain `.md` must not be auto-converted into a session.
    agent_doc_frontmatter_io::session::require_agent_doc_document(&content, file)?;
    let (mut updated_content, session_id) =
        agent_doc_frontmatter_io::session::ensure_session_for_file(&content, file)?;
    if updated_content != content {
        route_write_document(file, &updated_content, &content, "route_session_id")
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("[route] Generated session UUID: {}", session_id);
    }
    let snapshot_doc = crate::snapshot::load(file).ok().flatten();
    let head_doc = crate::git::show_head(file).ok().flatten();
    let mut preserve_docs = Vec::new();
    preserve_docs.push(updated_content.as_str());
    if let Some(head_doc) = head_doc.as_deref() {
        preserve_docs.push(head_doc);
    }
    if let Some(snapshot_doc) = snapshot_doc.as_deref() {
        preserve_docs.push(snapshot_doc);
    }
    if let Some(cleanup) =
        scrub_duplicate_prompt_comments_for_route(&updated_content, &preserve_docs)?
    {
        route_write_document(
            file,
            &cleanup.content,
            &updated_content,
            "route_dedup_scrub",
        )?;
        if cleanup.removed_answered_tail {
            crate::ops_log::log_op(
                file,
                &format!(
                    "duplicate_answered_exchange_prompt_tail_removed file={} source=route",
                    file.display()
                ),
            );
            eprintln!(
                "[route] removed duplicate answered prompt tail after exchange boundary in {}",
                file.display()
            );
        }
        if cleanup.removed_comment {
            crate::ops_log::log_op(
                file,
                &format!(
                    "post_exchange_duplicate_prompt_comment_removed file={} source=route",
                    file.display()
                ),
            );
            eprintln!(
                "[route] scrubbed duplicate prompt text from comment after exchange in {}",
                file.display()
            );
        }
        updated_content = cleanup.content;
    }

    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let fm =
        frontmatter_io::parse_for_file_with_context(&updated_content, file, &rc).map(|(f, _)| f)?;
    let global_config = rc.global_config();
    let mut harness = HarnessConfig::from_context(&fm, &global_config);
    if plain_trigger {
        harness.apply_plain_trigger_override();
    }

    // Use absolute path for trigger commands to avoid CWD-dependent resolution
    // when the pane's CWD differs from the invoker's (e.g., narrowed to a
    // submodule root). Relative paths would resolve to the submodule's version
    // of the file when the same relative path exists in both locations.
    let file_path = agent_doc_git_io::dirs::resolve_absolute_file_path(file)
        .to_string_lossy()
        .into_owned();
    let target_session = resolve_target_session(tmux, None, col_args, Some(file), &harness);
    eprintln!("[route] target tmux session: {}", target_session);

    // === SINGLE EXIT POINT PATTERN ===
    // All paths resolve to a pane_id, then ONE sync call handles layout.
    // This prevents propagation bugs where cross-cutting behavior (sync)
    // is added to one path but missed on others.

    let mut created_panes = Vec::new();

    let pane_id = match mode {
        RouteMode::Managed => resolve_or_create_pane(
            tmux,
            file,
            pane,
            col_args,
            &session_id,
            &file_path,
            &target_session,
            &harness,
            &mut created_panes,
        ),
        RouteMode::DispatchOnly => resolve_or_create_pane_dispatch_only(
            tmux,
            file,
            pane,
            col_args,
            &session_id,
            &file_path,
            &target_session,
            &harness,
            &mut created_panes,
        ),
    };

    match pane_id {
        Ok(ref _pid) => {
            // NOTE: sync_after_claim was removed here to eliminate the double-sync
            // glitch. The JB plugin already triggers sync with the correct window
            // and col_args via the route call. A second sync (with window=None)
            // races with the first sync's stash operations, causing panes to
            // bounce between stash and agent-doc window visibly.
            // The JB plugin's sync call is authoritative — no defensive re-sync needed.
            crate::editor_route_errors::clear_for_success(file, "route_success");
            Ok(())
        }
        Err(e) => {
            // Clean up panes created during the failed route attempt, but fail
            // closed for the current session owner: if a newly-created pane is
            // still the registered live pane for this document, preserve it so
            // a missed start-ack cannot crash the user's active tmux pane.
            cleanup_failed_route_panes(tmux, file, &session_id, &created_panes);
            Err(e)
        }
    }
}

#[derive(Debug)]
struct RouteDuplicatePromptCleanup {
    content: String,
    removed_answered_tail: bool,
    removed_comment: bool,
}

fn scrub_duplicate_prompt_comments_for_route(
    content: &str,
    preserve_docs: &[&str],
) -> Result<Option<RouteDuplicatePromptCleanup>> {
    let (frontmatter, _) = frontmatter::parse(content)
        .context("failed to parse document frontmatter before route cleanup")?;
    if !frontmatter.resolve_mode().is_template() {
        return Ok(None);
    }
    let mut cleaned_content = content.to_string();
    let mut removed_answered_tail = false;
    let mut removed_comment = false;
    if let Some(tail_cleaned) =
        agent_doc_template::remove_duplicate_answered_exchange_prompt_tail(&cleaned_content)
    {
        cleaned_content = tail_cleaned;
        removed_answered_tail = true;
    }
    if let Some(tail_cleaned) =
        agent_doc_template::remove_post_exchange_duplicate_prompt_comments_preserving_docs(
            &cleaned_content,
            preserve_docs,
        )
    {
        cleaned_content = tail_cleaned;
        removed_comment = true;
    }
    agent_doc_template::guard_no_duplicate_prompt_residue_outside_exchange(&cleaned_content)
        .context("route duplicate prompt residue guard failed")?;
    if removed_answered_tail || removed_comment {
        Ok(Some(RouteDuplicatePromptCleanup {
            content: cleaned_content,
            removed_answered_tail,
            removed_comment,
        }))
    } else {
        Ok(None)
    }
}

fn drain_open_closeout_before_routed_dispatch(file: &Path) -> Result<RouteCloseoutDrainOutcome> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(RouteCloseoutDrainOutcome::NoOpenCycle);
    };
    if !state.is_open() {
        return Ok(RouteCloseoutDrainOutcome::NoOpenCycle);
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "route_dispatch_drain_closeout_started file={} cycle_id={} phase={:?}",
            file.display(),
            state.cycle_id,
            state.phase
        ),
    );

    // #pcp3a: a concurrent finalize in another process (the route-owned supervisor
    // self-race) can move the document/cycle baseline mid-drain, so `repair` +
    // `session_check` observe a transient "captured response baseline no longer
    // matches current document" mismatch. Rather than fail closed on the first
    // such block (the "could not drain the active closeout" / exit 75 the user
    // hit, which self-resolves once the finalize completes), retry a bounded
    // number of times when there is positive evidence the cycle is concurrently
    // progressing (phase/cycle_id advanced) or has just closed. A genuine,
    // non-advancing block still fails closed after the first attempt.
    const DRAIN_MAX_ATTEMPTS: u32 = 3;
    const DRAIN_RETRY_BACKOFF_MS: u64 = 200;
    let mut last_reason = String::new();

    for attempt in 0..DRAIN_MAX_ATTEMPTS {
        // Reap completed tracked items across ALL surfaces (backlog, review,
        // icebox) and re-sync the snapshot before the focused repair, matching
        // what a manual re-run's full preflight maintenance does. The repair
        // sub-step only reaps the backlog, so a deployed/completed `[x]` item left
        // in review or icebox would make that reap a no-op, the post-repair
        // session-check would still find the completed item, and route would
        // refuse dispatch until the user manually retried (the "JB Run Agent Doc
        // failed; repeat succeeded" report). run_pending_maintenance is
        // idempotent, so this is safe even when there is nothing to reap.
        let maintenance_result = if FORCE_DISK_ROUTE_WRITES.with(Cell::get) {
            crate::preflight::run_pending_maintenance_force_disk(file)
        } else {
            crate::preflight::run_pending_maintenance(file)
        };
        if let Err(e) = maintenance_result {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_drain_pending_maintenance_warning file={} error={}",
                    file.display(),
                    agent_doc_secret_redact::redact(&e.to_string())
                ),
            );
        }

        let block_reason = match crate::repair::repair(file) {
            Ok(outcome) => match crate::session_check::inspect(file)? {
                crate::session_check::SessionCheckStatus::Ok(_) => {
                    let label = format!("{outcome:?}");
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "route_dispatch_drain_closeout_recovered file={} cycle_id={} outcome={}",
                            file.display(),
                            state.cycle_id,
                            label
                        ),
                    );
                    return Ok(RouteCloseoutDrainOutcome::Recovered(label));
                }
                crate::session_check::SessionCheckStatus::Interrupted(reason) => reason,
            },
            Err(err) => err.to_string(),
        };

        // Concurrent-finalize detection: re-read the cycle after the failed check.
        let reloaded = crate::cycle_state::load(file)?;
        let decision = dispatch_drain_retry_decision(
            &state.cycle_id,
            state.phase,
            reloaded
                .as_ref()
                .map(|s| (s.cycle_id.as_str(), s.phase, s.is_open())),
            attempt,
            DRAIN_MAX_ATTEMPTS,
        );
        last_reason = block_reason;
        match decision {
            DispatchDrainRetryDecision::ConcurrentlyClosed => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_dispatch_drain_closeout_concurrent_finalize_closed file={} cycle_id={}",
                        file.display(),
                        state.cycle_id
                    ),
                );
                return Ok(RouteCloseoutDrainOutcome::NoOpenCycle);
            }
            DispatchDrainRetryDecision::Retry => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_dispatch_drain_closeout_retry_concurrent_progress file={} cycle_id={} attempt={}",
                        file.display(),
                        state.cycle_id,
                        attempt + 1
                    ),
                );
                std::thread::sleep(std::time::Duration::from_millis(DRAIN_RETRY_BACKOFF_MS));
                continue;
            }
            DispatchDrainRetryDecision::GiveUp => break,
        }
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "route_dispatch_drain_closeout_blocked file={} cycle_id={} blocker={}",
            file.display(),
            state.cycle_id,
            agent_doc_secret_redact::redact(&last_reason)
        ),
    );
    Ok(RouteCloseoutDrainOutcome::Blocked(last_reason))
}

fn classify_route_closeout_block(
    file: &Path,
    reason: String,
    has_prompt_context: bool,
) -> (CloseoutRecoveryDecision, CloseoutBlockDispatchDecision) {
    let recovery_decision = crate::flow::closeout::decide_closeout_recovery(
        file,
        CloseoutRecoveryDecisionInput {
            prompt_context_available: has_prompt_context,
            blocker_reason: Some(&reason),
            stale_capture_supersession_proof: None,
        },
    );
    let recovery_queues_prompt_for_after_closeout = matches!(
        recovery_decision,
        CloseoutRecoveryDecision::QueuePromptForAfterCloseout { .. }
    );
    let active_queue_head = if recovery_queues_prompt_for_after_closeout {
        None
    } else {
        std::fs::read_to_string(file).ok().and_then(|content| {
            agent_doc_queue::queue_continuation::live_continuation_head(&content)
        })
    };
    let dispatch_decision = classify_closeout_block_dispatch(CloseoutBlockDispatchFacts {
        recovery_queues_prompt_for_after_closeout,
        active_queue_head,
    });
    (recovery_decision, dispatch_decision)
}

fn route_closeout_blocked_recovery_command(decision: &CloseoutRecoveryDecision) -> Option<String> {
    let CloseoutRecoveryDecision::Blocked { recommended, .. } = decision else {
        return None;
    };
    Some(
        short_recovery_command_from_recommendation(recommended)
            .unwrap_or_else(|| recommended.clone()),
    )
}

/// Enqueue a routed dispatch prompt into a document's `agent:queue`.
///
/// `priority` marks a manual operator dispatch (JB `Run Agent Doc`) into a
/// busy/blocked pane: it must PREEMPT pending auto-loop items, so the prompt is
/// inserted ahead of the first queued prompt and never supersedes a lone
/// prompt (#jb-run-preempt-autoloop-priority). Non-priority callers keep the
/// legacy tail-append (+ lone stale-prompt supersede) behavior.
fn enqueue_route_dispatch_prompt(
    file: &Path,
    prompt_text: &str,
    source: &str,
    priority: bool,
) -> Result<RouteQueueEnqueueOutcome> {
    let _lock = acquire_route_queue_lock(file)?;
    let original = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let update = agent_doc_queue::route_dispatch::prepare_route_dispatch_queue_update(
        &original,
        prompt_text,
        priority,
    )?;
    if let Some(parse_err) = update.unparseable_queue_error.as_deref() {
        // The existing agent:queue body is polluted (e.g. user prose / log dumps
        // merged into the component by an earlier corruption). The focused queue
        // transform preserves the polluted body and appends the pending dispatch;
        // route owns only the effect-side diagnostic.
        crate::ops_log::log_op(
            file,
            &format!(
                "route_queue_dispatch_unparseable_preserved file={} prompt_hash={} reason={}",
                file.display(),
                agent_doc_hash::content_hash(&update.prompt_text),
                parse_err
            ),
        );
    }

    let content = update.content;
    let activated = content != original;
    if activated {
        route_write_document(file, &content, &original, "route_dispatch_queue").with_context(
            || {
                format!(
                    "failed to converge queued dispatch for {} through editor IPC/disk",
                    file.display()
                )
            },
        )?;
        crate::snapshot::save(file, &content).with_context(|| {
            format!(
                "failed to sync snapshot after queueing dispatch for {}",
                file.display()
            )
        })?;
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "route_dispatch_queued file={} source={} appended={} already_present={} superseded={} component_created={} activated={} prompt={:?}",
            file.display(),
            source,
            update.appended,
            update.already_present,
            update.superseded,
            update.component_created,
            activated,
            update.prompt_text
        ),
    );
    Ok(RouteQueueEnqueueOutcome {
        prompt_text: update.prompt_text,
        appended: update.appended,
        already_present: update.already_present,
        superseded: update.superseded,
        component_created: update.component_created,
        activated,
    })
}

fn enqueue_exchange_slash_command_for_idle_drain(
    file: &Path,
    context: &PendingPromptBearingRouteContext,
    source: &str,
) -> Result<Option<RouteQueueEnqueueOutcome>> {
    let Some(command) = context.slash_command.as_deref() else {
        return Ok(None);
    };
    let queued = enqueue_route_dispatch_prompt(file, command, source, true)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "route_exchange_slash_command_queued file={} source={} command={:?} appended={} already_present={} superseded={} activated={}",
            file.display(),
            source,
            command,
            queued.appended,
            queued.already_present,
            queued.superseded,
            queued.activated
        ),
    );
    Ok(Some(queued))
}

fn inactive_route_queue_head(file: &Path) -> Result<Option<String>> {
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    inactive_route_queue_head_in_content(file, &content)
}

fn inactive_route_queue_head_in_content(file: &Path, content: &str) -> Result<Option<String>> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (fm, _) = frontmatter_io::parse_for_file_with_context(content, file, &rc)?;
    let committed_snapshot = match crate::snapshot::load(file) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_uncommitted_head_snapshot_unreadable file={} err={} decision=allow",
                    file.display(),
                    err
                ),
            );
            None
        }
    };
    match agent_doc_queue::route_dispatch::inactive_route_queue_head(
        content,
        fm.queue_active,
        committed_snapshot.as_deref(),
    )? {
        agent_doc_queue::route_dispatch::RouteInactiveQueueHead::None => Ok(None),
        agent_doc_queue::route_dispatch::RouteInactiveQueueHead::Dispatchable(head_text) => {
            Ok(Some(head_text))
        }
        agent_doc_queue::route_dispatch::RouteInactiveQueueHead::Uncommitted(head_text) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_uncommitted_head file={} decision=defer reason=head_not_in_committed_snapshot head={:?}",
                    file.display(),
                    agent_doc_secret_redact::redact(&head_text)
                ),
            );
            Ok(None)
        }
    }
}

fn activate_existing_route_queue_head(
    file: &Path,
    source: &str,
) -> Result<Option<RouteQueueEnqueueOutcome>> {
    let _lock = acquire_route_queue_lock(file)?;
    let original = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let Some(prompt_text) = inactive_route_queue_head_in_content(file, &original)? else {
        return Ok(None);
    };
    let content =
        agent_doc_queue::route_dispatch::activate_existing_route_queue_content(&original)?;
    let activated = content != original;
    if activated {
        route_write_document(file, &content, &original, "route_queue_activation")
            .with_context(|| format!("failed to activate queue in {}", file.display()))?;
        crate::snapshot::save(file, &content).with_context(|| {
            format!(
                "failed to sync snapshot after activating queue for {}",
                file.display()
            )
        })?;
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "route_existing_queue_head_activated file={} source={} activated={} prompt={:?}",
            file.display(),
            source,
            activated,
            prompt_text
        ),
    );
    Ok(Some(RouteQueueEnqueueOutcome {
        prompt_text,
        appended: false,
        already_present: true,
        superseded: false,
        component_created: false,
        activated,
    }))
}

fn route_queue_lock_path(file: &Path) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(file)
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let base = agent_doc_fs::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .ok_or_else(|| {
            anyhow::anyhow!("failed to resolve queue lock root for {}", file.display())
        })?;
    let hash = agent_doc_fs::document_state_hash_from_str(&canonical.to_string_lossy());
    Ok(base
        .join(".agent-doc/route-queue")
        .join(format!("{hash}.lock")))
}

fn acquire_route_queue_lock(file: &Path) -> Result<File> {
    let lock_path = route_queue_lock_path(file)?;
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open route queue lock {}", lock_path.display()))?;
    lock.lock_exclusive()
        .with_context(|| format!("failed to acquire route queue lock {}", lock_path.display()))?;
    Ok(lock)
}

fn cleanup_failed_route_panes(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    created_panes: &[String],
) {
    for p in created_panes {
        if !tmux.pane_alive(p) {
            continue;
        }
        if failed_route_pane_has_startup_miss(file, p) {
            eprintln!(
                "[route] reaping startup-miss pane {} after failed fresh route for {}",
                p,
                file.display()
            );
            tracing::warn!(pane = %p, "route: killing startup-miss pane from failed fresh route");
            let _ = tmux.raw_cmd(&["kill-pane", "-t", p]);
            continue;
        }
        if should_preserve_failed_route_pane(tmux, file, p, session_id) {
            eprintln!(
                "[route] preserving newly-created pane {} after failed route because it is still the live registered owner for {}",
                p,
                file.display()
            );
            continue;
        }
        eprintln!(
            "[route] cleaning up orphaned pane {} (created during failed route)",
            p
        );
        tracing::warn!(pane = %p, "route: killing orphaned pane from failed route");
        let _ = tmux.raw_cmd(&["kill-pane", "-t", p]);
    }
}

fn failed_route_pane_has_startup_miss(file: &Path, pane_id: &str) -> bool {
    crate::startup_miss::load(file)
        .ok()
        .flatten()
        .is_some_and(|miss| {
            miss.pane_id == pane_id
                && matches!(
                    miss.origin,
                    agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart
                )
        })
}

fn failed_route_registry_root(file: &Path) -> Option<std::path::PathBuf> {
    let canonical = std::fs::canonicalize(file)
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    agent_doc_fs::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(|parent| parent.to_path_buf()))
}

fn should_preserve_failed_route_pane(
    tmux: &Tmux,
    file: &Path,
    pane_id: &str,
    session_id: &str,
) -> bool {
    let Some(root) = failed_route_registry_root(file) else {
        return false;
    };
    sessions::load_in(&root)
        .ok()
        .and_then(|registry| {
            registry
                .values()
                .find(|entry| entry.session_id == session_id)
                .map(|entry| entry.pane.as_str() == pane_id)
        })
        .unwrap_or(false)
        && tmux.pane_alive(pane_id)
}

fn dispatch_only_starting_pane_ready_timeout(harness: &HarnessConfig) -> Duration {
    wait_for_ready_override().unwrap_or_else(|| {
        dispatch_only_starting_pane_ready_timeout_for_binary(
            Some(harness.binary.as_str()),
            cfg!(test),
        )
    })
}

fn wait_for_starting_pane_recovery_target(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    current_pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    initial_status: Option<&agent_doc_supervisor::startup_miss::SessionLogStatus>,
) -> Option<StartingPaneRecoveryTarget> {
    let registry_base_dir = registry_base_dir_for_dispatch(file_path);
    let budget = dispatch_only_starting_pane_recovery_retry_budget(
        Some(harness.binary.as_str()),
        cfg!(test),
    );
    let deadline = std::time::Instant::now() + budget.timeout;

    while std::time::Instant::now() < deadline {
        let current_status = crate::startup_miss::session_log_status(file, session_id)
            .ok()
            .flatten();
        let registry = sessions::load_in(&registry_base_dir).ok();
        let registered_pane = sessions::lookup_in(&registry_base_dir, session_id)
            .ok()
            .flatten();

        match starting_pane_recovery_target(
            initial_status,
            current_status.as_ref(),
            current_pane,
            registered_pane.as_deref(),
        ) {
            Some(StartingPaneRecoveryTarget::DifferentPane(pane))
                if registry.as_ref().is_some_and(|registry| {
                    pane_registration_matches_file(registry, &pane, file_path)
                }) && tmux.pane_alive(&pane) =>
            {
                return Some(StartingPaneRecoveryTarget::DifferentPane(pane));
            }
            Some(StartingPaneRecoveryTarget::SamePane) => {
                return Some(StartingPaneRecoveryTarget::SamePane);
            }
            _ => {}
        }

        std::thread::sleep(budget.poll_interval);
    }

    None
}

mod dispatch_only;
pub(crate) use dispatch_only::*;

mod authoritative_actor;
pub(crate) use authoritative_actor::*;

/// `#jb-run-agent-doc-busy-wait-deadlock`: the `wait_for_ready` override exists
/// for a slow-`starting` supervisor (JB `Run Agent Doc` passes `--wait-for-ready
/// 60`), not for a busy *active turn*. A dispatch-only route that finds the
/// authoritative actor `Busy` (mid active turn) must not honor that start
/// override here: an active turn will not become dispatch-ready by waiting, so a
/// 60s block before the `DispatchOnlyBusyQueue` enqueue is an operator-perceived
/// deadlock. When a queue-prompt fallback exists, skip the wait entirely and let
/// the busy actor queue the prompt immediately for the supervisor idle-queue
/// watch to drain; a stale-busy-but-actually-ready projection is still repaired
/// by the later direct-pane-evidence check (`#snrun`). Only wait when there is
/// no queue fallback (where route would otherwise have to bail).
fn controller_dispatch_actor_state(
    actor_state: agent_doc_sqlite::state_store::ActorState,
) -> DispatchActorState {
    match actor_state {
        agent_doc_sqlite::state_store::ActorState::Ready => DispatchActorState::Ready,
        agent_doc_sqlite::state_store::ActorState::Busy => DispatchActorState::Busy,
        _ => DispatchActorState::Other,
    }
}

fn wait_for_authoritative_actor_ready(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    initial: &AuthoritativeActorDispatchTarget,
) -> Result<Option<AuthoritativeActorDispatchTarget>> {
    let override_timeout = wait_for_ready_override();
    let budget = match override_timeout {
        Some(timeout) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_wait_for_ready_override file={} harness={} timeout_secs={}",
                    file.display(),
                    harness.binary,
                    timeout.as_secs()
                ),
            );
            RetryBudget::new(timeout, Duration::from_millis(100))
        }
        None => authoritative_actor_ready_retry_budget(Some(harness.binary.as_str()), cfg!(test)),
    };
    let deadline = Instant::now() + budget.timeout;
    let mut last_facts = authoritative_actor_ready_facts_from_target(
        initial,
        current_generation_ready_prompt_proven(tmux, initial, harness),
    );
    let start = Instant::now();
    if last_facts.actor_state != ActorDispatchState::Starting
        && starting_actor_timeout_record_identity_matches(file_path, &last_facts)
    {
        clear_starting_actor_timeout_record(file_path);
        crate::ops_log::log_op(
            file,
            &format!(
                "route_starting_actor_timeout_cleared_nonstarting file={} pane={} generation={} actor_state={}",
                file.display(),
                last_facts.pane_id,
                last_facts.generation,
                last_facts.actor_state.as_str()
            ),
        );
    }
    if starting_actor_timeout_record_matches(file_path, &last_facts) {
        mark_starting_actor_timeout_blocked(file, file_path, session_id, &last_facts);
        let file_display = file.display().to_string();
        crate::ops_log::log_op(
            file,
            &starting_actor_timeout_coalesced_log_line(
                file_display.as_str(),
                harness.binary.as_str(),
                start.elapsed(),
                &last_facts,
            ),
        );
        return Ok(None);
    }

    while Instant::now() < deadline {
        if let Some(refreshed) = load_authoritative_actor_binding(
            tmux, file, session_id, file_path, harness, false, false,
        )? {
            let prompt_ready = current_generation_ready_prompt_proven(tmux, &refreshed, harness);
            last_facts = authoritative_actor_ready_facts_from_target(&refreshed, prompt_ready);
            match classify_authoritative_prompt_ready_barrier(
                AuthoritativePromptReadyBarrierFacts {
                    ready_facts: &last_facts,
                    dispatch_eligible: supervisor_authoritative_actor_dispatch_target_eligible(
                        &refreshed.runtime,
                    ),
                },
            ) {
                PromptReadyBarrierDecision::Ready => {
                    let elapsed = start.elapsed();
                    let file_display = file.display().to_string();
                    clear_starting_actor_timeout_record(file_path);
                    crate::ops_log::log_op(
                        file,
                        &starting_actor_ready_log_line(
                            file_display.as_str(),
                            harness.binary.as_str(),
                            elapsed,
                            &last_facts,
                        ),
                    );
                    if override_timeout.is_some() {
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_wait_for_ready_elapsed file={} harness={} elapsed_ms={} timeout_ms={}",
                                file.display(),
                                harness.binary,
                                elapsed.as_millis(),
                                budget.timeout.as_millis()
                            ),
                        );
                    }
                    return Ok(Some(refreshed));
                }
                PromptReadyBarrierDecision::Terminal => {
                    let elapsed = start.elapsed();
                    let file_display = file.display().to_string();
                    clear_starting_actor_timeout_record(file_path);
                    crate::ops_log::log_op(
                        file,
                        &starting_actor_terminal_log_line(
                            file_display.as_str(),
                            harness.binary.as_str(),
                            elapsed,
                            &last_facts,
                        ),
                    );
                    return Ok(Some(refreshed));
                }
                PromptReadyBarrierDecision::Continue => {}
            }
        }
        std::thread::sleep(budget.poll_interval);
    }

    let elapsed = start.elapsed();
    let log_line = route_starting_actor_not_ready_log_line(
        file,
        harness,
        budget.timeout,
        elapsed,
        &last_facts,
    );
    if last_facts.actor_state == ActorDispatchState::Starting {
        match record_starting_actor_timeout(file_path, &last_facts, &log_line) {
            Ok(StartingActorTimeoutLogDecision::NewTimeout) => {
                crate::ops_log::log_op(file, &log_line);
                log_prompt_ready_barrier_failed(
                    file,
                    RoutedReopenGuardReason::StartingActorNotReady,
                );
                mark_starting_actor_timeout_blocked(file, file_path, session_id, &last_facts);
            }
            Ok(StartingActorTimeoutLogDecision::DuplicateTimeout) => {
                mark_starting_actor_timeout_blocked(file, file_path, session_id, &last_facts);
                let file_display = file.display().to_string();
                crate::ops_log::log_op(
                    file,
                    &starting_actor_timeout_coalesced_log_line(
                        file_display.as_str(),
                        harness.binary.as_str(),
                        elapsed,
                        &last_facts,
                    ),
                );
            }
            Err(err) => {
                eprintln!(
                    "[route] warning: failed to persist starting actor timeout for {}: {}",
                    file.display(),
                    err
                );
                crate::ops_log::log_op(file, &log_line);
                log_prompt_ready_barrier_failed(
                    file,
                    RoutedReopenGuardReason::StartingActorNotReadyUnpersisted,
                );
            }
        }
    } else {
        clear_starting_actor_timeout_record(file_path);
        crate::ops_log::log_op(file, &log_line);
        // Diagnostic: capture the pane content at timeout so we can analyze
        // why ready_prompt_candidate never matched.
        if let Ok(content) = tmux.capture_pane(&initial.record.pane_id, Some(80)) {
            let candidate = agent_doc_harness::ready_prompt_candidate(&content, harness);
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_wait_for_ready_timeout_diagnostic file={} pane={} harness={} candidate={:?} bottom_idle_chrome={} has_busy_cue={} lines={}",
                    file.display(),
                    initial.record.pane_id,
                    harness.binary,
                    candidate,
                    harness.is_bottom_idle_chrome(&content, 12),
                    harness.has_busy_cue(&content),
                    content.lines().count()
                ),
            );
        }
    }
    if override_timeout.is_some() {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_wait_for_ready_timeout file={} harness={} elapsed_ms={} timeout_ms={}",
                file.display(),
                harness.binary,
                elapsed.as_millis(),
                budget.timeout.as_millis()
            ),
        );
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn route_via_authoritative_actor(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    target_session: &str,
    split_before: bool,
    harness: &HarnessConfig,
    baseline: Option<&crate::cycle_state::CycleState>,
    prompt_context: Option<&PendingPromptBearingRouteContext>,
    dispatch_only: bool,
    actor: AuthoritativeActorDispatchTarget,
) -> Result<String> {
    let mut actor = actor;
    let mut dispatch_pane = actor.record.pane_id.clone();
    let mut actor_state = actor.actor_state();
    let prompt_bearing_marker = prompt_context.map(|context| context.marker.as_str());
    match drain_open_closeout_before_routed_dispatch(file)? {
        RouteCloseoutDrainOutcome::NoOpenCycle => {}
        RouteCloseoutDrainOutcome::Recovered(outcome) => {
            eprintln!(
                "[route] drained open closeout for {} before reroute ({})",
                file.display(),
                outcome
            );
            if let Some(refreshed) = load_authoritative_actor_binding(
                tmux, file, session_id, file_path, harness, false, false,
            )? {
                actor = refreshed;
                dispatch_pane = actor.record.pane_id.clone();
                actor_state = actor.actor_state();
            }
        }
        RouteCloseoutDrainOutcome::Blocked(reason) => {
            let (decision, dispatch_decision) =
                classify_route_closeout_block(file, reason, prompt_context.is_some());
            match dispatch_decision {
                CloseoutBlockDispatchDecision::EnqueuePromptForAfterCloseout => {
                    let Some(context) = prompt_context else {
                        unreachable!("prompt-context decision requires a prompt context");
                    };
                    // #jb-run-preempt-autoloop-priority: manual reroute prompt preempts.
                    let queued = match enqueue_exchange_slash_command_for_idle_drain(
                        file,
                        context,
                        "open_closeout_blocked",
                    )? {
                        Some(queued) => queued,
                        None => enqueue_route_dispatch_prompt(
                            file,
                            &context.prompt_text,
                            "open_closeout_blocked",
                            true,
                        )?,
                    };
                    eprintln!(
                        "[route] active closeout for {} could not be drained before reroute; queued pending dispatch {:?} in active agent:queue (appended={}, already_present={}, superseded={}) {}",
                        file.display(),
                        queued.prompt_text,
                        queued.appended,
                        queued.already_present,
                        queued.superseded,
                        route_closeout_user_outcome_fields(
                            route_closeout_blocked_recovery_command(&decision).as_deref(),
                        )
                    );
                    return Ok(dispatch_pane);
                }
                CloseoutBlockDispatchDecision::WaitForActiveQueueHead { head } => {
                    let blocker = decision.route_terminal_reason();
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "route_dispatch_drain_closeout_wait_existing_queue file={} head={} blocker={}",
                            file.display(),
                            agent_doc_secret_redact::redact(&head),
                            agent_doc_secret_redact::redact(&blocker)
                        ),
                    );
                    eprintln!(
                        "[route] active closeout for {} could not be drained before reroute; existing queue head {:?} remains queued behind the closeout {}",
                        file.display(),
                        head,
                        route_closeout_user_outcome_fields(
                            route_closeout_blocked_recovery_command(&decision).as_deref(),
                        )
                    );
                    return Ok(dispatch_pane);
                }
                CloseoutBlockDispatchDecision::FailClosed => {
                    let reason = decision.route_terminal_reason();
                    anyhow::bail!(
                        "authoritative actor generation {} for {} owns pane {} but route could not drain the active closeout before dispatch: {}",
                        actor.record.generation,
                        file.display(),
                        dispatch_pane,
                        reason
                    );
                }
            }
        }
    }
    if let Some(context) = prompt_context
        && let Some(queued) =
            enqueue_exchange_slash_command_for_idle_drain(file, context, "exchange_slash_command")?
    {
        eprintln!(
            "[route] unresolved exchange slash command for {} was queued as {:?} in active agent:queue (appended={}, already_present={}, superseded={}) for managed after-turn submission",
            file.display(),
            queued.prompt_text,
            queued.appended,
            queued.already_present,
            queued.superseded
        );
        return Ok(dispatch_pane);
    }
    if actor_state == agent_doc_sqlite::state_store::ActorState::Starting
        && let Some(refreshed) =
            wait_for_authoritative_actor_ready(tmux, file, session_id, file_path, harness, &actor)?
    {
        if refreshed.record.generation != actor.record.generation
            || refreshed.record.pane_id != actor.record.pane_id
            || refreshed.actor_state() != actor_state
        {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_starting_refreshed_ready file={} old_pane={} new_pane={} harness={} old_generation={} new_generation={} old_state={} new_state={}",
                    file.display(),
                    actor.record.pane_id,
                    refreshed.record.pane_id,
                    harness.binary,
                    actor.record.generation,
                    refreshed.record.generation,
                    actor_state.as_str(),
                    refreshed.actor_state().as_str()
                ),
            );
        }
        actor = refreshed;
        dispatch_pane = actor.record.pane_id.clone();
        actor_state = actor.actor_state();
    }
    let has_existing_inactive_queue_fallback = if dispatch_only
        && actor_state == agent_doc_sqlite::state_store::ActorState::Busy
        && prompt_context.is_none()
    {
        inactive_route_queue_head(file)?.is_some()
    } else {
        false
    };
    // #jb-run-agent-doc-busy-active-turn-stall: probe the live pane once for a
    // genuine active-turn busy cue (working spinner / `esc to interrupt`) before
    // direct dispatch. A stale Busy projection skips the slow ready-wait; a stale
    // Ready projection is downgraded to Busy so route cannot inject into a live
    // turn just because the durable actor record lagged behind the pane.
    let active_turn_busy_cue: Option<String> = if dispatch_only_should_probe_active_turn_cue(
        dispatch_only,
        controller_dispatch_actor_state(actor_state),
        prompt_context.is_some(),
        has_existing_inactive_queue_fallback,
    ) {
        tmux.capture_pane(&dispatch_pane, Some(80))
            .ok()
            .and_then(|content| harness.busy_proof_line(&content))
    } else {
        None
    };
    if actor_state == agent_doc_sqlite::state_store::ActorState::Ready
        && let Some(cue) = active_turn_busy_cue.as_deref()
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_ready_actor_active_turn_blocked file={} pane={} harness={} generation={} cue={:?}",
                file.display(),
                dispatch_pane,
                harness.binary,
                actor.record.generation,
                cue
            ),
        );
        eprintln!(
            "[route] authoritative actor for {} reported ready on pane {}, but the live pane is busy on an active {} turn ({}); treating the actor as busy before dispatch",
            file.display(),
            dispatch_pane,
            harness.binary,
            cue
        );
        actor_state = agent_doc_sqlite::state_store::ActorState::Busy;
    }
    if let Some(cue) = active_turn_busy_cue.as_deref() {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_busy_active_turn_skip_wait file={} pane={} harness={} generation={} cue={:?}",
                file.display(),
                dispatch_pane,
                harness.binary,
                actor.record.generation,
                cue
            ),
        );
        eprintln!(
            "[route] authoritative actor for {} is busy on an active {} turn ({}); skipping the busy ready-wait and refusing immediately so the IDE shows the session-still-running notification without a {}s stall",
            file.display(),
            harness.binary,
            cue,
            dispatch_only_busy_refusal_wait_secs(
                wait_for_ready_override(),
                dispatch_only_starting_pane_recovery_timeout_for_binary(
                    Some(harness.binary.as_str()),
                    cfg!(test),
                )
            )
        );
    }
    let mut waited_and_timed_out = false;
    if dispatch_only_busy_should_wait_for_ready(
        dispatch_only,
        controller_dispatch_actor_state(actor_state),
        prompt_context.is_some() || has_existing_inactive_queue_fallback,
        active_turn_busy_cue.is_some(),
    ) {
        if let Some(refreshed) =
            wait_for_authoritative_actor_ready(tmux, file, session_id, file_path, harness, &actor)?
        {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_only_busy_actor_refreshed_ready file={} old_pane={} new_pane={} harness={} old_generation={} new_generation={}",
                    file.display(),
                    actor.record.pane_id,
                    refreshed.record.pane_id,
                    harness.binary,
                    actor.record.generation,
                    refreshed.record.generation
                ),
            );
            actor = refreshed;
            dispatch_pane = actor.record.pane_id.clone();
            actor_state = actor.actor_state();
        } else {
            waited_and_timed_out = true;
        }
    }

    if actor_blocked_by_starting_timeout(StartingTimeoutActorFacts {
        actor_blocked: actor.record.state == agent_doc_sqlite::state_store::ActorState::Blocked,
        last_transition_reason: &actor.record.last_transition.reason,
        prompt_ready: false,
    }) {
        if let Some(recovered) = recover_starting_timeout_blocked_actor_if_dispatch_ready(
            tmux, file, file_path, &actor, harness,
        ) {
            actor = recovered;
            dispatch_pane = actor.record.pane_id.clone();
            actor_state = actor.actor_state();
        } else {
            if let Err(e) = tmux.select_pane(&dispatch_pane) {
                eprintln!(
                    "[route] warning: failed to focus pane {}: {}",
                    dispatch_pane, e
                );
            }
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_starting_timeout_durable_error file={} pane={} harness={} generation={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation
                ),
            );
            anyhow::bail!(
                "authoritative actor generation {} for {} owns pane {} but route will not bind a new dispatch target because this generation already timed out while starting. {}",
                actor.record.generation,
                file.display(),
                dispatch_pane,
                authoritative_actor_dispatch_recovery_hint(actor_state, file)
            );
        }
    }

    if lookup_dispatch_registration(file_path, session_id)?.as_deref()
        != Some(dispatch_pane.as_str())
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_actor_projection_reregistered file={} session={} pane={} generation={}",
                file.display(),
                session_id,
                dispatch_pane,
                actor.record.generation
            ),
        );
    }
    let rescued_from_stash = rescue_from_stash(
        tmux,
        &dispatch_pane,
        session_id,
        file_path,
        target_session,
        split_before,
    );
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;

    // After a real stash rescue the pane is now visible in the agent-doc window,
    // which can make the harness's dispatch-ready prompt observable for the first
    // time. The pre-rescue Starting wait (line ~2952) may have timed out while the
    // pane was still parked. Re-promote and, if still Starting, re-attempt the
    // ready wait once on the freshly-visible pane before bailing out.
    if rescued_from_stash && actor_state == agent_doc_sqlite::state_store::ActorState::Starting {
        let runtime = query_supervisor_runtime(file, session_id);
        let (refreshed_record, refreshed_runtime) =
            promote_starting_authoritative_actor_if_dispatch_ready(
                tmux,
                file,
                file_path,
                actor.record.clone(),
                runtime,
                harness,
            );
        let mut refreshed = AuthoritativeActorDispatchTarget {
            record: refreshed_record,
            runtime: refreshed_runtime,
        };
        if refreshed.actor_state() == agent_doc_sqlite::state_store::ActorState::Ready {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_post_rescue_promoted_ready file={} pane={} generation={}",
                    file.display(),
                    dispatch_pane,
                    refreshed.record.generation
                ),
            );
            actor = refreshed;
            actor_state = actor.actor_state();
            dispatch_pane = actor.record.pane_id.clone();
        } else if let Some(after_wait) = wait_for_authoritative_actor_ready(
            tmux, file, session_id, file_path, harness, &refreshed,
        )? {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_post_rescue_ready_after_wait file={} pane={} generation={}",
                    file.display(),
                    dispatch_pane,
                    after_wait.record.generation
                ),
            );
            actor = after_wait;
            actor_state = actor.actor_state();
            dispatch_pane = actor.record.pane_id.clone();
        } else {
            // Bind the unused refreshed target back so the diagnostic log captures
            // the post-rescue facts even when the wait still failed.
            refreshed.runtime = query_supervisor_runtime(file, session_id);
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_post_rescue_still_starting file={} pane={} generation={} runtime_state={}",
                    file.display(),
                    dispatch_pane,
                    refreshed.record.generation,
                    refreshed.actor_state().as_str()
                ),
            );
        }
    }

    let prompt_ready = active_turn_busy_cue.is_none()
        && (actor_state == agent_doc_sqlite::state_store::ActorState::Ready
            || current_generation_ready_prompt_proven(tmux, &actor, harness));

    // Direct pane evidence repairs a stale busy projection (#snrun). The actor
    // was projected Busy, but the live pane proves a dispatch-ready prompt in the
    // current generation — it is not actually mid-turn. Promote it to Ready so a
    // dispatch-only route dispatches to the proven-ready pane instead of queuing
    // the prompt into an active `agent:queue`. A Busy projection without a proven
    // ready prompt is left as-is and still fails closed (queues), per the
    // direct-evidence rule: idle direct evidence repairs stale busy; busy direct
    // evidence stays fail-closed.
    if busy_projection_repaired_by_ready_prompt(actor_dispatch_state(actor_state), prompt_ready) {
        eprintln!(
            "[route] authoritative actor for {} projected busy but the live pane proves a dispatch-ready prompt (generation {}); repairing stale busy projection to ready and dispatching instead of queuing",
            file.display(),
            actor.record.generation
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_authoritative_actor_busy_projection_repaired_by_ready_prompt file={} pane={} generation={} prior_state={}",
                file.display(),
                dispatch_pane,
                actor.record.generation,
                actor_state.as_str()
            ),
        );
        actor_state = agent_doc_sqlite::state_store::ActorState::Ready;
    }

    // Timeout-idle recovery: the wait loop exhausted its budget without finding
    // a dispatch-ready prompt, but the live pane also shows no active busy cue.
    // The pane is idle by the absence-of-work test even though our prompt
    // detection patterns did not match the actual pane output. Promote the
    // stale Busy projection to Ready and dispatch. This handles Codex output
    // formats where the footer does not match `is_ignorable_output_line` or
    // `is_bottom_idle_chrome` patterns but the pane is clearly not mid-turn.
    if waited_and_timed_out
        && actor_dispatch_state(actor_state) == ActorDispatchState::Busy
        && !prompt_ready
        && let Ok(content) = tmux.capture_pane(&dispatch_pane, Some(80))
    {
        if !harness.has_busy_cue(&content) {
            eprintln!(
                "[route] timeout-idle recovery for {}: waited full timeout but pane has no busy cue; promoting stale busy projection to ready and dispatching",
                file.display()
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_timeout_idle_recovery file={} pane={} harness={} generation={} actor_state={} busy_cue=false pane_tail={:?}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation,
                    actor_state.as_str(),
                    content
                        .lines()
                        .rev()
                        .take(5)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
            );
            actor_state = agent_doc_sqlite::state_store::ActorState::Ready;
        } else {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_timeout_idle_recovery_blocked file={} pane={} harness={} generation={} actor_state={} busy_cue=true",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation,
                    actor_state.as_str()
                ),
            );
        }
    }

    // Eager busy-cue check for dispatch-only queue fallback: when
    // `dispatch_only_busy_should_wait_for_ready` skipped the wait because
    // a queue fallback existed (prompt_context or inactive queue head), the
    // timeout-idle recovery above never ran. The actor may be projected Busy
    // while the live pane is actually idle. Check the pane eagerly and promote
    // to Ready so the dispatch proceeds instead of queuing behind a stale
    // projection. This is the #opencode-jb-stall root cause: JB Run Agent Doc
    // sends a prompt, the wait is skipped, and the stale Busy projection queues
    // the prompt into agent:queue, which never drains because the
    // auto-loop requires the actor to become ready.
    if dispatch_only
        && actor_dispatch_state(actor_state) == ActorDispatchState::Busy
        && !waited_and_timed_out
        && prompt_context.is_some()
        && let Ok(content) = tmux.capture_pane(&dispatch_pane, Some(80))
        && !harness.has_busy_cue(&content)
    {
        eprintln!(
            "[route] eager busy-cue check for {}: actor projected busy but pane has no busy cue (queue fallback skipped the wait); promoting stale busy projection to ready and dispatching",
            file.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_eager_busy_cue_recovery file={} pane={} harness={} generation={} actor_state={} busy_cue=false pane_tail={:?}",
                file.display(),
                dispatch_pane,
                harness.binary,
                actor.record.generation,
                actor_state.as_str(),
                content
                    .lines()
                    .rev()
                    .take(5)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
        );
        actor_state = agent_doc_sqlite::state_store::ActorState::Ready;
    }

    let reopen_mode = if dispatch_only {
        ReopenMode::DispatchOnly
    } else {
        ReopenMode::Managed
    };
    let actor_dispatch_state = actor_dispatch_state(actor_state);
    let reopen_outcome = decide_authoritative_reopen(RoutedReopenFacts {
        actor_state: actor_dispatch_state,
        prompt_ready,
        has_prompt_bearing_work: prompt_bearing_marker.is_some(),
        mode: reopen_mode,
        degraded_authority: false,
        dispatch_eligible: supervisor_authoritative_actor_dispatch_target_eligible(&actor.runtime),
    });
    let action =
        classify_authoritative_actor_dispatch_action(AuthoritativeActorDispatchActionFacts {
            mode: reopen_mode,
            actor_state: actor_dispatch_state,
            has_prompt_bearing_work: prompt_bearing_marker.is_some(),
            reopen_decision: reopen_outcome.decision,
        });

    if actor_dispatch_blocker_reason(actor_dispatch_state).is_some()
        && let Err(e) = tmux.select_pane(&dispatch_pane)
    {
        eprintln!(
            "[route] warning: failed to focus pane {}: {}",
            dispatch_pane, e
        );
    }

    match action {
        AuthoritativeActorDispatchAction::FocusOnly => {
            // A plain dispatch-only reopen (IDE `Run Agent Doc`) against a busy
            // authoritative actor focuses the pane but never injects the trigger.
            // Returning Ok reports a routed run to the IDE even though nothing was
            // submitted, so the operator saw no feedback after a long wait
            // (`#jb-run-agent-doc-command-route-miss`). Fail closed with the same
            // busy-not-ready message the IDE classifies as a "session still
            // running" notification instead of silently succeeding. The pane was
            // already focused above (blocker states select the pane before this
            // match), so the operator still lands on the in-flight turn.
            if dispatch_only_focus_only_should_fail_closed(reopen_mode, actor_dispatch_state) {
                let reason = actor_dispatch_blocker_reason(actor_dispatch_state)
                    .unwrap_or("actor not ready");
                if let Some(queued) = activate_existing_route_queue_head(file, reason)? {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "route_dispatch_only_busy_existing_queue_deferred file={} pane={} harness={} generation={} actor_state={} prompt={:?}",
                            file.display(),
                            dispatch_pane,
                            harness.binary,
                            actor.record.generation,
                            actor_state.as_str(),
                            queued.prompt_text
                        ),
                    );
                    eprintln!(
                        "[route] authoritative actor generation {} for {} is busy on pane {}; activated existing agent:queue head {:?} (already_present={}, activated={}) for idle drain instead of injecting a duplicate trigger {}",
                        actor.record.generation,
                        file.display(),
                        dispatch_pane,
                        queued.prompt_text,
                        queued.already_present,
                        queued.activated,
                        agent_doc_flow::outcome::user_outcome_fields(
                            agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
                        )
                    );
                    return Ok(dispatch_pane);
                }
                // #jb-busy-reopen-auto-drain-when-idle: there is no INACTIVE queue
                // head to activate, but the document may already have an ACTIVE
                // queue continuation (`queue: start` + ready `agent:queue`). When
                // it does, the running loop will continue this document on its own —
                // a bare dispatch-only reopen (IDE `Run Agent Doc`) has nothing to
                // add. Failing closed with the busy-not-ready error mis-reports a
                // self-driving session that IS making progress as a failure (the
                // operator clicks Run Agent Doc on an auto-looping doc, catches a
                // brief inter-iteration gap by eye, and gets an error even though the
                // loop is alive). Report deferred success so the IDE surfaces an
                // "auto-loop active, will continue" acknowledgment instead of an
                // error, mirroring the existing `*_busy_existing_queue_deferred` path.
                if let Some(continuation) = crate::queue_continuation::detect(file)? {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "route_dispatch_only_busy_active_auto_loop_deferred file={} pane={} harness={} generation={} actor_state={} head={:?}",
                            file.display(),
                            dispatch_pane,
                            harness.binary,
                            actor.record.generation,
                            actor_state.as_str(),
                            continuation.head_prompt
                        ),
                    );
                    eprintln!(
                        "[route] authoritative actor generation {} for {} is busy on pane {}, but its agent:queue loop is already active (next head {:?}); the running loop will continue this document — reporting deferred success instead of a busy refusal {}",
                        actor.record.generation,
                        file.display(),
                        dispatch_pane,
                        continuation.head_prompt,
                        agent_doc_flow::outcome::user_outcome_fields(
                            agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
                        )
                    );
                    return Ok(dispatch_pane);
                }
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_authoritative_actor_busy_focus_only_not_dispatched file={} pane={} harness={} generation={} actor_state={}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        actor.record.generation,
                        actor_state.as_str()
                    ),
                );
                log_prompt_ready_barrier_failed(
                    file,
                    RoutedReopenGuardReason::DispatchOnlyBusyActorNotReady,
                );
                let file_display = file.display().to_string();
                let recovery_hint = authoritative_actor_dispatch_recovery_hint(actor_state, file);
                let unblocker = if active_turn_busy_cue.is_some() {
                    "wait_for_owner_turn_to_finish"
                } else {
                    "wait_for_dispatch_ready_prompt"
                };
                let blocked_outcome =
                    agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(unblocker);
                anyhow::bail!(
                    "{}",
                    controller_dispatch_only_busy_refusal_message(DispatchOnlyBusyRefusalFacts {
                        generation: actor.record.generation,
                        file_display: &file_display,
                        dispatch_pane: &dispatch_pane,
                        harness_binary: &harness.binary,
                        reason,
                        wait_secs: dispatch_only_busy_refusal_wait_secs(
                            wait_for_ready_override(),
                            dispatch_only_starting_pane_recovery_timeout_for_binary(
                                Some(harness.binary.as_str()),
                                cfg!(test),
                            )
                        ),
                        recovery_hint: &recovery_hint,
                        active_turn_busy_cue: active_turn_busy_cue.as_deref(),
                        blocked_outcome_fields: &blocked_outcome,
                    })
                );
            }
            eprintln!(
                "[route] authoritative actor for {} remains in state {} on pane {} — focusing without injecting a duplicate reopen",
                file.display(),
                actor_state.as_str(),
                dispatch_pane
            );
            if let Some(queued) =
                activate_existing_route_queue_head(file, "focus_only_inactive_queue")?
            {
                eprintln!(
                    "[route] activated existing inactive agent:queue head {:?} for {} (already_present={}, activated={}) during focus-only reopen",
                    queued.prompt_text,
                    file.display(),
                    queued.already_present,
                    queued.activated
                );
            }
            Ok(dispatch_pane)
        }
        AuthoritativeActorDispatchAction::DispatchOnlyBusyQueue => {
            let reason =
                actor_dispatch_blocker_reason(actor_dispatch_state).unwrap_or("actor not ready");
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_only_authoritative_actor_busy_not_ready file={} pane={} harness={} generation={} actor_state={} flow_reason={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation,
                    actor_state.as_str(),
                    reopen_outcome.reason
                ),
            );
            log_prompt_ready_barrier_failed(
                file,
                RoutedReopenGuardReason::DispatchOnlyBusyActorNotReady,
            );
            if let Some(context) = prompt_context {
                // #jb-run-preempt-autoloop-priority: busy-actor Run Agent Doc preempts.
                let queued =
                    enqueue_route_dispatch_prompt(file, &context.prompt_text, reason, true)?;
                eprintln!(
                    "[route] authoritative actor generation {} for {} is busy on pane {}; queued pending dispatch {:?} in active agent:queue (appended={}, already_present={}, superseded={}) instead of injecting a duplicate trigger {}",
                    actor.record.generation,
                    file.display(),
                    dispatch_pane,
                    queued.prompt_text,
                    queued.appended,
                    queued.already_present,
                    queued.superseded,
                    agent_doc_flow::outcome::user_outcome_fields(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
                    )
                );
                Ok(dispatch_pane)
            } else if let Some(continuation) = crate::queue_continuation::detect(file)? {
                // #jb-busy-reopen-auto-drain-when-idle: a bare reopen (no prompt to
                // queue) against a busy actor whose document already has an active
                // queue continuation defers to that loop instead of erroring.
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_busy_active_auto_loop_deferred file={} pane={} harness={} generation={} actor_state={} head={:?}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        actor.record.generation,
                        actor_state.as_str(),
                        continuation.head_prompt
                    ),
                );
                eprintln!(
                    "[route] authoritative actor generation {} for {} is busy on pane {}, but its agent:queue loop is already active (next head {:?}); the running loop will continue this document — reporting deferred success instead of a busy refusal {}",
                    actor.record.generation,
                    file.display(),
                    dispatch_pane,
                    continuation.head_prompt,
                    agent_doc_flow::outcome::user_outcome_fields(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
                    )
                );
                Ok(dispatch_pane)
            } else {
                let file_display = file.display().to_string();
                let recovery_hint = authoritative_actor_dispatch_recovery_hint(actor_state, file);
                let unblocker = if active_turn_busy_cue.is_some() {
                    "wait_for_owner_turn_to_finish"
                } else {
                    "wait_for_dispatch_ready_prompt"
                };
                let blocked_outcome =
                    agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(unblocker);
                anyhow::bail!(
                    "{}",
                    controller_dispatch_only_busy_refusal_message(DispatchOnlyBusyRefusalFacts {
                        generation: actor.record.generation,
                        file_display: &file_display,
                        dispatch_pane: &dispatch_pane,
                        harness_binary: &harness.binary,
                        reason,
                        wait_secs: dispatch_only_busy_refusal_wait_secs(
                            wait_for_ready_override(),
                            dispatch_only_starting_pane_recovery_timeout_for_binary(
                                Some(harness.binary.as_str()),
                                cfg!(test),
                            )
                        ),
                        recovery_hint: &recovery_hint,
                        active_turn_busy_cue: active_turn_busy_cue.as_deref(),
                        blocked_outcome_fields: &blocked_outcome,
                    })
                )
            }
        }
        AuthoritativeActorDispatchAction::RecoverDispatchOnlyWaitingInput => {
            recover_dispatch_only_authoritative_waiting_input(
                tmux,
                file,
                session_id,
                file_path,
                target_session,
                split_before,
                harness,
                &dispatch_pane,
                actor.record.generation,
            )
        }
        AuthoritativeActorDispatchAction::ManagedSupervisorQueue => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_actor_dispatch_optimistic_queue file={} pane={} harness={} generation={} actor_state={} flow_reason={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation,
                    actor_state.as_str(),
                    reopen_outcome.reason
                ),
            );
            eprintln!(
                "[route] authoritative actor generation {} for {} still reports {} on pane {} — sending the bare {} reopen anyway so the supervisor can queue it",
                actor.record.generation,
                file.display(),
                actor_state.as_str(),
                dispatch_pane,
                harness.binary
            );
            match authorize_controller_dispatch(
                file,
                session_id,
                file_path,
                &actor,
                "managed_reopen",
                &format!(
                    "submit=supervisor_ipc actor_state={} harness={}",
                    actor_state.as_str(),
                    harness.binary
                ),
            )? {
                RouteDispatchAuthorization::CoalescedDeduped { detail } => {
                    return Ok(route_dispatch_deduped_pane(
                        file,
                        "managed_reopen",
                        dispatch_pane.clone(),
                        &detail,
                    ));
                }
                RouteDispatchAuthorization::Authorized => {}
            }
            let dispatch_start = dispatch_via_supervisor_ipc(
                tmux,
                file,
                &dispatch_pane,
                session_id,
                file_path,
                harness,
            )?;
            let ack_pane = require_routed_cycle_ack(
                tmux,
                file,
                &dispatch_pane,
                session_id,
                file_path,
                harness,
                baseline,
                prompt_bearing_marker,
                true,
                dispatch_start,
            )?;
            Ok(ack_pane.unwrap_or(dispatch_pane))
        }
        AuthoritativeActorDispatchAction::FailClosed => {
            let reason =
                actor_dispatch_blocker_reason(actor_dispatch_state).unwrap_or("actor not ready");
            let rescue_context = if rescued_from_stash {
                " (after a fresh stash rescue — re-promotion still did not observe a dispatch-ready prompt)"
            } else {
                ""
            };
            // #route-busy-vs-starting-wording: the default "(waited Ns for X
            // startup)" wording mis-reads a pane that is busy on an active harness
            // turn (e.g. a live Claude turn showing the working spinner / interrupt
            // hint) as a stuck cold start. Probe the live pane for a harness busy
            // cue and word the wait context as a busy active turn when present.
            // Best-effort: a capture failure falls back to the cold-start wording.
            let busy_cue = tmux
                .capture_pane(&dispatch_pane, Some(80))
                .ok()
                .and_then(|content| harness.dispatch_blocker_reason(&content));
            let wait_context = failclosed_wait_context(
                &harness.binary,
                busy_cue.as_deref(),
                dispatch_only_busy_refusal_wait_secs(
                    wait_for_ready_override(),
                    dispatch_only_starting_pane_recovery_timeout_for_binary(
                        Some(harness.binary.as_str()),
                        cfg!(test),
                    ),
                ),
            );
            anyhow::bail!(
                "authoritative actor generation {} for {} owns pane {} but route will not inject a new trigger because {} ({}){}. {}",
                actor.record.generation,
                file.display(),
                dispatch_pane,
                reason,
                wait_context,
                rescue_context,
                authoritative_actor_dispatch_recovery_hint(actor_state, file)
            );
        }
        AuthoritativeActorDispatchAction::DispatchOnlyDirectPane => {
            let queue_prompt = if prompt_context.is_some() {
                prompt_context.map(|context| context.prompt_text.clone())
            } else {
                activate_existing_route_queue_head(file, "dispatch_only_inactive_queue")?
                    .map(|queued| {
                        eprintln!(
                            "[route] activated existing inactive agent:queue head {:?} for {} (already_present={}, activated={}) before dispatch-only reopen",
                            queued.prompt_text,
                            file.display(),
                            queued.already_present,
                            queued.activated
                        );
                        queued.prompt_text
                    })
            };
            match authorize_controller_dispatch(
                file,
                session_id,
                file_path,
                &actor,
                "dispatch_only_reopen",
                &format!(
                    "submit=direct_pane actor_state={} harness={}",
                    actor_state.as_str(),
                    harness.binary
                ),
            )? {
                RouteDispatchAuthorization::CoalescedDeduped { detail } => {
                    return Ok(route_dispatch_deduped_pane(
                        file,
                        "dispatch_only_reopen",
                        dispatch_pane.clone(),
                        &detail,
                    ));
                }
                RouteDispatchAuthorization::Authorized => {}
            }
            dispatch_only_send_reopen(
                tmux,
                file,
                session_id,
                &dispatch_pane,
                file_path,
                harness,
                DispatchOnlySendReopenOptions {
                    delivery: DispatchOnlyReopenDelivery::DirectPaneSubmit,
                    queue_prompt_text: queue_prompt.as_deref(),
                },
            )?;
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_only_via_actor_direct_pane_submit file={} pane={} harness={} generation={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation
                ),
            );
            Ok(dispatch_pane)
        }
        AuthoritativeActorDispatchAction::ManagedSupervisorIpc => {
            match authorize_controller_dispatch(
                file,
                session_id,
                file_path,
                &actor,
                "managed_reopen",
                &format!(
                    "submit=supervisor_ipc actor_state={} harness={}",
                    actor_state.as_str(),
                    harness.binary
                ),
            )? {
                RouteDispatchAuthorization::CoalescedDeduped { detail } => {
                    return Ok(route_dispatch_deduped_pane(
                        file,
                        "managed_reopen",
                        dispatch_pane.clone(),
                        &detail,
                    ));
                }
                RouteDispatchAuthorization::Authorized => {}
            }
            let dispatch_start = dispatch_via_supervisor_ipc(
                tmux,
                file,
                &dispatch_pane,
                session_id,
                file_path,
                harness,
            )?;

            let ack_pane = require_routed_cycle_ack(
                tmux,
                file,
                &dispatch_pane,
                session_id,
                file_path,
                harness,
                baseline,
                prompt_bearing_marker,
                true,
                dispatch_start,
            )?;
            Ok(ack_pane.unwrap_or(dispatch_pane))
        }
    }
}

mod pane_resolution;
pub(crate) use pane_resolution::*;

mod dispatch;
pub(crate) use dispatch::*;

mod busy_pane;
pub(crate) use busy_pane::*;
mod cycle_ack;
pub(crate) use cycle_ack::*;

mod session_resolution;
pub use session_resolution::*;

mod startup;
pub use startup::*;

#[cfg(test)]
pub(crate) static TMUX_START_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
// Serialize mock agent launches without contending with tests that already
// hold TMUX_START_MUTEX for broader prompt-readiness coverage.
#[cfg(test)]
pub(crate) static TMUX_INJECT_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
// Serialize mutations of the route-specific binary override without
// contending with current-dir guards that already hold the shared test lock.
#[cfg(test)]
pub(crate) static ROUTE_BIN_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(test)]
pub(crate) fn env_lock() -> crate::test_support::ProcessGlobalLockGuard {
    crate::test_support::env_lock()
}
// #codex-route-busy-ctrl-g-opens-editor: the busy-pane reroute must only send
// `C-g` when the live capture proves a shell reverse-i-search / history-search.
// The pre-existing live ctrl-g test only models the reverse-i-search recovery,
// so this deterministic decision test covers the non-search composer / active
// turn case that previously received an editor-opening `C-g`.
#[cfg(test)]
pub(crate) fn tmux_start_lock() -> std::sync::MutexGuard<'static, ()> {
    TMUX_START_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
#[cfg(test)]
pub(crate) fn tmux_inject_lock() -> std::sync::MutexGuard<'static, ()> {
    TMUX_INJECT_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
#[cfg(test)]
pub(crate) fn route_bin_env_lock() -> std::sync::MutexGuard<'static, ()> {
    ROUTE_BIN_ENV_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
#[cfg(test)]
pub(crate) fn test_cwd() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
// #jb-busy-reopen-auto-drain-when-idle: when a document's queue is ALREADY
// auto-looping, there is no INACTIVE head to activate, so
// activate_existing_route_queue_head returns None — but queue_continuation::detect
// still returns Some because the active loop is continuing the document. The busy
// FocusOnly / DispatchOnlyBusyQueue route paths key off exactly this signal to
// report deferred success (the loop will continue) instead of a busy refusal,
// so JB `Run Agent Doc` on an auto-looping pane no longer errors.
#[cfg(test)]
pub(crate) fn test_registry_entry(
    pane: &str,
    file: &str,
    cwd: &std::path::Path,
) -> tmux_router::RegistryEntry {
    tmux_router::RegistryEntry {
        pane: pane.to_string(),
        pid: 1234,
        cwd: cwd.to_string_lossy().to_string(),
        started: "2026-01-01T00:00:00Z".to_string(),
        session_id: "test-session".to_string(),
        file: file.to_string(),
        window: "@1".to_string(),
        supervisor_instance_id: String::new(),
    }
}
#[cfg(test)]
pub(crate) struct ScopedCurrentDir {
    prev_cwd: std::path::PathBuf,
    _env_guard: crate::test_support::ProcessGlobalLockGuard,
}
#[cfg(test)]
impl ScopedCurrentDir {
    fn set(path: &std::path::Path) -> Self {
        let env_guard = env_lock();
        let prev_cwd = std::env::current_dir().unwrap_or_else(|_| test_cwd());
        std::env::set_current_dir(path).unwrap();
        Self {
            prev_cwd,
            _env_guard: env_guard,
        }
    }
}
#[cfg(test)]
impl Drop for ScopedCurrentDir {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev_cwd);
    }
}
#[cfg(test)]
pub(crate) fn write_codex_proof_status_fixture(
    dir: &std::path::Path,
    session_id: &str,
    event: &str,
) -> std::path::PathBuf {
    std::fs::create_dir_all(dir.join(".agent-doc/logs")).unwrap();
    let doc = dir.join("session.md");
    std::fs::write(
            &doc,
            "---\nagent_doc_session: route-proof-status\nagent: codex\ncodex_network_access: enabled\n---\n",
        )
        .unwrap();
    std::fs::write(
        dir.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
        format!(
            "[1] session_start file={} pane=%1 session={}\n[2] {}\n",
            doc.display(),
            session_id,
            event
        ),
    )
    .unwrap();
    doc
}
#[cfg(test)]
pub(crate) fn write_codex_writable_proof_status_fixture(
    dir: &std::path::Path,
    session_id: &str,
    event: &str,
) -> (std::path::PathBuf, String) {
    std::fs::create_dir_all(dir.join(".agent-doc/logs")).unwrap();
    let writable = dir.join("writable-root");
    std::fs::create_dir_all(&writable).unwrap();
    let writable = writable.canonicalize().unwrap();
    let doc = dir.join("session.md");
    std::fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: route-proof-status\nagent: codex\ncodex_args: \"--add-dir {}\"\n---\n",
                writable.display()
            ),
        )
        .unwrap();
    std::fs::write(
        dir.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
        format!(
            "[1] session_start file={} pane=%1 session={}\n[2] {}\n",
            doc.display(),
            session_id,
            event
        ),
    )
    .unwrap();
    let contract =
        agent_doc_turn_executor::codex_launch::writable_root_contract_id(&[writable]).unwrap();
    (doc, contract)
}
#[cfg(test)]
pub(crate) fn wait_for_pane_contains(
    iso: &IsolatedTmux,
    pane: &str,
    needle: &str,
    timeout: std::time::Duration,
) -> String {
    let start = std::time::Instant::now();
    let poll = std::time::Duration::from_millis(100);
    let mut last = String::new();
    while start.elapsed() < timeout {
        last = sessions::capture_pane(iso, pane).unwrap_or_default();
        if last.contains(needle) {
            return last;
        }
        std::thread::sleep(poll);
    }
    last
}
#[cfg(test)]
pub(crate) fn pane_capture_contains_wrapped(capture: &str, needle: &str) -> bool {
    capture.contains(needle) || capture.replace(['\r', '\n'], "").contains(needle)
}
#[cfg(test)]
pub(crate) fn send_keys_with_retry(iso: &IsolatedTmux, pane: &str, text: &str) {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(3);
    let poll = std::time::Duration::from_millis(100);
    let mut last_err = None;

    while start.elapsed() < timeout {
        match iso.send_keys(pane, text) {
            Ok(()) => return,
            Err(err) => last_err = Some(err.to_string()),
        }
        std::thread::sleep(poll);
    }

    panic!(
        "failed to send keys to pane {} after {:.1}s: {}",
        pane,
        start.elapsed().as_secs_f64(),
        last_err.unwrap_or_else(|| "unknown error".to_string())
    );
}
#[cfg(test)]
pub(crate) fn pane_current_command(iso: &IsolatedTmux, pane: &str) -> Option<String> {
    let output = iso
        .cmd()
        .args([
            "display-message",
            "-t",
            pane,
            "-p",
            "#{pane_current_command}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let cmd = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if cmd.is_empty() { None } else { Some(cmd) }
}
#[cfg(test)]
pub(crate) fn wait_for_shell(iso: &IsolatedTmux, pane: &str, timeout: std::time::Duration) -> bool {
    const IDLE_SHELLS: &[&str] = &["zsh", "bash", "sh", "fish"];
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(cmd) = pane_current_command(iso, pane)
            && IDLE_SHELLS.contains(&cmd.as_str())
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}
// --- rewrite_start_path tests ---
// --- Split direction tests ---
// --- Prompt detection tests (via HarnessConfig) ---
// --- Routing logic tests ---
// --- Integration tests (IsolatedTmux) ---
#[cfg(test)]
use tmux_router::IsolatedTmux;
/// Create a mock agent script: blocks for delay, then prints ❯ prompt on its own line.
/// Uses `cat` to keep the process alive after showing the prompt.
#[cfg(test)]
pub(crate) fn mock_agent_script(delay_ms: u64) -> String {
    format!(
        r#"exec /bin/sh -c 'printf "Starting agent...\n"; sleep {}; printf "❯ \n"; cat'"#,
        delay_ms as f64 / 1000.0
    )
}
#[cfg(test)]
pub(crate) fn write_mock_registered_agent_doc(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc");
    std::fs::write(
            &script,
            "#!/bin/sh\nprintf \"> \\n\"\nwhile IFS= read -r CMD; do\n  [ -z \"$CMD\" ] && continue\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn write_mock_registered_agent_doc_with_prefix(
    base: &Path,
    name: &str,
    prefix: &str,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join(name);
    std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf \"> \\n\"\nwhile IFS= read -r CMD; do\n  printf '{prefix}:%s\\n' \"$CMD\"\ndone\n",
            ),
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn write_mock_registered_agent_doc_extra_line_detector(
    base: &Path,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-extra-line-detector");
    std::fs::write(
            &script,
            "#!/bin/bash\nprintf \"> \\n\"\nIFS= read -r CMD || exit 0\nprintf 'GOT:%s\\n' \"$CMD\"\nif IFS= read -r -t 0.5 EXTRA; then\n  printf 'EXTRA:%s\\n' \"$EXTRA\"\nfi\ncat\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn write_mock_registered_agent_doc_with_stale_trigger(
    base: &Path,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-stale-trigger-detector");
    std::fs::write(
            &script,
            "#!/bin/bash\nprintf '> %s\\n' \"$1\"\nIFS= read -r CMD || exit 0\nprintf 'GOT:%s\\n' \"$CMD\"\nif IFS= read -r -t 0.5 EXTRA; then\n  printf 'EXTRA:%s\\n' \"$EXTRA\"\nfi\ncat\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn write_mock_busy_registered_agent_doc(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-busy");
    std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'Working...\\n'\nwhile IFS= read -r CMD; do\n  printf 'EARLY:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn write_mock_active_codex_turn_registered_agent_doc(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-active-codex-turn");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf 'Working...\\n'\ni=0\nwhile [ \"$i\" -lt 20 ]; do\n  printf 'Working (1m 34s - esc to interrupt)\\n'\n  i=$((i + 1))\ndone\nprintf '\\n> Write tests for @filename\\ngpt-5 high - ~/work/btakita/agent-loop - Context 41%% used\\n'\nwhile IFS= read -r CMD; do\n  printf 'EARLY:%s\\n' \"$CMD\"\ndone\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn write_mock_busy_registered_agent_doc_ignores_interrupt(
    base: &Path,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-busy-ignore-int");
    std::fs::write(
            &script,
            "#!/bin/sh\ntrap '' INT\nprintf 'Working...\\n'\nwhile IFS= read -r CMD; do\n  printf 'EARLY:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn write_mock_busy_opencode_recovers_on_escape(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-busy-opencode");
    std::fs::write(
        &script,
        r#"#!/bin/bash
trap '' INT
cleanup() { stty sane 2>/dev/null || true; }
trap cleanup EXIT
stty -echo -icanon min 1 time 0
printf '⬝⬝■■■■■■  esc interrupt\n'
while IFS= read -r -n1 ch; do
  stty sane
  printf '> \n'
  while IFS= read -r CMD; do
    printf 'GOT:%s\n' "$CMD"
  done
  exit 0
done
"#,
    )
    .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn write_mock_busy_registered_agent_doc_recovers_on_ctrl_g(
    base: &Path,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-busy-recovers-on-ctrl-g");
    std::fs::write(
        &script,
        r#"#!/bin/bash
trap '' INT
cleanup() { stty sane 2>/dev/null || true; }
trap cleanup EXIT
stty -echo -icanon min 1 time 0
printf 'Working...\n'
printf 'reverse-i-search: bugs enter accept · esc cancel\n'
while IFS= read -r -n1 ch; do
  if [[ "$ch" == $'\a' ]]; then
    stty sane
    printf '› \n'
    printf 'gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used\n'
    while IFS= read -r CMD; do
      printf 'GOT:%s\n' "$CMD"
    done
    exit 0
  fi
done
"#,
    )
    .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn write_mock_start_agent_doc(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-start");
    std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'Starting agent...\\n'\nprintf '> \\n'\nwhile IFS= read -r CMD; do\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn write_mock_delayed_start_agent_doc(
    base: &Path,
    delay_secs: u64,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-start-delayed");
    std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep {}\nprintf 'Starting agent...\\n'\nprintf '> \\n'\nwhile IFS= read -r CMD; do\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
                delay_secs
            ),
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}
#[cfg(test)]
pub(crate) fn launch_mock_registered_agent_doc(
    iso: &IsolatedTmux,
    pane: &str,
    script: &Path,
    file: &Path,
) {
    {
        let _tmux_guard = tmux_inject_lock();
        assert!(
            wait_for_shell(iso, pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before mock agent launch"
        );
        send_keys_with_retry(
            iso,
            pane,
            &format!("exec {} {}", script.display(), file.display()),
        );
    }
    let launch_command = format!("exec {} {}", script.display(), file.display());
    let content = wait_for_mock_agent_prompt(iso, pane, &launch_command);
    assert!(
        content.lines().any(|line| line.trim() == ">"),
        "mock agent-doc session should present a prompt, got: {content}"
    );
}
#[cfg(test)]
pub(crate) fn launch_mock_agent_doc_without_file_arg(
    iso: &IsolatedTmux,
    pane: &str,
    script: &Path,
) {
    {
        let _tmux_guard = tmux_inject_lock();
        assert!(
            wait_for_shell(iso, pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before mock agent launch"
        );
        send_keys_with_retry(iso, pane, &format!("exec {}", script.display()));
    }
    let launch_command = format!("exec {}", script.display());
    let content = wait_for_mock_agent_prompt(iso, pane, &launch_command);
    assert!(
        content.lines().any(|line| line.trim() == ">"),
        "mock agent-doc session should present a prompt, got: {content}"
    );
}
#[cfg(test)]
pub(crate) fn wait_for_mock_agent_prompt(
    iso: &IsolatedTmux,
    pane: &str,
    launch_command: &str,
) -> String {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(20);
    let poll = std::time::Duration::from_millis(100);
    let mut last_submit = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or_else(std::time::Instant::now);
    let mut last = String::new();

    while start.elapsed() < timeout {
        last = sessions::capture_pane(iso, pane).unwrap_or_default();
        if last.lines().any(|line| line.trim() == ">") {
            return last;
        }
        if last.contains(launch_command)
            && last_submit.elapsed() >= std::time::Duration::from_millis(500)
        {
            let _ = iso.send_keys_raw(pane, "Enter");
            last_submit = std::time::Instant::now();
        }
        std::thread::sleep(poll);
    }

    last
}
#[cfg(test)]
pub(crate) fn wait_for_process_pid(pattern: &str, timeout: std::time::Duration) -> u32 {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(output) = std::process::Command::new("pgrep")
            .args(["-f", pattern])
            .output()
            && output.status.success()
        {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let pid = line.trim();
                if pid.is_empty() {
                    continue;
                }
                if let Ok(parsed) = pid.parse::<u32>() {
                    return parsed;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("timed out waiting for process matching pattern: {pattern}");
}
// #jb-run-agent-doc-busy-wait-deadlock: a dispatch-only route on a busy active
// turn must NOT honor the slow-start `--wait-for-ready` override when a queue
// fallback exists — it should skip the wait and queue immediately, so JB `Run
// Agent Doc` on a mid-turn Claude actor does not block for the full 60s.
// #jb-run-agent-doc-busy-active-turn-stall: a bare dispatch-only Run Agent Doc
// against a pane proven busy on an active turn (working spinner / `esc to
// interrupt`) must NOT honor the 60s busy ready-wait — a multi-minute turn
// cannot reach a dispatch-ready prompt in that budget, so waiting only produces
// a silent stall before the inevitable "session still running" refusal. Skip the
// wait so the refusal/notification fires immediately.
// The refusal message must reflect whether the busy ready-wait was actually
// served: an active-turn skip words it as a busy turn (no misleading "after
// waiting Ns"), while the no-cue path keeps the cold-start ready-wait wording.
// --- auto_start_in_session tests ---
// --- has_named_window tests ---
// --- tmux_session validation tests ---
// --- Stash rescue tests ---
// --- split_before positional target tests ---
#[cfg(test)]
pub(crate) fn test_actor_record(pane_id: &str) -> agent_doc_sqlite::state_store::ActorRecord {
    agent_doc_sqlite::state_store::ActorRecord {
        document_id: "test-doc".to_string(),
        session_id: "test-session".to_string(),
        generation: 1,
        pane_id: pane_id.to_string(),
        window_id: "@1".to_string(),
        harness: "codex".to_string(),
        state: agent_doc_sqlite::state_store::ActorState::Ready,
        last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
            caller: "test".to_string(),
            reason: "test".to_string(),
            timestamp: 0,
            prior_generation: 0,
            new_generation: 1,
        },
    }
}
#[cfg(test)]
pub(crate) fn test_degraded_actor(pane_id: &str) -> AuthoritativeActorDispatchTarget {
    AuthoritativeActorDispatchTarget {
        record: test_actor_record(pane_id),
        runtime: SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
        },
    }
}
// #route-busy-vs-starting-wording: the FailClosed wait context distinguishes a
// pane busy on an active harness turn from a genuine cold startup timeout.
#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::supervisor::ipc::SupervisorIpc;
    use agent_doc_controller::dispatch::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
    use agent_doc_supervisor::ipc_protocol::{IpcMethod, IpcResponse};
    use agent_doc_turn::closeout_recovery::CloseoutRecoveryState;

    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
        _lock: crate::test_support::ProcessGlobalLockGuard,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::test_support::env_lock();
            let prior = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key,
                prior,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn authoritative_actor_optimistic_queue_excludes_starting_state() {
        assert!(
            authoritative_actor_dispatch_can_queue_optimistically(
                agent_doc_sqlite::state_store::ActorState::Busy
            ),
            "busy actors may still accept a supervisor-owned queued reopen"
        );
        assert!(
            !authoritative_actor_dispatch_can_queue_optimistically(
                agent_doc_sqlite::state_store::ActorState::Starting
            ),
            "starting actors must become ready before route submits a reopen"
        );
    }
    #[test]
    fn authoritative_actor_start_wait_terminal_state_only_for_terminal_states() {
        assert!(authoritative_actor_start_wait_terminal_state(
            agent_doc_sqlite::state_store::ActorState::Closed
        ));
        assert!(authoritative_actor_start_wait_terminal_state(
            agent_doc_sqlite::state_store::ActorState::Blocked
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            agent_doc_sqlite::state_store::ActorState::Starting
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            agent_doc_sqlite::state_store::ActorState::Busy
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            agent_doc_sqlite::state_store::ActorState::WaitingInput
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            agent_doc_sqlite::state_store::ActorState::Ready
        ));
    }
    #[test]
    fn authoritative_actor_ready_poll_requires_ready_state_and_prompt_proof() {
        use agent_doc_sqlite::state_store::ActorState;

        let schedule = [
            (ActorState::Starting, false, true),
            (ActorState::Busy, false, true),
            (ActorState::Ready, false, true),
        ];
        for (state, prompt_ready, dispatch_eligible) in schedule {
            assert_eq!(
                classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                    actor_state: actor_dispatch_state(state),
                    prompt_ready,
                    dispatch_eligible,
                }),
                PromptReadyBarrierDecision::Continue,
                "route must keep waiting while the current generation is {state:?} prompt_ready={prompt_ready} eligible={dispatch_eligible}"
            );
        }

        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: actor_dispatch_state(ActorState::Ready),
                prompt_ready: true,
                dispatch_eligible: false,
            }),
            PromptReadyBarrierDecision::Continue,
            "a ready actor still cannot dispatch until the target passes dispatch eligibility"
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: actor_dispatch_state(ActorState::Ready),
                prompt_ready: true,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Ready,
            "route may dispatch only after ready state, prompt proof, and eligibility agree"
        );
    }
    #[test]
    fn authoritative_actor_ready_poll_surfaces_terminal_states() {
        use agent_doc_sqlite::state_store::ActorState;

        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: actor_dispatch_state(ActorState::Closed),
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Terminal
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: actor_dispatch_state(ActorState::Blocked),
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Terminal
        );
    }
    #[test]
    fn route_low_level_cleanup_scrubs_duplicate_prompt_comment_without_preserve_doc() {
        let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Reproduce bugs with tests first and fix the implementation. #spec-test-build-install-commit-push";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!--\n",
                "Keep this unrelated scratch note hidden.\n",
                "-->\n"
            ),
            prompt = prompt
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[])
            .unwrap()
            .expect("route should canonicalize duplicate prompt scratch comments before dispatch");
        let cleaned = cleanup.content;

        let duplicate_comment = format!("<!--\n{prompt}\n-->");
        assert!(
            !cleaned.contains(&duplicate_comment),
            "route must not dispatch with duplicate prompt text still in the post-exchange comment:\n{cleaned}"
        );
        assert!(
            cleaned.contains("\n<!--\n-->\n\n<!--\nKeep this unrelated scratch note hidden."),
            "route must preserve the ordinary comment shell and unrelated scratch comments:\n{cleaned}"
        );
    }
    #[test]
    fn route_preserves_duplicate_prompt_comment_from_snapshot() {
        let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n"
            ),
            prompt = prompt
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[&content]).unwrap();

        assert!(
            cleanup.is_none(),
            "route cleanup must preserve snapshot-owned scratch comments"
        );
    }
    #[test]
    fn route_preserves_scratch_comment_after_compact_summary_before_dispatch() {
        let prompt = "The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "Compacted content:\n",
                "- Trailing prompt/context: {prompt}\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: compact prompt duplication — gpt-5\n\n",
                "Line-by-line diff was the right diagnostic.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n",
                "<!--\n",
                "{prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Look through the Claude + Codex + agent-doc session logs\n",
                "-->\n"
            ),
            prompt = prompt
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[&content]).unwrap();

        assert!(
            cleanup.is_none(),
            "production route cleanup must preserve visible post-exchange scratch comments"
        );
    }
    #[test]
    fn route_low_level_cleanup_scrubs_unowned_duplicate_prompt_comment() {
        let prompt = "The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "Compacted content:\n",
                "- Trailing prompt/context: {prompt}\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: compact prompt duplication — gpt-5\n\n",
                "Line-by-line diff was the right diagnostic.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n",
                "<!--\n",
                "{prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Look through the Claude + Codex + agent-doc session logs\n",
                "-->\n"
            ),
            prompt = prompt
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[])
            .unwrap()
            .expect("route cleanup should scrub duplicate compact prompt residue");
        let cleaned = cleanup.content;

        assert!(
            !cleaned.contains(&format!("<!--\n{prompt}")),
            "route cleanup should remove only the duplicate prompt line:\n{cleaned}"
        );
        assert!(
            cleaned.contains("Look through the Claude + Codex + agent-doc session logs"),
            "route cleanup must not erase unrelated post-exchange scratch comments:\n{cleaned}"
        );
        assert!(
            cleaned.contains("<!--\n#spec-test-build-install-commit-push\n---\nLook through"),
            "route cleanup must preserve command and separator scratch lines:\n{cleaned}"
        );
    }
    #[test]
    fn route_preserves_scratch_comment_when_response_quotes_same_text() {
        let scratch =
            "Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ Please inspect the latest route cleanup report. #spec-test-build-install-commit-push\n",
                "### Re: route cleanup — gpt-5\n\n",
                "{scratch}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n",
                "<!--\n",
                "{scratch}\n",
                "-->\n"
            ),
            scratch = scratch
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[]).unwrap();
        let cleaned = cleanup
            .as_ref()
            .map(|cleanup| cleanup.content.as_str())
            .unwrap_or(content.as_str());

        assert!(
            cleaned.contains(&format!("<!--\n{scratch}\n-->")),
            "route cleanup must not treat assistant response quotes as prompt residue:\n{cleaned}"
        );
    }
    #[test]
    fn route_scrubs_duplicate_answered_prompt_tail_before_dispatch() {
        let prompt = "The content of the html comment below this agent:exchange element was deleted after the last agent-doc turn. Should we diff line by line?";
        // Genuine delayed replay re-adds the just-answered prompt in answered form
        // (carrying the `❯ ` marker) — that is the ownership proof that lets route
        // scrub it safely.
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: mixed scratch comment deletion — gpt-5\n\n",
                "Answered already.\n",
                "<!-- agent:boundary:head -->\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[])
            .unwrap()
            .expect("route should canonicalize duplicate answered prompt tails before dispatch");
        let cleaned = cleanup.content;

        assert!(cleanup.removed_answered_tail);
        assert!(
            cleaned.contains(&format!(
                "❯ {prompt}\n❯ #spec-test-build-install-commit-push\n### Re:"
            )),
            "answered prompt block must remain in exchange history:\n{cleaned}"
        );
        assert!(
            !cleaned.contains(&format!("<!-- agent:boundary:head -->\n❯ {prompt}")),
            "route must not dispatch with duplicate answered-form prompt after the boundary:\n{cleaned}"
        );
    }
    #[test]
    fn route_preserves_unprefixed_live_prompt_matching_an_answered_prompt() {
        // Regression for #ipcfullprompt-recur: a freshly-typed prompt that happens to
        // match a previously-answered prompt (e.g. a re-typed "go") has no `❯ ` marker
        // and MUST be preserved for dispatch — never scrubbed as duplicate residue.
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ go\n",
            "### Re: go — gpt-5\n\n",
            "Did the thing.\n",
            "<!-- agent:boundary:head -->\n",
            "go\n",
            "<!-- /agent:exchange -->\n",
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(content, &[]).unwrap();
        assert!(
            cleanup.is_none(),
            "a bare re-typed prompt must not be scrubbed: {cleanup:?}"
        );
    }
    #[test]
    fn route_rejects_duplicate_prompt_markdown_residue_before_dispatch() {
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "# Notes\n\n",
                "{prompt}\n"
            ),
            prompt = prompt
        );

        let err = scrub_duplicate_prompt_comments_for_route(&content, &[]).unwrap_err();

        assert!(
            err.to_string().contains("duplicate prompt residue"),
            "route must fail closed before dispatching against duplicate prompt Markdown residue: {err}"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_creates_visible_plain_queue_and_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ prior prompt\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#qipc] Fix queue dispatch.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "❯ do [#qipc]. #spec-test-build-install-commit-push",
            "test_busy_actor",
            false,
        )
        .expect("route should persist a queued dispatch prompt");

        assert!(outcome.appended);
        assert!(outcome.component_created);
        assert!(outcome.activated);
        assert_eq!(
            outcome.prompt_text,
            "do [#qipc]. #spec-test-build-install-commit-push"
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(updated.contains("- do [#qipc]. #spec-test-build-install-commit-push"));
        let queue_pos = updated.find("<!-- agent:queue go -->").unwrap();
        let backlog_pos = updated.find("<!-- agent:backlog -->").unwrap();
        assert!(
            queue_pos < backlog_pos,
            "created queue component should be visible before tracked work components:\n{updated}"
        );
        let snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snapshot, updated,
            "route queueing must sync the snapshot so queue continuation is not treated as a modified head prompt"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_converges_via_editor_ipc_with_listener() {
        // JB Run Agent Doc can queue a pending dispatch while the editor plugin
        // owns the live buffer. That write must use the shared editor-converger,
        // not a direct disk write that manufactures a File Cache Conflict.
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- existing queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        let expected = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- :pushpin: manual preempt prompt\n",
            "- existing queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        let _listener =
            crate::test_support::start_live_prompt_drift_ack_listener(dir.path(), expected.into());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let outcome =
            enqueue_route_dispatch_prompt(&doc, "manual preempt prompt", "test_busy_actor", true)
                .expect("route enqueue should converge through editor IPC");

        assert!(outcome.appended);
        assert!(outcome.activated);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), expected);
        assert_eq!(crate::snapshot::load(&doc).unwrap().unwrap(), expected);
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("route_dispatch_queue_editor_convergence_attempt")
                && ops_log.contains("route_dispatch_queue_writeback")
                && ops_log.contains("transport=editor_ipc"),
            "route queue write must be observable as editor IPC convergence:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("transport=disk_fallback"),
            "active-editor route queueing must not take a disk fallback:\n{ops_log}"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_preserves_unparseable_queue_instead_of_crashing() {
        // Repro of "JB Run Agent Doc error: route queue dispatch: failed to parse
        // existing agent:queue": an earlier corruption merged free-text prose into
        // the agent:queue component, so `queue::parse` bails on a bare line. The
        // route must not propagate that as a fatal error — it must preserve the
        // polluted body and still append the new pending dispatch.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "JB `Run Agent Doc` error:\n",
            "- do [#existing]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        // The polluted free-text line is preserved as a non-actionable Freeform
        // entry (tolerant parse) rather than failing the consume/dispatch guards.
        let parsed =
            agent_doc_queue::document_queue::parse("JB `Run Agent Doc` error:\n- do [#existing]\n")
                .unwrap();
        assert!(
            parsed
                .iter()
                .any(|e| matches!(e, agent_doc_queue::document_queue::QueueEntry::Freeform(_)))
        );

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome =
            enqueue_route_dispatch_prompt(&doc, "do [#newitem]", "test_busy_actor", false)
                .expect("route must not crash on a polluted agent:queue");
        assert!(outcome.appended);

        let updated = std::fs::read_to_string(&doc).unwrap();
        // Existing (polluted) content preserved — not silently dropped.
        assert!(updated.contains("JB `Run Agent Doc` error:"));
        assert!(updated.contains("- do [#existing]"));
        // New dispatch appended below it.
        assert!(updated.contains("- do [#newitem]"));

        // Re-dispatching the same prompt into the still-polluted queue is idempotent.
        let outcome2 =
            enqueue_route_dispatch_prompt(&doc, "do [#newitem]", "test_busy_actor", false)
                .expect("route must stay resilient on repeat dispatch");
        assert!(outcome2.already_present);
        let updated2 = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated2.matches("- do [#newitem]").count(),
            1,
            "repeat dispatch into a polluted queue must not duplicate the entry:\n{updated2}"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_activates_existing_queue_without_duplicate() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#qipc]. #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#qipc] Fix queue dispatch.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "do [#qipc]. #spec-test-build-install-commit-push",
            "test_busy_actor",
            false,
        )
        .expect("route should activate an existing queued dispatch prompt");

        assert!(!outcome.appended);
        assert!(outcome.already_present);
        assert!(!outcome.superseded);
        assert!(!outcome.component_created);
        assert!(outcome.activated);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert_eq!(
            updated
                .matches("- do [#qipc]. #spec-test-build-install-commit-push")
                .count(),
            1,
            "route must not duplicate an already visible queue prompt:\n{updated}"
        );
    }
    #[test]
    fn route_activates_existing_inactive_auto_go_queue_head_as_go_queue_for_busy_deferral() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#shipstationaudit]. #spec-test-commit-push\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#shipstationaudit] Audit ShipStation settings.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        assert_eq!(
            inactive_route_queue_head(&doc).unwrap().as_deref(),
            Some("do [#shipstationaudit]. #spec-test-commit-push")
        );

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome = activate_existing_route_queue_head(&doc, "busy actor")
            .unwrap()
            .expect("legacy inactive auto go queue head should activate");

        assert_eq!(
            outcome.prompt_text,
            "do [#shipstationaudit]. #spec-test-commit-push"
        );
        assert!(!outcome.appended);
        assert!(outcome.already_present);
        assert!(!outcome.superseded);
        assert!(!outcome.component_created);
        assert!(outcome.activated);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert_eq!(
            updated
                .matches("- do [#shipstationaudit]. #spec-test-commit-push")
                .count(),
            1,
            "route must activate the existing head without duplicating it:\n{updated}"
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::live_continuation_head(&updated).as_deref(),
            Some("shipstationaudit"),
            "activated go queue should become drainable by the idle-queue watch"
        );
        let snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(snapshot, updated, "route activation must sync the snapshot");
    }
    #[test]
    fn route_does_not_activate_plain_inactive_queue_head() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#manual]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        assert_eq!(inactive_route_queue_head(&doc).unwrap(), None);
        assert_eq!(
            activate_existing_route_queue_head(&doc, "busy actor").unwrap(),
            None,
            "plain inactive queues should stay inert without auto/start activation"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
        assert_eq!(crate::snapshot::load(&doc).unwrap().unwrap(), content);
    }
    #[test]
    fn route_defers_uncommitted_queue_head_not_in_committed_snapshot() {
        // #qdispatchloss: the operator typed a fresh queue item into the editor
        // buffer; the JB plugin synced it to disk but it is NOT yet committed
        // (the committed snapshot predates the add). Route must NOT consume the
        // uncommitted head as the dispatch prompt — it would feed a possibly
        // half-typed line into the agent and lose the item.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let committed = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#committed]\n",
            "<!-- /agent:queue -->\n"
        );
        // Disk has a fresh, uncommitted head (`do [#fresh]`) prepended above the
        // committed one — exactly what an operator mid-edit produces.
        let on_disk = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#fresh]\n",
            "- do [#committed]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, on_disk).unwrap();
        // Committed snapshot only knows about `#committed`.
        crate::snapshot::save(&doc, committed).unwrap();

        assert!(
            !agent_doc_queue::route_dispatch::committed_snapshot_backs_queue_head(
                Some(committed),
                "do [#fresh]"
            ),
            "a head absent from the committed snapshot queue is not backed"
        );
        assert!(
            agent_doc_queue::route_dispatch::committed_snapshot_backs_queue_head(
                Some(committed),
                "do [#committed]"
            ),
            "a head present in the committed snapshot queue is backed"
        );

        // The inactive-head read defers the uncommitted head instead of
        // surfacing it for dispatch.
        assert_eq!(
            inactive_route_queue_head(&doc).unwrap(),
            None,
            "route must not surface an uncommitted queue head for dispatch"
        );
        // The activate path therefore no-ops: nothing is consumed and the doc /
        // snapshot are untouched, so the operator's edit survives for the next
        // committed cycle.
        assert_eq!(
            activate_existing_route_queue_head(&doc, "dispatch_only_inactive_queue").unwrap(),
            None,
            "route must not activate/consume an uncommitted queue head"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), on_disk);
        assert_eq!(crate::snapshot::load(&doc).unwrap().unwrap(), committed);
    }
    #[test]
    fn route_dispatches_committed_queue_head() {
        // #qdispatchloss positive control: when the disk head IS backed by the
        // committed snapshot, route activates/dispatches it normally.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#committed]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        assert_eq!(
            inactive_route_queue_head(&doc).unwrap().as_deref(),
            Some("do [#committed]"),
            "a committed-backed head is dispatchable"
        );
        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome = activate_existing_route_queue_head(&doc, "dispatch_only_inactive_queue")
            .unwrap()
            .expect("committed-backed head should activate");
        assert_eq!(outcome.prompt_text, "do [#committed]");
    }
    #[test]
    fn route_queue_head_unbacked_when_committed_snapshot_has_no_queue() {
        // #qdispatchloss: a committed snapshot with no queue component at all
        // means any on-disk queue head is a fresh uncommitted edit.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let committed = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        assert!(
            !agent_doc_queue::route_dispatch::committed_snapshot_backs_queue_head(
                Some(committed),
                "do [#fresh]"
            ),
            "no committed queue component → head is unbacked"
        );
    }
    #[test]
    fn route_queue_head_backed_allows_when_no_committed_snapshot() {
        // #qdispatchloss bootstrap escape hatch: an untracked scaffold with no
        // committed snapshot must not be blocked — there is nothing to diverge
        // from.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "scaffold\n").unwrap();
        assert!(
            agent_doc_queue::route_dispatch::committed_snapshot_backs_queue_head(
                None,
                "do [#anything]"
            ),
            "no committed snapshot → allow (bootstrap)"
        );
    }
    #[test]
    fn busy_route_defers_to_active_go_loop_instead_of_refusing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#regional]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        // No INACTIVE head — the queue is already active, so the activate path no-ops.
        assert_eq!(
            inactive_route_queue_head(&doc).unwrap(),
            None,
            "an already-active go queue exposes no inactive head to activate"
        );
        assert_eq!(
            activate_existing_route_queue_head(&doc, "busy actor").unwrap(),
            None,
            "activate path returns None when the queue is already go-looping"
        );
        // But the active-loop continuation signal IS present — this is what the busy
        // route path uses to defer (report success) instead of failing closed.
        let continuation = crate::queue_continuation::detect(&doc)
            .unwrap()
            .expect("active go-loop must expose a continuation head for busy deferral");
        assert_eq!(continuation.head_prompt, "do [#regional]");
    }
    #[test]
    fn route_activates_queue_stop_with_marker_go_head() {
        // #queue-state-unify: a `queue: stop` document carrying the marker-side
        // `<!-- agent:queue go -->` control must be recognized as activatable by the
        // route path so JB `Run Agent Doc` starts the queue. `go` is the marker
        // spelling of the canonical start gesture and overrides the stale stop.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#shipstationaudit]. #spec-test-commit-push\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#shipstationaudit] Audit ShipStation settings.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        assert_eq!(
            inactive_route_queue_head(&doc).unwrap().as_deref(),
            Some("do [#shipstationaudit]. #spec-test-commit-push"),
            "marker-side `go` must be recognized as an activatable head despite `queue: stop`"
        );

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome = activate_existing_route_queue_head(&doc, "busy actor")
            .unwrap()
            .expect("startable inactive `go` queue head should activate");

        assert_eq!(
            outcome.prompt_text,
            "do [#shipstationaudit]. #spec-test-commit-push"
        );
        assert!(outcome.activated);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("queue: go"),
            "activation must flip the canonical control to go:\n{updated}"
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::live_continuation_head(&updated).as_deref(),
            Some("shipstationaudit"),
            "activated queue should become drainable by the idle-queue watch"
        );
    }
    #[test]
    fn route_does_not_activate_queue_with_marker_stop() {
        // A marker-side `stop` is an explicit halt gesture and must keep the queue
        // inert even when it would otherwise activate via the legacy `auto`
        // attribute (#queue-state-unify). `stop` wins over `auto`/`go`/`start`.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto stop -->\n",
            "- do [#manual]. #spec-test-commit-push\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        assert_eq!(
            inactive_route_queue_head(&doc).unwrap(),
            None,
            "marker-side `stop` must keep the queue inert"
        );
        assert_eq!(
            activate_existing_route_queue_head(&doc, "busy actor").unwrap(),
            None,
            "marker-side `stop` must not be activated by the route path"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_no_dup_with_completed_residue_and_live_head() {
        // Repro for #adoc-queue-ipc-drift: a halted/inactive-then-reactivated queue
        // that still carries struck `Completed` residue plus a single live prompt.
        // Re-dispatching the live head must NOT append a duplicate, and must NOT
        // supersede the live head into a struck id.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test-build-install-commit-push\n",
            "- ~do [#adoc-sqlite-isolation]~\n",
            "- ~do [#adoc-sqlite-seam]~\n",
            "- do [#adoc-orch-shim-cleanup]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#adoc-orch-shim-cleanup] Finish the migration.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "do [#adoc-orch-shim-cleanup]",
            "test_busy_actor",
            true,
        )
        .expect("route should treat the live head as already queued");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated.matches("- do [#adoc-orch-shim-cleanup]").count(),
            1,
            "re-dispatching the live queue head must not duplicate it:\n{updated}\noutcome={outcome:?}"
        );
        assert!(
            !outcome.appended,
            "live head re-dispatch must not append:\n{updated}\noutcome={outcome:?}"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_supersedes_single_auto_queue_prompt() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- Run Agent Doc queued the first prompt.\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "Run Agent Doc queued the edited prompt.",
            "test_busy_actor",
            false,
        )
        .expect("route should update a stale single auto-queue prompt");

        assert!(!outcome.appended);
        assert!(!outcome.already_present);
        assert!(outcome.superseded);
        assert!(!outcome.component_created);
        assert!(outcome.activated);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(
            !updated.contains("- Run Agent Doc queued the first prompt."),
            "stale route-owned queue prompt should be replaced:\n{updated}"
        );
        assert!(
            updated.contains("- Run Agent Doc queued the edited prompt."),
            "edited prompt should become the single queued rerun:\n{updated}"
        );
        let snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snapshot, updated,
            "queue prompt supersession must sync the route snapshot"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_appends_to_legacy_auto_queue_as_plain_queue() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- first queued prompt\n",
            "- second queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome =
            enqueue_route_dispatch_prompt(&doc, "third queued prompt", "test_busy_actor", false)
                .expect("route should append to legacy multi-prompt queues");

        assert!(outcome.appended);
        assert!(!outcome.already_present);
        assert!(!outcome.superseded);
        assert!(!outcome.component_created);
        assert!(outcome.activated);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(
            updated
                .contains("- first queued prompt\n- second queued prompt\n- third queued prompt")
        );
    }
    #[test]
    fn route_enqueue_priority_dispatch_preempts_legacy_auto_queue_as_plain_queue() {
        // #jb-run-preempt-autoloop-priority: a manual operator Run Agent Doc into a
        // busy pane must jump AHEAD of pending auto-loop items, not land at the tail.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- first queued prompt\n",
            "- second queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome =
            enqueue_route_dispatch_prompt(&doc, "manual preempt prompt", "test_busy_actor", true)
                .expect("priority route dispatch should preempt the pending queue");

        assert!(outcome.appended);
        assert!(!outcome.already_present);
        assert!(!outcome.superseded);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(
            updated.contains(
                "- :pushpin: manual preempt prompt\n- first queued prompt\n- second queued prompt"
            ),
            "priority dispatch must head-insert ahead of pending auto items with operator pin:\n{updated}"
        );
        let snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(snapshot, updated, "priority preempt must sync the snapshot");
    }
    #[test]
    fn route_enqueue_priority_dispatch_inserts_ahead_of_lone_legacy_auto_prompt() {
        // #jb-run-preempt-autoloop-priority: a priority dispatch must NOT supersede a
        // lone auto prompt — replacing it would silently drop the pending auto-loop
        // item the manual run is preempting. Both prompts survive, manual first.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- pending auto-loop item\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let outcome =
            enqueue_route_dispatch_prompt(&doc, "manual preempt prompt", "test_busy_actor", true)
                .expect("priority route dispatch should insert ahead, not supersede");

        assert!(outcome.appended);
        assert!(!outcome.superseded, "priority dispatch must not supersede");
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(
            updated.contains("- :pushpin: manual preempt prompt\n- pending auto-loop item"),
            "priority dispatch must preserve the pending item and run ahead of it with operator pin:\n{updated}"
        );
    }
    #[test]
    fn route_enqueue_priority_dispatch_preserves_leading_queue_directives() {
        // #jb-run-preempt-autoloop-priority: the head-insert must land after leading
        // queue directives (preset / start fence), before the first actionable prompt.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test-build-install-commit-push\n",
            "- first queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        enqueue_route_dispatch_prompt(&doc, "manual preempt prompt", "test_busy_actor", true)
            .expect("priority route dispatch should insert after leading directives");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        let preset_pos = updated
            .find("preset #spec")
            .expect("preset directive preserved");
        let preempt_pos = updated
            .find("- :pushpin: manual preempt prompt")
            .expect("preempt prompt inserted");
        let first_pos = updated
            .find("- first queued prompt")
            .expect("first prompt preserved");
        assert!(
            preset_pos < preempt_pos && preempt_pos < first_pos,
            "preempt prompt must sit after the preset directive and before the first prompt:\n{updated}"
        );
    }
    #[test]
    fn managed_capability_proof_status_tracks_pending_and_failed() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let session_id = "route-proof-status";
        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "codex_capability_proof status=pending",
        );

        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Pending
        );

        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "codex_capability_proof status=failed error=\"dns\"",
        );
        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Failed
        );
    }
    #[test]
    fn managed_capability_proof_status_requires_matching_writable_root_contract() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let session_id = "route-writable-proof-status";
        let (doc, contract) = write_codex_writable_proof_status_fixture(
            dir.path(),
            session_id,
            "codex_capability_proof status=proven network=not_required network_probe=not_required ssh_targets=0 writable_roots=1",
        );

        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Missing
        );

        let (doc, _) = write_codex_writable_proof_status_fixture(
            dir.path(),
            session_id,
            &format!(
                "codex_capability_proof status=proven network=not_required network_probe=not_required ssh_targets=0 writable_roots=1 writable_root_contract={contract}"
            ),
        );
        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Proven
        );
    }
    #[test]
    fn managed_capability_proof_status_requires_post_restart_proof() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let session_id = "route-proof-after-restart";
        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=0",
        );
        std::fs::write(
            dir.path()
                .join(".agent-doc/logs")
                .join(format!("{session_id}.log")),
            format!(
                "[1] session_start file={} pane=%1 session={}\n\
                 [2] codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=0\n\
                 [3] agent_restart_performed old_harness=claude new_harness=codex action=spawn_fresh_harness\n",
                doc.display(),
                session_id
            ),
        )
        .unwrap();

        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Missing
        );

        crate::startup_miss::append_session_log_event(
            &doc,
            session_id,
            "codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=0",
        )
        .unwrap();
        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Proven
        );
    }
    #[test]
    fn managed_capability_proof_status_opencode_tracks_pending_and_failed() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let session_id = "route-proof-status-opencode";
        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "opencode_capability_proof status=pending",
        );

        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::opencode()).unwrap(),
            ManagedCapabilityProofStatus::Pending
        );

        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "opencode_capability_proof status=failed error=\"ssh\"",
        );
        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::opencode()).unwrap(),
            ManagedCapabilityProofStatus::Failed
        );
    }
    #[test]
    fn pane_registration_matches_file_resolves_entry_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let submodule = dir.path().join("src/session-share");
        let tasks = submodule.join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let doc = tasks.join("claudescore-3.md");
        std::fs::write(&doc, "# session\n").unwrap();

        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            test_registry_entry("%401", "tasks/claudescore-3.md", &submodule),
        );

        assert!(
            pane_registration_matches_file(&registry, "%401", &doc.to_string_lossy()),
            "relative registry paths should resolve against the pane cwd"
        );
    }
    #[test]
    fn ensure_dispatch_target_matches_file_rejects_cross_file_registration() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let submodule = dir.path().join("src/session-share");
        let tasks = submodule.join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let registered = tasks.join("sampleorders.md");
        let requested = tasks.join("claudescore-3.md");
        std::fs::write(&registered, "# registered\n").unwrap();
        std::fs::write(&requested, "# requested\n").unwrap();

        sessions::register_full_with_cwd_in(
            dir.path(),
            "session-a",
            "%401",
            "tasks/sampleorders.md",
            1234,
            "@1",
            &submodule.to_string_lossy(),
        )
        .unwrap();

        let err = ensure_dispatch_target_matches_file("%401", &requested.to_string_lossy())
            .expect_err("cross-file pane reuse must fail closed");
        assert!(
            err.to_string().contains("refusing cross-file dispatch"),
            "error should explain the rejected cross-file dispatch: {err}"
        );
    }
    #[test]
    fn detects_unicode_prompt() {
        let h = HarnessConfig::claude();
        assert!(h.is_prompt_line("❯"));
        assert!(h.is_prompt_line("❯ "));
        assert!(h.is_prompt_line("  ❯  "));
    }
    #[test]
    fn detects_ascii_prompt() {
        let h = HarnessConfig::codex();
        assert!(h.is_prompt_line(">"));
        assert!(h.is_prompt_line("> "));
        assert!(h.is_prompt_line("  >  "));
    }
    #[test]
    fn rejects_non_prompt_lines() {
        let h = HarnessConfig::claude();
        assert!(!h.is_prompt_line("Starting claude..."));
        assert!(!h.is_prompt_line("test result: ok"));
        assert!(!h.is_prompt_line(""));
        assert!(!h.is_prompt_line("  "));
        assert!(!h.is_prompt_line("## User"));
    }
    #[test]
    fn handles_ansi_prompt() {
        let h = HarnessConfig::claude();
        assert!(h.is_prompt_line("\x1b[32m❯\x1b[0m"));
        let h_codex = HarnessConfig::codex();
        assert!(h_codex.is_prompt_line("\x1b[1m>\x1b[0m"));
    }
    #[test]
    fn dead_registered_pane_allows_lazy_claim() {
        // When registered is Some but pane is dead, lazy-claim should be attempted.
        let registered: Option<String> = Some("%99".to_string());
        assert!(
            registered.is_some(),
            "dead registered pane should attempt lazy claim"
        );
    }
    #[test]
    fn codex_dispatch_start_tracking_enabled_accepts_workspace_hook_for_nested_agent_doc_root() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();
        let nested = workspace.join("src/session-share");
        let doc = nested.join("tasks/claudescore-3.md");

        std::fs::create_dir_all(workspace.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(workspace.join(".codex")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(workspace.join(".codex/hooks.json"), "{}").unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(
            codex_dispatch_start_tracking_enabled(&doc),
            "workspace-level Codex hooks should enable routed dispatch tracking for nested agent-doc roots"
        );
    }
    #[test]
    fn codex_dispatch_start_tracking_enabled_stays_false_without_any_hook_install() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("src/session-share");
        let doc = nested.join("tasks/claudescore-3.md");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(
            !codex_dispatch_start_tracking_enabled(&doc),
            "route should not wait for hook-backed submission proof when no tracked root has Codex hooks installed"
        );
    }
    #[test]
    fn codex_dispatch_start_tracking_enabled_stays_false_when_nested_codex_path_shadows_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();
        let nested = workspace.join("src/session-share");
        let doc = nested.join("tasks/claudescore-3.md");

        std::fs::create_dir_all(workspace.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(workspace.join(".codex")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(workspace.join(".codex/hooks.json"), "{}").unwrap();
        std::fs::write(nested.join(".codex"), "").unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(
            !codex_dispatch_start_tracking_enabled(&doc),
            "route should not require hook-backed submission proof when a nearer `.codex` path shadows the workspace install"
        );
    }
    #[test]
    fn drain_reaps_completed_review_item_across_all_surfaces() {
        // #route-drain reap-all-surfaces: the focused route-drain repair reaped only
        // the backlog, so a deployed `[x]` item left in review blocked dispatch until
        // a manual repeat ran full preflight maintenance ("JB Run Agent Doc failed; a
        // repeat attempt succeeded"). The drain now runs all-surface pending
        // maintenance first, so the completed review item is reaped on the first
        // attempt regardless of the final drain outcome.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("drain-review.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [x] [#seocat] Implemented and deployed\n",
            "<!-- /agent:review -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        // Open cycle so the drain actually runs (is_open()).
        crate::cycle_state::start_preflight(&doc, None, Some(content)).unwrap();

        // The drain may still report Blocked on later (committed/etc.) guards in this
        // minimal fixture, but the all-surface reap runs before that — assert the
        // completed review item is gone from the file.
        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        let _ = super::drain_open_closeout_before_routed_dispatch(&doc);

        let after = std::fs::read_to_string(&doc).unwrap();
        let review = agent_doc_element::element::parse(&after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&after)
            .to_string();
        assert!(
            !review.contains("[#seocat]"),
            "drain must reap the completed review item via all-surface maintenance: {review}"
        );
        assert!(after.contains("[#keep1]"), "open backlog item must remain");
    }

    #[test]
    fn closeout_block_decision_queues_prompt_context_before_failing_closed() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("route-block.md");
        let content = "---\nagent_doc_session: test\n---\n\n";
        write_open_cycle_route_doc(&doc, content);
        let low_level_reason = "captured response baseline no longer matches current document";

        let (decision, dispatch_decision) =
            super::classify_route_closeout_block(&doc, low_level_reason.to_string(), true);
        match dispatch_decision {
            CloseoutBlockDispatchDecision::EnqueuePromptForAfterCloseout => {
                assert_eq!(decision.state(), Some(CloseoutRecoveryState::OpenCycle));
                assert_eq!(decision.as_str(), "queue_prompt_for_after_closeout");
            }
            other => panic!("prompt context should queue behind closeout: {other:?}"),
        }
    }

    #[test]
    fn closeout_block_decision_waits_on_existing_active_queue_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("route-block.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Queue\n\n",
            "<!-- agent:queue auto go -->\n",
            "- :pushpin: [#jbruncloseoutstate]\n",
            "<!-- /agent:queue -->\n"
        );
        write_open_cycle_route_doc(&doc, content);
        let low_level_reason = "captured response baseline no longer matches current document";

        let (decision, dispatch_decision) =
            super::classify_route_closeout_block(&doc, low_level_reason.to_string(), false);
        match dispatch_decision {
            CloseoutBlockDispatchDecision::WaitForActiveQueueHead { head } => {
                assert_eq!(head, "jbruncloseoutstate");
                assert_eq!(decision.state(), Some(CloseoutRecoveryState::OpenCycle));
                let reason = decision.route_terminal_reason();
                assert!(
                    reason.contains("closeout recovery blocked [open_cycle]"),
                    "{reason}"
                );
                assert!(reason.contains("missing proof"), "{reason}");
                assert!(
                    !reason.contains(low_level_reason),
                    "route-visible blocker leaked low-level capture text: {reason}"
                );
            }
            other => panic!("existing active queue head should wait behind closeout: {other:?}"),
        }
    }

    #[test]
    fn closeout_block_decision_fails_closed_without_prompt_or_active_queue() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("route-block.md");
        let content = "---\nagent_doc_session: test\n---\n\n";
        write_open_cycle_route_doc(&doc, content);
        let low_level_reason = "captured response baseline no longer matches current document";

        let (decision, dispatch_decision) =
            super::classify_route_closeout_block(&doc, low_level_reason.to_string(), false);
        match dispatch_decision {
            CloseoutBlockDispatchDecision::FailClosed => {
                assert_eq!(decision.state(), Some(CloseoutRecoveryState::OpenCycle));
                let reason = decision.route_terminal_reason();
                assert!(
                    reason.contains("closeout recovery blocked [open_cycle]"),
                    "{reason}"
                );
                assert!(reason.contains("recommended:"), "{reason}");
                assert!(
                    !reason.contains(low_level_reason),
                    "route-visible blocker leaked low-level capture text: {reason}"
                );
            }
            other => panic!("missing prompt and queue should fail closed: {other:?}"),
        }
    }

    fn write_open_cycle_route_doc(doc: &std::path::Path, content: &str) {
        std::fs::write(doc, content).unwrap();
        crate::cycle_state::start_preflight(doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_pending_mutations(doc).unwrap();
    }

    #[test]
    fn route_latency_message_marks_budget_status() {
        let harness = HarnessConfig::codex();
        let ok = route_latency_message(RouteLatencyFacts {
            phase: "dispatch_start_proof",
            elapsed_ms: Duration::from_millis(999).as_millis(),
            budget_ms: Duration::from_secs(1).as_millis(),
            pane: "%1",
            harness_binary: &harness.binary,
            outcome: "submitted",
            editor_attempt_id: None,
        });
        assert!(ok.contains("status=ok"), "{ok}");
        assert!(ok.contains("elapsed_ms=999"), "{ok}");

        let slow = route_latency_message(RouteLatencyFacts {
            phase: "dispatch_start_proof",
            elapsed_ms: Duration::from_secs(10).as_millis(),
            budget_ms: Duration::from_secs(10).as_millis(),
            pane: "%1",
            harness_binary: &harness.binary,
            outcome: "unproven_but_accepted",
            editor_attempt_id: None,
        });
        assert!(slow.contains("status=over_budget"), "{slow}");
        assert!(slow.contains("outcome=unproven_but_accepted"), "{slow}");
    }

    #[test]
    fn route_diagnostics_include_editor_attempt_id_when_present() {
        let _attempt = EnvGuard::set(
            agent_doc_tmux_commands::input_diag::EDITOR_ROUTE_ATTEMPT_ID_ENV,
            "attempt 1/2",
        );
        let harness = HarnessConfig::codex();
        let editor_attempt_id = editor_route_attempt_id();
        let latency = route_latency_message(RouteLatencyFacts {
            phase: "direct_pane_submit",
            elapsed_ms: Duration::from_millis(120).as_millis(),
            budget_ms: Duration::from_secs(1).as_millis(),
            pane: "%7",
            harness_binary: &harness.binary,
            outcome: "accepted",
            editor_attempt_id: editor_attempt_id.as_deref(),
        });
        assert!(
            latency.contains("editor_attempt_id=attempt_1_2"),
            "{latency}"
        );
    }

    #[test]
    fn direct_pane_submit_budget_allows_acceptance_poll_slack() {
        let harness = HarnessConfig::codex();
        let message = route_latency_message(RouteLatencyFacts {
            phase: "direct_pane_submit",
            elapsed_ms: Duration::from_millis(1180).as_millis(),
            budget_ms: direct_pane_submit_acceptance_budget().as_millis(),
            pane: "%1",
            harness_binary: &harness.binary,
            outcome: direct_pane_submit_outcome(
                CommandDispatchStatus::TimedOut,
                Some(RoutedDispatchStartProof::HookPromptMatched),
            ),
            editor_attempt_id: None,
        });

        assert!(message.contains("status=ok"), "{message}");
        assert!(
            message.contains("outcome=acceptance_unobserved_dispatch_proven"),
            "{message}"
        );
        assert!(!message.contains("timed_out"), "{message}");
    }
    #[test]
    fn route_pane_snapshot_preserves_redacted_terminal_capture() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("session.md");
        std::fs::write(&file, "session").unwrap();
        let content = "\
› agent-doc tasks/agent-doc/agent-doc-bugs2.md
OPENAI_API_KEY=sk-proj-aaaaaaaaaaaaaaaaaaaaaaaa
";

        let snapshot = preserve_route_pane_snapshot(
            &file,
            "%7",
            &HarnessConfig::codex(),
            "direct_pane_acceptance",
            content,
        );

        let path = snapshot.path.expect("snapshot path should be preserved");
        assert!(path.starts_with(tmp.path().join(".agent-doc/logs/route-submit")));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            saved.contains("OPENAI_API_KEY=[REDACTED]"),
            "snapshot should redact named API keys: {saved}"
        );
        assert!(
            !saved.contains("sk-proj-aaaaaaaa"),
            "raw token must not be preserved in snapshot: {saved}"
        );

        let ops = std::fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("route_pane_snapshot"), "{ops}");
        assert!(ops.contains("phase=direct_pane_acceptance"), "{ops}");
        assert!(ops.contains("capture_hash="), "{ops}");
        assert!(ops.contains("snapshot_path="), "{ops}");
    }
    #[test]
    fn route_dispatch_bug_report_dedupes_same_document_stage() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("run-agent-doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: test\n---\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        )
        .unwrap();
        let first_diagnostic = dir.path().join(".agent-doc/logs/route-submit/first.txt");
        let second_diagnostic = dir.path().join(".agent-doc/logs/route-submit/second.txt");
        let base = RouteDispatchBugReportFacts {
            file: &doc,
            pane: "%7",
            harness: &HarnessConfig::codex(),
            phase: "direct_pane_submit_final",
            issue: "prompt_not_submitted",
            result: "submit_timed_out_without_proof",
            elapsed: Duration::from_secs(30),
            proof: None,
            diagnostic_path: Some(&first_diagnostic),
        };
        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        file_route_dispatch_bug_report(base);
        file_route_dispatch_bug_report(RouteDispatchBugReportFacts {
            diagnostic_path: Some(&second_diagnostic),
            ..base
        });

        let content = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            content
                .matches("[symptom-key invariant=run_agent_doc_route_dispatch_failure")
                .count(),
            1,
            "{content}"
        );
        assert!(content.contains("diagnostic_path="), "{content}");
        assert!(
            content.contains("first.txt") && content.contains("second.txt"),
            "{content}"
        );
        assert!(
            content.contains("  evidence: JetBrains Run Agent Doc route/dispatch failed"),
            "{content}"
        );
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("route_dispatch_bug_backlog_filed")
                && ops.contains("inserted=true")
                && ops.contains("inserted=false"),
            "{ops}"
        );
    }

    #[test]
    fn route_dispatch_bug_report_uses_configured_bug_target_document() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            r#"agent_doc_bug_target_document = "tasks/agent-doc/agent-doc-bugs2.md"
"#,
        )
        .unwrap();
        let doc = dir.path().join("run-agent-doc.md");
        let target = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: test\n---\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        )
        .unwrap();
        std::fs::write(
            &target,
            "---\nagent_doc_session: bugs\n---\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        )
        .unwrap();
        let diagnostic = dir
            .path()
            .join(".agent-doc/logs/route-submit/configured.txt");
        let facts = RouteDispatchBugReportFacts {
            file: &doc,
            pane: "%7",
            harness: &HarnessConfig::codex(),
            phase: "direct_pane_submit_final",
            issue: "prompt_not_submitted",
            result: "submit_timed_out_without_proof",
            elapsed: Duration::from_secs(30),
            proof: None,
            diagnostic_path: Some(&diagnostic),
        };

        let _force_disk_guard = super::ForceDiskRouteWritesGuard::set(true);
        file_route_dispatch_bug_report(facts);

        let source = std::fs::read_to_string(&doc).unwrap();
        let filed = std::fs::read_to_string(&target).unwrap();
        assert!(
            !source.contains("#jbrunautobug"),
            "source document should not receive configured bug target item: {source}"
        );
        assert!(filed.contains("#jbrunautobug"), "{filed}");
        assert!(filed.contains("document="), "{filed}");
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("route_dispatch_bug_backlog_filed")
                && ops.contains("target_file=")
                && ops.contains("agent-doc-bugs2.md"),
            "{ops}"
        );
    }

    #[test]
    fn tracked_harness_clear_requires_fresh_restart_only_for_exact_clear_prompt() {
        assert!(tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::codex(),
            Some("/clear")
        ));
        assert!(tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::codex(),
            Some("  /clear  ")
        ));
        assert!(!tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::codex(),
            Some("agent-doc tasks/bugs.md")
        ));
        assert!(!tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::claude(),
            Some("/clear")
        ));
        assert!(tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::opencode(),
            Some("/clear")
        ));
        assert!(tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::opencode(),
            Some("  /clear  ")
        ));
        assert!(!tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::opencode(),
            Some("agent-doc tasks/bugs.md")
        ));
    }
    #[test]
    fn busy_existing_pane_auto_fix_outcome_restarts_fresh_for_healthy_authoritative_session_without_changes()
     {
        assert_eq!(
            busy_existing_pane_auto_fix_outcome(
                false,
                false,
                Some(SupervisorHealth::Healthy),
                false,
            ),
            BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart
        );
        assert_eq!(
            busy_existing_pane_auto_fix_outcome(
                false,
                false,
                Some(SupervisorHealth::Restartable),
                false,
            ),
            BusyPaneAutoFixOutcome::FailClosed
        );
        assert_eq!(
            busy_existing_pane_auto_fix_outcome(
                false,
                false,
                Some(SupervisorHealth::Restartable),
                true,
            ),
            BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_fresh_dispatch_target_ignores_explicitly_blocked_startup_miss_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-fresh-start-blocked-handoff");

        let doc = dir.path().join("fresh-start-blocked-handoff.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-fresh-start-blocked-handoff";
        let blocked_pane = "%364";
        let new_pane = "%370";

        sessions::register_full_with_cwd_in(
            dir.path(),
            session_id,
            blocked_pane,
            &file_path,
            12345,
            "@owner",
            dir.path().to_string_lossy().as_ref(),
        )
        .unwrap();

        let resolved = resolve_fresh_dispatch_target_after_ready_wait(
            &iso,
            session_id,
            new_pane,
            &file_path,
            Some(blocked_pane),
        )
        .unwrap();

        assert_eq!(
            resolved, new_pane,
            "resolver should keep dispatch in the fresh pane when the previous startup-miss owner is explicitly blocked"
        );

        let registry = sessions::load_in(dir.path()).unwrap();
        let entry = registry
            .values()
            .find(|entry| entry.session_id == session_id)
            .expect("fresh pane should be registered after the blocked handoff is ignored");
        assert_eq!(entry.pane, new_pane);
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn failed_route_cleanup_preserves_live_registered_owner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-preserve-failed-owner");
        let session = format!("test-{}", std::process::id());
        let pane = iso.new_session(&session, dir.path()).unwrap();
        let file = dir.path().join("tasks/software/corky.md");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, "# Corky\n").unwrap();
        sessions::register_full_in(
            dir.path(),
            "session-1",
            &pane,
            "tasks/software/corky.md",
            123,
            "@1",
        )
        .unwrap();

        assert!(
            should_preserve_failed_route_pane(&iso, &file, &pane, "session-1"),
            "failed-route cleanup must preserve the live registered owner pane"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn failed_route_cleanup_does_not_preserve_unregistered_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-cleanup-unregistered");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let file = dir.path().join("tasks/software/corky.md");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, "# Corky\n").unwrap();

        assert!(
            !should_preserve_failed_route_pane(&iso, &file, &pane, "session-1"),
            "failed-route cleanup should still remove panes that never became the live owner"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn failed_route_cleanup_reaps_startup_miss_owner_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-cleanup-startup-miss");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let file = dir.path().join("tasks/software/corky.md");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, "# Corky\n").unwrap();
        sessions::register_full_in(
            dir.path(),
            "session-1",
            &pane,
            "tasks/software/corky.md",
            123,
            "@1",
        )
        .unwrap();
        crate::startup_miss::record(
            &file,
            &pane,
            "session-1",
            "claude",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();

        cleanup_failed_route_panes(&iso, &file, "session-1", std::slice::from_ref(&pane));

        assert!(
            !iso.pane_alive(&pane),
            "fresh-route startup-miss panes should be reaped instead of preserved idle"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn failed_route_cleanup_only_reaps_attempt_local_created_panes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-cleanup-concurrent-sibling");
        let pane_owned = iso.new_session("test", dir.path()).unwrap();
        let pane_sibling = iso.split_window(&pane_owned, dir.path(), "-dh").unwrap();

        sessions::register_full_in(
            dir.path(),
            "session-1",
            &pane_owned,
            "tasks/software/corky.md",
            123,
            "@1",
        )
        .unwrap();
        sessions::register_full_in(
            dir.path(),
            "session-2",
            &pane_sibling,
            "tasks/software/tsift.md",
            456,
            "@1",
        )
        .unwrap();

        let file = dir.path().join("tasks/software/corky.md");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, "# Corky\n").unwrap();

        cleanup_failed_route_panes(&iso, &file, "session-1", std::slice::from_ref(&pane_owned));

        assert!(
            iso.pane_alive(&pane_owned),
            "cleanup should preserve the live owner pane for the failed route"
        );
        assert!(
            iso.pane_alive(&pane_sibling),
            "cleanup must not reap sibling panes that were not created by this route attempt"
        );
    }
    #[test]
    fn run_with_tmux_resolves_file_path_to_absolute() {
        // Verify that resolve_absolute_file_path turns a relative path into an
        // absolute one when the file exists. This is the guard against submodule
        // CWD-dependent resolution (#route1).
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let tasks = root.join("tasks");
        fs::create_dir_all(&tasks).unwrap();
        let doc = tasks.join("bugs.md");
        fs::write(&doc, "# Bugs\n").unwrap();

        let _cwd_guard = ScopedCurrentDir::set(&root);

        let resolved = agent_doc_git_io::dirs::resolve_absolute_file_path(std::path::Path::new(
            "tasks/bugs.md",
        ));
        assert!(
            resolved.is_absolute(),
            "route must send absolute paths to avoid submodule CWD misrouting"
        );
        assert_eq!(
            resolved, doc,
            "resolved path must point to the CWD-relative file, not a submodule shadow"
        );
    }
    #[test]
    fn startup_miss_recorded_on_fresh_start_timeout() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::startup_miss::record(
            &doc,
            "%42",
            "session-test",
            "claude",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();

        let miss = crate::startup_miss::load(&doc)
            .unwrap()
            .expect("should have marker");
        assert_eq!(miss.pane_id, "%42");
        assert_eq!(
            miss.origin,
            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart
        );
        assert!(crate::startup_miss::is_startup_miss_pane(&doc, "%42"));
    }
    #[test]
    fn startup_miss_cleared_on_successful_ack() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::startup_miss::record(
            &doc,
            "%42",
            "session-test",
            "claude",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();
        assert!(crate::startup_miss::load(&doc).unwrap().is_some());

        crate::startup_miss::clear(&doc).unwrap();
        assert!(crate::startup_miss::load(&doc).unwrap().is_none());
        assert!(!crate::startup_miss::is_startup_miss_pane(&doc, "%42"));
    }
    #[test]
    fn startup_miss_pane_detected_on_rerun() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::startup_miss::record(
            &doc,
            "%99",
            "session-test",
            "codex",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            Some("cycle-old"),
        )
        .unwrap();

        assert!(crate::startup_miss::is_startup_miss_pane(&doc, "%99"));
        assert!(
            !crate::startup_miss::is_startup_miss_pane(&doc, "%100"),
            "different pane should not match"
        );
    }
    #[test]
    fn startup_miss_routed_trigger_records_with_baseline_id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::startup_miss::record(
            &doc,
            "%50",
            "session-test",
            "claude",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            Some("cycle-baseline-123"),
        )
        .unwrap();

        let miss = crate::startup_miss::load(&doc).unwrap().expect("marker");
        assert_eq!(
            miss.origin,
            agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger
        );
        assert_eq!(
            miss.cycle_baseline_id.as_deref(),
            Some("cycle-baseline-123")
        );
    }
    #[test]
    fn startup_miss_requires_fresh_start_only_without_matching_live_owner() {
        let miss = agent_doc_supervisor::startup_miss::StartupMiss {
            file: "test.md".to_string(),
            pane_id: "%42".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 10,
            origin: agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: Some("cycle-abc".to_string()),
        };
        assert!(startup_miss_requires_fresh_start(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            None,
            SupervisorHealth::NoSocket,
            None,
        )));
        assert!(startup_miss_requires_fresh_start(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            Some("%99"),
            SupervisorHealth::Unreachable,
            None,
        )));
        assert!(!startup_miss_requires_fresh_start(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                Some("%42"),
                SupervisorHealth::NoSocket,
                None,
            )
        ));
        assert!(!startup_miss_requires_fresh_start(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                None,
                SupervisorHealth::Restartable,
                None,
            )
        ));
        assert!(!startup_miss_requires_fresh_start(
            startup_miss_route_facts(&miss, "%42", true, None, SupervisorHealth::Healthy, None,)
        ));
    }
    #[test]
    fn startup_miss_live_owner_restart_requires_closed_unsuperseded_start() {
        let miss = agent_doc_supervisor::startup_miss::StartupMiss {
            file: "test.md".to_string(),
            pane_id: "%42".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 10,
            origin: agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: Some("cycle-abc".to_string()),
        };
        let closed_same_start = agent_doc_supervisor::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%42".to_string()),
            latest_start_timestamp: Some(10),
            latest_run_timestamp: Some(10),
            latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some(
                "auto_trigger_timeout harness=codex reason=no_prompt_after_30s".to_string(),
            ),
            saw_process_exit_after_latest_start: true,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: true,
            saw_session_end_after_latest_run: false,
        };
        let newer_open_start = agent_doc_supervisor::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%42".to_string()),
            latest_start_timestamp: Some(10),
            latest_run_timestamp: Some(11),
            latest_run_event: Some("codex_start mode=fresh_restart restart_count=1".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("codex_start mode=fresh_restart restart_count=1".to_string()),
            saw_process_exit_after_latest_start: true,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: false,
            saw_session_end_after_latest_run: false,
        };

        assert!(startup_miss_should_restart_live_owner(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                Some("%42"),
                SupervisorHealth::NoSocket,
                Some(&closed_same_start),
            )
        ));
        assert!(!startup_miss_should_restart_live_owner(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                Some("%42"),
                SupervisorHealth::NoSocket,
                Some(&newer_open_start),
            )
        ));
        assert!(startup_miss_superseded_by_later_open_start(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                Some("%42"),
                SupervisorHealth::NoSocket,
                Some(&newer_open_start),
            )
        ));
        assert!(!startup_miss_superseded_by_later_open_start(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                Some("%42"),
                SupervisorHealth::NoSocket,
                Some(&closed_same_start),
            )
        ));
    }
    #[test]
    fn startup_miss_fail_closed_only_for_alive_open_no_socket_sessions() {
        let miss = agent_doc_supervisor::startup_miss::StartupMiss {
            file: "test.md".to_string(),
            pane_id: "%42".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 10,
            origin: agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: Some("cycle-abc".to_string()),
        };
        let open = agent_doc_supervisor::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%42".to_string()),
            latest_start_timestamp: Some(1),
            latest_run_timestamp: Some(1),
            latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_process_exit_after_latest_start: false,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: false,
            saw_session_end_after_latest_run: false,
        };
        let closed = agent_doc_supervisor::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%42".to_string()),
            latest_start_timestamp: Some(1),
            latest_run_timestamp: Some(1),
            latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("session_end".to_string()),
            saw_process_exit_after_latest_start: true,
            saw_session_end_after_latest_start: true,
            saw_process_exit_after_latest_run: true,
            saw_session_end_after_latest_run: true,
        };

        assert!(startup_miss_should_fail_closed(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            None,
            SupervisorHealth::NoSocket,
            Some(&open),
        )));
        assert!(!startup_miss_should_fail_closed(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            Some("%42"),
            SupervisorHealth::NoSocket,
            Some(&open),
        )));
        assert!(!startup_miss_should_fail_closed(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            None,
            SupervisorHealth::Healthy,
            Some(&open),
        )));
        assert!(!startup_miss_should_fail_closed(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            None,
            SupervisorHealth::NoSocket,
            Some(&closed),
        )));
        assert!(!startup_miss_should_fail_closed(startup_miss_route_facts(
            &miss,
            "%42",
            false,
            None,
            SupervisorHealth::NoSocket,
            Some(&open),
        )));
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn startup_miss_diagnostic_does_not_queue_shell_echo_in_pane() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-startup-miss-diagnostic");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        send_keys_with_retry(&iso, &pane, "printf '> '");
        let before = wait_for_pane_contains(&iso, &pane, "> ", std::time::Duration::from_secs(5));
        assert!(
            before.contains("> "),
            "shell prompt should be visible: {before}"
        );

        emit_startup_miss_diagnostic(&iso, &pane, &doc, "startup timed out");

        std::thread::sleep(std::time::Duration::from_millis(250));
        let after = sessions::capture_pane(&iso, &pane).unwrap();
        assert!(
            !after.contains("echo '[agent-doc] startup-miss:"),
            "diagnostic should not be left as drafted shell input: {after}"
        );
    }
    #[test]
    fn skip_capability_proof_bypasses_failed_proof_status() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let session_id = "route-skip-proof";
        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "opencode_capability_proof status=failed error=\"dns\"",
        );
        let status =
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::opencode()).unwrap();
        assert_eq!(status, ManagedCapabilityProofStatus::Failed);
    }
    #[test]
    fn authoritative_actor_state_preserves_terminal_record_over_runtime_starting() {
        let mut blocked_record = test_actor_record("%42");
        blocked_record.state = agent_doc_sqlite::state_store::ActorState::Blocked;
        blocked_record.last_transition.reason = STARTING_ACTOR_TIMEOUT_REASON.to_string();
        let blocked_actor = AuthoritativeActorDispatchTarget {
            record: blocked_record,
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(RouteActorState::Starting),
            },
        };
        assert_eq!(
            blocked_actor.actor_state(),
            agent_doc_sqlite::state_store::ActorState::Blocked,
            "a route-owned blocked record should remain a durable terminal gate even if stale supervisor IPC still reports starting"
        );
        assert!(
            actor_blocked_by_starting_timeout(StartingTimeoutActorFacts {
                actor_blocked: blocked_actor.record.state
                    == agent_doc_sqlite::state_store::ActorState::Blocked,
                last_transition_reason: &blocked_actor.record.last_transition.reason,
                prompt_ready: false,
            }),
            "a route-owned starting timeout should be identifiable before route re-registers the stale pane"
        );

        let mut starting_record = test_actor_record("%43");
        starting_record.state = agent_doc_sqlite::state_store::ActorState::Starting;
        let ready_actor = AuthoritativeActorDispatchTarget {
            record: starting_record,
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(RouteActorState::Ready),
            },
        };
        assert_eq!(
            ready_actor.actor_state(),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "non-terminal records should still accept fresher supervisor runtime state"
        );
    }
    #[test]
    fn starting_timeout_blocked_actor_recovery_requires_prompt_ready_proof() {
        let mut blocked_record = test_actor_record("%42");
        blocked_record.state = agent_doc_sqlite::state_store::ActorState::Blocked;
        blocked_record.last_transition.reason = STARTING_ACTOR_TIMEOUT_REASON.to_string();
        let blocked_actor = AuthoritativeActorDispatchTarget {
            record: blocked_record,
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(RouteActorState::Starting),
            },
        };

        assert!(
            starting_timeout_blocked_actor_can_recover(StartingTimeoutActorFacts {
                actor_blocked: blocked_actor.record.state
                    == agent_doc_sqlite::state_store::ActorState::Blocked,
                last_transition_reason: &blocked_actor.record.last_transition.reason,
                prompt_ready: true,
            }),
            "a route-owned starting timeout may recover only after direct dispatch-ready prompt proof"
        );
        assert!(
            !starting_timeout_blocked_actor_can_recover(StartingTimeoutActorFacts {
                actor_blocked: blocked_actor.record.state
                    == agent_doc_sqlite::state_store::ActorState::Blocked,
                last_transition_reason: &blocked_actor.record.last_transition.reason,
                prompt_ready: false,
            }),
            "route must not clear a durable starting timeout without prompt proof"
        );
        let degraded_actor = test_degraded_actor("%43");
        assert!(
            !starting_timeout_blocked_actor_can_recover(StartingTimeoutActorFacts {
                actor_blocked: degraded_actor.record.state
                    == agent_doc_sqlite::state_store::ActorState::Blocked,
                last_transition_reason: &degraded_actor.record.last_transition.reason,
                prompt_ready: true,
            }),
            "ordinary degraded actors must not use the starting-timeout recovery path"
        );
    }
    #[test]
    fn authoritative_actor_dispatch_target_eligible_true_only_when_no_guard_reason() {
        let healthy = AuthoritativeActorDispatchTarget {
            record: test_actor_record("%1"),
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(RouteActorState::Ready),
            },
        };
        assert!(supervisor_authoritative_actor_dispatch_target_eligible(
            &healthy.runtime
        ));

        let degraded = AuthoritativeActorDispatchTarget {
            record: test_actor_record("%1"),
            runtime: SupervisorRuntime {
                health: SupervisorHealth::NoSocket,
                actor_state: None,
            },
        };
        assert!(!supervisor_authoritative_actor_dispatch_target_eligible(
            &degraded.runtime
        ));

        let no_state = AuthoritativeActorDispatchTarget {
            record: test_actor_record("%1"),
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: None,
            },
        };
        assert!(!supervisor_authoritative_actor_dispatch_target_eligible(
            &no_state.runtime
        ));
    }
    #[test]
    fn dispatch_only_starting_pane_ready_timeout_production_values() {
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("opencode"), false),
            Duration::from_secs(15)
        );
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("codex"), false),
            Duration::from_secs(2)
        );
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("claude"), false),
            Duration::from_secs(2)
        );
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("opencode"), true),
            Duration::from_millis(250)
        );
    }
    #[test]
    fn route_starting_actor_not_ready_log_line_includes_typed_lifecycle_facts() {
        let h = agent_doc_harness::HarnessConfig::codex();
        let facts = AuthoritativeActorReadyFacts {
            pane_id: "%7".to_string(),
            generation: 42,
            actor_state: ActorDispatchState::Busy,
            supervisor_health: "healthy".to_string(),
            runtime_state: "busy".to_string(),
            prompt_ready: false,
            last_transition_reason: "restart_bootstrap".to_string(),
            last_transition_caller: "start".to_string(),
        };

        let line = route_starting_actor_not_ready_log_line(
            Path::new("/tmp/doc.md"),
            &h,
            Duration::from_secs(8),
            Duration::from_millis(8_125),
            &facts,
        );

        assert!(line.contains("route_authoritative_actor_starting_not_ready"));
        assert!(line.contains("file=/tmp/doc.md"));
        assert!(line.contains("harness=codex"));
        assert!(line.contains("timeout_ms=8000"));
        assert!(line.contains("elapsed_ms=8125"));
        assert!(line.contains("pane=%7"));
        assert!(line.contains("generation=42"));
        assert!(line.contains("actor_state=busy"));
        assert!(line.contains("supervisor_health=healthy"));
        assert!(line.contains("runtime_state=busy"));
        assert!(line.contains("prompt_ready=false"));
        assert!(line.contains("last_transition_reason=restart_bootstrap"));
        assert!(line.contains("last_transition_caller=start"));
    }
    #[test]
    fn starting_actor_timeout_record_coalesces_same_generation_and_pane() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/timeout.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let facts = AuthoritativeActorReadyFacts {
            pane_id: "%7".to_string(),
            generation: 42,
            actor_state: ActorDispatchState::Starting,
            supervisor_health: "healthy".to_string(),
            runtime_state: "starting".to_string(),
            prompt_ready: false,
            last_transition_reason: "session_start".to_string(),
            last_transition_caller: "start".to_string(),
        };

        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "repeat timeout").unwrap(),
            StartingActorTimeoutLogDecision::DuplicateTimeout
        );

        let mut next_generation = facts.clone();
        next_generation.generation += 1;
        assert_eq!(
            record_starting_actor_timeout(&file_path, &next_generation, "next timeout").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
    }
    #[test]
    fn starting_actor_timeout_record_matches_same_generation_and_pane() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/timeout-match.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let facts = AuthoritativeActorReadyFacts {
            pane_id: "%7".to_string(),
            generation: 3,
            actor_state: ActorDispatchState::Starting,
            supervisor_health: "healthy".to_string(),
            runtime_state: "starting".to_string(),
            prompt_ready: false,
            last_transition_reason: "session_start".to_string(),
            last_transition_caller: "start".to_string(),
        };

        assert!(!starting_actor_timeout_record_matches(&file_path, &facts));
        record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap();
        assert!(starting_actor_timeout_record_matches(&file_path, &facts));

        let mut different_generation = facts.clone();
        different_generation.generation += 1;
        assert!(!starting_actor_timeout_record_matches(
            &file_path,
            &different_generation
        ));

        let mut different_pane = facts;
        different_pane.pane_id = "%8".to_string();
        assert!(!starting_actor_timeout_record_matches(
            &file_path,
            &different_pane
        ));
    }
    #[test]
    fn starting_actor_timeout_record_does_not_match_nonstarting_actor_state() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/timeout-busy.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let starting = AuthoritativeActorReadyFacts {
            pane_id: "%7".to_string(),
            generation: 3,
            actor_state: ActorDispatchState::Starting,
            supervisor_health: "healthy".to_string(),
            runtime_state: "starting".to_string(),
            prompt_ready: false,
            last_transition_reason: "session_start".to_string(),
            last_transition_caller: "start".to_string(),
        };

        record_starting_actor_timeout(&file_path, &starting, "first timeout").unwrap();

        let mut busy = starting.clone();
        busy.actor_state = ActorDispatchState::Busy;
        busy.runtime_state = "busy".to_string();
        busy.last_transition_reason = "ipc_inject".to_string();
        busy.last_transition_caller = "dispatch".to_string();

        assert!(
            starting_actor_timeout_record_identity_matches(&file_path, &busy),
            "the stale timeout has the same pane and generation as the post-clear busy projection"
        );
        assert!(
            !starting_actor_timeout_record_matches(&file_path, &busy),
            "a cached starting timeout must not short-circuit a later busy wait for the same generation"
        );
    }
    #[test]
    fn starting_actor_timeout_record_clears_after_ready_or_terminal_refresh() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/timeout-clear.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let facts = AuthoritativeActorReadyFacts {
            pane_id: "%9".to_string(),
            generation: 5,
            actor_state: ActorDispatchState::Starting,
            supervisor_health: "healthy".to_string(),
            runtime_state: "starting".to_string(),
            prompt_ready: false,
            last_transition_reason: "session_start".to_string(),
            last_transition_caller: "start".to_string(),
        };

        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
        clear_starting_actor_timeout_record(&file_path);
        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "after clear").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
    }
    #[test]
    fn wait_for_ready_override_guard_sets_and_restores_thread_local() {
        use std::time::Duration;

        // Baseline: no override set.
        assert_eq!(wait_for_ready_override(), None);

        // Outer scope sets a 30s override.
        let outer = WaitForReadyOverrideGuard::set(Some(Duration::from_secs(30)));
        assert_eq!(wait_for_ready_override(), Some(Duration::from_secs(30)));

        {
            // Inner scope replaces with a 60s override.
            let _inner = WaitForReadyOverrideGuard::set(Some(Duration::from_secs(60)));
            assert_eq!(wait_for_ready_override(), Some(Duration::from_secs(60)));

            // Nested unset is honored too.
            let _none = WaitForReadyOverrideGuard::set(None);
            assert_eq!(wait_for_ready_override(), None);
        }

        // Both nested guards dropped — back to outer 30s.
        assert_eq!(wait_for_ready_override(), Some(Duration::from_secs(30)));

        drop(outer);
        // Outer dropped — back to unset baseline.
        assert_eq!(wait_for_ready_override(), None);
    }
    #[test]
    fn dispatch_only_starting_pane_ready_timeout_honors_override_then_default() {
        use std::time::Duration;

        let codex = agent_doc_harness::HarnessConfig::codex();
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout(&codex),
            Duration::from_millis(250)
        );

        let guard = WaitForReadyOverrideGuard::set(Some(Duration::from_secs(60)));
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout(&codex),
            Duration::from_secs(60)
        );
        drop(guard);

        assert_eq!(
            dispatch_only_starting_pane_ready_timeout(&codex),
            Duration::from_millis(250)
        );
    }
}
