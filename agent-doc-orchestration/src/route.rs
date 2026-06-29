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

use crate::flow::closeout::CloseoutRecoveryDecision;
use crate::flow::routed_reopen::{
    ActorDispatchState, ActorRuntimeHealth, AuthoritativeActorDispatchAction,
    AuthoritativeActorDispatchActionFacts, AuthoritativeActorReadyFacts,
    AuthoritativePromptReadyBarrierFacts, AuthoritativeRuntimeFacts, BusyPaneAutoFixFacts,
    BusyPaneAutoFixOutcome, DegradedAuthoritativeActorDirectSubmit,
    DegradedAuthoritativeActorFacts, DirectPaneSubmitStatus as CommandDispatchStatus,
    DispatchOnlyProofOutcomeFacts, DispatchOnlyReopenDelivery, DispatchStartProofDecision,
    DispatchStartProofFacts, PromptReadyBarrierDecision, ReopenMode, RoutedDispatchStartProof,
    RoutedReopenFacts, RoutedReopenGuardReason, StartingActorLogFacts,
    accepted_only_dispatch_start_log_message, accepted_only_dispatch_start_refusal_message,
    actor_dispatch_blocker_reason, actor_recovery_hint,
    authoritative_actor_dispatch_guard_reason as flow_authoritative_actor_dispatch_guard_reason,
    authoritative_actor_ready_retry_budget,
    busy_existing_pane_auto_fix_outcome as flow_busy_existing_pane_auto_fix_outcome,
    busy_projection_repaired_by_ready_prompt, can_use_degraded_authoritative_actor,
    classify_authoritative_actor_dispatch_action, classify_authoritative_prompt_ready_barrier,
    classify_dispatch_start_proof, decide_authoritative_reopen,
    degraded_authoritative_actor_direct_submit_log_message,
    direct_pane_submit_outcome as flow_direct_pane_submit_outcome,
    dispatch_only_blocked_guard_reason,
    dispatch_only_dispatch_start_proof_required as flow_dispatch_only_dispatch_start_proof_required,
    dispatch_only_focus_only_should_fail_closed, dispatch_only_sent_console_message,
    dispatch_only_sent_log_message, dispatch_only_starting_pane_ready_retry_budget,
    dispatch_only_starting_pane_recovery_retry_budget, log_dispatch_proof_failed,
    log_prompt_ready_barrier_failed,
    should_print_dispatch_only_unproven_progress as flow_should_print_dispatch_only_unproven_progress,
    starting_actor_not_ready_log_line, starting_actor_ready_log_line,
    starting_actor_terminal_log_line, starting_actor_timeout_coalesced_log_line,
};
use crate::harness::HarnessConfig;
use crate::sessions::Tmux;
use crate::supervisor::ipc::IpcMethod;
use crate::{frontmatter, prompt, resync, sessions, snapshot, sync};
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
                crate::ops_log::content_hash(next_content)
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

/// Seconds to report as the dispatch-only busy / not-ready wait in operator-facing
/// refusal messages: the caller's explicit `--wait-for-ready` override when set
/// (the time route actually waited), otherwise the harness recovery-timeout
/// `default`. Reporting the default alone is misleading when an editor passed a
/// longer override — the JetBrains plugin's `--wait-for-ready 60` made the refusal
/// claim "waiting 8s" (the Codex recovery constant) after a real 60s wait
/// (`#busy-not-ready-message-reports-actual-wait`).
fn dispatch_only_busy_refusal_wait_secs(default: Duration) -> u64 {
    wait_for_ready_override().unwrap_or(default).as_secs()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandDispatchResult {
    status: CommandDispatchStatus,
    elapsed: Duration,
    diagnostic_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteSubmitObservation {
    Accepted,
    TriggerStillVisible,
    CaptureFailed,
    DispatchStartProven,
    AcceptedWithoutDispatchProof,
}

impl RouteSubmitObservation {
    fn label(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::TriggerStillVisible => "trigger_still_visible",
            Self::CaptureFailed => "capture_failed",
            Self::DispatchStartProven => "dispatch_start_proven",
            Self::AcceptedWithoutDispatchProof => "accepted_without_dispatch_start_proof",
        }
    }

    fn issue(self) -> Option<&'static str> {
        match self {
            Self::TriggerStillVisible => Some("prompt_not_submitted"),
            Self::CaptureFailed => Some("submit_unverified_capture_failed"),
            Self::AcceptedWithoutDispatchProof => Some("accepted_without_dispatch_start_proof"),
            Self::Accepted | Self::DispatchStartProven => None,
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupervisorHealth {
    Healthy,
    Restartable,
    Halted { restart_count: u32 },
    Unreachable,
    NoSocket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SupervisorRuntime {
    health: SupervisorHealth,
    actor_state: Option<crate::session_actor::ActorState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthoritativeActorDispatchTarget {
    record: crate::session_actor::ActorRecord,
    runtime: SupervisorRuntime,
}

impl AuthoritativeActorDispatchTarget {
    fn actor_state(&self) -> crate::session_actor::ActorState {
        if matches!(
            self.record.state,
            crate::session_actor::ActorState::Blocked | crate::session_actor::ActorState::Closed
        ) {
            return self.record.state;
        }
        self.runtime.actor_state.unwrap_or(self.record.state)
    }
}

fn actor_blocked_by_starting_timeout(actor: &AuthoritativeActorDispatchTarget) -> bool {
    actor.record.state == crate::session_actor::ActorState::Blocked
        && actor.record.last_transition.reason == "starting_actor_timeout"
}

fn starting_timeout_blocked_actor_can_recover(
    actor: &AuthoritativeActorDispatchTarget,
    prompt_ready: bool,
) -> bool {
    actor_blocked_by_starting_timeout(actor) && prompt_ready
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingPromptBearingRouteContext {
    marker: String,
    prompt_text: String,
    slash_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RouteCloseoutDrainOutcome {
    NoOpenCycle,
    Recovered(String),
    Blocked(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RouteCloseoutBlockDecision {
    EnqueuePromptForAfterCloseout {
        decision: CloseoutRecoveryDecision,
    },
    WaitForActiveQueueHead {
        head: String,
        decision: CloseoutRecoveryDecision,
    },
    FailClosed {
        decision: CloseoutRecoveryDecision,
    },
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

fn route_latency_status(elapsed: Duration, budget: Duration) -> &'static str {
    if elapsed >= budget {
        "over_budget"
    } else {
        "ok"
    }
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

fn direct_pane_submit_acceptance_timeout() -> Duration {
    crate::flow::routed_reopen::direct_pane_submit_acceptance_timeout()
}

fn direct_pane_submit_acceptance_budget() -> Duration {
    crate::flow::routed_reopen::direct_pane_submit_acceptance_budget()
}

fn direct_pane_submit_outcome(
    status: CommandDispatchStatus,
    dispatch_start_proof: Option<RoutedDispatchStartProof>,
) -> &'static str {
    flow_direct_pane_submit_outcome(status, dispatch_start_proof)
}

fn route_latency_message(
    phase: &str,
    elapsed: Duration,
    budget: Duration,
    pane: &str,
    harness: &HarnessConfig,
    outcome: &str,
) -> String {
    let mut message = format!(
        "route_latency phase={} elapsed_ms={} budget_ms={} status={} pane={} harness={} outcome={}",
        phase,
        elapsed.as_millis(),
        budget.as_millis(),
        route_latency_status(elapsed, budget),
        pane,
        harness.binary,
        outcome
    );
    append_editor_route_attempt(&mut message);
    message
}

const EDITOR_ROUTE_ATTEMPT_ID_ENV: &str = "AGENT_DOC_EDITOR_ROUTE_ATTEMPT_ID";

fn editor_route_attempt_id() -> Option<String> {
    std::env::var(EDITOR_ROUTE_ATTEMPT_ID_ENV)
        .ok()
        .map(|value| route_snapshot_field(&value))
        .filter(|value| !value.is_empty())
}

fn route_current_actor_generation(file: &Path) -> Option<u64> {
    let canonical = file.canonicalize().ok()?;
    let root = crate::snapshot::find_project_root(&canonical)?;
    crate::session_actor::load_record_in(&root, canonical.to_string_lossy().as_ref())
        .ok()
        .flatten()
        .map(|record| record.generation)
}

fn route_ops_log_path(file: &Path) -> Option<PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let root = crate::snapshot::find_project_root(&canonical)?;
    Some(root.join(".agent-doc/logs/ops.log"))
}

fn append_editor_route_attempt(message: &mut String) {
    if let Some(attempt_id) = editor_route_attempt_id() {
        message.push_str(&format!(" editor_attempt_id={attempt_id}"));
    }
}

fn short_content_hash(content: &str) -> String {
    let hash = crate::ops_log::content_hash(content);
    hash[..hash.len().min(12)].to_string()
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

fn route_snapshot_field(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "none".to_string()
    } else {
        out
    }
}

fn preserve_route_pane_snapshot(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    phase: &str,
    content: &str,
) -> RoutePaneSnapshot {
    let redacted = crate::secret_redact::redact(content);
    let hash = crate::ops_log::content_hash(&redacted);
    let short_hash = &hash[..hash.len().min(12)];
    let snapshot = RoutePaneSnapshot {
        len: redacted.len(),
        hash: short_hash.to_string(),
        path: None,
    };

    let path = (|| -> Result<PathBuf> {
        let canonical = file
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", file.display()))?;
        let root = crate::snapshot::find_project_root(&canonical)
            .with_context(|| format!("could not find .agent-doc root for {}", file.display()))?;
        let dir = root.join(".agent-doc/logs/route-submit");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        let name = format!(
            "{}-{}-{}-{}-{}.txt",
            route_snapshot_timestamp_millis(),
            route_snapshot_field(phase),
            route_snapshot_field(&harness.binary),
            route_snapshot_field(pane),
            short_hash
        );
        let path = dir.join(name);
        std::fs::write(&path, redacted)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    })();

    match path {
        Ok(path) => {
            let mut message = format!(
                "route_pane_snapshot file={} pane={} harness={} phase={} capture_len={} capture_hash={} snapshot_path={}",
                file.display(),
                pane,
                harness.binary,
                phase,
                snapshot.len,
                snapshot.hash,
                path.display()
            );
            append_editor_route_attempt(&mut message);
            crate::ops_log::log_op(file, &message);
            RoutePaneSnapshot {
                path: Some(path),
                ..snapshot
            }
        }
        Err(err) => {
            let mut message = format!(
                "route_pane_snapshot_failed file={} pane={} harness={} phase={} capture_len={} capture_hash={} error={}",
                file.display(),
                pane,
                harness.binary,
                phase,
                snapshot.len,
                snapshot.hash,
                err.to_string().replace(char::is_whitespace, "_")
            );
            append_editor_route_attempt(&mut message);
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
    let mut message = format!(
        "[route] preserved dispatch-start proof snapshot for {} pane {} harness={} phase={} capture_len={} capture_hash={}",
        file.display(),
        pane,
        harness.binary,
        phase,
        snapshot.len,
        snapshot.hash
    );
    if let Some(path) = snapshot.path.as_ref() {
        message.push_str(&format!(" snapshot_path={}", path.display()));
    }
    append_editor_route_attempt(&mut message);
    eprintln!("{message}");
}

#[derive(Debug, Clone, Copy)]
struct RouteSubmitObservationFacts<'a> {
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

fn route_dispatch_bug_report_item(facts: RouteDispatchBugReportFacts<'_>) -> Result<String> {
    let doc_id = crate::pending_cmd::doc_id_for(facts.file);
    let component = format!("route/{}", route_snapshot_field(facts.phase));
    let content_hash =
        crate::ops_log::content_hash(&format!("{}:{}:{}", doc_id, facts.phase, facts.issue));
    let symptom_key = agent_doc_element_backlog::backlog::SymptomDedupeKey::new(
        "run_agent_doc_route_dispatch_failure",
        doc_id,
        component,
        format!("sha256:{content_hash}"),
    )?;
    let generation = route_current_actor_generation(facts.file)
        .map(|generation| generation.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let editor_attempt = editor_route_attempt_id().unwrap_or_else(|| "unknown".to_string());
    let proof = facts
        .proof
        .map(|proof| proof.dispatch_stage_label().to_string())
        .unwrap_or_else(|| "none".to_string());
    let diagnostic_path = facts
        .diagnostic_path
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "none".to_string());
    let ops_log_path = route_ops_log_path(facts.file)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let marker = format!(
        "route_submit_issue(issue={},phase={},result={})",
        facts.issue,
        route_snapshot_field(facts.phase),
        route_snapshot_field(facts.result)
    );

    Ok(format!(
        "JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class={} document={} stage={} pane={} actor_generation={} editor_attempt_id={} dispatch_proof_state={} elapsed_ms={} diagnostic_path={} ops_log_path={} ops_log_marker={} {}",
        facts.issue,
        facts.file.display(),
        facts.phase,
        facts.pane,
        generation,
        editor_attempt,
        proof,
        facts.elapsed.as_millis(),
        diagnostic_path,
        ops_log_path,
        marker,
        symptom_key.marker()
    ))
}

fn file_route_dispatch_bug_report(facts: RouteDispatchBugReportFacts<'_>) {
    let item = match route_dispatch_bug_report_item(facts) {
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
                    crate::secret_redact::redact(&err.to_string())
                        .replace(char::is_whitespace, "_")
                ),
            );
            return;
        }
    };
    let target_file = match crate::project_config::agent_doc_bug_target_document_for_doc(facts.file)
    {
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
                    crate::secret_redact::redact(&err.to_string())
                        .replace(char::is_whitespace, "_")
                ),
            );
            facts.file.to_path_buf()
        }
    };
    let items = [item];
    match crate::pending_cmd::with_force_disk_pending_writes(
        FORCE_DISK_ROUTE_WRITES.with(Cell::get),
        || crate::pending_cmd::add_many(&target_file, &items, false),
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
                    crate::secret_redact::redact(&err.to_string())
                        .replace(char::is_whitespace, "_")
                ),
            );
        }
    }
}

fn route_submit_observation_message(facts: RouteSubmitObservationFacts<'_>) -> String {
    let mut message = format!(
        "route_submit_observation file={} pane={} harness={} phase={} result={} elapsed_ms={}",
        facts.file.display(),
        facts.pane,
        facts.harness.binary,
        facts.phase,
        facts.observation.label(),
        facts.elapsed.as_millis()
    );
    if let Some(trigger_visible) = facts.trigger_visible {
        message.push_str(&format!(" trigger_visible={trigger_visible}"));
    }
    if let Some(capture_len) = facts.capture_len {
        message.push_str(&format!(" capture_len={capture_len}"));
    }
    if let Some(capture_hash) = facts.capture_hash {
        message.push_str(&format!(" capture_hash={capture_hash}"));
    }
    if let Some(proof) = facts.proof {
        message.push_str(&format!(" proof={}", proof.dispatch_stage_label()));
    }
    if let Some(issue) = facts.observation.issue() {
        message.push_str(&format!(" issue={issue}"));
    }
    append_editor_route_attempt(&mut message);
    message
}

fn route_submit_issue_message(facts: RouteSubmitObservationFacts<'_>) -> Option<String> {
    let issue = facts.observation.issue()?;
    let mut message = format!(
        "route_submit_issue file={} pane={} harness={} phase={} issue={} result={} elapsed_ms={}",
        facts.file.display(),
        facts.pane,
        facts.harness.binary,
        facts.phase,
        issue,
        facts.observation.label(),
        facts.elapsed.as_millis()
    );
    if let Some(trigger_visible) = facts.trigger_visible {
        message.push_str(&format!(" trigger_visible={trigger_visible}"));
    }
    if let Some(capture_len) = facts.capture_len {
        message.push_str(&format!(" capture_len={capture_len}"));
    }
    if let Some(capture_hash) = facts.capture_hash {
        message.push_str(&format!(" capture_hash={capture_hash}"));
    }
    if let Some(proof) = facts.proof {
        message.push_str(&format!(" proof={}", proof.dispatch_stage_label()));
    }
    append_editor_route_attempt(&mut message);
    Some(message)
}

fn log_route_submit_observation(facts: RouteSubmitObservationFacts<'_>) {
    crate::ops_log::log_op(facts.file, &route_submit_observation_message(facts));
    if let Some(issue) = route_submit_issue_message(facts) {
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
    let message = route_latency_message(phase, elapsed, budget, pane, harness, outcome);
    crate::ops_log::log_op(file, &message);
    if route_latency_status(elapsed, budget) == "over_budget" {
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

fn routed_dispatch_start_timeout(harness: &HarnessConfig) -> Duration {
    crate::flow::routed_reopen::routed_dispatch_start_timeout_for_binary(
        Some(harness.binary.as_str()),
        cfg!(test),
    )
}

fn codex_state_advanced(
    tracker: &RoutedDispatchStartTracker,
    state: &crate::codex_hook::ActiveSessionState,
) -> bool {
    let RoutedDispatchStartTracker::CodexHook {
        previous_session_id,
        previous_turn_id,
        previous_updated_at,
        ..
    } = tracker
    else {
        return false;
    };
    match (
        previous_session_id.as_deref(),
        previous_turn_id.as_deref(),
        *previous_updated_at,
    ) {
        (None, None, None) => true,
        (previous_session_id, previous_turn_id, previous_updated_at) => {
            previous_session_id != Some(state.session_id.as_str())
                || previous_turn_id != Some(state.last_turn_id.as_str())
                || previous_updated_at.is_none_or(|updated_at| state.updated_at > updated_at)
        }
    }
}

fn codex_routed_dispatch_start_proof(
    tracker: &RoutedDispatchStartTracker,
    state: &crate::codex_hook::ActiveSessionState,
) -> Option<RoutedDispatchStartProof> {
    let RoutedDispatchStartTracker::CodexHook { trigger, .. } = tracker else {
        return None;
    };
    if !codex_state_advanced(tracker, state) {
        return None;
    }

    if state.last_prompt.trim() == trigger.trim() {
        Some(RoutedDispatchStartProof::HookPromptMatched)
    } else {
        Some(RoutedDispatchStartProof::HookStateAdvanced)
    }
}

fn opencode_pane_state_changed_from_idle(
    harness: &HarnessConfig,
    trigger: &str,
    pre_dispatch_content: &str,
    current_content: &str,
) -> bool {
    if current_content == pre_dispatch_content
        || recent_lines_contain_trigger(current_content, trigger)
    {
        return false;
    }
    if ready_prompt_candidate(current_content, harness).is_some()
        || harness.is_idle_chrome_only_output(current_content)
    {
        return false;
    }
    harness.has_busy_cue(current_content)
        || current_content
            .lines()
            .map(crate::prompt::strip_ansi)
            .any(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty()
                    && !harness.is_ignorable_output_line(trimmed)
                    && !harness.is_dispatch_ready_prompt_line(trimmed)
            })
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
                    && let Some(proof) = codex_routed_dispatch_start_proof(tracker, &state)
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
                if opencode_pane_state_changed_from_idle(
                    harness,
                    trigger,
                    pre_dispatch_content,
                    &content,
                ) {
                    return Ok(Some(RoutedDispatchStartProof::PaneStateChanged));
                }
            }
        }
        std::thread::sleep(poll);
    }

    Ok(None)
}

fn format_associated_pane_resolution_error(
    file: &Path,
    candidates: &[crate::sync::AssociatedPaneCandidate],
    preferred_window: Option<&str>,
) -> String {
    let mut lines = vec![format!(
        "multiple tmux panes are associated with {}; route cannot safely auto-pick one.",
        file.display()
    )];
    if let Some(window_id) = preferred_window {
        lines.push(format!(
            "Preferred active window: {}. Resolve by inspecting one pane, claiming it explicitly, then killing the redundant panes.",
            window_id
        ));
    } else {
        lines.push(
            "Resolve by inspecting one pane, claiming it explicitly, then killing the redundant panes."
                .to_string(),
        );
    }
    for candidate in candidates {
        lines.push(format!(
            "  - {} session={} window={} ({}) cmd={} sources={}",
            candidate.pane_id,
            candidate.session_name,
            candidate.window_id,
            candidate.window_name,
            candidate.current_command,
            candidate.source_summary()
        ));
        lines.push(format!(
            "    view: tmux capture-pane -pt {} | tail -n 80",
            candidate.pane_id
        ));
        lines.push(format!(
            "    assign: agent-doc claim {} --pane {} --force",
            file.display(),
            candidate.pane_id
        ));
        lines.push(format!("    kill: tmux kill-pane -t {}", candidate.pane_id));
    }
    lines.join("\n")
}

fn format_associated_pane_selected_error(
    file: &Path,
    winner: &crate::sync::AssociatedPaneCandidate,
    redundant: &[crate::sync::AssociatedPaneCandidate],
) -> String {
    let mut lines = vec![format!(
        "route found legacy pane-association evidence for {}, but the normal path will not re-elect ownership from {}.",
        file.display(),
        winner.pane_id
    )];
    lines.push(
        "Inspect the candidate, claim it explicitly if it is authoritative, or kill it before rerouting."
            .to_string(),
    );
    lines.push(format!(
        "  - {} session={} window={} ({}) cmd={} sources={}",
        winner.pane_id,
        winner.session_name,
        winner.window_id,
        winner.window_name,
        winner.current_command,
        winner.source_summary()
    ));
    lines.push(format!(
        "    view: tmux capture-pane -pt {} | tail -n 80",
        winner.pane_id
    ));
    lines.push(format!(
        "    assign: agent-doc claim {} --pane {} --force",
        file.display(),
        winner.pane_id
    ));
    lines.push(format!("    kill: tmux kill-pane -t {}", winner.pane_id));
    for candidate in redundant {
        lines.push(format!(
            "  - redundant {} session={} window={} ({}) cmd={} sources={}",
            candidate.pane_id,
            candidate.session_name,
            candidate.window_id,
            candidate.window_name,
            candidate.current_command,
            candidate.source_summary()
        ));
    }
    lines.join("\n")
}

fn format_duplicate_pane_policy_error(
    session_name: &str,
    file_path: &str,
    anchor_pane: Option<&str>,
    cause: &str,
) -> String {
    let mut lines = vec![
        format!(
            "refusing to provision a duplicate tmux pane for {} in session '{}': {}",
            file_path, session_name, cause
        ),
        "Inspect the existing panes first:".to_string(),
        format!(
            "  tmux list-panes -t {}:agent-doc -F '#{{pane_id}} #{{window_name}} #{{pane_current_command}} #{{pane_current_path}}'",
            session_name
        ),
        format!(
            "  tmux list-panes -a -F '#{{session_name}} #{{window_name}} #{{pane_id}} #{{pane_current_command}} #{{pane_current_path}}' | grep ' {}$'",
            file_path
        ),
    ];
    if let Some(anchor_pane) = anchor_pane {
        lines.push(format!(
            "  tmux capture-pane -pt {} | tail -n 80",
            anchor_pane
        ));
        lines.push(format!("  tmux kill-pane -t {}", anchor_pane));
    } else {
        lines.push("  tmux kill-pane -t <pane_id>".to_string());
    }
    lines.push(format!("Then rerun: agent-doc {}", file_path));
    lines.join("\n")
}

fn parse_actor_state(raw: &str) -> Option<crate::session_actor::ActorState> {
    match raw.trim() {
        "starting" => Some(crate::session_actor::ActorState::Starting),
        "ready" => Some(crate::session_actor::ActorState::Ready),
        "busy" => Some(crate::session_actor::ActorState::Busy),
        "waiting_input" => Some(crate::session_actor::ActorState::WaitingInput),
        "closed" => Some(crate::session_actor::ActorState::Closed),
        "blocked" => Some(crate::session_actor::ActorState::Blocked),
        _ => None,
    }
}

fn actor_dispatch_state(state: crate::session_actor::ActorState) -> ActorDispatchState {
    match state {
        crate::session_actor::ActorState::Ready => ActorDispatchState::Ready,
        crate::session_actor::ActorState::Starting => ActorDispatchState::Starting,
        crate::session_actor::ActorState::Busy => ActorDispatchState::Busy,
        crate::session_actor::ActorState::WaitingInput => ActorDispatchState::WaitingInput,
        crate::session_actor::ActorState::Blocked => ActorDispatchState::Blocked,
        crate::session_actor::ActorState::Closed => ActorDispatchState::Closed,
    }
}

fn actor_runtime_health(health: SupervisorHealth) -> ActorRuntimeHealth {
    match health {
        SupervisorHealth::Healthy => ActorRuntimeHealth::Healthy,
        SupervisorHealth::Restartable => ActorRuntimeHealth::Restartable,
        SupervisorHealth::Halted { restart_count } => ActorRuntimeHealth::Halted { restart_count },
        SupervisorHealth::Unreachable => ActorRuntimeHealth::Unreachable,
        SupervisorHealth::NoSocket => ActorRuntimeHealth::NoSocket,
    }
}

fn supervisor_health_label(health: SupervisorHealth) -> String {
    actor_runtime_health(health).label()
}

fn runtime_actor_state_label(runtime: &SupervisorRuntime) -> &'static str {
    runtime
        .actor_state
        .map(crate::session_actor::ActorState::as_str)
        .unwrap_or("missing")
}

fn authoritative_actor_ready_facts_from_target(
    target: &AuthoritativeActorDispatchTarget,
    prompt_ready: bool,
) -> AuthoritativeActorReadyFacts {
    AuthoritativeActorReadyFacts {
        pane_id: target.record.pane_id.clone(),
        generation: target.record.generation,
        actor_state: actor_dispatch_state(target.actor_state()),
        supervisor_health: supervisor_health_label(target.runtime.health),
        runtime_state: runtime_actor_state_label(&target.runtime).to_string(),
        prompt_ready,
        last_transition_reason: target.record.last_transition.reason.clone(),
        last_transition_caller: target.record.last_transition.caller.clone(),
    }
}

fn authoritative_actor_dispatch_guard_reason(runtime: &SupervisorRuntime) -> Option<String> {
    flow_authoritative_actor_dispatch_guard_reason(AuthoritativeRuntimeFacts {
        health: actor_runtime_health(runtime.health),
        actor_state_present: runtime.actor_state.is_some(),
    })
}

fn authoritative_actor_dispatch_target_eligible(actor: &AuthoritativeActorDispatchTarget) -> bool {
    authoritative_actor_dispatch_guard_reason(&actor.runtime).is_none()
}

fn supervisor_socket_path(file: &Path, session_id: &str) -> Option<std::path::PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let project_root = snapshot::find_project_root(&canonical)?;
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
                    .and_then(parse_actor_state);
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
    let project_root = match snapshot::find_project_root(&canonical) {
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
        fresh_route_start_ack_timeout(),
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
    let fm = frontmatter::parse_for_file_with_context(&content, file, &rc).map(|(fm, _)| fm)?;
    #[cfg(test)]
    let global_config = crate::config::Config::default();
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
        fresh_route_start_ack_timeout(),
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

fn startup_miss_requires_fresh_start(
    registered_pane: &str,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
) -> bool {
    if live_owner == Some(registered_pane) {
        return false;
    }
    matches!(
        supervisor_health,
        SupervisorHealth::Unreachable | SupervisorHealth::NoSocket
    )
}

fn startup_miss_superseded_by_later_open_start(
    miss: &crate::startup_miss::StartupMiss,
    registered_pane: &str,
    log_status: Option<&crate::startup_miss::SessionLogStatus>,
) -> bool {
    log_status.is_some_and(|status| {
        status.latest_session_open()
            && status.latest_start_pane.as_deref() == Some(registered_pane)
            && crate::startup_miss::latest_open_run_timestamp(status)
                .is_some_and(|ts| ts > miss.timestamp)
    })
}

fn startup_miss_should_restart_live_owner(
    miss: &crate::startup_miss::StartupMiss,
    registered_pane: &str,
    live_owner: Option<&str>,
    log_status: Option<&crate::startup_miss::SessionLogStatus>,
) -> bool {
    live_owner == Some(registered_pane)
        && log_status.is_some_and(|status| {
            status.latest_session_closed()
                && status.latest_start_pane.as_deref() == Some(registered_pane)
                && status
                    .latest_start_timestamp
                    .is_some_and(|ts| ts <= miss.timestamp)
        })
}

fn startup_miss_should_fail_closed(
    pane_alive: bool,
    registered_pane: &str,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
    log_status: Option<&crate::startup_miss::SessionLogStatus>,
) -> bool {
    pane_alive
        && live_owner != Some(registered_pane)
        && matches!(
            supervisor_health,
            SupervisorHealth::Unreachable | SupervisorHealth::NoSocket
        )
        && log_status.is_some_and(crate::startup_miss::SessionLogStatus::latest_session_open)
}

fn startup_miss_route_provenance(
    tmux: &Tmux,
    pane_id: &str,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
    log_status: Option<&crate::startup_miss::SessionLogStatus>,
) -> String {
    let log_detail = match log_status {
        Some(status) => format!(
            "session_log={} {} last_event={}",
            crate::startup_miss::latest_log_outcome(status),
            crate::startup_miss::latest_log_anchor(status),
            crate::startup_miss::latest_log_last_event(status)
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

fn startup_miss_diagnostic_message(file: &Path, reason: &str) -> String {
    format!(
        "[agent-doc] startup-miss: {}. Run 'agent-doc start {}' to retry.",
        reason,
        file.display()
    )
}

fn busy_route_diagnostic_message(file: &Path, harness: &HarnessConfig) -> String {
    format!(
        "[agent-doc] routed follow-up for {} is still pending because the live {} session is busy. Finish or interrupt the current task, then rerun `Run Agent Doc` or `agent-doc route {}`.",
        file.display(),
        harness.binary,
        file.display()
    )
}

fn fail_if_recent_session_loss_window(file: &Path, session_id: &str) -> Result<()> {
    let Some(window) = crate::startup_miss::recent_session_loss_window(file, session_id)? else {
        return Ok(());
    };

    let first = crate::startup_miss::format_timestamp(window.first_timestamp);
    let last = crate::startup_miss::format_timestamp(window.last_timestamp);
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
    let msg = startup_miss_diagnostic_message(file, reason);
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
    let msg = busy_route_diagnostic_message(file, harness);
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

/// `#claude-busy-status-during-active-turn` (decision: status-surfacing only):
/// message for the dispatch-only path where the routed Run Agent Doc landed on a
/// pane that is busy on an active turn and was *queued* rather than refused. The
/// queued path previously returned `Ok` silently, so the operator saw nothing and
/// the session looked idle/unresponsive while a turn was in flight. This makes the
/// turn-in-progress state and the auto-queue outcome visible. Unlike the generic
/// busy diagnostic it does NOT tell the operator to rerun — the prompt will run on
/// its own when the current turn finishes.
fn busy_route_queued_diagnostic_message(file: &Path, harness: &HarnessConfig) -> String {
    format!(
        "[agent-doc] turn in progress — the live {} session is busy, so Run Agent Doc for {} was queued and will run when the current turn finishes. No need to rerun. {}",
        harness.binary,
        file.display(),
        user_outcome_fields(crate::flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner)
    )
}

fn user_outcome_fields(kind: crate::flow::outcome::UserFacingOutcomeKind) -> String {
    crate::flow::outcome::UserFacingOutcome::new(kind)
        .expect("static user-facing outcome is valid")
        .log_fields()
}

fn blocked_with_unblocker_fields(unblocker: &str) -> String {
    crate::flow::outcome::UserFacingOutcome::with_unblocker(
        crate::flow::outcome::UserFacingOutcomeKind::BlockedWithExactUnblocker,
        unblocker,
    )
    .expect("static user-facing unblocker is valid")
    .log_fields()
}

fn emit_busy_route_queued_diagnostic(
    tmux: &Tmux,
    pane_id: &str,
    file: &Path,
    harness: &HarnessConfig,
) {
    let msg = busy_route_queued_diagnostic_message(file, harness);
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

/// Determine if the file is in the first column of the editor layout.
/// When true, the new pane should be split BEFORE (left of) the existing pane.
/// Returns false when col_args is empty (no layout context — default to split right).
pub fn is_first_column(file: &Path, col_args: &[String]) -> bool {
    if col_args.len() < 2 {
        return false;
    }
    let file_str = file.to_string_lossy();
    // Check if file appears in the first --col arg
    if let Some(first_col) = col_args.first() {
        first_col.split(',').any(|f| f.trim() == file_str.as_ref())
    } else {
        false
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
    let _ = resync::prune_with_tmux(tmux); // Clean stale entries before lookup

    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    if let Err(err) = crate::queue_continuation::clear_cooldown_marker(file) {
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
    frontmatter::require_agent_doc_document(&content, file)?;
    let (mut updated_content, session_id) = frontmatter::ensure_session_for_file(&content, file)?;
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
        frontmatter::parse_for_file_with_context(&updated_content, file, &rc).map(|(f, _)| f)?;
    let global_config = rc.global_config();
    let mut harness = HarnessConfig::from_context(&fm, &global_config);
    if plain_trigger {
        apply_plain_trigger_override(&mut harness);
    }

    // Use absolute path for trigger commands to avoid CWD-dependent resolution
    // when the pane's CWD differs from the invoker's (e.g., narrowed to a
    // submodule root). Relative paths would resolve to the submodule's version
    // of the file when the same relative path exists in both locations.
    let file_path = crate::git::resolve_absolute_file_path(file)
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
        crate::template::remove_duplicate_answered_exchange_prompt_tail(&cleaned_content)
    {
        cleaned_content = tail_cleaned;
        removed_answered_tail = true;
    }
    if let Some(tail_cleaned) =
        crate::template::remove_post_exchange_duplicate_prompt_comments_preserving_docs(
            &cleaned_content,
            preserve_docs,
        )
    {
        cleaned_content = tail_cleaned;
        removed_comment = true;
    }
    crate::template::guard_no_duplicate_prompt_residue_outside_exchange(&cleaned_content)
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

/// Decision for the #pcp3a drain retry loop after a mid-drain `repair` +
/// `session_check` failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrainRetryDecision {
    /// A concurrent finalize in another process closed the cycle — the drain is
    /// satisfied; routed dispatch may proceed.
    ConcurrentlyClosed,
    /// The cycle advanced concurrently and attempts remain — back off and retry.
    Retry,
    /// No concurrent progress observed, or attempts exhausted — fail closed.
    GiveUp,
}

/// Classify whether a mid-drain `repair` + `session_check` failure is a transient
/// concurrent-finalize race (#pcp3a). `original_*` is the cycle observed when the
/// drain started; `reloaded` is the cycle re-read after the failed check as
/// `(cycle_id, phase, is_open)`, or `None` when no open cycle remains on disk.
///
/// The route-owned supervisor can race the agent's own finalize writes on the
/// same document: a finalize in the *other* process moves the cycle/baseline
/// mid-drain, so `session_check` sees "captured response baseline no longer
/// matches current document". That is transient — the finalize will close the
/// cycle. We only retry when there is positive evidence of concurrent progress
/// (the cycle closed, or its `cycle_id`/`phase` advanced); a genuine,
/// non-advancing block fails closed immediately so we never paper over a real
/// stuck cycle.
fn classify_drain_retry(
    original_cycle_id: &str,
    original_phase: crate::cycle_state::CyclePhase,
    reloaded: Option<(&str, crate::cycle_state::CyclePhase, bool)>,
    attempt: u32,
    max_attempts: u32,
) -> DrainRetryDecision {
    match reloaded {
        None => DrainRetryDecision::ConcurrentlyClosed,
        Some((_, _, false)) => DrainRetryDecision::ConcurrentlyClosed,
        Some((cycle_id, phase, true)) => {
            let progressed = cycle_id != original_cycle_id || phase != original_phase;
            if progressed && attempt + 1 < max_attempts {
                DrainRetryDecision::Retry
            } else {
                DrainRetryDecision::GiveUp
            }
        }
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
                    crate::secret_redact::redact(&e.to_string())
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
        let decision = classify_drain_retry(
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
            DrainRetryDecision::ConcurrentlyClosed => {
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
            DrainRetryDecision::Retry => {
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
            DrainRetryDecision::GiveUp => break,
        }
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "route_dispatch_drain_closeout_blocked file={} cycle_id={} blocker={}",
            file.display(),
            state.cycle_id,
            crate::secret_redact::redact(&last_reason)
        ),
    );
    Ok(RouteCloseoutDrainOutcome::Blocked(last_reason))
}

fn classify_route_closeout_block(
    file: &Path,
    reason: String,
    has_prompt_context: bool,
) -> RouteCloseoutBlockDecision {
    let recovery_decision = crate::flow::closeout::decide_closeout_recovery(
        file,
        crate::flow::closeout::CloseoutRecoveryDecisionInput {
            prompt_context_available: has_prompt_context,
            blocker_reason: Some(&reason),
            stale_capture_supersession_proof: None,
        },
    );
    if matches!(
        recovery_decision,
        crate::flow::closeout::CloseoutRecoveryDecision::QueuePromptForAfterCloseout { .. }
    ) {
        return RouteCloseoutBlockDecision::EnqueuePromptForAfterCloseout {
            decision: recovery_decision,
        };
    }
    let active_queue_head = std::fs::read_to_string(file)
        .ok()
        .and_then(|content| crate::queue_continuation::live_continuation_head(file, &content));
    if let Some(head) = active_queue_head {
        return RouteCloseoutBlockDecision::WaitForActiveQueueHead {
            head,
            decision: recovery_decision,
        };
    }
    RouteCloseoutBlockDecision::FailClosed {
        decision: recovery_decision,
    }
}

/// `#routedrainnextaction`: format the user-facing outcome fields for a route
/// closeout block. When the underlying closeout recovery decision is `Blocked`
/// (a stuck cycle with a recommended recovery command — captured-response
/// baseline drift, IPC no_ack, etc.), surface `BlockedWithExactUnblocker` so
/// the operator sees the actual next action instead of the misleading
/// `wait_for_owner_turn_to_drain` (which implies a live owner turn is running).
///
/// The recovery command (`agent-doc reset ...` / `agent-doc write --commit
/// ...`) contains spaces, so it cannot ride in the validated single-token
/// `unblocker` field. Keep `unblocker` a short action token
/// (`run_recovery_command`) and append the literal command as a trailing
/// free-text `recovery_command=` field — it is always last on the log line, so
/// `key=value` parsing of the structured fields still works.
///
/// For every other decision variant (genuine queue-behind, replay-safe, etc.)
/// keep the historical `QueuedBehindOwner` outcome — those really are "wait for
/// the owner turn to drain" cases.
fn route_closeout_user_outcome_fields(
    decision: &crate::flow::closeout::CloseoutRecoveryDecision,
) -> String {
    use crate::flow::closeout::CloseoutRecoveryDecision as Decision;
    use crate::flow::outcome::UserFacingOutcomeKind;
    if let Decision::Blocked { recommended, .. } = decision {
        let command = extract_recovery_command(recommended).unwrap_or_else(|| recommended.clone());
        if let Ok(outcome) = crate::flow::outcome::UserFacingOutcome::with_unblocker(
            UserFacingOutcomeKind::BlockedWithExactUnblocker,
            "run_recovery_command",
        ) {
            return format!("{} recovery_command={}", outcome.log_fields(), command);
        }
    }
    user_outcome_fields(UserFacingOutcomeKind::QueuedBehindOwner)
}

/// Pull the first `agent-doc <subcommand> <FILE>` invocation out of a
/// closeout-recovery `recommended` string so the surfaced `recovery_command`
/// stays short and copy-pasteable. Markdown backticks are stripped first so a
/// command wrapped in `` `...` `` is detected (the leading backtick would
/// otherwise prevent the `agent-doc` start match). Returns `None` if no
/// `agent-doc` invocation is present (caller falls back to the full text).
fn extract_recovery_command(recommended: &str) -> Option<String> {
    // Strip markdown backticks so a `\`agent-doc ...\`` command is detected and
    // the trailing backtick does not glue onto the final path token.
    let cleaned = recommended.replace('`', " ");
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let mut start = None;
    let mut end = 0;
    for (i, &tok) in words.iter().enumerate() {
        if start.is_none() {
            if tok == "agent-doc" {
                start = Some(i);
            }
            continue;
        }
        // Stop at the first token that is not part of an agent-doc CLI word
        // (subcommand, file path, or a known short flag). Keep the surface
        // tight: just the command + subcommand + file (or short flag + arg).
        let is_cli_word = tok
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '='));
        if !is_cli_word {
            break;
        }
        end = i + 1;
    }
    let start = start?;
    if end <= start {
        return None;
    }
    let command = words[start..end].join(" ");
    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

fn queue_prompt_text_for_route_change(change_text: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in change_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!-- agent:boundary:") {
            continue;
        }
        let without_prompt_prefix = trimmed
            .strip_prefix('❯')
            .or_else(|| trimmed.strip_prefix('>'))
            .map(str::trim)
            .unwrap_or(trimmed);
        if !without_prompt_prefix.is_empty() {
            lines.push(without_prompt_prefix.to_string());
        }
    }
    let text = lines.join("\n").trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn operator_prioritize_route_prompt(prompt_text: String) -> String {
    if crate::queue_command::is_slash_command(&prompt_text) {
        return prompt_text;
    }
    if crate::queue::is_prioritized(&prompt_text) {
        prompt_text
    } else {
        format!(
            "{} {}",
            crate::queue::PRIORITIZED_MARKER,
            crate::queue::strip_priority_markers(&prompt_text)
        )
    }
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
    let prompt_text = queue_prompt_text_for_route_change(prompt_text)
        .ok_or_else(|| anyhow::anyhow!("route queue prompt is empty"))?;
    let prompt_text = if priority {
        operator_prioritize_route_prompt(prompt_text)
    } else {
        prompt_text
    };
    let prompt_identity = crate::queue::strip_priority_markers(&prompt_text);
    let _lock = acquire_route_queue_lock(file)?;
    let original = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let mut content = frontmatter::merge_queue_state(&original, true)?;
    let components = agent_doc_element::element::parse(&content)?;
    let mut component_created = false;
    let mut already_present = false;
    let mut appended = false;
    let mut superseded = false;

    if let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
        .cloned()
    {
        let body = &content[queue_component.open_end..queue_component.close_start];
        match crate::queue::parse(body) {
            Ok(mut entries) => {
                already_present = crate::queue::prompts(&entries).iter().any(|prompt| {
                    crate::queue::strip_priority_markers(&prompt.text) == prompt_identity
                });
                if !already_present {
                    let active_prompt_count = entries
                        .iter()
                        .filter(|entry| matches!(entry, crate::queue::QueueEntry::Prompt(_)))
                        .count();
                    // #jb-run-preempt-autoloop-priority: a priority dispatch must
                    // preempt, so it never supersedes a lone prompt — replacing
                    // would silently drop the pending queue item the manual run
                    // is jumping ahead of. Non-priority keeps the stale-prompt update.
                    let replace_single_auto_prompt = !priority
                        && crate::queue::has_auto_attr(&queue_component.attrs)
                        && active_prompt_count == 1;
                    if replace_single_auto_prompt {
                        for entry in &mut entries {
                            if let crate::queue::QueueEntry::Prompt(prompt) = entry {
                                prompt.multiline = prompt_text.contains('\n');
                                prompt.text = prompt_text.clone();
                                superseded = true;
                                break;
                            }
                        }
                    } else {
                        let new_prompt =
                            crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                                multiline: prompt_text.contains('\n'),
                                text: prompt_text.clone(),
                            });
                        if priority {
                            // Insert ahead of the first actionable prompt, preserving
                            // any leading queue directives (preset / start fence).
                            let insert_at = entries
                                .iter()
                                .position(|entry| {
                                    matches!(entry, crate::queue::QueueEntry::Prompt(_))
                                })
                                .unwrap_or(entries.len());
                            entries.insert(insert_at, new_prompt);
                        } else {
                            entries.push(new_prompt);
                        }
                        appended = true;
                    }
                }
                let rendered = crate::queue::render(&entries);
                content = queue_component.replace_content(&content, &rendered);
            }
            Err(parse_err) => {
                // The existing agent:queue body is polluted (e.g. user prose /
                // log dumps merged into the component by an earlier corruption).
                // Do NOT brick the route by propagating a fatal parse error —
                // preserve the existing body verbatim and append the new pending
                // dispatch as a well-formed entry beneath it, leaving the
                // corruption for separate repair (#jb-run-agent-doc-response-queue-contamination).
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_queue_dispatch_unparseable_preserved file={} prompt_hash={} reason={}",
                        file.display(),
                        crate::ops_log::content_hash(&prompt_text),
                        parse_err
                    ),
                );
                let new_rendered = crate::queue::render(std::slice::from_ref(
                    &crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                        multiline: prompt_text.contains('\n'),
                        text: prompt_text.clone(),
                    }),
                ));
                // Dedup against the raw body so a repeated dispatch into an
                // already-polluted queue stays idempotent.
                if body.lines().any(|line| line.trim() == new_rendered.trim())
                    || body.contains(prompt_text.as_str())
                {
                    already_present = true;
                    content = queue_component.replace_content(&content, body);
                } else {
                    let mut preserved = body.to_string();
                    if !preserved.is_empty() && !preserved.ends_with('\n') {
                        preserved.push('\n');
                    }
                    preserved.push_str(&new_rendered);
                    appended = true;
                    content = queue_component.replace_content(&content, &preserved);
                }
            }
        }
        content = strip_queue_component_auto_attr(&content)?;
    } else {
        component_created = true;
        appended = true;
        content = insert_queue_component(&content, &prompt_text)?;
    }

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
            appended,
            already_present,
            superseded,
            component_created,
            activated,
            prompt_text
        ),
    );
    Ok(RouteQueueEnqueueOutcome {
        prompt_text,
        appended,
        already_present,
        superseded,
        component_created,
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
    let (fm, _) = frontmatter::parse_for_file_with_context(content, file, &rc)?;
    if fm.queue_active == Some(true) {
        return Ok(None);
    }
    let components = agent_doc_element::element::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(None);
    };
    // A marker-side queue control (`start`/`go`/`stop`, #queue-state-unify) is
    // the marker spelling of the canonical `queue:` frontmatter control:
    // `start`/`go` are a fresh-activation gesture equivalent to the legacy `auto`
    // attribute, and `stop` forces the queue inactive. Mirror preflight's
    // `has_auto` resolution so JB `Run Agent Doc` activates a `queue: stop` +
    // `<!-- agent:queue go -->` document instead of treating `go` as inert.
    let marker_control = crate::queue::marker_control(&queue_component.attrs);
    if matches!(
        marker_control,
        Some(agent_doc_core::frontmatter::QueueControl::Stop)
    ) {
        return Ok(None);
    }
    let has_auto = crate::queue::has_auto_attr(&queue_component.attrs)
        || matches!(
            marker_control,
            Some(agent_doc_core::frontmatter::QueueControl::Start)
        );
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = crate::queue::parse(body)?;
    let activation = crate::queue::resolve_activation(&entries, has_auto, false, false);
    if !activation.active
        || crate::queue::has_stop_fence_at_head(&activation.entries_after)
        || crate::queue::time_gate_at_head(&activation.entries_after).is_some()
    {
        return Ok(None);
    }
    let Some(head) = crate::queue::first_prompt(&activation.entries_after) else {
        return Ok(None);
    };
    let head_text = head.text.clone();
    // #qdispatchloss: never let route consume/dispatch an inactive queue head
    // that is not backed by the committed snapshot. The head is read from the
    // live disk buffer, which the JB plugin may have synced from an
    // *uncommitted* operator edit (possibly half-typed). Activating/dispatching
    // it moves a bad/partial line into the agent prompt and then loses it — the
    // consume never lands in a committed snapshot, so the item is gone and the
    // turn stalls uncommitted. When the head diverges from the committed
    // snapshot, treat it as "still being edited" and fail closed (defer) so the
    // next preflight commits the queue edit first and dispatches the head
    // through the committed path. (Active-queue continuation heads go through
    // `queue_continuation::live_continuation_head`, not this inactive-activation
    // path, so the running auto-loop is unaffected.)
    if !route_queue_head_backed_by_committed_snapshot(file, &head_text) {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_uncommitted_head file={} decision=defer reason=head_not_in_committed_snapshot head={:?}",
                file.display(),
                crate::secret_redact::redact(&head_text)
            ),
        );
        return Ok(None);
    }
    Ok(Some(head_text))
}

/// `#qdispatchloss`: prove a candidate inactive queue head is backed by the
/// committed snapshot before route consumes/dispatches it.
///
/// Route selects the head from the live on-disk document
/// (`std::fs::read_to_string`), but the JB plugin may have synced an
/// uncommitted operator edit to disk before it reaches a git-committed
/// snapshot. Comparing the candidate head text against the queue prompts in the
/// committed snapshot (`snapshot::load`) distinguishes a durable, committed head
/// (safe to dispatch) from a fresh editor-buffer-only edit (must defer).
///
/// Conservative by design — only a head that is provably absent from a present
/// committed queue is treated as uncommitted:
/// - no committed snapshot yet (untracked scaffold) → allow (bootstrap escape
///   hatch; nothing to diverge from);
/// - snapshot unreadable / unparseable / queue body unparseable → allow (cannot
///   prove divergence, so do not stall a legitimate drain);
/// - committed snapshot has a queue component but the head text is not among its
///   prompt/completed entries → NOT backed (fail closed);
/// - committed snapshot has no queue component at all → NOT backed (the head is
///   a fresh uncommitted queue edit).
fn route_queue_head_backed_by_committed_snapshot(file: &Path, head_text: &str) -> bool {
    let snapshot = match crate::snapshot::load(file) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return true,
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_uncommitted_head_snapshot_unreadable file={} err={} decision=allow",
                    file.display(),
                    err
                ),
            );
            return true;
        }
    };
    let components = match agent_doc_element::element::parse(&snapshot) {
        Ok(components) => components,
        Err(_) => return true,
    };
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return false;
    };
    let body = &snapshot[queue_component.open_end..queue_component.close_start];
    let entries = match crate::queue::parse(body) {
        Ok(entries) => entries,
        Err(_) => return true,
    };
    entries.iter().any(|entry| match entry {
        crate::queue::QueueEntry::Prompt(prompt) | crate::queue::QueueEntry::Completed(prompt) => {
            prompt.text == head_text
        }
        _ => false,
    })
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
    let mut content = frontmatter::merge_queue_state(&original, true)?;
    content = strip_queue_component_auto_attr(&content)?;
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
    let base = snapshot::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .ok_or_else(|| {
            anyhow::anyhow!("failed to resolve queue lock root for {}", file.display())
        })?;
    let hash = crate::snapshot::doc_hash_from_str(&canonical.to_string_lossy());
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

fn strip_queue_component_auto_attr(content: &str) -> Result<String> {
    let components = agent_doc_element::element::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(content.to_string());
    };
    if !crate::queue::has_auto_attr(&queue_component.attrs) {
        return Ok(content.to_string());
    }
    let open_tag = &content[queue_component.open_start..queue_component.open_end];
    let new_tag = crate::queue::strip_auto_from_tag(open_tag);
    let mut result = String::with_capacity(content.len());
    result.push_str(&content[..queue_component.open_start]);
    result.push_str(&new_tag);
    result.push_str(&content[queue_component.open_end..]);
    Ok(result)
}

fn insert_queue_component(content: &str, prompt_text: &str) -> Result<String> {
    let body = crate::queue::render(&[crate::queue::QueueEntry::Prompt(
        crate::queue::QueuePrompt {
            multiline: prompt_text.contains('\n'),
            text: prompt_text.to_string(),
        },
    )]);
    let block = format!("<!-- agent:queue -->\n{}<!-- /agent:queue -->\n\n", body);
    let components = agent_doc_element::element::parse(content)?;
    let insert_at = components
        .iter()
        .find(|component| agent_doc_element::element::is_tracked_work_component(&component.name))
        .map(|component| component.open_start)
        .or_else(|| {
            components
                .iter()
                .find(|component| component.name == "exchange")
                .map(|component| component.close_end)
        })
        .unwrap_or(content.len());
    let mut result = String::with_capacity(content.len() + block.len() + 2);
    result.push_str(&content[..insert_at]);
    if insert_at > 0 && !result.ends_with("\n\n") {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
    }
    result.push_str(&block);
    result.push_str(&content[insert_at..]);
    Ok(result)
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
                    crate::startup_miss::StartupMissOrigin::FreshStart
                )
        })
}

fn failed_route_registry_root(file: &Path) -> Option<std::path::PathBuf> {
    let canonical = std::fs::canonicalize(file)
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    snapshot::find_project_root(&canonical)
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

fn dispatch_only_starting_pane_ready_timeout_for_binary(
    binary: Option<&str>,
    test_mode: bool,
) -> Duration {
    dispatch_only_starting_pane_ready_retry_budget(binary, test_mode).timeout
}

fn dispatch_only_starting_pane_ready_timeout(harness: &HarnessConfig) -> Duration {
    wait_for_ready_override().unwrap_or_else(|| {
        dispatch_only_starting_pane_ready_timeout_for_binary(
            Some(harness.binary.as_str()),
            cfg!(test),
        )
    })
}

fn dispatch_only_starting_pane_recovery_timeout(harness: Option<&HarnessConfig>) -> Duration {
    dispatch_only_starting_pane_recovery_retry_budget(
        harness.map(|h| h.binary.as_str()),
        cfg!(test),
    )
    .timeout
}

/// #route-busy-vs-starting-wording: word the authoritative-actor `FailClosed`
/// wait context. When the live pane shows a harness busy cue the actor is busy
/// on an active turn, not cold-starting, so the "(waited Ns for X startup)"
/// phrasing is misleading. `busy_cue` is the harness-specific reason from
/// [`HarnessConfig::dispatch_blocker_reason`] (e.g. `active claude turn`); `None`
/// keeps the cold-startup timeout wording.
fn failclosed_wait_context(
    harness: &HarnessConfig,
    busy_cue: Option<&str>,
    startup_secs: u64,
) -> String {
    match busy_cue {
        Some(cue) => format!(
            "the pane is busy on an active {} turn ({}), not cold-starting",
            harness.binary, cue
        ),
        None => format!("waited {}s for {} startup", startup_secs, harness.binary),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartingPaneRecoveryTarget {
    SamePane,
    DifferentPane(String),
}

fn starting_pane_generation_changed(
    initial_status: Option<&crate::startup_miss::SessionLogStatus>,
    current_status: &crate::startup_miss::SessionLogStatus,
    pane: &str,
) -> bool {
    if current_status.latest_start_pane.as_deref() != Some(pane)
        || !current_status.latest_session_open()
    {
        return false;
    }

    let Some(initial_status) = initial_status else {
        return false;
    };

    current_status.latest_start_timestamp != initial_status.latest_start_timestamp
        || current_status.latest_run_timestamp != initial_status.latest_run_timestamp
        || current_status.latest_run_event != initial_status.latest_run_event
}

fn starting_pane_recovery_target(
    initial_status: Option<&crate::startup_miss::SessionLogStatus>,
    current_status: Option<&crate::startup_miss::SessionLogStatus>,
    current_pane: &str,
    registered_pane: Option<&str>,
) -> Option<StartingPaneRecoveryTarget> {
    let current_status = current_status?;

    if let Some(registered_pane) = registered_pane
        && registered_pane != current_pane
        && current_status.latest_start_pane.as_deref() == Some(registered_pane)
        && current_status.latest_session_open()
    {
        return Some(StartingPaneRecoveryTarget::DifferentPane(
            registered_pane.to_string(),
        ));
    }

    if starting_pane_generation_changed(initial_status, current_status, current_pane) {
        return Some(StartingPaneRecoveryTarget::SamePane);
    }

    None
}

fn wait_for_starting_pane_recovery_target(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    current_pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    initial_status: Option<&crate::startup_miss::SessionLogStatus>,
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
fn busy_dispatch_only_should_wait_for_ready(
    dispatch_only: bool,
    actor_state: crate::session_actor::ActorState,
    has_queue_fallback: bool,
    pane_active_turn_busy: bool,
) -> bool {
    dispatch_only
        && actor_state == crate::session_actor::ActorState::Busy
        && !has_queue_fallback
        // #jb-run-agent-doc-busy-active-turn-stall: when the live pane proves a
        // genuine active turn (working spinner / `esc to interrupt`), the actor
        // will not return to a dispatch-ready prompt inside the busy ready-wait
        // budget — a multi-minute turn just produces a silent 60s stall before
        // the inevitable "session still running" refusal. Skip the wait so the
        // refusal (and the IDE's session-still-running notification) fires
        // immediately. A Busy projection WITHOUT a live active-turn cue
        // (transient/stale) still waits, so a turn about to finish is picked up.
        && !pane_active_turn_busy
}

fn dispatch_only_should_probe_active_turn_cue(
    dispatch_only: bool,
    actor_state: crate::session_actor::ActorState,
    prompt_context_present: bool,
    has_existing_inactive_queue_fallback: bool,
) -> bool {
    if !dispatch_only {
        return false;
    }
    match actor_state {
        crate::session_actor::ActorState::Ready => true,
        crate::session_actor::ActorState::Busy => {
            !prompt_context_present && !has_existing_inactive_queue_fallback
        }
        _ => false,
    }
}

/// Build the dispatch-only busy refusal message. When the live pane proved a
/// genuine active turn (`active_turn_busy_cue`), the busy ready-wait was skipped
/// (#jb-run-agent-doc-busy-active-turn-stall), so the "after waiting Ns" wording
/// would be misleading — word it as a busy active turn instead. Otherwise keep
/// the cold-start ready-wait wording.
fn dispatch_only_busy_refusal_message(
    harness: &HarnessConfig,
    generation: u64,
    file: &Path,
    dispatch_pane: &str,
    reason: &str,
    active_turn_busy_cue: Option<&str>,
    actor_state: crate::session_actor::ActorState,
) -> String {
    match active_turn_busy_cue {
        Some(cue) => format!(
            "authoritative actor generation {} for {} owns pane {} but dispatch-only route will not inject a new trigger because the pane is busy on an active {} turn ({}), not at a dispatch-ready prompt. {} {}",
            generation,
            file.display(),
            dispatch_pane,
            harness.binary,
            cue,
            authoritative_actor_dispatch_recovery_hint(actor_state, file),
            blocked_with_unblocker_fields("wait_for_owner_turn_to_finish")
        ),
        None => format!(
            "authoritative actor generation {} for {} owns pane {} but dispatch-only route will not inject a new trigger because {} did not return to a dispatch-ready prompt in the current generation after waiting {}s. {} {}",
            generation,
            file.display(),
            dispatch_pane,
            reason,
            dispatch_only_busy_refusal_wait_secs(dispatch_only_starting_pane_recovery_timeout(
                Some(harness)
            )),
            authoritative_actor_dispatch_recovery_hint(actor_state, file),
            blocked_with_unblocker_fields("wait_for_dispatch_ready_prompt")
        ),
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
            crate::flow::routed_reopen::RetryBudget::new(timeout, Duration::from_millis(100))
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
                    dispatch_eligible: authoritative_actor_dispatch_target_eligible(&refreshed),
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
            let candidate = ready_prompt_candidate(&content, harness);
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
            match classify_route_closeout_block(file, reason, prompt_context.is_some()) {
                RouteCloseoutBlockDecision::EnqueuePromptForAfterCloseout { decision } => {
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
                        route_closeout_user_outcome_fields(&decision)
                    );
                    return Ok(dispatch_pane);
                }
                RouteCloseoutBlockDecision::WaitForActiveQueueHead { head, decision } => {
                    let blocker = decision.route_terminal_reason();
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "route_dispatch_drain_closeout_wait_existing_queue file={} head={} blocker={}",
                            file.display(),
                            crate::secret_redact::redact(&head),
                            crate::secret_redact::redact(&blocker)
                        ),
                    );
                    eprintln!(
                        "[route] active closeout for {} could not be drained before reroute; existing queue head {:?} remains queued behind the closeout {}",
                        file.display(),
                        head,
                        route_closeout_user_outcome_fields(&decision)
                    );
                    return Ok(dispatch_pane);
                }
                RouteCloseoutBlockDecision::FailClosed { decision } => {
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
    if actor_state == crate::session_actor::ActorState::Starting
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
        && actor_state == crate::session_actor::ActorState::Busy
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
        actor_state,
        prompt_context.is_some(),
        has_existing_inactive_queue_fallback,
    ) {
        tmux.capture_pane(&dispatch_pane, Some(80))
            .ok()
            .and_then(|content| harness.busy_proof_line(&content))
    } else {
        None
    };
    if actor_state == crate::session_actor::ActorState::Ready
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
        actor_state = crate::session_actor::ActorState::Busy;
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
            dispatch_only_busy_refusal_wait_secs(dispatch_only_starting_pane_recovery_timeout(
                Some(harness)
            ))
        );
    }
    let mut waited_and_timed_out = false;
    if busy_dispatch_only_should_wait_for_ready(
        dispatch_only,
        actor_state,
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

    if actor_blocked_by_starting_timeout(&actor) {
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
    if rescued_from_stash && actor_state == crate::session_actor::ActorState::Starting {
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
        if refreshed.actor_state() == crate::session_actor::ActorState::Ready {
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
        && (actor_state == crate::session_actor::ActorState::Ready
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
        actor_state = crate::session_actor::ActorState::Ready;
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
            actor_state = crate::session_actor::ActorState::Ready;
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
    // `busy_dispatch_only_should_wait_for_ready` skipped the wait because
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
        actor_state = crate::session_actor::ActorState::Ready;
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
        dispatch_eligible: authoritative_actor_dispatch_target_eligible(&actor),
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
                        user_outcome_fields(
                            crate::flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
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
                        user_outcome_fields(
                            crate::flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
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
                anyhow::bail!(
                    "{}",
                    dispatch_only_busy_refusal_message(
                        harness,
                        actor.record.generation,
                        file,
                        &dispatch_pane,
                        reason,
                        active_turn_busy_cue.as_deref(),
                        actor_state,
                    )
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
                    user_outcome_fields(
                        crate::flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
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
                    user_outcome_fields(
                        crate::flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
                    )
                );
                Ok(dispatch_pane)
            } else {
                anyhow::bail!(
                    "{}",
                    dispatch_only_busy_refusal_message(
                        harness,
                        actor.record.generation,
                        file,
                        &dispatch_pane,
                        reason,
                        active_turn_busy_cue.as_deref(),
                        actor_state,
                    )
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
                harness,
                busy_cue.as_deref(),
                dispatch_only_busy_refusal_wait_secs(dispatch_only_starting_pane_recovery_timeout(
                    Some(harness),
                )),
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
) -> sessions::SessionEntry {
    sessions::SessionEntry {
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
    let contract = crate::agent::codex::writable_root_contract_id(&[writable]).unwrap();
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
use sessions::IsolatedTmux;
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
pub(crate) fn test_actor_record(pane_id: &str) -> crate::session_actor::ActorRecord {
    crate::session_actor::ActorRecord {
        document_id: "test-doc".to_string(),
        session_id: "test-session".to_string(),
        generation: 1,
        pane_id: pane_id.to_string(),
        window_id: "@1".to_string(),
        harness: "codex".to_string(),
        state: crate::session_actor::ActorState::Ready,
        last_transition: crate::session_actor::ActorLastTransition {
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
// #pcp3a: classify_drain_retry — the route-drain concurrent-finalize race
// hardening decision. A mid-drain repair+session_check failure should retry
// (not fail closed) when there is positive evidence a finalize in another
// process is concurrently progressing or has just closed the cycle.
#[cfg(test)]
use crate::cycle_state::CyclePhase;

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::flow::routed_reopen::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
    use crate::supervisor::ipc::{IpcMethod, IpcResponse, SupervisorIpc};

    #[test]
    fn route_closeout_user_outcome_surfaces_unblocker_for_stuck_cycle() {
        // #routedrainnextaction: a stuck `Blocked` closeout recovery decision
        // (captured-response baseline drift / IPC no_ack) must surface the
        // specific recovery command via BlockedWithExactUnblocker instead of
        // the misleading `wait_for_owner_turn_to_drain` (no live owner turn).
        use crate::flow::closeout::{CloseoutRecoveryDecision, CloseoutRecoveryState};
        let decision = CloseoutRecoveryDecision::Blocked {
            state: CloseoutRecoveryState::OpenCycle,
            missing_proof: "open cycle must finish, be replayed, or be explicitly queued behind"
                .to_string(),
            recommended: "finish the response, then `agent-doc finalize /abs/path/session.md` (or `agent-doc write --commit /abs/path/session.md` to absorb an already-visible response)".to_string(),
        };
        let fields = route_closeout_user_outcome_fields(&decision);
        assert!(
            fields.contains("ui_outcome=blocked_with_exact_unblocker"),
            "stuck-cycle decision must surface BlockedWithExactUnblocker, not QueuedBehindOwner: {fields}"
        );
        assert!(
            fields.contains("next_action=follow_unblocker"),
            "stuck-cycle next_action must point at the unblocker: {fields}"
        );
        assert!(
            fields.contains("unblocker=run_recovery_command"),
            "stuck-cycle unblocker must be the short run-recovery action token: {fields}"
        );
        assert!(
            fields.contains("recovery_command=agent-doc finalize /abs/path/session.md"),
            "stuck-cycle must surface the literal recovery command as trailing free text: {fields}"
        );
        assert!(
            !fields.contains("wait_for_owner_turn_to_drain"),
            "stuck-cycle must NOT use the live-owner-turn next_action: {fields}"
        );
    }

    #[test]
    fn route_closeout_user_outcome_keeps_queued_behind_owner_for_genuine_wait() {
        // #routedrainnextaction: a non-Blocked recovery decision (the operator's
        // turn is genuinely running, prompt is queued behind it) keeps the
        // historical QueuedBehindOwner / wait_for_owner_turn_to_drain wording.
        use crate::flow::closeout::{CloseoutRecoveryDecision, CloseoutRecoveryState};
        let decision = CloseoutRecoveryDecision::QueuePromptForAfterCloseout {
            state: CloseoutRecoveryState::OpenCycle,
            reason: "live owner turn in progress".to_string(),
        };
        let fields = route_closeout_user_outcome_fields(&decision);
        assert!(
            fields.contains("ui_outcome=queued_behind_owner"),
            "genuine queue-behind must keep QueuedBehindOwner: {fields}"
        );
        assert!(
            fields.contains("next_action=wait_for_owner_turn_to_drain"),
            "genuine queue-behind must keep the live-owner-turn next_action: {fields}"
        );
    }

    #[test]
    fn extract_recovery_command_picks_first_agent_doc_invocation() {
        // Recovery prose typically looks like: "finish the response, then
        // `agent-doc finalize <FILE>` (or `agent-doc write --commit <FILE>`
        // to absorb an already-visible response)" — pull just the first
        // `agent-doc ...` invocation so the surfaced unblocker token stays
        // short and copy-pasteable.
        let recommended = "finish the response, then `agent-doc finalize /abs/session.md` (or `agent-doc write --commit /abs/session.md` to absorb an already-visible response)";
        assert_eq!(
            extract_recovery_command(recommended).as_deref(),
            Some("agent-doc finalize /abs/session.md")
        );
        // No agent-doc in the prose → None (caller falls back to full text).
        assert!(extract_recovery_command("just finish the response").is_none());
        // Backticks are stripped because they aren't CLI word characters; the
        // command still extracts cleanly across the boundary.
        let mixed = "Rebuild sidecars: `agent-doc reset --from-current --preserve-session /path/session.md`";
        assert_eq!(
            extract_recovery_command(mixed).as_deref(),
            Some("agent-doc reset --from-current --preserve-session /path/session.md")
        );
    }

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
                crate::session_actor::ActorState::Busy
            ),
            "busy actors may still accept a supervisor-owned queued reopen"
        );
        assert!(
            !authoritative_actor_dispatch_can_queue_optimistically(
                crate::session_actor::ActorState::Starting
            ),
            "starting actors must become ready before route submits a reopen"
        );
    }
    #[test]
    fn authoritative_actor_start_wait_terminal_state_only_for_terminal_states() {
        assert!(authoritative_actor_start_wait_terminal_state(
            crate::session_actor::ActorState::Closed
        ));
        assert!(authoritative_actor_start_wait_terminal_state(
            crate::session_actor::ActorState::Blocked
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            crate::session_actor::ActorState::Starting
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            crate::session_actor::ActorState::Busy
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            crate::session_actor::ActorState::WaitingInput
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            crate::session_actor::ActorState::Ready
        ));
    }
    #[test]
    fn authoritative_actor_ready_poll_requires_ready_state_and_prompt_proof() {
        use crate::session_actor::ActorState;

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
        use crate::session_actor::ActorState;

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
        assert!(updated.contains("queue: start"));
        assert!(updated.contains("<!-- agent:queue -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(updated.contains("- do [#qipc]. #spec-test-build-install-commit-push"));
        let queue_pos = updated.find("<!-- agent:queue -->").unwrap();
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
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- existing queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        let expected = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
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
        let parsed = crate::queue::parse("JB `Run Agent Doc` error:\n- do [#existing]\n").unwrap();
        assert!(
            parsed
                .iter()
                .any(|e| matches!(e, crate::queue::QueueEntry::Freeform(_)))
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
        assert!(updated.contains("queue: start"));
        assert!(updated.contains("<!-- agent:queue -->"));
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
    fn route_activates_existing_inactive_auto_queue_head_as_plain_queue_for_busy_deferral() {
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
            "<!-- agent:queue auto -->\n",
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
            .expect("legacy inactive auto queue head should activate");

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
        assert!(updated.contains("queue: start"));
        assert!(updated.contains("<!-- agent:queue -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert_eq!(
            updated
                .matches("- do [#shipstationaudit]. #spec-test-commit-push")
                .count(),
            1,
            "route must activate the existing head without duplicating it:\n{updated}"
        );
        assert_eq!(
            crate::queue_continuation::live_continuation_head(&doc, &updated).as_deref(),
            Some("shipstationaudit"),
            "activated queue should become drainable by the idle-queue watch"
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
            "<!-- agent:queue auto -->\n",
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
            "<!-- agent:queue auto -->\n",
            "- do [#fresh]\n",
            "- do [#committed]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, on_disk).unwrap();
        // Committed snapshot only knows about `#committed`.
        crate::snapshot::save(&doc, committed).unwrap();

        assert!(
            !route_queue_head_backed_by_committed_snapshot(&doc, "do [#fresh]"),
            "a head absent from the committed snapshot queue is not backed"
        );
        assert!(
            route_queue_head_backed_by_committed_snapshot(&doc, "do [#committed]"),
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
            "<!-- agent:queue auto -->\n",
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
            !route_queue_head_backed_by_committed_snapshot(&doc, "do [#fresh]"),
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
            route_queue_head_backed_by_committed_snapshot(&doc, "do [#anything]"),
            "no committed snapshot → allow (bootstrap)"
        );
    }
    #[test]
    fn busy_route_defers_to_active_auto_loop_instead_of_refusing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#regional]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        // No INACTIVE head — the queue is already active, so the activate path no-ops.
        assert_eq!(
            inactive_route_queue_head(&doc).unwrap(),
            None,
            "an already-active auto-queue exposes no inactive head to activate"
        );
        assert_eq!(
            activate_existing_route_queue_head(&doc, "busy actor").unwrap(),
            None,
            "activate path returns None when the queue is already auto-looping"
        );
        // But the active-loop continuation signal IS present — this is what the busy
        // route path uses to defer (report success) instead of failing closed.
        let continuation = crate::queue_continuation::detect(&doc)
            .unwrap()
            .expect("active auto-loop must expose a continuation head for busy deferral");
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
            updated.contains("queue: start"),
            "activation must flip the canonical control to start:\n{updated}"
        );
        assert_eq!(
            crate::queue_continuation::live_continuation_head(&doc, &updated).as_deref(),
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
        assert!(updated.contains("<!-- agent:queue -->"));
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
        assert!(updated.contains("<!-- agent:queue -->"));
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
        assert!(updated.contains("<!-- agent:queue -->"));
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
        assert!(updated.contains("<!-- agent:queue -->"));
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
        assert!(updated.contains("<!-- agent:queue -->"));
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

        let mut registry = sessions::SessionRegistry::new();
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
    fn is_first_column_empty_cols() {
        let file = Path::new("tasks/agent-doc.md");
        assert!(!is_first_column(file, &[]));
    }
    #[test]
    fn is_first_column_single_col() {
        let file = Path::new("tasks/agent-doc.md");
        let cols = vec!["tasks/agent-doc.md".to_string()];
        // Single column — no need to split before
        assert!(!is_first_column(file, &cols));
    }
    #[test]
    fn is_first_column_in_first_col() {
        let file = Path::new("tasks/agent-doc.md");
        let cols = vec![
            "tasks/agent-doc.md".to_string(),
            "tasks/email.md".to_string(),
        ];
        assert!(is_first_column(file, &cols));
    }
    #[test]
    fn is_first_column_in_second_col() {
        let file = Path::new("tasks/email.md");
        let cols = vec![
            "tasks/agent-doc.md".to_string(),
            "tasks/email.md".to_string(),
        ];
        assert!(!is_first_column(file, &cols));
    }
    #[test]
    fn is_first_column_comma_separated() {
        let file = Path::new("tasks/agent-doc.md");
        let cols = vec![
            "tasks/agent-doc.md,tasks/corky.md".to_string(),
            "tasks/email.md".to_string(),
        ];
        assert!(is_first_column(file, &cols));
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
    fn codex_routed_dispatch_start_proof_accepts_any_newer_state_for_same_file() {
        let tracker = RoutedDispatchStartTracker::CodexHook {
            trigger: "agent-doc /tmp/task.md".to_string(),
            previous_session_id: Some("codex-session".to_string()),
            previous_turn_id: Some("turn-1".to_string()),
            previous_updated_at: Some(10),
        };
        let state = crate::codex_hook::ActiveSessionState {
            session_id: "codex-session".to_string(),
            doc_path: "/tmp/task.md".to_string(),
            last_turn_id: "turn-2".to_string(),
            last_prompt: "/review current changes".to_string(),
            updated_at: 11,
        };
        assert_eq!(
            codex_routed_dispatch_start_proof(&tracker, &state),
            Some(RoutedDispatchStartProof::HookStateAdvanced)
        );
    }
    #[test]
    fn opencode_pane_state_change_proof_requires_trigger_to_leave_composer() {
        let harness = HarnessConfig::opencode();
        let trigger = harness.trigger_command("tasks/bugs.md");
        let before = ">\n";
        let drafted = format!("> {trigger}\n");
        assert!(
            !opencode_pane_state_changed_from_idle(&harness, &trigger, before, &drafted),
            "drafted trigger text is pane input, not dispatch-start proof"
        );

        let active = "\
Working (2s - esc to interrupt)
zai/glm-5 · ~/work/btakita/agent-loop · context 0% used
";
        assert!(
            opencode_pane_state_changed_from_idle(&harness, &trigger, before, active),
            "OpenCode leaving idle chrome for active output should prove dispatch start"
        );

        let idle_status = "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n";
        assert!(
            !opencode_pane_state_changed_from_idle(&harness, &trigger, before, idle_status),
            "idle status chrome alone must not prove dispatch start"
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
    fn busy_dispatch_only_skips_ready_wait_when_queue_fallback_exists() {
        use crate::session_actor::ActorState;
        // Busy + dispatch-only + a queue prompt available → do NOT wait (queue now).
        assert!(
            !busy_dispatch_only_should_wait_for_ready(true, ActorState::Busy, true, false),
            "a busy active turn with a queue fallback must skip the start-oriented ready wait"
        );
        // Busy + dispatch-only + no queue fallback + no live active-turn cue → still
        // wait (the actor may be a transient/stale busy projection about to clear).
        assert!(
            busy_dispatch_only_should_wait_for_ready(true, ActorState::Busy, false, false),
            "without a queue fallback the bounded ready wait is still the only recourse before bailing"
        );
        // Not dispatch-only, or not busy → the busy-wait guard does not apply.
        assert!(!busy_dispatch_only_should_wait_for_ready(
            false,
            ActorState::Busy,
            false,
            false
        ));
        assert!(!busy_dispatch_only_should_wait_for_ready(
            true,
            ActorState::Ready,
            false,
            false
        ));
    }
    #[test]
    fn dispatch_only_probes_active_turn_cue_for_stale_ready_actor() {
        use crate::session_actor::ActorState;
        assert!(
            dispatch_only_should_probe_active_turn_cue(true, ActorState::Ready, false, false),
            "ready actor records still need a live pane active-turn probe before direct dispatch"
        );
        assert!(
            dispatch_only_should_probe_active_turn_cue(true, ActorState::Ready, true, true),
            "prompt-bearing routes must also block stale-ready active turns before injection"
        );
        assert!(
            dispatch_only_should_probe_active_turn_cue(true, ActorState::Busy, false, false),
            "existing busy no-fallback path still probes for active-turn wording"
        );
        assert!(
            !dispatch_only_should_probe_active_turn_cue(true, ActorState::Busy, true, false),
            "busy prompt-bearing routes queue without the slow active-turn probe"
        );
        assert!(
            !dispatch_only_should_probe_active_turn_cue(false, ActorState::Ready, false, false),
            "managed reopen path keeps its existing supervisor queue behavior"
        );
    }
    #[test]
    fn busy_dispatch_only_skips_ready_wait_on_proven_active_turn() {
        use crate::session_actor::ActorState;
        // Busy + dispatch-only + no queue fallback + a live active-turn cue → skip
        // the wait (immediate refusal), instead of stalling for the full budget.
        assert!(
            !busy_dispatch_only_should_wait_for_ready(true, ActorState::Busy, false, true),
            "a proven active turn must skip the busy ready-wait and refuse immediately"
        );
    }
    #[test]
    fn dispatch_only_busy_refusal_message_distinguishes_active_turn_from_cold_wait() {
        use crate::session_actor::ActorState;
        let harness = HarnessConfig::claude();
        let file = std::path::Path::new("/tmp/sampleorders.md");

        let active = dispatch_only_busy_refusal_message(
            &harness,
            282,
            file,
            "%1",
            "actor not ready",
            Some("Working (7m 29s · esc to interrupt)"),
            ActorState::Busy,
        );
        assert!(
            active.contains("busy on an active") && active.contains("esc to interrupt"),
            "active-turn refusal must name the busy turn cue: {active}"
        );
        assert!(
            active.contains("ui_outcome=blocked_with_exact_unblocker")
                && active.contains("unblocker=wait_for_owner_turn_to_finish"),
            "active-turn refusal must carry the typed unblocker outcome: {active}"
        );
        assert!(
            !active.contains("after waiting"),
            "active-turn refusal must not claim a ready-wait that was skipped: {active}"
        );

        let cold = dispatch_only_busy_refusal_message(
            &harness,
            282,
            file,
            "%1",
            "actor not ready",
            None,
            ActorState::Busy,
        );
        assert!(
            cold.contains("after waiting") && cold.contains("dispatch-ready prompt"),
            "no-cue refusal keeps the cold-start ready-wait wording: {cold}"
        );
        assert!(
            cold.contains("ui_outcome=blocked_with_exact_unblocker")
                && cold.contains("unblocker=wait_for_dispatch_ready_prompt"),
            "cold-wait refusal must carry the typed unblocker outcome: {cold}"
        );
    }
    #[test]
    fn routed_trigger_payload_keeps_bare_reopen() {
        let codex_trigger = HarnessConfig::codex().trigger_command("test.md");
        assert_eq!(routed_trigger_payload(&codex_trigger), "agent-doc test.md");
        assert_eq!(
            routed_trigger_payload("/agent-doc test.md"),
            "/agent-doc test.md"
        );
    }
    #[test]
    fn plain_trigger_override_uses_bare_agent_doc_reopen_for_route() {
        let mut claude = HarnessConfig::claude();
        apply_plain_trigger_override(&mut claude);
        assert_eq!(claude.trigger_command("test.md"), "agent-doc test.md");

        let mut opencode = HarnessConfig::opencode();
        apply_plain_trigger_override(&mut opencode);
        assert_eq!(opencode.trigger_command("test.md"), "agent-doc test.md");
    }
    #[test]
    fn routed_trigger_submit_payload_strips_trailing_line_endings() {
        assert_eq!(
            routed_trigger_submit_payload("agent-doc test.md\r\n"),
            "agent-doc test.md"
        );
    }
    #[test]
    fn validate_routed_trigger_payload_accepts_bare_codex_reopen() {
        let harness = HarnessConfig::codex();
        let trigger = harness.trigger_command("test.md");
        let payload = routed_trigger_payload(&trigger);
        validate_routed_trigger_payload(&harness, &trigger, &payload)
            .expect("bare Codex reopen should remain dispatchable");
    }
    #[test]
    fn validate_routed_trigger_payload_rejects_multiline_codex_payload() {
        let harness = HarnessConfig::codex();
        let trigger = harness.trigger_command("test.md");
        let err = validate_routed_trigger_payload(
            &harness,
            &trigger,
            "agent-doc test.md\nfollow-up text",
        )
        .expect_err("Codex reroute payload must fail before injecting extra lines");
        assert!(
            err.to_string().contains("bare `agent-doc <FILE>` reopen"),
            "unexpected error: {err:#}"
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

        match super::classify_route_closeout_block(&doc, low_level_reason.to_string(), true) {
            super::RouteCloseoutBlockDecision::EnqueuePromptForAfterCloseout { decision } => {
                assert_eq!(
                    decision.state(),
                    Some(crate::flow::closeout::CloseoutRecoveryState::OpenCycle)
                );
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
            "<!-- agent:queue auto -->\n",
            "- :pushpin: [#jbruncloseoutstate]\n",
            "<!-- /agent:queue -->\n"
        );
        write_open_cycle_route_doc(&doc, content);
        let low_level_reason = "captured response baseline no longer matches current document";

        match super::classify_route_closeout_block(&doc, low_level_reason.to_string(), false) {
            super::RouteCloseoutBlockDecision::WaitForActiveQueueHead { head, decision } => {
                assert_eq!(head, "jbruncloseoutstate");
                assert_eq!(
                    decision.state(),
                    Some(crate::flow::closeout::CloseoutRecoveryState::OpenCycle)
                );
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

        match super::classify_route_closeout_block(&doc, low_level_reason.to_string(), false) {
            super::RouteCloseoutBlockDecision::FailClosed { decision } => {
                assert_eq!(
                    decision.state(),
                    Some(crate::flow::closeout::CloseoutRecoveryState::OpenCycle)
                );
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
        let ok = route_latency_message(
            "dispatch_start_proof",
            Duration::from_millis(999),
            Duration::from_secs(1),
            "%1",
            &harness,
            "submitted",
        );
        assert!(ok.contains("status=ok"), "{ok}");
        assert!(ok.contains("elapsed_ms=999"), "{ok}");

        let slow = route_latency_message(
            "dispatch_start_proof",
            Duration::from_secs(10),
            Duration::from_secs(10),
            "%1",
            &harness,
            "unproven_but_accepted",
        );
        assert!(slow.contains("status=over_budget"), "{slow}");
        assert!(slow.contains("outcome=unproven_but_accepted"), "{slow}");
    }

    #[test]
    fn route_diagnostics_include_editor_attempt_id_when_present() {
        let _attempt = EnvGuard::set(EDITOR_ROUTE_ATTEMPT_ID_ENV, "attempt 1/2");
        let harness = HarnessConfig::codex();
        let latency = route_latency_message(
            "direct_pane_submit",
            Duration::from_millis(120),
            Duration::from_secs(1),
            "%7",
            &harness,
            "accepted",
        );
        assert!(
            latency.contains("editor_attempt_id=attempt_1_2"),
            "{latency}"
        );

        let facts = RouteSubmitObservationFacts {
            file: Path::new("/tmp/run-agent-doc.md"),
            pane: "%7",
            harness: &harness,
            phase: "direct_pane_acceptance",
            observation: RouteSubmitObservation::TriggerStillVisible,
            trigger_visible: Some(true),
            elapsed: Duration::from_millis(5123),
            capture_len: Some(2048),
            capture_hash: Some("abc123def456"),
            proof: None,
        };
        let observation = route_submit_observation_message(facts);
        assert!(
            observation.contains("editor_attempt_id=attempt_1_2"),
            "{observation}"
        );
        let issue = route_submit_issue_message(facts).expect("trigger-still-visible issue");
        assert!(issue.contains("editor_attempt_id=attempt_1_2"), "{issue}");
    }

    #[test]
    fn direct_pane_submit_budget_allows_acceptance_poll_slack() {
        assert_eq!(
            direct_pane_submit_acceptance_timeout(),
            Duration::from_secs(1)
        );
        assert_eq!(
            direct_pane_submit_acceptance_budget(),
            Duration::from_millis(1500)
        );

        let message = route_latency_message(
            "direct_pane_submit",
            Duration::from_millis(1180),
            direct_pane_submit_acceptance_budget(),
            "%1",
            &HarnessConfig::codex(),
            direct_pane_submit_outcome(
                CommandDispatchStatus::TimedOut,
                Some(RoutedDispatchStartProof::HookPromptMatched),
            ),
        );

        assert!(message.contains("status=ok"), "{message}");
        assert!(
            message.contains("outcome=acceptance_unobserved_dispatch_proven"),
            "{message}"
        );
        assert!(!message.contains("timed_out"), "{message}");
    }
    #[test]
    fn direct_pane_submit_outcome_separates_acceptance_from_dispatch_proof() {
        assert_eq!(
            direct_pane_submit_outcome(CommandDispatchStatus::Accepted, None),
            "accepted"
        );
        assert_eq!(
            direct_pane_submit_outcome(CommandDispatchStatus::TimedOut, None),
            "acceptance_unobserved"
        );
        assert_eq!(
            direct_pane_submit_outcome(
                CommandDispatchStatus::TimedOut,
                Some(RoutedDispatchStartProof::HookStateAdvanced),
            ),
            "acceptance_unobserved_dispatch_proven"
        );
    }
    #[test]
    fn route_submit_observation_marks_prompt_not_submitted_without_prompt_text() {
        let facts = RouteSubmitObservationFacts {
            file: Path::new("/tmp/run-agent-doc.md"),
            pane: "%7",
            harness: &HarnessConfig::codex(),
            phase: "direct_pane_acceptance",
            observation: RouteSubmitObservation::TriggerStillVisible,
            trigger_visible: Some(true),
            elapsed: Duration::from_millis(5123),
            capture_len: Some(2048),
            capture_hash: Some("abc123def456"),
            proof: None,
        };

        let message = route_submit_observation_message(facts);
        assert!(message.contains("route_submit_observation"), "{message}");
        assert!(
            message.contains("result=trigger_still_visible"),
            "{message}"
        );
        assert!(message.contains("trigger_visible=true"), "{message}");
        assert!(message.contains("issue=prompt_not_submitted"), "{message}");
        assert!(message.contains("capture_hash=abc123def456"), "{message}");
        assert!(!message.contains("agent-doc "), "{message}");

        let issue =
            route_submit_issue_message(facts).expect("prompt-not-submitted should be an issue");
        assert!(issue.contains("route_submit_issue"), "{issue}");
        assert!(issue.contains("issue=prompt_not_submitted"), "{issue}");
        assert!(issue.contains("result=trigger_still_visible"), "{issue}");
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
    fn route_submit_observation_marks_dispatch_start_proof_without_issue() {
        let facts = RouteSubmitObservationFacts {
            file: Path::new("/tmp/run-agent-doc.md"),
            pane: "%7",
            harness: &HarnessConfig::codex(),
            phase: "dispatch_start_proof",
            observation: RouteSubmitObservation::DispatchStartProven,
            trigger_visible: None,
            elapsed: Duration::from_millis(800),
            capture_len: None,
            capture_hash: None,
            proof: Some(RoutedDispatchStartProof::HookStateAdvanced),
        };

        let message = route_submit_observation_message(facts);
        assert!(
            message.contains("result=dispatch_start_proven"),
            "{message}"
        );
        assert!(message.contains("proof=submitted"), "{message}");
        assert!(
            route_submit_issue_message(facts).is_none(),
            "dispatch-start proof should not emit an issue"
        );
    }
    #[test]
    fn route_submit_observation_marks_accepted_without_dispatch_proof_as_issue() {
        let facts = RouteSubmitObservationFacts {
            file: Path::new("/tmp/run-agent-doc.md"),
            pane: "%7",
            harness: &HarnessConfig::codex(),
            phase: "dispatch_start_proof",
            observation: RouteSubmitObservation::AcceptedWithoutDispatchProof,
            trigger_visible: None,
            elapsed: Duration::from_secs(10),
            capture_len: None,
            capture_hash: None,
            proof: None,
        };

        let issue = route_submit_issue_message(facts)
            .expect("required dispatch-start proof absence should be an issue");
        assert!(
            issue.contains("issue=accepted_without_dispatch_start_proof"),
            "{issue}"
        );
        assert!(
            issue.contains("result=accepted_without_dispatch_start_proof"),
            "{issue}"
        );
    }

    #[test]
    fn route_dispatch_bug_report_item_includes_required_evidence() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("run-agent-doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: test\n---\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        )
        .unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            doc.to_str().unwrap(),
            "session-1",
            "%7",
            "@1",
            "test",
            "route",
        )
        .unwrap();
        let diagnostic = dir.path().join(".agent-doc/logs/route-submit/snapshot.txt");
        let facts = RouteDispatchBugReportFacts {
            file: &doc,
            pane: "%7",
            harness: &HarnessConfig::codex(),
            phase: "dispatch_start_proof",
            issue: "accepted_without_dispatch_start_proof",
            result: "accepted_without_dispatch_start_proof",
            elapsed: Duration::from_secs(10),
            proof: None,
            diagnostic_path: Some(&diagnostic),
        };

        let item = route_dispatch_bug_report_item(facts).unwrap();

        assert!(item.contains("#jbrunautobug"), "{item}");
        assert!(item.contains("#agent-doc-bug"), "{item}");
        assert!(
            item.contains("failure_class=accepted_without_dispatch_start_proof"),
            "{item}"
        );
        assert!(item.contains("stage=dispatch_start_proof"), "{item}");
        assert!(item.contains("pane=%7"), "{item}");
        assert!(item.contains("actor_generation=1"), "{item}");
        assert!(item.contains("dispatch_proof_state=none"), "{item}");
        assert!(item.contains("diagnostic_path="), "{item}");
        assert!(item.contains("ops_log_path="), "{item}");
        assert!(
            item.contains(
                "ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof"
            ),
            "{item}"
        );
        assert!(
            item.contains("[symptom-key invariant=run_agent_doc_route_dispatch_failure"),
            "{item}"
        );
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
    fn starting_pane_recovery_target_follows_same_file_handoff() {
        let initial = crate::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%151".to_string()),
            latest_start_timestamp: Some(10),
            latest_run_timestamp: Some(11),
            latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_process_exit_after_latest_start: false,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: false,
            saw_session_end_after_latest_run: false,
        };
        let handed_off = crate::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%183".to_string()),
            latest_start_timestamp: Some(20),
            latest_run_timestamp: Some(21),
            latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_process_exit_after_latest_start: false,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: false,
            saw_session_end_after_latest_run: false,
        };

        assert_eq!(
            starting_pane_recovery_target(Some(&initial), Some(&handed_off), "%151", Some("%183")),
            Some(StartingPaneRecoveryTarget::DifferentPane(
                "%183".to_string()
            ))
        );
    }
    #[test]
    fn starting_pane_recovery_target_retries_same_pane_after_new_generation() {
        let initial = crate::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%151".to_string()),
            latest_start_timestamp: Some(10),
            latest_run_timestamp: Some(11),
            latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_process_exit_after_latest_start: false,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: false,
            saw_session_end_after_latest_run: false,
        };
        let restarted = crate::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%151".to_string()),
            latest_start_timestamp: Some(12),
            latest_run_timestamp: Some(13),
            latest_run_event: Some("codex_start mode=fresh_restart restart_count=1".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("codex_start mode=fresh_restart restart_count=1".to_string()),
            saw_process_exit_after_latest_start: false,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: false,
            saw_session_end_after_latest_run: false,
        };

        assert_eq!(
            starting_pane_recovery_target(Some(&initial), Some(&restarted), "%151", Some("%151")),
            Some(StartingPaneRecoveryTarget::SamePane)
        );
    }
    #[test]
    fn starting_pane_recovery_target_ignores_unchanged_open_start() {
        let initial = crate::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%151".to_string()),
            latest_start_timestamp: Some(10),
            latest_run_timestamp: Some(11),
            latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_process_exit_after_latest_start: false,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: false,
            saw_session_end_after_latest_run: false,
        };

        assert_eq!(
            starting_pane_recovery_target(Some(&initial), Some(&initial), "%151", Some("%151")),
            None
        );
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
    fn duplicate_pane_policy_error_includes_manual_tmux_commands() {
        let iso = IsolatedTmux::new("route-test-duplicate-policy");
        let session = "test";
        let rendered = format_duplicate_pane_policy_error(
            session,
            "tasks/agent-doc/agent-doc-bugs2.md",
            Some("%42"),
            "split-window failed alongside pane %42 (too small)",
        );
        assert!(rendered.contains("tmux list-panes -t test:agent-doc"));
        assert!(rendered.contains("tmux kill-pane -t %42"));
        assert!(rendered.contains("agent-doc tasks/agent-doc/agent-doc-bugs2.md"));
        assert!(rendered.contains("split-window failed alongside pane %42"));
        let _ = iso;
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
            crate::startup_miss::StartupMissOrigin::FreshStart,
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

        let resolved =
            crate::git::resolve_absolute_file_path(std::path::Path::new("tasks/bugs.md"));
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
            crate::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();

        let miss = crate::startup_miss::load(&doc)
            .unwrap()
            .expect("should have marker");
        assert_eq!(miss.pane_id, "%42");
        assert_eq!(
            miss.origin,
            crate::startup_miss::StartupMissOrigin::FreshStart
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
            crate::startup_miss::StartupMissOrigin::FreshStart,
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
            crate::startup_miss::StartupMissOrigin::RoutedTrigger,
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
            crate::startup_miss::StartupMissOrigin::RoutedTrigger,
            Some("cycle-baseline-123"),
        )
        .unwrap();

        let miss = crate::startup_miss::load(&doc).unwrap().expect("marker");
        assert_eq!(
            miss.origin,
            crate::startup_miss::StartupMissOrigin::RoutedTrigger
        );
        assert_eq!(
            miss.cycle_baseline_id.as_deref(),
            Some("cycle-baseline-123")
        );
    }
    #[test]
    fn startup_miss_requires_fresh_start_only_without_matching_live_owner() {
        assert!(startup_miss_requires_fresh_start(
            "%42",
            None,
            SupervisorHealth::NoSocket
        ));
        assert!(startup_miss_requires_fresh_start(
            "%42",
            Some("%99"),
            SupervisorHealth::Unreachable
        ));
        assert!(!startup_miss_requires_fresh_start(
            "%42",
            Some("%42"),
            SupervisorHealth::NoSocket
        ));
        assert!(!startup_miss_requires_fresh_start(
            "%42",
            None,
            SupervisorHealth::Restartable
        ));
        assert!(!startup_miss_requires_fresh_start(
            "%42",
            None,
            SupervisorHealth::Healthy
        ));
    }
    #[test]
    fn startup_miss_live_owner_restart_requires_closed_unsuperseded_start() {
        let miss = crate::startup_miss::StartupMiss {
            file: "test.md".to_string(),
            pane_id: "%42".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 10,
            origin: crate::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: Some("cycle-abc".to_string()),
        };
        let closed_same_start = crate::startup_miss::SessionLogStatus {
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
        let newer_open_start = crate::startup_miss::SessionLogStatus {
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
            &miss,
            "%42",
            Some("%42"),
            Some(&closed_same_start)
        ));
        assert!(!startup_miss_should_restart_live_owner(
            &miss,
            "%42",
            Some("%42"),
            Some(&newer_open_start)
        ));
        assert!(startup_miss_superseded_by_later_open_start(
            &miss,
            "%42",
            Some(&newer_open_start)
        ));
        assert!(!startup_miss_superseded_by_later_open_start(
            &miss,
            "%42",
            Some(&closed_same_start)
        ));
    }
    #[test]
    fn startup_miss_fail_closed_only_for_alive_open_no_socket_sessions() {
        let open = crate::startup_miss::SessionLogStatus {
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
        let closed = crate::startup_miss::SessionLogStatus {
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

        assert!(startup_miss_should_fail_closed(
            true,
            "%42",
            None,
            SupervisorHealth::NoSocket,
            Some(&open)
        ));
        assert!(!startup_miss_should_fail_closed(
            true,
            "%42",
            Some("%42"),
            SupervisorHealth::NoSocket,
            Some(&open)
        ));
        assert!(!startup_miss_should_fail_closed(
            true,
            "%42",
            None,
            SupervisorHealth::Healthy,
            Some(&open)
        ));
        assert!(!startup_miss_should_fail_closed(
            true,
            "%42",
            None,
            SupervisorHealth::NoSocket,
            Some(&closed)
        ));
        assert!(!startup_miss_should_fail_closed(
            false,
            "%42",
            None,
            SupervisorHealth::NoSocket,
            Some(&open)
        ));
    }
    #[test]
    fn startup_miss_diagnostic_message_includes_retry_command() {
        let doc = std::path::Path::new("tasks/agent-doc/agent-doc-bugs2.md");
        let message = startup_miss_diagnostic_message(
            doc,
            "routed trigger accepted but no document cycle started for pending #smdq",
        );
        assert!(message.contains("[agent-doc] startup-miss:"));
        assert!(message.contains("agent-doc start tasks/agent-doc/agent-doc-bugs2.md"));
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
        blocked_record.state = crate::session_actor::ActorState::Blocked;
        blocked_record.last_transition.reason = "starting_actor_timeout".to_string();
        let blocked_actor = AuthoritativeActorDispatchTarget {
            record: blocked_record,
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(crate::session_actor::ActorState::Starting),
            },
        };
        assert_eq!(
            blocked_actor.actor_state(),
            crate::session_actor::ActorState::Blocked,
            "a route-owned blocked record should remain a durable terminal gate even if stale supervisor IPC still reports starting"
        );
        assert!(
            actor_blocked_by_starting_timeout(&blocked_actor),
            "a route-owned starting timeout should be identifiable before route re-registers the stale pane"
        );

        let mut starting_record = test_actor_record("%43");
        starting_record.state = crate::session_actor::ActorState::Starting;
        let ready_actor = AuthoritativeActorDispatchTarget {
            record: starting_record,
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(crate::session_actor::ActorState::Ready),
            },
        };
        assert_eq!(
            ready_actor.actor_state(),
            crate::session_actor::ActorState::Ready,
            "non-terminal records should still accept fresher supervisor runtime state"
        );
    }
    #[test]
    fn starting_timeout_blocked_actor_recovery_requires_prompt_ready_proof() {
        let mut blocked_record = test_actor_record("%42");
        blocked_record.state = crate::session_actor::ActorState::Blocked;
        blocked_record.last_transition.reason = "starting_actor_timeout".to_string();
        let blocked_actor = AuthoritativeActorDispatchTarget {
            record: blocked_record,
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(crate::session_actor::ActorState::Starting),
            },
        };

        assert!(
            starting_timeout_blocked_actor_can_recover(&blocked_actor, true),
            "a route-owned starting timeout may recover only after direct dispatch-ready prompt proof"
        );
        assert!(
            !starting_timeout_blocked_actor_can_recover(&blocked_actor, false),
            "route must not clear a durable starting timeout without prompt proof"
        );
        assert!(
            !starting_timeout_blocked_actor_can_recover(&test_degraded_actor("%43"), true),
            "ordinary degraded actors must not use the starting-timeout recovery path"
        );
    }
    #[test]
    fn authoritative_actor_dispatch_guard_reason_returns_none_for_healthy_with_state() {
        let runtime = SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(crate::session_actor::ActorState::Ready),
        };
        assert!(authoritative_actor_dispatch_guard_reason(&runtime).is_none());
    }
    #[test]
    fn authoritative_actor_dispatch_guard_reason_returns_reason_for_restartable() {
        let runtime = SupervisorRuntime {
            health: SupervisorHealth::Restartable,
            actor_state: Some(crate::session_actor::ActorState::Ready),
        };
        let reason = authoritative_actor_dispatch_guard_reason(&runtime).unwrap();
        assert!(
            reason.contains("restartable"),
            "expected restartable in reason: {reason}"
        );
    }
    #[test]
    fn authoritative_actor_dispatch_guard_reason_returns_reason_for_halted() {
        let runtime = SupervisorRuntime {
            health: SupervisorHealth::Halted { restart_count: 3 },
            actor_state: Some(crate::session_actor::ActorState::Ready),
        };
        let reason = authoritative_actor_dispatch_guard_reason(&runtime).unwrap();
        assert!(
            reason.contains("halted"),
            "expected halted in reason: {reason}"
        );
    }
    #[test]
    fn authoritative_actor_dispatch_guard_reason_returns_reason_for_unreachable() {
        let runtime = SupervisorRuntime {
            health: SupervisorHealth::Unreachable,
            actor_state: Some(crate::session_actor::ActorState::Ready),
        };
        let reason = authoritative_actor_dispatch_guard_reason(&runtime).unwrap();
        assert!(
            reason.contains("unreachable"),
            "expected unreachable in reason: {reason}"
        );
    }
    #[test]
    fn authoritative_actor_dispatch_guard_reason_returns_reason_for_no_socket() {
        let runtime = SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: Some(crate::session_actor::ActorState::Ready),
        };
        let reason = authoritative_actor_dispatch_guard_reason(&runtime).unwrap();
        assert!(
            reason.contains("no_socket"),
            "expected no_socket in reason: {reason}"
        );
    }
    #[test]
    fn authoritative_actor_dispatch_guard_reason_returns_reason_for_missing_actor_state() {
        let runtime = SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: None,
        };
        let reason = authoritative_actor_dispatch_guard_reason(&runtime).unwrap();
        assert!(
            reason.contains("missing"),
            "expected missing in reason: {reason}"
        );
    }
    #[test]
    fn authoritative_actor_dispatch_target_eligible_true_only_when_no_guard_reason() {
        let healthy = AuthoritativeActorDispatchTarget {
            record: test_actor_record("%1"),
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(crate::session_actor::ActorState::Ready),
            },
        };
        assert!(authoritative_actor_dispatch_target_eligible(&healthy));

        let degraded = AuthoritativeActorDispatchTarget {
            record: test_actor_record("%1"),
            runtime: SupervisorRuntime {
                health: SupervisorHealth::NoSocket,
                actor_state: None,
            },
        };
        assert!(!authoritative_actor_dispatch_target_eligible(&degraded));

        let no_state = AuthoritativeActorDispatchTarget {
            record: test_actor_record("%1"),
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: None,
            },
        };
        assert!(!authoritative_actor_dispatch_target_eligible(&no_state));
    }
    #[test]
    fn dispatch_only_starting_pane_recovery_timeout_default() {
        let timeout = dispatch_only_starting_pane_recovery_timeout(None);
        assert_eq!(timeout, Duration::from_millis(400));
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
    fn dispatch_only_starting_pane_recovery_timeout_opencode() {
        let h = crate::harness::HarnessConfig::opencode();
        let timeout = dispatch_only_starting_pane_recovery_timeout(Some(&h));
        assert_eq!(timeout, Duration::from_millis(400));
    }
    #[test]
    fn dispatch_only_starting_pane_recovery_timeout_claude() {
        let h = crate::harness::HarnessConfig::claude();
        let timeout = dispatch_only_starting_pane_recovery_timeout(Some(&h));
        assert_eq!(timeout, Duration::from_millis(400));
    }
    #[test]
    fn dispatch_only_starting_pane_recovery_timeout_codex() {
        let h = crate::harness::HarnessConfig::codex();
        let timeout = dispatch_only_starting_pane_recovery_timeout(Some(&h));
        assert_eq!(timeout, Duration::from_millis(400));
    }
    #[test]
    fn route_starting_actor_not_ready_log_line_includes_typed_lifecycle_facts() {
        let h = crate::harness::HarnessConfig::codex();
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
    fn busy_refusal_wait_secs_reports_override_then_default() {
        use std::time::Duration;

        // `#busy-not-ready-message-reports-actual-wait`: the busy/not-ready refusal
        // must report the caller's `--wait-for-ready` override (the time route really
        // waited), not the harness recovery constant. The JetBrains plugin passes 60,
        // so the message must say 60 even when the Codex default is 8.
        let guard = WaitForReadyOverrideGuard::set(Some(Duration::from_secs(60)));
        assert_eq!(
            dispatch_only_busy_refusal_wait_secs(Duration::from_secs(8)),
            60
        );
        drop(guard);

        // Without an override, the harness recovery-timeout default is reported.
        let _none = WaitForReadyOverrideGuard::set(None);
        assert_eq!(
            dispatch_only_busy_refusal_wait_secs(Duration::from_secs(8)),
            8
        );
    }
    #[test]
    fn dispatch_only_starting_pane_ready_timeout_honors_override_then_default() {
        use std::time::Duration;

        let codex = crate::harness::HarnessConfig::codex();
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
    #[test]
    fn failclosed_wait_context_distinguishes_busy_turn_from_cold_startup() {
        let claude = crate::harness::HarnessConfig::claude();
        // No busy cue → cold-startup timeout wording (unchanged behavior).
        assert_eq!(
            failclosed_wait_context(&claude, None, 12),
            "waited 12s for claude startup"
        );
        // A live busy cue → the pane is busy on an active turn, not cold-starting.
        assert_eq!(
            failclosed_wait_context(&claude, Some("active claude turn"), 12),
            "the pane is busy on an active claude turn (active claude turn), not cold-starting"
        );
    }
    #[test]
    fn busy_route_queued_diagnostic_names_turn_in_progress_and_no_rerun() {
        // #claude-busy-status-during-active-turn: the dispatch-only queued path
        // surfaces a turn-in-progress + queued status (not the generic "rerun" busy
        // message), so the operator sees why Run Agent Doc did not start now and that
        // it will run on its own when the current turn finishes.
        let claude = crate::harness::HarnessConfig::claude();
        let msg = busy_route_queued_diagnostic_message(std::path::Path::new("plan.md"), &claude);
        assert!(msg.contains("turn in progress"), "{msg}");
        assert!(msg.contains("queued"), "{msg}");
        assert!(msg.contains("plan.md"), "{msg}");
        assert!(msg.contains("claude"), "{msg}");
        assert!(msg.contains("ui_outcome=queued_behind_owner"), "{msg}");
        assert!(msg.contains("No need to rerun"), "{msg}");
        // Must NOT carry the generic busy diagnostic's rerun instruction.
        assert!(!msg.contains("rerun `Run Agent Doc`"), "{msg}");
    }
    #[test]
    fn drain_retry_concurrent_close_when_cycle_gone() {
        // The cycle is no longer on disk — a concurrent finalize closed it.
        assert_eq!(
            classify_drain_retry("cyc-1", CyclePhase::PreflightStarted, None, 0, 3),
            DrainRetryDecision::ConcurrentlyClosed
        );
    }
    #[test]
    fn drain_retry_concurrent_close_when_cycle_no_longer_open() {
        // The cycle reloaded as not-open (Committed) — a concurrent finalize closed it.
        assert_eq!(
            classify_drain_retry(
                "cyc-1",
                CyclePhase::PreflightStarted,
                Some(("cyc-1", CyclePhase::Committed, false)),
                0,
                3
            ),
            DrainRetryDecision::ConcurrentlyClosed
        );
    }
    #[test]
    fn drain_retry_retries_when_phase_advanced_and_attempts_remain() {
        // The cycle is still open but its phase advanced (PreflightStarted ->
        // WriteApplied) — a finalize is actively progressing in another process.
        assert_eq!(
            classify_drain_retry(
                "cyc-1",
                CyclePhase::PreflightStarted,
                Some(("cyc-1", CyclePhase::WriteApplied, true)),
                0,
                3
            ),
            DrainRetryDecision::Retry
        );
    }
    #[test]
    fn drain_retry_retries_when_cycle_id_changed_and_attempts_remain() {
        // A different, newer open cycle replaced ours — concurrent progress.
        assert_eq!(
            classify_drain_retry(
                "cyc-1",
                CyclePhase::PreflightStarted,
                Some(("cyc-2", CyclePhase::PreflightStarted, true)),
                1,
                3
            ),
            DrainRetryDecision::Retry
        );
    }
    #[test]
    fn drain_retry_gives_up_when_no_progress_observed() {
        // Same cycle, same phase, still open — no concurrent finalize. A genuine
        // stuck cycle must fail closed immediately rather than retry-spin.
        assert_eq!(
            classify_drain_retry(
                "cyc-1",
                CyclePhase::PreflightStarted,
                Some(("cyc-1", CyclePhase::PreflightStarted, true)),
                0,
                3
            ),
            DrainRetryDecision::GiveUp
        );
    }
    #[test]
    fn drain_retry_gives_up_when_attempts_exhausted_despite_progress() {
        // Concurrent progress, but this is the last attempt — fail closed instead
        // of looping forever.
        assert_eq!(
            classify_drain_retry(
                "cyc-1",
                CyclePhase::PreflightStarted,
                Some(("cyc-1", CyclePhase::WriteApplied, true)),
                2,
                3
            ),
            DrainRetryDecision::GiveUp
        );
    }
}
