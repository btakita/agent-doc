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
//!   Hook-visible Codex dispatch-only reroutes still require routed submit proof after the
//!   pane accepts the reopen; acceptance without hook-backed dispatch-start proof is terminal
//!   for ready actors as well as startup-window reroutes.
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

use crate::flow::routed_reopen::{
    ActorDispatchState, ActorRuntimeHealth, AuthoritativeActorDispatchAction,
    AuthoritativeActorDispatchActionFacts, AuthoritativeActorReadyFacts,
    AuthoritativePromptReadyBarrierFacts, AuthoritativeRuntimeFacts, BusyPaneAutoFixFacts,
    BusyPaneAutoFixOutcome, DegradedAuthoritativeActorDirectSubmit,
    DegradedAuthoritativeActorFacts, DirectPaneSubmitStatus as CommandDispatchStatus,
    DispatchOnlyProofOutcomeFacts, DispatchOnlyProofPolicyFacts, DispatchOnlyReopenDelivery,
    DispatchStartProofDecision, DispatchStartProofFacts, PromptReadyBarrierDecision, ReopenMode,
    RoutedDispatchStartProof, RoutedReopenFacts, RoutedReopenGuardReason, StartingActorLogFacts,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommandDispatchResult {
    status: CommandDispatchStatus,
    elapsed: Duration,
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
    format!(
        "route_latency phase={} elapsed_ms={} budget_ms={} status={} pane={} harness={} outcome={}",
        phase,
        elapsed.as_millis(),
        budget.as_millis(),
        route_latency_status(elapsed, budget),
        pane,
        harness.binary,
        outcome
    )
}

fn short_content_hash(content: &str) -> String {
    let hash = crate::ops_log::content_hash(content);
    hash[..hash.len().min(12)].to_string()
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

fn wait_for_managed_capability_proof(
    file: &Path,
    session_id: &str,
    harness: &HarnessConfig,
    timeout: Duration,
) -> Result<ManagedCapabilityProofStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        let status = managed_capability_proof_status(file, session_id, harness)?;
        if status != ManagedCapabilityProofStatus::Pending || Instant::now() >= deadline {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
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
    let proof_status = wait_for_managed_capability_proof(
        file,
        session_id,
        harness,
        fresh_route_start_ack_timeout(),
    )?;
    let reason = match proof_status {
        ManagedCapabilityProofStatus::NotRequired | ManagedCapabilityProofStatus::Proven => {
            return Ok(pane.to_string());
        }
        ManagedCapabilityProofStatus::Pending => {
            anyhow::bail!(
                "managed {} capability proof for {} on pane {} is still pending after waiting {}s; prompt dispatch remains gated until the proof succeeds",
                harness.binary,
                file.display(),
                pane,
                fresh_route_start_ack_timeout().as_secs()
            );
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
    match wait_for_managed_capability_proof(
        file,
        session_id,
        harness,
        fresh_route_start_ack_timeout(),
    )? {
        ManagedCapabilityProofStatus::NotRequired | ManagedCapabilityProofStatus::Proven => {}
        ManagedCapabilityProofStatus::Pending => anyhow::bail!(
            "{} for {}, and the fresh recovery session in pane {} did not finish capability proof within {}s. Prompt dispatch remains gated until the proof succeeds",
            reason,
            file.display(),
            dispatch_pane,
            fresh_route_start_ack_timeout().as_secs()
        ),
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
        "[agent-doc] turn in progress — the live {} session is busy, so Run Agent Doc for {} was queued and will run when the current turn finishes. No need to rerun.",
        harness.binary,
        file.display()
    )
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
    run_with_tmux(
        file,
        &Tmux::default_server(),
        pane,
        debounce_ms,
        col_args,
        mode,
        plain_trigger,
        wait_for_ready,
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
    let _wait_for_ready_guard = WaitForReadyOverrideGuard::set(wait_for_ready);
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
        std::fs::write(file, &updated_content)
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
        crate::write::atomic_write_pub(file, &cleanup.content)?;
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
        if let Err(e) = crate::preflight::run_pending_maintenance(file) {
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
    let components = crate::component::parse(&content)?;
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
        crate::write::atomic_write_pub(file, &content)
            .with_context(|| format!("failed to write queued dispatch to {}", file.display()))?;
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
    let components = crate::component::parse(content)?;
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
    Ok(crate::queue::first_prompt(&activation.entries_after).map(|head| head.text.clone()))
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
        crate::write::atomic_write_pub(file, &content)
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
    let components = crate::component::parse(content)?;
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
    let components = crate::component::parse(content)?;
    let insert_at = components
        .iter()
        .find(|component| crate::component::is_tracked_work_component(&component.name))
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
    dispatch_only_starting_pane_ready_timeout_for_binary(Some(harness.binary.as_str()), cfg!(test))
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
            "authoritative actor generation {} for {} owns pane {} but dispatch-only route will not inject a new trigger because the pane is busy on an active {} turn ({}), not at a dispatch-ready prompt. {}",
            generation,
            file.display(),
            dispatch_pane,
            harness.binary,
            cue,
            authoritative_actor_dispatch_recovery_hint(actor_state, file)
        ),
        None => format!(
            "authoritative actor generation {} for {} owns pane {} but dispatch-only route will not inject a new trigger because {} did not return to a dispatch-ready prompt in the current generation after waiting {}s. {}",
            generation,
            file.display(),
            dispatch_pane,
            reason,
            dispatch_only_busy_refusal_wait_secs(dispatch_only_starting_pane_recovery_timeout(
                Some(harness)
            )),
            authoritative_actor_dispatch_recovery_hint(actor_state, file)
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
            if let Some(context) = prompt_context {
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
                    "[route] active closeout for {} could not be drained before reroute; queued pending dispatch {:?} in active agent:queue (appended={}, already_present={}, superseded={})",
                    file.display(),
                    queued.prompt_text,
                    queued.appended,
                    queued.already_present,
                    queued.superseded
                );
                return Ok(dispatch_pane);
            }
            anyhow::bail!(
                "authoritative actor generation {} for {} owns pane {} but route could not drain the active closeout before dispatch: {}",
                actor.record.generation,
                file.display(),
                dispatch_pane,
                reason
            );
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
    // genuine active-turn busy cue (working spinner / `esc to interrupt`) when a
    // bare dispatch-only reopen targets a busy actor with no queue/prompt
    // fallback. A multi-minute active turn cannot reach a dispatch-ready prompt
    // inside the busy ready-wait budget, so waiting only yields a silent stall
    // before the inevitable refusal. Record the cue so the wait is skipped and
    // the refusal message words it as an active turn (not a cold-start wait).
    let active_turn_busy_cue: Option<String> = if dispatch_only
        && actor_state == crate::session_actor::ActorState::Busy
        && prompt_context.is_none()
        && !has_existing_inactive_queue_fallback
    {
        tmux.capture_pane(&dispatch_pane, Some(80))
            .ok()
            .and_then(|content| harness.busy_proof_line(&content))
    } else {
        None
    };
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

    let prompt_ready = actor_state == crate::session_actor::ActorState::Ready
        || current_generation_ready_prompt_proven(tmux, &actor, harness);

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
                        "[route] authoritative actor generation {} for {} is busy on pane {}; activated existing agent:queue head {:?} (already_present={}, activated={}) for idle drain instead of injecting a duplicate trigger",
                        actor.record.generation,
                        file.display(),
                        dispatch_pane,
                        queued.prompt_text,
                        queued.already_present,
                        queued.activated
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
                        "[route] authoritative actor generation {} for {} is busy on pane {}, but its agent:queue loop is already active (next head {:?}); the running loop will continue this document — reporting deferred success instead of a busy refusal",
                        actor.record.generation,
                        file.display(),
                        dispatch_pane,
                        continuation.head_prompt
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
                    "[route] authoritative actor generation {} for {} is busy on pane {}; queued pending dispatch {:?} in active agent:queue (appended={}, already_present={}, superseded={}) instead of injecting a duplicate trigger",
                    actor.record.generation,
                    file.display(),
                    dispatch_pane,
                    queued.prompt_text,
                    queued.appended,
                    queued.already_present,
                    queued.superseded
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
                    "[route] authoritative actor generation {} for {} is busy on pane {}, but its agent:queue loop is already active (next head {:?}); the running loop will continue this document — reporting deferred success instead of a busy refusal",
                    actor.record.generation,
                    file.display(),
                    dispatch_pane,
                    continuation.head_prompt
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
            let _authorization = authorize_controller_dispatch(
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
            )?;
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
            let _authorization = authorize_controller_dispatch(
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
            )?;
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
            let _authorization = authorize_controller_dispatch(
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
            )?;
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
mod tests;
