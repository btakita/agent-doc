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
//! - **`await_idle(file, debounce)`**: Polls file mtime and the shared editor typing
//!   indicator every 100ms until both have been idle for `debounce`, or fails closed
//!   after the `10 × debounce` safety cap expires.
//! - **`wait_for_agent_ready(tmux, pane_id, timeout, harness)`**: Polls pane content every 500ms
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
    authoritative_actor_ready_retry_budget, busy_projection_repaired_by_ready_prompt,
    busy_existing_pane_auto_fix_outcome as flow_busy_existing_pane_auto_fix_outcome,
    can_use_degraded_authoritative_actor, classify_authoritative_actor_dispatch_action,
    classify_authoritative_prompt_ready_barrier, classify_dispatch_start_proof,
    decide_authoritative_reopen, degraded_authoritative_actor_direct_submit_log_message,
    direct_pane_submit_outcome as flow_direct_pane_submit_outcome,
    dispatch_only_dispatch_start_proof_required as flow_dispatch_only_dispatch_start_proof_required,
    dispatch_only_focus_only_should_fail_closed,
    dispatch_only_sent_console_message, dispatch_only_sent_log_message,
    dispatch_only_starting_pane_ready_retry_budget,
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
struct CommandDispatchResult {
    status: CommandDispatchStatus,
    elapsed: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteMode {
    Managed,
    DispatchOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExistingPaneDispatchReadiness {
    Ready,
    BusyAlreadyRunning,
    BusyNeedsAutoFix {
        provenance: String,
        blocker_reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BusyPaneInterruptRecoveryOutcome {
    Recovered,
    Blocked { reason: String },
    TimedOut,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupervisorHealth {
    Healthy,
    Restartable,
    Halted { restart_count: u32 },
    Unreachable,
    NoSocket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SupervisorRuntime {
    health: SupervisorHealth,
    actor_state: Option<crate::session_actor::ActorState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthoritativeActorDispatchTarget {
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
struct PendingPromptBearingRouteContext {
    marker: String,
    prompt_text: String,
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
enum AgentReadyWaitOutcome {
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

    crate::ops_log::log_op(
        file,
        &format!(
            "route_harness_clear_restart_fresh file={} pane={} harness={} latest_prompt=/clear",
            file.display(),
            pane,
            harness.binary
        ),
    );
    eprintln!(
        "[route] latest tracked {} prompt for {} was `/clear` — restarting the live session fresh before reroute so sandbox, writable roots, and network policy are reapplied",
        harness.binary,
        file.display()
    );

    if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
        anyhow::bail!(
            "latest tracked {} prompt for {} was `/clear`, but route could not restart the live session fresh to reapply the original launch policy. Run `agent-doc start {}` manually to recover",
            harness.binary,
            file.display(),
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
            "latest tracked {} prompt for {} was `/clear`, and the fresh recovery session in pane {} never became ready. Run `agent-doc start {}` manually to recover",
            harness.binary,
            file.display(),
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
    let global_config = crate::config::load().unwrap_or_default();
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

    // Debounce: wait for file mtime and editor typing indicator to settle before
    // route performs visible mutations such as session UUID insertion or
    // duplicate-prompt cleanup.
    if debounce_ms > 0 {
        await_idle(file, Duration::from_millis(debounce_ms))?;
    }

    // Ensure session UUID exists in frontmatter (generate if missing)
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
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
    let fm = frontmatter::parse_for_file_with_context(&updated_content, file, &rc).map(|(f, _)| f)?;
    let global_config = crate::config::load().unwrap_or_default();
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

    // Reap completed tracked items across ALL surfaces (backlog, review, icebox)
    // and re-sync the snapshot before the focused repair, matching what a manual
    // re-run's full preflight maintenance does. The repair sub-step only reaps
    // the backlog, so a deployed/completed `[x]` item left in review or icebox
    // would make that reap a no-op, the post-repair session-check would still
    // find the completed item, and route would refuse dispatch until the user
    // manually retried (the "JB Run Agent Doc failed; repeat succeeded" report).
    // run_pending_maintenance is idempotent, so this is safe even when there is
    // nothing to reap.
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
    match crate::repair::repair(file) {
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
                Ok(RouteCloseoutDrainOutcome::Recovered(label))
            }
            crate::session_check::SessionCheckStatus::Interrupted(reason) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_dispatch_drain_closeout_blocked file={} cycle_id={} blocker={}",
                        file.display(),
                        state.cycle_id,
                        crate::secret_redact::redact(&reason)
                    ),
                );
                Ok(RouteCloseoutDrainOutcome::Blocked(reason))
            }
        },
        Err(err) => {
            let reason = err.to_string();
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_drain_closeout_blocked file={} cycle_id={} blocker={}",
                    file.display(),
                    state.cycle_id,
                    crate::secret_redact::redact(&reason)
                ),
            );
            Ok(RouteCloseoutDrainOutcome::Blocked(reason))
        }
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

fn enqueue_route_dispatch_prompt(
    file: &Path,
    prompt_text: &str,
    source: &str,
) -> Result<RouteQueueEnqueueOutcome> {
    let prompt_text = queue_prompt_text_for_route_change(prompt_text)
        .ok_or_else(|| anyhow::anyhow!("route queue prompt is empty"))?;
    let _lock = acquire_route_queue_lock(file)?;
    let original = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let mut content = frontmatter::merge_fields(&original, "queue_active: true")?;
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
                already_present = crate::queue::prompts(&entries)
                    .iter()
                    .any(|prompt| prompt.text.trim() == prompt_text);
                if !already_present {
                    let active_prompt_count = entries
                        .iter()
                        .filter(|entry| matches!(entry, crate::queue::QueueEntry::Prompt(_)))
                        .count();
                    let replace_single_auto_prompt =
                        crate::queue::has_auto_attr(&queue_component.attrs)
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
                        entries.push(crate::queue::QueueEntry::Prompt(
                            crate::queue::QueuePrompt {
                                multiline: prompt_text.contains('\n'),
                                text: prompt_text.clone(),
                            },
                        ));
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
                if body
                    .lines()
                    .any(|line| line.trim() == new_rendered.trim())
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
        content = ensure_queue_component_auto_attr(&content)?;
    } else {
        component_created = true;
        appended = true;
        content = insert_auto_queue_component(&content, &prompt_text)?;
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

fn ensure_queue_component_auto_attr(content: &str) -> Result<String> {
    let components = crate::component::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(content.to_string());
    };
    if crate::queue::has_auto_attr(&queue_component.attrs) {
        return Ok(content.to_string());
    }
    let open_tag = &content[queue_component.open_start..queue_component.open_end];
    let newline = if open_tag.ends_with('\n') { "\n" } else { "" };
    let trimmed = open_tag.trim_end_matches('\n');
    let new_tag = trimmed.replacen("<!-- agent:queue", "<!-- agent:queue auto", 1);
    let mut result = String::with_capacity(content.len() + " auto".len());
    result.push_str(&content[..queue_component.open_start]);
    result.push_str(&new_tag);
    result.push_str(newline);
    result.push_str(&content[queue_component.open_end..]);
    Ok(result)
}

fn insert_auto_queue_component(content: &str, prompt_text: &str) -> Result<String> {
    let body = crate::queue::render(&[crate::queue::QueueEntry::Prompt(
        crate::queue::QueuePrompt {
            multiline: prompt_text.contains('\n'),
            text: prompt_text.to_string(),
        },
    )]);
    let block = format!(
        "<!-- agent:queue auto -->\n{}<!-- /agent:queue -->\n\n",
        body
    );
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

fn dispatch_only_requires_ready_probe(
    status: Option<&crate::startup_miss::SessionLogStatus>,
    pane: &str,
    harness: &HarnessConfig,
) -> bool {
    let Some(status) = status else {
        return false;
    };
    if !status.latest_session_open()
        || status.latest_start_pane.as_deref() != Some(pane)
        || status.saw_committed_cycle_after_latest_run
    {
        return false;
    }

    status
        .latest_run_event
        .as_deref()
        .and_then(|event| event.split_whitespace().next())
        .is_some_and(|token| {
            token == format!("{}_start", harness.binary)
                || token == format!("{}_restart", harness.binary)
        })
}

fn dispatch_only_starting_pane_not_ready_error(
    harness: &HarnessConfig,
    pane: &str,
    file: &Path,
    detail: &str,
) -> String {
    format!(
        "dispatch-only {} reopen refused to inject into pane {} for {} because the latest run is still booting and never reached a dispatch-ready prompt ({detail}); wait for the pane to become ready and reroute again",
        harness.binary,
        pane,
        file.display()
    )
}

#[derive(Debug, Clone, Copy)]
struct DispatchOnlySendReopenOptions<'a> {
    delivery: DispatchOnlyReopenDelivery,
    queue_prompt_text: Option<&'a str>,
}

fn dispatch_only_send_reopen(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    options: DispatchOnlySendReopenOptions<'_>,
) -> Result<String> {
    let delivery = options.delivery;
    let mut dispatch_pane = pane.to_string();
    let mut log_status = crate::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten();
    let mut recovery_attempts = 0usize;
    let requires_ready_probe =
        dispatch_only_requires_ready_probe(log_status.as_ref(), &dispatch_pane, harness);
    if requires_ready_probe {
        loop {
            let ready_outcome = wait_for_agent_ready_outcome(
                tmux,
                &dispatch_pane,
                dispatch_only_starting_pane_ready_timeout(harness),
                harness,
            );
            if ready_outcome.is_ready() {
                break;
            }

            if recovery_attempts < 2
                && let Some(target) = wait_for_starting_pane_recovery_target(
                    tmux,
                    file,
                    session_id,
                    &dispatch_pane,
                    file_path,
                    harness,
                    log_status.as_ref(),
                )
            {
                recovery_attempts += 1;
                match target {
                    StartingPaneRecoveryTarget::SamePane => {
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_dispatch_only_starting_pane_retry_same_pane file={} pane={} harness={} attempt={}",
                                file.display(),
                                dispatch_pane,
                                harness.binary,
                                recovery_attempts
                            ),
                        );
                        log_status = crate::startup_miss::session_log_status(file, session_id)
                            .ok()
                            .flatten();
                        continue;
                    }
                    StartingPaneRecoveryTarget::DifferentPane(next_pane) => {
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_dispatch_only_starting_pane_handoff file={} old_pane={} new_pane={} harness={} attempt={}",
                                file.display(),
                                dispatch_pane,
                                next_pane,
                                harness.binary,
                                recovery_attempts
                            ),
                        );
                        dispatch_pane = next_pane;
                        log_status = crate::startup_miss::session_log_status(file, session_id)
                            .ok()
                            .flatten();
                        continue;
                    }
                }
            }

            let detail = ready_outcome.blocker_reason().unwrap_or("timed_out");
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_only_starting_pane_not_ready file={} pane={} harness={} outcome={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    detail
                ),
            );
            anyhow::bail!(dispatch_only_starting_pane_not_ready_error(
                harness,
                &dispatch_pane,
                file,
                detail
            ));
        }
    }

    if let Ok(content) = sessions::capture_pane(tmux, &dispatch_pane)
        && let Some(reason) = dispatch_only_blocker_reason(harness, &content)
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_blocked file={} pane={} harness={} reason={}",
                file.display(),
                dispatch_pane,
                harness.binary,
                reason
            ),
        );
        if let Some(source) = dispatch_active_turn_queue_source(harness, &reason)
            && let Some(prompt_text) = options.queue_prompt_text
        {
            let queued = enqueue_route_dispatch_prompt(file, prompt_text, source)?;
            eprintln!(
                "[route] dispatch-only {} reopen for {} found {} on pane {}; queued pending dispatch {:?} in agent:queue auto (appended={}, already_present={}, superseded={}) instead of injecting a duplicate trigger",
                harness.binary,
                file.display(),
                reason,
                dispatch_pane,
                queued.prompt_text,
                queued.appended,
                queued.already_present,
                queued.superseded
            );
            return Ok(dispatch_pane);
        }
        let recovery = dispatch_blocker_recovery_hint(harness, &reason, file);
        anyhow::bail!(
            "dispatch-only {} reopen refused to inject into pane {} for {} because the pane still shows {}; {}",
            harness.binary,
            dispatch_pane,
            file.display(),
            reason,
            recovery
        );
    }

    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    let dispatch_start = match delivery {
        DispatchOnlyReopenDelivery::SupervisorIpcOnce => dispatch_via_supervisor_ipc_with_mode(
            tmux,
            file,
            &dispatch_pane,
            session_id,
            file_path,
            harness,
            SupervisorIpcDispatchOptions {
                await_start_proof: true,
                print_unproven_progress: should_print_dispatch_only_unproven_progress(
                    file, harness,
                ),
            },
        )?,
        DispatchOnlyReopenDelivery::DirectPaneSubmit => dispatch_routed_reopen_with_mode(
            tmux,
            file,
            &dispatch_pane,
            file_path,
            harness,
            should_print_dispatch_only_unproven_progress(file, harness),
        )?,
    };
    require_dispatch_only_dispatch_start_proof(
        file,
        &dispatch_pane,
        harness,
        delivery,
        dispatch_start,
    )?;
    crate::ops_log::log_op(
        file,
        &route_dispatch_only_sent_log_message(
            file,
            &dispatch_pane,
            harness,
            delivery,
            dispatch_start,
        ),
    );
    eprintln!(
        "{}",
        route_dispatch_only_sent_console_message(
            file,
            &dispatch_pane,
            harness,
            delivery,
            dispatch_start,
        )
    );
    Ok(dispatch_pane)
}

fn should_print_dispatch_only_unproven_progress(file: &Path, harness: &HarnessConfig) -> bool {
    flow_should_print_dispatch_only_unproven_progress(DispatchOnlyProofPolicyFacts {
        harness_binary: harness.binary.as_str(),
        codex_dispatch_start_tracking_enabled: codex_dispatch_start_tracking_enabled(file),
    })
}

fn dispatch_only_dispatch_start_proof_required(file: &Path, harness: &HarnessConfig) -> bool {
    flow_dispatch_only_dispatch_start_proof_required(DispatchOnlyProofPolicyFacts {
        harness_binary: harness.binary.as_str(),
        codex_dispatch_start_tracking_enabled: codex_dispatch_start_tracking_enabled(file),
    })
}

fn require_dispatch_only_dispatch_start_proof(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
) -> Result<()> {
    let proof_required = dispatch_only_dispatch_start_proof_required(file, harness);
    let classification = classify_dispatch_start_proof(DispatchStartProofFacts {
        proof: dispatch_start,
        dispatch_start_proof_required: proof_required,
    });
    if classification.decision == DispatchStartProofDecision::Accepted {
        return Ok(());
    }

    let timeout = routed_dispatch_start_timeout(harness).as_secs();
    let file_display = file.display().to_string();
    let facts = DispatchOnlyProofOutcomeFacts {
        file_display: file_display.as_str(),
        pane,
        harness_binary: harness.binary.as_str(),
        delivery,
        dispatch_start,
        timeout_secs: timeout,
    };
    log_dispatch_proof_failed(
        file,
        RoutedReopenGuardReason::AcceptedOnlyDispatchStartProof,
    );
    crate::ops_log::log_op(file, &accepted_only_dispatch_start_log_message(facts));
    anyhow::bail!(accepted_only_dispatch_start_refusal_message(facts));
}

fn route_dispatch_only_sent_log_message(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
) -> String {
    let file_display = file.display().to_string();
    dispatch_only_sent_log_message(DispatchOnlyProofOutcomeFacts {
        file_display: file_display.as_str(),
        pane,
        harness_binary: harness.binary.as_str(),
        delivery,
        dispatch_start,
        timeout_secs: routed_dispatch_start_timeout(harness).as_secs(),
    })
}

fn route_dispatch_only_sent_console_message(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
) -> String {
    let file_display = file.display().to_string();
    dispatch_only_sent_console_message(DispatchOnlyProofOutcomeFacts {
        file_display: file_display.as_str(),
        pane,
        harness_binary: harness.binary.as_str(),
        delivery,
        dispatch_start,
        timeout_secs: routed_dispatch_start_timeout(harness).as_secs(),
    })
}

#[allow(clippy::too_many_arguments)]
fn dispatch_only_reopen_existing_pane(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
    prompt_bearing_marker: Option<&str>,
    queue_prompt_text: Option<&str>,
    allow_auto_fix_retry: bool,
    allow_busy_interrupt_retry: bool,
    auto_fix_attempted: bool,
    pane_id: &str,
    delivery: DispatchOnlyReopenDelivery,
    skip_capability_proof: bool,
) -> Result<String> {
    let dispatch_pane = reapply_codex_launch_contract_before_reuse(
        tmux, file, pane_id, session_id, file_path, harness, false, false,
    )?;
    if !skip_capability_proof {
        match wait_for_managed_capability_proof(
            file,
            session_id,
            harness,
            fresh_route_start_ack_timeout(),
        )? {
            ManagedCapabilityProofStatus::NotRequired | ManagedCapabilityProofStatus::Proven => {}
            ManagedCapabilityProofStatus::Pending => anyhow::bail!(
                "dispatch-only {} reopen for {} on pane {} is gated because managed capability proof is still pending after waiting {}s",
                harness.binary,
                file.display(),
                dispatch_pane,
                fresh_route_start_ack_timeout().as_secs()
            ),
            ManagedCapabilityProofStatus::Failed => anyhow::bail!(
                "dispatch-only {} reopen for {} on pane {} is disabled because managed capability proof failed",
                harness.binary,
                file.display(),
                dispatch_pane
            ),
            ManagedCapabilityProofStatus::Missing => anyhow::bail!(
                "dispatch-only {} reopen for {} on pane {} is disabled because this network/SSH/write-root session has no current capability proof",
                harness.binary,
                file.display(),
                dispatch_pane
            ),
        }
    } else {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_skip_capability_proof file={} pane={} harness={} reason=degraded_supervisor_unreachable",
                file.display(),
                dispatch_pane,
                harness.binary
            ),
        );
    }
    let log_status = crate::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten();
    if dispatch_only_requires_ready_probe(log_status.as_ref(), &dispatch_pane, harness) {
        return dispatch_only_send_reopen(
            tmux,
            file,
            session_id,
            &dispatch_pane,
            file_path,
            harness,
            DispatchOnlySendReopenOptions {
                delivery,
                queue_prompt_text,
            },
        );
    }
    if harness.binary == "codex"
        && crate::startup_miss::load(file)
            .ok()
            .flatten()
            .is_some_and(|miss| miss.pane_id == dispatch_pane)
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_startup_miss_bypass file={} pane={} harness={}",
                file.display(),
                dispatch_pane,
                harness.binary
            ),
        );
        return dispatch_only_send_reopen(
            tmux,
            file,
            session_id,
            &dispatch_pane,
            file_path,
            harness,
            DispatchOnlySendReopenOptions {
                delivery,
                queue_prompt_text,
            },
        );
    }
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    match ensure_existing_pane_ready_for_dispatch(
        tmux,
        file,
        &dispatch_pane,
        harness,
        prompt_bearing_marker,
    )? {
        ExistingPaneDispatchReadiness::Ready => dispatch_only_send_reopen(
            tmux,
            file,
            session_id,
            &dispatch_pane,
            file_path,
            harness,
            DispatchOnlySendReopenOptions {
                delivery,
                queue_prompt_text,
            },
        ),
        ExistingPaneDispatchReadiness::BusyAlreadyRunning => Ok(dispatch_pane),
        ExistingPaneDispatchReadiness::BusyNeedsAutoFix {
            provenance,
            blocker_reason,
        } => retry_dispatch_only_after_busy_pane(
            tmux,
            file,
            pane,
            col_args,
            session_id,
            file_path,
            target_session,
            harness,
            created_panes,
            prompt_bearing_marker,
            queue_prompt_text,
            allow_auto_fix_retry,
            allow_busy_interrupt_retry,
            auto_fix_attempted,
            &dispatch_pane,
            &provenance,
            blocker_reason.as_deref(),
            delivery,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn retry_dispatch_only_after_busy_pane(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
    prompt_bearing_marker: Option<&str>,
    queue_prompt_text: Option<&str>,
    allow_auto_fix_retry: bool,
    allow_busy_interrupt_retry: bool,
    auto_fix_attempted: bool,
    busy_pane: &str,
    provenance: &str,
    blocker_reason: Option<&str>,
    delivery: DispatchOnlyReopenDelivery,
) -> Result<String> {
    let fallback_detail = blocker_reason.map(|reason| format!("still shows {reason}"));
    if allow_auto_fix_retry {
        match attempt_busy_existing_pane_auto_fix(tmux, file, session_id, busy_pane, file_path)? {
            BusyPaneAutoFixOutcome::RetryRoute => {
                return dispatch_only_reopen_existing_pane(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    prompt_bearing_marker,
                    queue_prompt_text,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                    busy_pane,
                    delivery,
                    false,
                );
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart => {
                wait_for_busy_restart_handoff(tmux, file, file_path, session_id, busy_pane);
                return dispatch_only_reopen_existing_pane(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    prompt_bearing_marker,
                    queue_prompt_text,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                    busy_pane,
                    delivery,
                    false,
                );
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_retry_after_fresh_restart file={} pane={} harness={}",
                        file.display(),
                        busy_pane,
                        harness.binary
                    ),
                );
                eprintln!(
                    "[route] dispatch-only {} reopen for {} found busy authoritative pane {} after the scoped recovery path — restarting the live session fresh once before retrying",
                    harness.binary,
                    file.display(),
                    busy_pane
                );
                if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
                    emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
                    anyhow::bail!(format_busy_existing_pane_error(
                        file,
                        busy_pane,
                        harness,
                        provenance,
                        fallback_detail.as_deref(),
                        true
                    ));
                }
                wait_for_busy_restart_handoff(tmux, file, file_path, session_id, busy_pane);
                return dispatch_only_reopen_existing_pane(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    prompt_bearing_marker,
                    queue_prompt_text,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                    busy_pane,
                    delivery,
                    false,
                );
            }
            BusyPaneAutoFixOutcome::FailClosed => {}
        }
    }
    if allow_busy_interrupt_retry {
        match attempt_busy_existing_pane_interrupt_recovery(
            tmux,
            file,
            busy_pane,
            harness,
            blocker_reason,
        )? {
            BusyPaneInterruptRecoveryOutcome::Recovered => {
                return dispatch_only_reopen_existing_pane(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    prompt_bearing_marker,
                    queue_prompt_text,
                    false,
                    false,
                    true,
                    busy_pane,
                    delivery,
                    false,
                );
            }
            BusyPaneInterruptRecoveryOutcome::Blocked { reason } => {
                emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
                let detail = format!("bounded interrupt recovery still shows {reason}");
                anyhow::bail!(format_busy_existing_pane_error(
                    file,
                    busy_pane,
                    harness,
                    provenance,
                    Some(detail.as_str()),
                    auto_fix_attempted || allow_auto_fix_retry
                ));
            }
            BusyPaneInterruptRecoveryOutcome::TimedOut => {
                emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
                anyhow::bail!(format_busy_existing_pane_error(
                    file,
                    busy_pane,
                    harness,
                    provenance,
                    Some("bounded interrupt recovery never restored a dispatch-ready prompt"),
                    auto_fix_attempted || allow_auto_fix_retry
                ));
            }
            BusyPaneInterruptRecoveryOutcome::Skipped => {}
        }
    }
    emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
    anyhow::bail!(format_busy_existing_pane_error(
        file,
        busy_pane,
        harness,
        provenance,
        fallback_detail.as_deref(),
        auto_fix_attempted || allow_auto_fix_retry
    ));
}

fn dispatch_only_blocker_reason(harness: &HarnessConfig, content: &str) -> Option<String> {
    if let Some(reason) = harness.dispatch_blocker_reason(content) {
        return Some(reason);
    }
    if harness.binary != "codex" {
        return None;
    }

    let normalized = crate::prompt::strip_ansi(content).to_ascii_lowercase();
    if normalized.contains("reverse-i-search") {
        Some("interactive shell reverse-i-search".to_string())
    } else if normalized.contains("i-search")
        && normalized.contains("accept")
        && normalized.contains("cancel")
    {
        Some("interactive shell history search".to_string())
    } else {
        None
    }
}

fn dispatch_blocker_recovery_hint(harness: &HarnessConfig, reason: &str, file: &Path) -> String {
    if harness.binary == "codex" && reason == "codex hook review prompt" {
        return format!(
            "open `/hooks` in that Codex pane, approve or disable the pending hook change, wait for the idle composer, then rerun `agent-doc route --dispatch-only {}` or the editor Run Agent Doc action",
            file.display()
        );
    }

    "restore an idle prompt and retry".to_string()
}

fn dispatch_active_turn_queue_source(
    harness: &HarnessConfig,
    reason: &str,
) -> Option<&'static str> {
    match (harness.binary.as_str(), reason) {
        ("codex", "active codex turn") => Some("dispatch_only_codex_active_turn"),
        ("opencode", "opencode active turn") => Some("dispatch_only_opencode_active_turn"),
        _ => None,
    }
}

fn load_authoritative_actor_binding(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    respect_tracked_clear_restart: bool,
    enforce_capability_proof: bool,
) -> Result<Option<AuthoritativeActorDispatchTarget>> {
    if respect_tracked_clear_restart
        && tracked_harness_clear_requires_fresh_restart(
            harness,
            crate::codex_hook::load_latest_prompt_for_file(file)?.as_deref(),
        )
    {
        return Ok(None);
    }

    let base_dir = registry_base_dir_for_dispatch(file_path);
    let Some(record) = crate::project_controller::authoritative_actor_binding(&base_dir, file)?
    else {
        return Ok(None);
    };
    if record.session_id != session_id {
        anyhow::bail!(
            "authoritative actor record for {} is bound to session {}, not {}",
            file.display(),
            record.session_id,
            session_id
        );
    }
    if !tmux.pane_alive(&record.pane_id) {
        return Ok(None);
    }
    let expected_harness = crate::session_actor::normalize_harness_name(&harness.binary);
    if !record.harness.trim().is_empty()
        && record.harness != "default"
        && record.harness != expected_harness
    {
        let runtime = query_supervisor_runtime(file, session_id);
        let effective_state = runtime.actor_state.unwrap_or(record.state);
        if mismatched_authoritative_actor_can_be_replaced(&runtime, effective_state) {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_harness_mismatch_stale file={} pane={} stored_harness={} expected_harness={} generation={} supervisor_health={} actor_state={}",
                    file.display(),
                    record.pane_id,
                    record.harness,
                    expected_harness,
                    record.generation,
                    supervisor_health_label(runtime.health),
                    effective_state.as_str()
                ),
            );
            return Ok(None);
        }
        anyhow::bail!(
            "authoritative actor record for {} is bound to harness {}, not {}",
            file.display(),
            record.harness,
            expected_harness
        );
    }
    if enforce_capability_proof {
        match wait_for_managed_capability_proof(
            file,
            session_id,
            harness,
            fresh_route_start_ack_timeout(),
        )? {
            ManagedCapabilityProofStatus::NotRequired | ManagedCapabilityProofStatus::Proven => {}
            ManagedCapabilityProofStatus::Pending => {
                anyhow::bail!(
                    "managed {} capability proof for {} on pane {} is still pending after waiting {}s; prompt dispatch remains gated until the proof succeeds",
                    harness.binary,
                    file.display(),
                    record.pane_id,
                    fresh_route_start_ack_timeout().as_secs()
                );
            }
            ManagedCapabilityProofStatus::Failed => {
                anyhow::bail!(
                    "managed {} capability proof for {} on pane {} failed; prompt dispatch is disabled for this pane. Inspect diagnostics, then run `agent-doc start {}` manually to recover",
                    harness.binary,
                    file.display(),
                    record.pane_id,
                    file.display()
                );
            }
            ManagedCapabilityProofStatus::Missing => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_authoritative_actor_missing_{}_capability_proof file={} pane={} harness={} generation={}",
                        harness.binary,
                        file.display(),
                        record.pane_id,
                        harness.binary,
                        record.generation
                    ),
                );
                return Ok(None);
            }
        }
    }

    let runtime = query_supervisor_runtime(file, session_id);
    let (record, runtime) = promote_starting_authoritative_actor_if_dispatch_ready(
        tmux, file, file_path, record, runtime, harness,
    );
    Ok(Some(AuthoritativeActorDispatchTarget { record, runtime }))
}

fn mismatched_authoritative_actor_can_be_replaced(
    runtime: &SupervisorRuntime,
    actor_state: crate::session_actor::ActorState,
) -> bool {
    runtime.health != SupervisorHealth::Healthy
        || actor_state == crate::session_actor::ActorState::Closed
}

fn promote_starting_authoritative_actor_if_dispatch_ready(
    tmux: &Tmux,
    file: &Path,
    file_path: &str,
    record: crate::session_actor::ActorRecord,
    mut runtime: SupervisorRuntime,
    harness: &HarnessConfig,
) -> (crate::session_actor::ActorRecord, SupervisorRuntime) {
    let effective_state = runtime.actor_state.unwrap_or(record.state);
    if runtime.health != SupervisorHealth::Healthy
        || effective_state != crate::session_actor::ActorState::Starting
    {
        return (record, runtime);
    }

    let _ = tmux.select_pane(&record.pane_id);
    let pane_ready = tmux
        .capture_pane(&record.pane_id, Some(80))
        .ok()
        .map(|content| ready_prompt_candidate(&content, harness).is_some())
        .unwrap_or(false);
    if !pane_ready {
        return (record, runtime);
    }

    let base_dir = registry_base_dir_for_dispatch(file_path);
    match crate::project_controller::mark_lifecycle(
        &base_dir,
        crate::project_controller::LifecycleRequest {
            file: file.to_path_buf(),
            session_id: record.session_id.clone(),
            pane_id: record.pane_id.clone(),
            generation: record.generation,
            state: crate::session_actor::ActorState::Ready,
            caller: "route".to_string(),
            reason: "dispatch_ready_prompt".to_string(),
        },
    ) {
        Ok(updated) => {
            runtime.actor_state = Some(crate::session_actor::ActorState::Ready);
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_promoted_ready file={} session={} pane={} generation={} reason=dispatch_ready_prompt",
                    file.display(),
                    updated.session_id,
                    updated.pane_id,
                    updated.generation
                ),
            );
            (updated, runtime)
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to promote starting authoritative actor {} for {} after seeing a dispatch-ready prompt: {}",
                record.pane_id,
                file.display(),
                err
            );
            (record, runtime)
        }
    }
}

fn recover_starting_timeout_blocked_actor_if_dispatch_ready(
    tmux: &Tmux,
    file: &Path,
    file_path: &str,
    actor: &AuthoritativeActorDispatchTarget,
    harness: &HarnessConfig,
) -> Option<AuthoritativeActorDispatchTarget> {
    if !starting_timeout_blocked_actor_can_recover(
        actor,
        current_generation_ready_prompt_proven(tmux, actor, harness),
    ) {
        return None;
    }

    let base_dir = registry_base_dir_for_dispatch(file_path);
    match crate::project_controller::mark_lifecycle(
        &base_dir,
        crate::project_controller::LifecycleRequest {
            file: file.to_path_buf(),
            session_id: actor.record.session_id.clone(),
            pane_id: actor.record.pane_id.clone(),
            generation: actor.record.generation,
            state: crate::session_actor::ActorState::Ready,
            caller: "route".to_string(),
            reason: "dispatch_ready_prompt".to_string(),
        },
    ) {
        Ok(updated) => {
            clear_starting_actor_timeout_record(file_path);
            let mut runtime = actor.runtime.clone();
            runtime.actor_state = Some(crate::session_actor::ActorState::Ready);
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_starting_timeout_recovered_ready file={} session={} pane={} generation={}",
                    file.display(),
                    updated.session_id,
                    updated.pane_id,
                    updated.generation
                ),
            );
            Some(AuthoritativeActorDispatchTarget {
                record: updated,
                runtime,
            })
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to recover timed-out starting actor {} generation {} for {} after seeing a dispatch-ready prompt: {}",
                actor.record.pane_id,
                actor.record.generation,
                file.display(),
                err
            );
            None
        }
    }
}

fn current_generation_ready_prompt_proven(
    tmux: &Tmux,
    target: &AuthoritativeActorDispatchTarget,
    harness: &HarnessConfig,
) -> bool {
    if tmux
        .capture_pane(&target.record.pane_id, Some(80))
        .ok()
        .map(|content| ready_prompt_candidate(&content, harness).is_some())
        .unwrap_or(false)
    {
        return true;
    }

    target.record.last_transition.new_generation == target.record.generation
        && matches!(
            target.record.last_transition.reason.as_str(),
            "prompt_ready" | "dispatch_ready_prompt"
        )
        && target.actor_state() == crate::session_actor::ActorState::Ready
}

fn authorize_controller_dispatch(
    file: &Path,
    session_id: &str,
    file_path: &str,
    actor: &AuthoritativeActorDispatchTarget,
    command_kind: &str,
    diagnostic_payload: &str,
) -> Result<crate::project_controller::DispatchAuthorization> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    crate::project_controller::authorize_dispatch(
        &base_dir,
        crate::project_controller::DispatchRequest {
            file: file.to_path_buf(),
            session_id: session_id.to_string(),
            pane_id: actor.record.pane_id.clone(),
            generation: actor.record.generation,
            command_kind: command_kind.to_string(),
            diagnostic_payload: diagnostic_payload.to_string(),
        },
    )
}

fn load_authoritative_actor_dispatch_target(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    respect_tracked_clear_restart: bool,
    enforce_capability_proof: bool,
) -> Result<Option<AuthoritativeActorDispatchTarget>> {
    Ok(load_authoritative_actor_binding(
        tmux,
        file,
        session_id,
        file_path,
        harness,
        respect_tracked_clear_restart,
        enforce_capability_proof,
    )?
    .filter(authoritative_actor_dispatch_target_eligible))
}

fn load_authoritative_actor_for_registered_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    pane: &str,
) -> Result<Option<AuthoritativeActorDispatchTarget>> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    let document_id = crate::session_actor::canonical_document_id_in(&base_dir, file_path);
    let record = crate::project_controller::load_actor_store(&base_dir)?
        .values()
        .find(|record| {
            record.document_id == document_id
                && record.session_id == session_id
                && record.pane_id == pane
        })
        .cloned();
    let Some(record) = record else {
        return Ok(None);
    };
    if !tmux.pane_alive(&record.pane_id) {
        return Ok(None);
    }
    Ok(Some(AuthoritativeActorDispatchTarget {
        record,
        runtime: query_supervisor_runtime(file, session_id),
    }))
}

fn dispatch_only_can_use_degraded_authoritative_actor(
    actor: &AuthoritativeActorDispatchTarget,
    registered: Option<&str>,
    live_owner: Option<&str>,
) -> bool {
    can_use_degraded_authoritative_actor(DegradedAuthoritativeActorFacts {
        actor_pane: actor.record.pane_id.as_str(),
        transition_caller: actor.record.last_transition.caller.as_str(),
        transition_reason: actor.record.last_transition.reason.as_str(),
        registered_pane: registered,
        live_owner_pane: live_owner,
    })
}

#[cfg(test)]
fn authoritative_actor_start_wait_terminal_state(state: crate::session_actor::ActorState) -> bool {
    crate::flow::routed_reopen::actor_start_wait_terminal_state(actor_dispatch_state(state))
}

fn route_starting_actor_not_ready_log_line(
    file: &Path,
    harness: &HarnessConfig,
    timeout: Duration,
    elapsed: Duration,
    facts: &AuthoritativeActorReadyFacts,
) -> String {
    let file_display = file.display().to_string();
    starting_actor_not_ready_log_line(StartingActorLogFacts {
        file_display: file_display.as_str(),
        harness_binary: harness.binary.as_str(),
        timeout,
        elapsed,
        ready_facts: facts,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct StartingActorTimeoutRecord {
    pane_id: String,
    generation: u64,
    log_line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartingActorTimeoutLogDecision {
    NewTimeout,
    DuplicateTimeout,
}

fn starting_actor_timeout_paths(file_path: &str) -> Option<(PathBuf, PathBuf)> {
    let requested = PathBuf::from(file_path);
    let root = crate::snapshot::find_project_root(&requested)?;
    let hash = crate::snapshot::doc_hash_from_str(file_path);
    let state_dir = root.join(".agent-doc/state/route-starting-timeouts");
    let lock_dir = root.join(".agent-doc/locks");
    Some((
        state_dir.join(format!("{hash}.json")),
        lock_dir.join(format!("route-starting-timeout-{hash}.lock")),
    ))
}

fn record_starting_actor_timeout(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
    log_line: &str,
) -> Result<StartingActorTimeoutLogDecision> {
    let Some((state_path, lock_path)) = starting_actor_timeout_paths(file_path) else {
        return Ok(StartingActorTimeoutLogDecision::NewTimeout);
    };

    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    lock.lock_exclusive()?;

    let existing = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|content| serde_json::from_str::<StartingActorTimeoutRecord>(&content).ok());
    if existing.as_ref().is_some_and(|record| {
        record.pane_id == facts.pane_id && record.generation == facts.generation
    }) {
        let _ = lock.unlock();
        return Ok(StartingActorTimeoutLogDecision::DuplicateTimeout);
    }

    let record = StartingActorTimeoutRecord {
        pane_id: facts.pane_id.clone(),
        generation: facts.generation,
        log_line: log_line.to_string(),
    };
    std::fs::write(&state_path, serde_json::to_string_pretty(&record)?)?;
    let _ = lock.unlock();
    Ok(StartingActorTimeoutLogDecision::NewTimeout)
}

fn load_starting_actor_timeout_record(file_path: &str) -> Option<StartingActorTimeoutRecord> {
    let (state_path, _) = starting_actor_timeout_paths(file_path)?;
    std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|content| serde_json::from_str::<StartingActorTimeoutRecord>(&content).ok())
}

fn starting_actor_timeout_record_matches(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
) -> bool {
    if facts.actor_state != ActorDispatchState::Starting {
        return false;
    }
    starting_actor_timeout_record_identity_matches(file_path, facts)
}

fn starting_actor_timeout_record_identity_matches(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
) -> bool {
    load_starting_actor_timeout_record(file_path).is_some_and(|record| {
        record.pane_id == facts.pane_id && record.generation == facts.generation
    })
}

fn clear_starting_actor_timeout_record(file_path: &str) {
    let Some((state_path, _)) = starting_actor_timeout_paths(file_path) else {
        return;
    };
    let _ = std::fs::remove_file(state_path);
}

fn mark_starting_actor_timeout_blocked(
    file: &Path,
    file_path: &str,
    session_id: &str,
    facts: &AuthoritativeActorReadyFacts,
) {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    match crate::project_controller::mark_lifecycle(
        &base_dir,
        crate::project_controller::LifecycleRequest {
            file: file.to_path_buf(),
            session_id: session_id.to_string(),
            pane_id: facts.pane_id.clone(),
            generation: facts.generation,
            state: crate::session_actor::ActorState::Blocked,
            caller: "route".to_string(),
            reason: "starting_actor_timeout".to_string(),
        },
    ) {
        Ok(updated) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_starting_marked_blocked file={} session={} pane={} generation={} blocker=starting_actor_timeout",
                    file.display(),
                    updated.session_id,
                    updated.pane_id,
                    updated.generation
                ),
            );
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to mark timed-out starting actor {} generation {} for {} as blocked: {}",
                facts.pane_id,
                facts.generation,
                file.display(),
                err
            );
        }
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
                let queued = enqueue_route_dispatch_prompt(
                    file,
                    &context.prompt_text,
                    "open_closeout_blocked",
                )?;
                eprintln!(
                    "[route] active closeout for {} could not be drained before reroute; queued pending dispatch {:?} in agent:queue auto (appended={}, already_present={}, superseded={})",
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
    if dispatch_only
        && actor_state == crate::session_actor::ActorState::Busy
        && let Some(refreshed) =
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
    // the prompt into `agent:queue auto`. A Busy projection without a proven
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
                    "authoritative actor generation {} for {} owns pane {} but dispatch-only route will not inject a new trigger because {} did not return to a dispatch-ready prompt in the current generation after waiting {}s. {}",
                    actor.record.generation,
                    file.display(),
                    dispatch_pane,
                    reason,
                    dispatch_only_busy_refusal_wait_secs(dispatch_only_starting_pane_recovery_timeout(Some(harness))),
                    authoritative_actor_dispatch_recovery_hint(actor_state, file)
                );
            }
            eprintln!(
                "[route] authoritative actor for {} remains in state {} on pane {} — focusing without injecting a duplicate reopen",
                file.display(),
                actor_state.as_str(),
                dispatch_pane
            );
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
                let queued = enqueue_route_dispatch_prompt(file, &context.prompt_text, reason)?;
                eprintln!(
                    "[route] authoritative actor generation {} for {} is busy on pane {}; queued pending dispatch {:?} in agent:queue auto (appended={}, already_present={}, superseded={}) instead of injecting a duplicate trigger",
                    actor.record.generation,
                    file.display(),
                    dispatch_pane,
                    queued.prompt_text,
                    queued.appended,
                    queued.already_present,
                    queued.superseded
                );
                Ok(dispatch_pane)
            } else {
                anyhow::bail!(
                    "authoritative actor generation {} for {} owns pane {} but dispatch-only route will not inject a new trigger because {} did not return to a dispatch-ready prompt in the current generation after waiting {}s. {}",
                    actor.record.generation,
                    file.display(),
                    dispatch_pane,
                    reason,
                    dispatch_only_busy_refusal_wait_secs(dispatch_only_starting_pane_recovery_timeout(Some(harness))),
                    authoritative_actor_dispatch_recovery_hint(actor_state, file)
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
                dispatch_only_busy_refusal_wait_secs(dispatch_only_starting_pane_recovery_timeout(Some(harness))),
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
                    queue_prompt_text: prompt_context.map(|context| context.prompt_text.as_str()),
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

#[allow(clippy::too_many_arguments)]
fn recover_dispatch_only_authoritative_waiting_input(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    target_session: &str,
    split_before: bool,
    harness: &HarnessConfig,
    pane: &str,
    generation: u64,
) -> Result<String> {
    crate::ops_log::log_op(
        file,
        &format!(
            "route_dispatch_only_waiting_input_restart file={} pane={} harness={} generation={}",
            file.display(),
            pane,
            harness.binary,
            generation
        ),
    );
    eprintln!(
        "[route] authoritative actor generation {} for {} is waiting for supervisor restart input on pane {} — restarting fresh once before the dispatch-only reroute",
        generation,
        file.display(),
        pane
    );
    let initial_status = crate::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten();

    if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
        anyhow::bail!(
            "authoritative actor generation {} for {} owns pane {} but route could not restart the waiting supervisor fresh. Run `agent-doc start {}` manually to recover",
            generation,
            file.display(),
            pane,
            file.display()
        );
    }

    let dispatch_pane = match wait_for_starting_pane_recovery_target(
        tmux,
        file,
        session_id,
        pane,
        file_path,
        harness,
        initial_status.as_ref(),
    ) {
        Some(StartingPaneRecoveryTarget::DifferentPane(recovered)) => recovered,
        Some(StartingPaneRecoveryTarget::SamePane) | None => {
            resolve_fresh_dispatch_target_after_ready_wait(tmux, session_id, pane, file_path, None)?
        }
    };

    rescue_from_stash(
        tmux,
        &dispatch_pane,
        session_id,
        file_path,
        target_session,
        split_before,
    );
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    dispatch_only_send_reopen(
        tmux,
        file,
        session_id,
        &dispatch_pane,
        file_path,
        harness,
        DispatchOnlySendReopenOptions {
            delivery: DispatchOnlyReopenDelivery::DirectPaneSubmit,
            queue_prompt_text: None,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_or_create_pane_dispatch_only(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
) -> Result<String> {
    let registered = lookup_dispatch_registration(file_path, session_id)?;
    let cycle_baseline = crate::cycle_state::load(file)?;
    let pending_prompt_context =
        pending_prompt_bearing_context_for_route(file, cycle_baseline.as_ref())?;
    let authoritative_actor =
        load_authoritative_actor_binding(tmux, file, session_id, file_path, harness, false, false)?;
    let registered_actor = if authoritative_actor.is_none() {
        registered.as_deref().map_or(Ok(None), |pane| {
            load_authoritative_actor_for_registered_pane(tmux, file, session_id, file_path, pane)
        })?
    } else {
        None
    };
    if let Some(actor) = authoritative_actor
        .as_ref()
        .filter(|actor| authoritative_actor_dispatch_target_eligible(actor))
    {
        return route_via_authoritative_actor(
            tmux,
            file,
            session_id,
            file_path,
            target_session,
            is_first_column(file, col_args),
            harness,
            cycle_baseline.as_ref(),
            pending_prompt_context.as_ref(),
            true,
            actor.clone(),
        );
    }
    let live_owner = if registered.is_some() {
        crate::sync::find_normal_path_owner_pane(tmux, file, session_id)
    } else {
        None
    };
    let preferred_active_window = tmux.active_window(target_session);
    let associated_candidates = crate::sync::find_associated_panes(tmux, file, session_id);
    let associated_resolution = crate::sync::resolve_associated_panes(
        associated_candidates.clone(),
        preferred_active_window.as_deref(),
    );

    let rescue_target = |pane_id: &str| {
        rescue_from_stash(
            tmux,
            pane_id,
            session_id,
            file_path,
            target_session,
            is_first_column(file, col_args),
        );
    };

    let degraded_authoritative_actor = authoritative_actor.as_ref().or(registered_actor.as_ref());
    if let Some(actor) = degraded_authoritative_actor
        && let Some(reason) = authoritative_actor_dispatch_guard_reason(&actor.runtime)
    {
        if dispatch_only_can_use_degraded_authoritative_actor(
            actor,
            registered.as_deref(),
            live_owner.as_deref(),
        ) {
            let dispatch_pane = actor.record.pane_id.clone();
            let file_display = file.display().to_string();
            let supervisor_health = supervisor_health_label(actor.runtime.health);
            crate::ops_log::log_op(
                file,
                &degraded_authoritative_actor_direct_submit_log_message(
                    DegradedAuthoritativeActorDirectSubmit {
                        file_display: file_display.as_str(),
                        pane_id: dispatch_pane.as_str(),
                        harness_binary: harness.binary.as_str(),
                        generation: actor.record.generation,
                        record_state: actor.record.state.as_str(),
                        supervisor_health: supervisor_health.as_str(),
                        runtime_actor_state: runtime_actor_state_label(&actor.runtime),
                        reason: reason.as_str(),
                    },
                ),
            );
            let _authorization = authorize_controller_dispatch(
                file,
                session_id,
                file_path,
                actor,
                "dispatch_only_reopen",
                &format!(
                    "submit=direct_pane actor_state={} harness={} degraded_supervisor={}",
                    actor.actor_state().as_str(),
                    harness.binary,
                    reason.replace(' ', "_")
                ),
            )?;
            rescue_target(dispatch_pane.as_str());
            return dispatch_only_reopen_existing_pane(
                tmux,
                file,
                pane,
                col_args,
                session_id,
                file_path,
                target_session,
                harness,
                created_panes,
                pending_prompt_context
                    .as_ref()
                    .map(|context| context.marker.as_str()),
                pending_prompt_context
                    .as_ref()
                    .map(|context| context.prompt_text.as_str()),
                true,
                true,
                false,
                dispatch_pane.as_str(),
                DispatchOnlyReopenDelivery::DirectPaneSubmit,
                true,
            );
        }

        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_authoritative_fallback_skipped file={} actor_pane={} harness={} generation={} record_state={} supervisor_health={} runtime_actor_state={} registered_pane={} live_owner={} reason={}",
                file.display(),
                actor.record.pane_id,
                harness.binary,
                actor.record.generation,
                actor.record.state.as_str(),
                supervisor_health_label(actor.runtime.health),
                runtime_actor_state_label(&actor.runtime),
                registered.as_deref().unwrap_or("none"),
                live_owner.as_deref().unwrap_or("none"),
                reason
            ),
        );
    }

    if let Some(ref registered_pane) = registered
        && tmux.pane_alive(registered_pane)
    {
        if let crate::sync::AssociatedPaneResolution::Ambiguous(candidates) = &associated_resolution
        {
            let error = format_associated_pane_resolution_error(
                file,
                candidates,
                preferred_active_window.as_deref(),
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_only_associated_pane_ambiguous file={} count={}",
                    file_path,
                    candidates.len()
                ),
            );
            anyhow::bail!(error);
        }
        if let crate::sync::AssociatedPaneResolution::Selected { winner, redundant } =
            &associated_resolution
            && winner.pane_id != *registered_pane
        {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_only_associated_pane_requires_manual_claim file={} pane={} sources={}",
                    file_path,
                    winner.pane_id,
                    winner.source_summary()
                ),
            );
            anyhow::bail!(format_associated_pane_selected_error(
                file, winner, redundant
            ));
        }
        let dispatch_pane = live_owner.as_deref().unwrap_or(registered_pane.as_str());
        rescue_target(dispatch_pane);
        return dispatch_only_reopen_existing_pane(
            tmux,
            file,
            pane,
            col_args,
            session_id,
            file_path,
            target_session,
            harness,
            created_panes,
            pending_prompt_context
                .as_ref()
                .map(|context| context.marker.as_str()),
            pending_prompt_context
                .as_ref()
                .map(|context| context.prompt_text.as_str()),
            true,
            true,
            false,
            dispatch_pane,
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            false,
        );
    }

    if let crate::sync::AssociatedPaneResolution::Ambiguous(candidates) = &associated_resolution {
        let error = format_associated_pane_resolution_error(
            file,
            candidates,
            preferred_active_window.as_deref(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_associated_pane_ambiguous file={} count={}",
                file_path,
                candidates.len()
            ),
        );
        anyhow::bail!(error);
    }

    if let crate::sync::AssociatedPaneResolution::Selected { winner, redundant } =
        &associated_resolution
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_associated_pane_requires_manual_claim file={} pane={} sources={}",
                file_path,
                winner.pane_id,
                winner.source_summary()
            ),
        );
        anyhow::bail!(format_associated_pane_selected_error(
            file, winner, redundant
        ));
    }

    let claimed_panes: std::collections::HashSet<String> = load_dispatch_registry(file_path)
        .unwrap_or_default()
        .values()
        .filter(|entry| tmux.pane_alive(&entry.pane))
        .map(|entry| entry.pane.clone())
        .collect();
    if registered.is_some()
        && let Some(new_pane) = find_target_pane(tmux, pane, target_session, &claimed_panes)
        && is_agent_process(tmux, &new_pane, harness)
    {
        rescue_target(&new_pane);
        return dispatch_only_reopen_existing_pane(
            tmux,
            file,
            pane,
            col_args,
            session_id,
            file_path,
            target_session,
            harness,
            created_panes,
            pending_prompt_context
                .as_ref()
                .map(|context| context.marker.as_str()),
            pending_prompt_context
                .as_ref()
                .map(|context| context.prompt_text.as_str()),
            true,
            true,
            false,
            &new_pane,
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            false,
        );
    }

    eprintln!("[route] No active pane found, auto-starting...");
    if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
        anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
    }
    fail_if_recent_session_loss_window(file, session_id)?;
    let split_before = is_first_column(file, col_args);
    ensure_auto_start_target_session(tmux, None, target_session, harness)?;
    auto_start_in_session(
        tmux,
        file,
        session_id,
        file_path,
        target_session,
        false,
        split_before,
        harness,
        None,
        Some(created_panes),
        true,
    )
}

/// Resolve an existing pane or create a new one. Returns the pane ID.
///
/// Three resolution strategies, tried in order:
/// 1. Alive registered pane → unconditionally send command. Pane IDs are
///    globally unique per tmux server, so session matching is not required.
/// 2. Lazy claim to an active pane (when registered pane is dead)
/// 3. Auto-start a new agent session
#[allow(clippy::too_many_arguments)]
fn resolve_or_create_pane(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
) -> Result<String> {
    resolve_or_create_pane_with_auto_fix_retry(
        tmux,
        file,
        pane,
        col_args,
        session_id,
        file_path,
        target_session,
        harness,
        created_panes,
        true,
        true,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_or_create_pane_with_auto_fix_retry(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
    allow_auto_fix_retry: bool,
    allow_busy_interrupt_retry: bool,
    auto_fix_attempted: bool,
) -> Result<String> {
    tracing::debug!(
        session_id = &session_id[..8.min(session_id.len())],
        file = file_path,
        target_session,
        "route::resolve_or_create_pane"
    );
    let registered = lookup_dispatch_registration(file_path, session_id)?;
    let cycle_baseline = crate::cycle_state::load(file)?;
    let pending_prompt_context =
        pending_prompt_bearing_context_for_route(file, cycle_baseline.as_ref())?;
    if let Some(actor) = load_authoritative_actor_dispatch_target(
        tmux, file, session_id, file_path, harness, true, true,
    )? {
        return route_via_authoritative_actor(
            tmux,
            file,
            session_id,
            file_path,
            target_session,
            is_first_column(file, col_args),
            harness,
            cycle_baseline.as_ref(),
            pending_prompt_context.as_ref(),
            false,
            actor,
        );
    }
    let live_owner = if registered.is_some() {
        crate::sync::find_normal_path_owner_pane(tmux, file, session_id)
    } else {
        None
    };
    let supervisor_health = if registered.is_some() {
        query_supervisor_health(file, session_id)
    } else {
        SupervisorHealth::NoSocket
    };
    let preferred_active_window = tmux.active_window(target_session);
    let associated_candidates = crate::sync::find_associated_panes(tmux, file, session_id);
    let associated_resolution = crate::sync::resolve_associated_panes(
        associated_candidates.clone(),
        preferred_active_window.as_deref(),
    );

    if let Ok(Some(miss)) = crate::startup_miss::load(file)
        && let Some(supersession) =
            crate::startup_miss::superseded_by_newer_registered_start(file, &miss)?
    {
        let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
        eprintln!(
            "[route] startup-miss on pane {} from {} for {} is superseded by newer registered owner {} — clearing stale marker",
            miss.pane_id, miss_ts, file_path, supersession.registered_pane
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_startup_miss_cleared_superseded_owner file={} stale_pane={} registered_pane={} miss_timestamp={} latest_start_timestamp={}",
                file_path,
                miss.pane_id,
                supersession.registered_pane,
                miss_ts,
                supersession.latest_start_timestamp
            ),
        );
        let _ = crate::startup_miss::clear(file);
    }

    // Strategy 0: If a previous startup-miss was recorded for the registered pane,
    // deregister it immediately so we fall through to auto-start instead of
    // reusing a pane that never successfully started a document cycle.
    if let Some(ref registered_pane) = registered
        && let Ok(Some(miss)) = crate::startup_miss::load(file)
        && miss.pane_id == *registered_pane
        && tmux.pane_alive(registered_pane)
    {
        let log_status = crate::startup_miss::session_log_status(file, &miss.session_id)
            .ok()
            .flatten();
        let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
        let provenance = startup_miss_route_provenance(
            tmux,
            registered_pane,
            live_owner.as_deref(),
            supervisor_health,
            log_status.as_ref(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_startup_miss_detected file={} origin={:?} miss_timestamp={} {}",
                file_path, miss.origin, miss_ts, provenance
            ),
        );
        if startup_miss_should_fail_closed(
            true,
            registered_pane,
            live_owner.as_deref(),
            supervisor_health,
            log_status.as_ref(),
        ) {
            eprintln!(
                "[route] startup-miss for {} is stranded, not crashed: {}",
                file_path, provenance
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_startup_miss_stranded file={} origin={:?} {}",
                    file_path, miss.origin, provenance
                ),
            );
            anyhow::bail!(
                "startup-miss for {} remains unresolved on alive pane {}: {}. The last session never recorded a child exit or session_end, so route will not auto-start a replacement pane over a stranded session",
                file.display(),
                registered_pane,
                provenance
            );
        }
        if startup_miss_requires_fresh_start(
            registered_pane,
            live_owner.as_deref(),
            supervisor_health,
        ) || startup_miss_should_restart_live_owner(
            &miss,
            registered_pane,
            live_owner.as_deref(),
            log_status.as_ref(),
        ) {
            eprintln!(
                "[route] registered pane {} has an unresolved startup-miss marker from {} for {} — deregistering and starting fresh",
                registered_pane, miss_ts, file_path
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_startup_miss_deregistered file={} pane={} miss_timestamp={}",
                    file_path, registered_pane, miss_ts
                ),
            );
            let _ = deregister_dispatch_registration(file_path, session_id)?;
            let _ = crate::startup_miss::clear(file);
            // Fall through to Strategy 3 (auto-start)
            eprintln!("[route] No active pane found, auto-starting...");
            if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
                anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
            }
            fail_if_recent_session_loss_window(file, session_id)?;
            let split_before = is_first_column(file, col_args);
            ensure_auto_start_target_session(tmux, None, target_session, harness)?;
            return auto_start_in_session(
                tmux,
                file,
                session_id,
                file_path,
                target_session,
                false,
                split_before,
                harness,
                Some(registered_pane.as_str()),
                Some(created_panes),
                false,
            );
        }

        if startup_miss_superseded_by_later_open_start(&miss, registered_pane, log_status.as_ref())
        {
            eprintln!(
                "[route] registered pane {} proves a newer open harness run after startup-miss {} for {} — clearing stale marker",
                registered_pane, miss_ts, file_path
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_startup_miss_cleared_live_owner file={} pane={} miss_timestamp={}",
                    file_path, registered_pane, miss_ts
                ),
            );
            let _ = crate::startup_miss::clear(file);
        } else {
            eprintln!(
                "[route] registered pane {} still owns {} but startup-miss {} is not superseded by a newer open harness run — keeping marker until dispatch proves recovery",
                registered_pane, file_path, miss_ts
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_startup_miss_retained_live_owner file={} pane={} miss_timestamp={}",
                    file_path, registered_pane, miss_ts
                ),
            );
        }
    }

    // Strategy 1: Alive registered pane — reuse only when the authoritative
    // actor projection or the registered supervisor path still proves the
    // document is running there. Pane IDs (%N) are globally unique per tmux
    // server, so target_session matching stays irrelevant once ownership is
    // proven.
    //
    // rescue_from_stash self-gates on target_session match, so it is a no-op
    // when the pane is in a different session — we leave it in place.
    if let Some(ref registered_pane) = registered {
        if tmux.pane_alive(registered_pane) {
            if let crate::sync::AssociatedPaneResolution::Ambiguous(candidates) =
                &associated_resolution
            {
                let error = format_associated_pane_resolution_error(
                    file,
                    candidates,
                    preferred_active_window.as_deref(),
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_associated_pane_ambiguous file={} count={}",
                        file_path,
                        candidates.len()
                    ),
                );
                anyhow::bail!(error);
            }
            if let crate::sync::AssociatedPaneResolution::Selected { winner, redundant } =
                &associated_resolution
                && winner.pane_id != *registered_pane
            {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_associated_pane_requires_manual_claim file={} pane={} sources={}",
                        file_path,
                        winner.pane_id,
                        winner.source_summary()
                    ),
                );
                anyhow::bail!(format_associated_pane_selected_error(
                    file, winner, redundant
                ));
            }
            let mut stale_registration_cleared = false;
            match live_owner.as_deref() {
                Some(_) => {}
                None => match supervisor_health {
                    SupervisorHealth::Healthy => {
                        eprintln!(
                            "[route] registered pane {} has a healthy supervisor for {} despite missing actor/registered-owner proof — reusing registered pane",
                            registered_pane, file_path
                        );
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_registered_pane_reused_via_supervisor file={} pane={} health=healthy",
                                file_path, registered_pane
                            ),
                        );
                    }
                    SupervisorHealth::Restartable => {
                        eprintln!(
                            "[route] registered pane {} has a restartable supervisor for {} — restarting in place",
                            registered_pane, file_path
                        );
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_registered_pane_restart_via_supervisor file={} pane={}",
                                file_path, registered_pane
                            ),
                        );
                        if restart_via_supervisor(file, session_id) {
                            if let Err(e) = tmux.select_pane(registered_pane) {
                                eprintln!(
                                    "[route] warning: failed to focus restarted pane {}: {}",
                                    registered_pane, e
                                );
                            }
                            require_routed_cycle_ack(
                                tmux,
                                file,
                                registered_pane,
                                session_id,
                                file_path,
                                harness,
                                cycle_baseline.as_ref(),
                                pending_prompt_context
                                    .as_ref()
                                    .map(|context| context.marker.as_str()),
                                false,
                                RoutedDispatchStartProof::CommandAcceptedOnly,
                            )?;
                            return Ok(registered_pane.clone());
                        }
                        eprintln!(
                            "[route] supervisor restart failed for pane {} — deregistering and continuing recovery",
                            registered_pane
                        );
                        let provenance = pane_route_provenance(tmux, registered_pane);
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_registered_pane_restart_failed file={} {}",
                                file_path, provenance
                            ),
                        );
                        let _ = deregister_dispatch_registration(file_path, session_id)?;
                        stale_registration_cleared = true;
                    }
                    SupervisorHealth::Halted { restart_count } => {
                        let provenance = pane_route_provenance(tmux, registered_pane);
                        eprintln!(
                            "[route] registered pane {} for {} has a halted supervisor after {} restarts — refusing automatic restart",
                            registered_pane, file_path, restart_count
                        );
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_registered_pane_halted file={} pane={} restart_count={} {}",
                                file_path, registered_pane, restart_count, provenance
                            ),
                        );
                        anyhow::bail!(
                            "registered pane {} for {} has a halted supervisor after {} restarts; route will not auto-restart or replace it automatically. Inspect the pane, then run `agent-doc start {}` manually to recover",
                            registered_pane,
                            file.display(),
                            restart_count,
                            file.display()
                        );
                    }
                    SupervisorHealth::Unreachable | SupervisorHealth::NoSocket => {
                        let provenance = pane_route_provenance(tmux, registered_pane);
                        eprintln!(
                            "[route] registered pane {} is alive but no actor/registered owner for {} was proven and supervisor is unavailable — deregistering stale entry and continuing recovery",
                            registered_pane, file_path
                        );
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_registered_pane_deregistered_no_live_owner file={} {}",
                                file_path, provenance
                            ),
                        );
                        let _ = deregister_dispatch_registration(file_path, session_id)?;
                        stale_registration_cleared = true;
                    }
                },
            }
            if !stale_registration_cleared {
                rescue_from_stash(
                    tmux,
                    registered_pane,
                    session_id,
                    file_path,
                    target_session,
                    is_first_column(file, col_args),
                );
                let registered_pane = reapply_codex_launch_contract_before_reuse(
                    tmux,
                    file,
                    registered_pane,
                    session_id,
                    file_path,
                    harness,
                    true,
                    true,
                )?;
                register_dispatch_target(tmux, session_id, &registered_pane, file_path)?;
                let supervisor_recovered_without_path_owner =
                    live_owner.is_none() && matches!(supervisor_health, SupervisorHealth::Healthy);
                match ensure_existing_pane_ready_for_dispatch(
                    tmux,
                    file,
                    &registered_pane,
                    harness,
                    pending_prompt_context
                        .as_ref()
                        .map(|context| context.marker.as_str()),
                )? {
                    ExistingPaneDispatchReadiness::Ready => {}
                    ExistingPaneDispatchReadiness::BusyAlreadyRunning
                        if supervisor_recovered_without_path_owner =>
                    {
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_registered_pane_dispatch_via_healthy_supervisor file={} pane={} reason=missing_path_owner_prompt_probe_not_authoritative",
                                file_path, registered_pane
                            ),
                        );
                    }
                    ExistingPaneDispatchReadiness::BusyAlreadyRunning => {
                        return Ok(registered_pane);
                    }
                    ExistingPaneDispatchReadiness::BusyNeedsAutoFix {
                        provenance,
                        blocker_reason,
                    } => {
                        return retry_route_after_busy_pane_auto_fix(
                            tmux,
                            file,
                            pane,
                            col_args,
                            session_id,
                            file_path,
                            target_session,
                            harness,
                            created_panes,
                            cycle_baseline.as_ref(),
                            pending_prompt_context
                                .as_ref()
                                .map(|context| context.marker.as_str()),
                            allow_auto_fix_retry,
                            allow_busy_interrupt_retry,
                            auto_fix_attempted,
                            &registered_pane,
                            &provenance,
                            blocker_reason.as_deref(),
                        );
                    }
                }
                register_dispatch_target(tmux, session_id, &registered_pane, file_path)?;
                eprintln!("[route] Pane {} is alive, sending command", registered_pane);
                let dispatch_start = dispatch_existing_managed_reopen(
                    tmux,
                    file,
                    session_id,
                    &registered_pane,
                    file_path,
                    harness,
                )?;
                require_routed_cycle_ack(
                    tmux,
                    file,
                    &registered_pane,
                    session_id,
                    file_path,
                    harness,
                    cycle_baseline.as_ref(),
                    pending_prompt_context
                        .as_ref()
                        .map(|context| context.marker.as_str()),
                    true,
                    dispatch_start,
                )?;
                return Ok(registered_pane);
            }
        }
        eprintln!("[route] Pane {} is dead", registered_pane);
    } else {
        eprintln!(
            "[route] No pane registered for session {}",
            &session_id[..std::cmp::min(8, session_id.len())]
        );
    }

    // Strategy 2: Lazy claim (only when a registered pane died)
    // Skip panes running non-agent processes to avoid claiming corky/shells.
    // Also skip panes already claimed by another document (pane theft prevention).
    let claimed_panes: std::collections::HashSet<String> = load_dispatch_registry(file_path)
        .unwrap_or_default()
        .values()
        .filter(|e| tmux.pane_alive(&e.pane))
        .map(|e| e.pane.clone())
        .collect();
    if let crate::sync::AssociatedPaneResolution::Ambiguous(candidates) = &associated_resolution {
        let error = format_associated_pane_resolution_error(
            file,
            candidates,
            preferred_active_window.as_deref(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_associated_pane_ambiguous file={} count={}",
                file_path,
                candidates.len()
            ),
        );
        anyhow::bail!(error);
    }
    if let crate::sync::AssociatedPaneResolution::Selected { winner, redundant } =
        &associated_resolution
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_associated_pane_requires_manual_claim file={} pane={} sources={}",
                file_path,
                winner.pane_id,
                winner.source_summary()
            ),
        );
        anyhow::bail!(format_associated_pane_selected_error(
            file, winner, redundant
        ));
    }
    if registered.is_some()
        && let Some(new_pane) = find_target_pane(tmux, pane, target_session, &claimed_panes)
        && is_agent_process(tmux, &new_pane, harness)
    {
        eprintln!("[route] Lazy-claiming to pane {} (dead pane)", new_pane);
        register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
        match ensure_existing_pane_ready_for_dispatch(
            tmux,
            file,
            &new_pane,
            harness,
            pending_prompt_context
                .as_ref()
                .map(|context| context.marker.as_str()),
        )? {
            ExistingPaneDispatchReadiness::Ready => {}
            ExistingPaneDispatchReadiness::BusyAlreadyRunning => return Ok(new_pane),
            ExistingPaneDispatchReadiness::BusyNeedsAutoFix {
                provenance,
                blocker_reason,
            } => {
                return retry_route_after_busy_pane_auto_fix(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    cycle_baseline.as_ref(),
                    pending_prompt_context
                        .as_ref()
                        .map(|context| context.marker.as_str()),
                    allow_auto_fix_retry,
                    allow_busy_interrupt_retry,
                    auto_fix_attempted,
                    &new_pane,
                    &provenance,
                    blocker_reason.as_deref(),
                );
            }
        }
        register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
        let dispatch_start = dispatch_existing_managed_reopen(
            tmux, file, session_id, &new_pane, file_path, harness,
        )?;
        let ack_pane = require_routed_cycle_ack(
            tmux,
            file,
            &new_pane,
            session_id,
            file_path,
            harness,
            cycle_baseline.as_ref(),
            pending_prompt_context
                .as_ref()
                .map(|context| context.marker.as_str()),
            false,
            dispatch_start,
        )?;
        return Ok(ack_pane.unwrap_or(new_pane));
    }

    // Strategy 3: Auto-start
    // Re-check associated panes after the earlier recovery branches. A stale
    // registered pane can be deregistered while a live legacy owner becomes
    // provable a little later in the turn; the normal path must still fail
    // closed instead of silently re-electing that pane via auto-start.
    let late_associated_resolution = crate::sync::resolve_associated_panes(
        crate::sync::find_associated_panes(tmux, file, session_id),
        tmux.active_window(target_session).as_deref(),
    );
    if let crate::sync::AssociatedPaneResolution::Ambiguous(candidates) =
        &late_associated_resolution
    {
        let error = format_associated_pane_resolution_error(
            file,
            candidates,
            tmux.active_window(target_session).as_deref(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_associated_pane_ambiguous_late file={} count={}",
                file_path,
                candidates.len()
            ),
        );
        anyhow::bail!(error);
    }
    if let crate::sync::AssociatedPaneResolution::Selected { winner, redundant } =
        &late_associated_resolution
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_associated_pane_requires_manual_claim_late file={} pane={} sources={}",
                file_path,
                winner.pane_id,
                winner.source_summary()
            ),
        );
        anyhow::bail!(format_associated_pane_selected_error(
            file, winner, redundant
        ));
    }

    eprintln!("[route] No active pane found, auto-starting...");
    if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
        anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
    }
    fail_if_recent_session_loss_window(file, session_id)?;
    let split_before = is_first_column(file, col_args);
    ensure_auto_start_target_session(tmux, None, target_session, harness)?;
    auto_start_in_session(
        tmux,
        file,
        session_id,
        file_path,
        target_session,
        false,
        split_before,
        harness,
        None,
        Some(created_panes),
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn retry_route_after_busy_pane_auto_fix(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
    cycle_baseline: Option<&crate::cycle_state::CycleState>,
    prompt_bearing_marker: Option<&str>,
    allow_auto_fix_retry: bool,
    allow_busy_interrupt_retry: bool,
    auto_fix_attempted: bool,
    busy_pane: &str,
    provenance: &str,
    blocker_reason: Option<&str>,
) -> Result<String> {
    let fallback_detail = blocker_reason.map(|reason| format!("still shows {reason}"));
    if allow_auto_fix_retry {
        match attempt_busy_existing_pane_auto_fix(tmux, file, session_id, busy_pane, file_path)? {
            BusyPaneAutoFixOutcome::RetryRoute => {
                return resolve_or_create_pane_with_auto_fix_retry(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                );
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart => {
                wait_for_busy_restart_handoff(tmux, file, file_path, session_id, busy_pane);
                return resolve_or_create_pane_with_auto_fix_retry(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                );
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_existing_pane_retry_route_after_fresh_restart file={} pane={} harness={}",
                        file.display(),
                        busy_pane,
                        harness.binary
                    ),
                );
                eprintln!(
                    "[route] scoped fix left pane {} authoritative for {} with a healthy supervisor — restarting the live {} session fresh once before one final reroute",
                    busy_pane,
                    file.display(),
                    harness.binary
                );
                if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
                    emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
                    anyhow::bail!(format_busy_existing_pane_error(
                        file,
                        busy_pane,
                        harness,
                        provenance,
                        fallback_detail.as_deref(),
                        true
                    ));
                }
                wait_for_busy_restart_handoff(tmux, file, file_path, session_id, busy_pane);
                return resolve_or_create_pane_with_auto_fix_retry(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                );
            }
            BusyPaneAutoFixOutcome::FailClosed => {}
        }
    }
    if allow_busy_interrupt_retry {
        match attempt_busy_existing_pane_interrupt_recovery(
            tmux,
            file,
            busy_pane,
            harness,
            blocker_reason,
        )? {
            BusyPaneInterruptRecoveryOutcome::Recovered => {
                return resolve_or_create_pane_with_auto_fix_retry(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    false,
                    false,
                    true,
                );
            }
            BusyPaneInterruptRecoveryOutcome::Blocked { reason } => {
                emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
                let detail = format!("bounded interrupt recovery still shows {reason}");
                if harness.binary == "codex" && tmux.pane_alive(busy_pane) {
                    return optimistic_busy_pane_dispatch(
                        tmux,
                        file,
                        session_id,
                        busy_pane,
                        file_path,
                        harness,
                        cycle_baseline,
                        prompt_bearing_marker,
                        detail.as_str(),
                    );
                }
                anyhow::bail!(format_busy_existing_pane_error(
                    file,
                    busy_pane,
                    harness,
                    provenance,
                    Some(detail.as_str()),
                    auto_fix_attempted || allow_auto_fix_retry
                ));
            }
            BusyPaneInterruptRecoveryOutcome::TimedOut => {
                emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
                if harness.binary == "codex" && tmux.pane_alive(busy_pane) {
                    return optimistic_busy_pane_dispatch(
                        tmux,
                        file,
                        session_id,
                        busy_pane,
                        file_path,
                        harness,
                        cycle_baseline,
                        prompt_bearing_marker,
                        "bounded interrupt recovery never restored a dispatch-ready prompt",
                    );
                }
                anyhow::bail!(format_busy_existing_pane_error(
                    file,
                    busy_pane,
                    harness,
                    provenance,
                    Some("bounded interrupt recovery never restored a dispatch-ready prompt"),
                    auto_fix_attempted || allow_auto_fix_retry
                ));
            }
            BusyPaneInterruptRecoveryOutcome::Skipped => {}
        }
    }
    if harness.binary == "codex" && tmux.pane_alive(busy_pane) {
        emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
        return optimistic_busy_pane_dispatch(
            tmux,
            file,
            session_id,
            busy_pane,
            file_path,
            harness,
            cycle_baseline,
            prompt_bearing_marker,
            fallback_detail
                .as_deref()
                .unwrap_or("still not showing an idle prompt"),
        );
    }
    anyhow::bail!(format_busy_existing_pane_error(
        file,
        busy_pane,
        harness,
        provenance,
        fallback_detail.as_deref(),
        auto_fix_attempted || allow_auto_fix_retry
    ));
}

#[allow(clippy::too_many_arguments)]
fn optimistic_busy_pane_dispatch(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    cycle_baseline: Option<&crate::cycle_state::CycleState>,
    prompt_bearing_marker: Option<&str>,
    detail: &str,
) -> Result<String> {
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_optimistic_dispatch file={} pane={} harness={} detail={}",
            file.display(),
            pane,
            harness.binary,
            detail
        ),
    );
    eprintln!(
        "[route] pane {} for {} is still busy ({}) but remains authoritative — sending the bare {} reopen anyway",
        pane,
        file.display(),
        detail,
        harness.binary
    );
    register_dispatch_target(tmux, session_id, pane, file_path)?;
    let dispatch_start =
        dispatch_existing_managed_reopen(tmux, file, session_id, pane, file_path, harness)?;
    let ack_pane = require_routed_cycle_ack(
        tmux,
        file,
        pane,
        session_id,
        file_path,
        harness,
        cycle_baseline,
        prompt_bearing_marker,
        true,
        dispatch_start,
    )?;
    Ok(ack_pane.unwrap_or_else(|| pane.to_string()))
}

fn wait_for_busy_restart_handoff(
    tmux: &Tmux,
    file: &Path,
    file_path: &str,
    session_id: &str,
    previous_pane: &str,
) {
    let registry_base_dir = registry_base_dir_for_dispatch(file_path);
    let timeout = if cfg!(test) {
        Duration::from_secs(20)
    } else {
        Duration::from_secs(5)
    };
    let poll = Duration::from_millis(100);
    let start = std::time::Instant::now();
    let mut handed_off_pane: Option<String> = None;
    while start.elapsed() < timeout {
        if let Ok(registry) = sessions::load_in(&registry_base_dir)
            && let Some(entry) = registry
                .values()
                .find(|entry| entry.session_id == session_id)
            && !entry.pane.is_empty()
        {
            if entry.pane != previous_pane {
                handed_off_pane = Some(entry.pane.clone());
                if crate::sync::find_normal_path_owner_pane(tmux, file, session_id).as_deref()
                    == Some(entry.pane.as_str())
                {
                    eprintln!(
                        "[route] supervisor restart handed {} from pane {} to authoritative pane {} before retry",
                        file_path, previous_pane, entry.pane
                    );
                    return;
                }
            } else {
                handed_off_pane = None;
            }
        }
        match crate::sync::resolve_associated_panes(
            crate::sync::find_associated_panes(tmux, file, session_id),
            None,
        ) {
            crate::sync::AssociatedPaneResolution::Selected { winner, .. }
                if winner.pane_id != previous_pane && !winner.is_stash() =>
            {
                if let Err(err) =
                    register_dispatch_target(tmux, session_id, &winner.pane_id, file_path)
                {
                    eprintln!(
                        "[route] warning: failed to project restart handoff pane {} into the registry for {}: {}",
                        winner.pane_id, file_path, err
                    );
                }
                eprintln!(
                    "[route] supervisor restart for {} has not refreshed the registry yet, but a unique associated pane {} is alive via {} — adopting it as the handoff target before retry",
                    file_path,
                    winner.pane_id,
                    winner.source_summary()
                );
                return;
            }
            _ => {}
        }
        std::thread::sleep(poll);
    }
    if let Some(pane) = handed_off_pane {
        eprintln!(
            "[route] supervisor restart handed {} from pane {} to authoritative pane {} before retry, but live-owner proof is still catching up",
            file_path, previous_pane, pane
        );
    }
}

/// Rescue a pane from a stash window back to the agent-doc window.
/// Only rescues if the pane is in the target session — never swaps across sessions.
///
/// Returns `true` when the pane was actually moved out of a stash window so callers
/// can re-evaluate state that depends on pane location (e.g. Starting→Ready
/// promotion after the rescue makes the pane visible). Returns `false` when the
/// rescue was a no-op (pane not in stash, or session guard tripped).
fn rescue_from_stash(
    tmux: &Tmux,
    pane_id: &str,
    session_id: &str,
    file_path: &str,
    target_session: &str,
    split_before: bool,
) -> bool {
    // Session guard: only rescue within the target session
    let pane_session = pane_session_name(tmux, pane_id).unwrap_or_default();
    if pane_session != target_session {
        eprintln!(
            "[route] Pane {} is in session '{}', not target '{}' — skipping stash rescue",
            pane_id, pane_session, target_session
        );
        return false;
    }

    let pane_win_name = pane_window_name(tmux, pane_id).unwrap_or_default();

    if is_stash_window_name(&pane_win_name) {
        tracing::debug!(pane_id, window = %pane_win_name, target_session, "route: rescuing pane from stash");
        eprintln!(
            "[route] Pane {} is in stash window '{}', rescuing to agent-doc window",
            pane_id, pane_win_name
        );
        let agent_doc_window = format!("{}:agent-doc", target_session);
        let target_panes = tmux
            .list_window_panes(&agent_doc_window)
            .unwrap_or_default();
        let target = if split_before {
            target_panes.first()
        } else {
            target_panes.last()
        };
        let mut moved = false;
        if let Some(target) = target {
            let join_flag = if split_before { "-dbh" } else { "-dh" };
            match sessions::join_pane_guarded(tmux, pane_id, target, target_session, join_flag) {
                Ok(()) => {
                    eprintln!("[route] Rescued pane {} via join-pane", pane_id);
                    moved = true;
                }
                Err(e) => eprintln!("[route] join-pane rescue failed for {} ({})", pane_id, e),
            }
        }
        if let Err(e) = register_dispatch_target(tmux, session_id, pane_id, file_path) {
            eprintln!("[route] warning: re-register failed: {}", e);
        }
        return moved;
    }
    false
}

fn send_command_unchecked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<CommandDispatchResult> {
    let trigger = send_command_once_unchecked(tmux, pane, file_path, harness)?;
    let start = std::time::Instant::now();
    let timeout = direct_pane_submit_acceptance_timeout();
    let poll_interval = std::time::Duration::from_millis(300);
    while start.elapsed() < timeout {
        std::thread::sleep(poll_interval);
        if let Ok(content) = sessions::capture_pane(tmux, pane) {
            let cmd_still_in_input = recent_lines_contain_trigger(&content, &trigger);

            if !cmd_still_in_input {
                return Ok(CommandDispatchResult {
                    status: CommandDispatchStatus::Accepted,
                    elapsed: start.elapsed(),
                });
            }
        }
    }
    Ok(CommandDispatchResult {
        status: CommandDispatchStatus::TimedOut,
        elapsed: start.elapsed(),
    })
}

fn send_command_once_unchecked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<String> {
    let short_name = std::path::Path::new(file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());
    let trigger = harness.trigger_command(file_path);
    let payload = routed_trigger_payload(&trigger);
    validate_routed_trigger_payload(harness, &trigger, &payload)?;
    let flash_msg = format!("⏳ {}", harness.trigger_command(&short_name));
    if let Err(e) = tmux
        .cmd()
        .args(["display-message", "-t", pane, "-d", "2000", &flash_msg])
        .status()
    {
        eprintln!("[route] warning: display-message failed: {}", e);
    }

    crate::input_diag::log_text_submit(
        Some(Path::new(file_path)),
        "route.direct_pane_submit",
        &format!("pane:{pane}"),
        &payload,
        Some(&harness.binary),
        if harness.binary == "opencode" {
            "routed_trigger_kitty_return"
        } else {
            "routed_trigger_enter"
        },
        if harness.binary == "opencode" {
            "KittyReturn"
        } else {
            "Enter"
        },
    );
    crate::sessions::send_submitted_text_for_harness(tmux, pane, &payload, &harness.binary)?;
    if let Err(e) = tmux.select_pane(pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
    }
    eprintln!("[route] Sent {} → pane {}", trigger, pane);
    Ok(trigger)
}

fn dispatch_via_supervisor_ipc_with_mode(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    options: SupervisorIpcDispatchOptions,
) -> Result<RoutedDispatchStartProof> {
    let Some(sock) = supervisor_socket_path(file, session_id) else {
        anyhow::bail!(
            "authoritative actor for {} has no supervisor socket; run `agent-doc start {}` to recover",
            file.display(),
            file.display()
        );
    };
    let short_name = std::path::Path::new(file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());
    let trigger = harness.trigger_command(file_path);
    let payload = routed_trigger_payload(&trigger);
    validate_routed_trigger_payload(harness, &trigger, &payload)?;
    let flash_msg = format!("⏳ {}", harness.trigger_command(&short_name));
    if let Err(e) = tmux
        .cmd()
        .args(["display-message", "-t", pane, "-d", "2000", &flash_msg])
        .status()
    {
        eprintln!("[route] warning: display-message failed: {}", e);
    }

    let tracker =
        build_routed_dispatch_start_tracker(file, file_path, harness, Some(tmux), Some(pane))?;
    let method = IpcMethod::Inject {
        bytes: routed_trigger_submit_payload(&payload),
    };
    crate::input_diag::log_text_submit(
        Some(file),
        "route.supervisor_ipc",
        &format!("socket:{}:pane:{pane}", sock.display()),
        &payload,
        Some(&harness.binary),
        "supervisor_ipc_inject",
        "Inject",
    );
    let submit_start = Instant::now();
    let response = crate::supervisor::ipc::send_command(&sock, &method).with_context(|| {
        format!(
            "failed to dispatch authoritative actor trigger for {} via supervisor IPC",
            file.display()
        )
    })?;
    log_route_latency(
        file,
        "supervisor_ipc_submit",
        submit_start.elapsed(),
        Duration::from_millis(500),
        pane,
        harness,
        if response.ok { "accepted" } else { "rejected" },
    );
    if !response.ok {
        let message = response
            .error
            .unwrap_or_else(|| "unknown supervisor error".to_string());
        anyhow::bail!(
            "authoritative actor for {} rejected routed trigger in pane {}: {}",
            file.display(),
            pane,
            message
        );
    }

    if let Err(e) = tmux.select_pane(pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
    }
    eprintln!(
        "[route] Dispatched {} via supervisor IPC → pane {}",
        trigger, pane
    );

    if !options.await_start_proof {
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    }

    let Some(tracker) = tracker else {
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    };

    let timeout = routed_dispatch_start_timeout(harness);
    let proof_start = Instant::now();
    if let Some(proof) = wait_for_routed_dispatch_start(tmux, file, &tracker, harness, timeout)? {
        log_route_latency(
            file,
            "dispatch_start_proof",
            proof_start.elapsed(),
            timeout,
            pane,
            harness,
            proof.dispatch_stage_label(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_actor_dispatch_start_proven file={} pane={} harness={} proof={} timeout_secs={}",
                file.display(),
                pane,
                harness.binary,
                proof.dispatch_stage_label(),
                timeout.as_secs()
            ),
        );
        return Ok(proof);
    }

    log_route_latency(
        file,
        "dispatch_start_proof",
        proof_start.elapsed(),
        timeout,
        pane,
        harness,
        "unproven_but_accepted",
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "route_actor_dispatch_start_unproven_but_accepted file={} pane={} harness={} timeout_secs={}",
            file.display(),
            pane,
            harness.binary,
            timeout.as_secs()
        ),
    );
    if options.print_unproven_progress {
        eprintln!(
            "[route] authoritative actor accepted the {} reopen for {} in pane {}, but no routed submission proof appeared after {}s",
            harness.binary,
            file.display(),
            pane,
            timeout.as_secs()
        );
    }
    Ok(RoutedDispatchStartProof::CommandAcceptedOnly)
}

#[derive(Debug, Clone, Copy)]
struct SupervisorIpcDispatchOptions {
    await_start_proof: bool,
    print_unproven_progress: bool,
}

fn dispatch_via_supervisor_ipc(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<RoutedDispatchStartProof> {
    dispatch_via_supervisor_ipc_with_mode(
        tmux,
        file,
        pane,
        session_id,
        file_path,
        harness,
        SupervisorIpcDispatchOptions {
            await_start_proof: true,
            print_unproven_progress: true,
        },
    )
}

fn authoritative_actor_dispatch_recovery_hint(
    state: crate::session_actor::ActorState,
    file: &Path,
) -> String {
    actor_recovery_hint(actor_dispatch_state(state), &file.display().to_string())
}

#[cfg(test)]
fn authoritative_actor_dispatch_can_queue_optimistically(
    state: crate::session_actor::ActorState,
) -> bool {
    crate::flow::routed_reopen::actor_can_queue_optimistically(actor_dispatch_state(state))
}

fn canonical_dispatch_file(path: &std::path::Path) -> std::path::PathBuf {
    let resolved = crate::git::resolve_absolute_file_path(path);
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

fn canonical_registered_file(entry: &sessions::SessionEntry) -> std::path::PathBuf {
    let path = std::path::Path::new(&entry.file);
    let resolved = if path.is_absolute() || entry.cwd.is_empty() {
        path.to_path_buf()
    } else {
        std::path::Path::new(&entry.cwd).join(path)
    };
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

fn registry_base_dir_for_dispatch(file_path: &str) -> std::path::PathBuf {
    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    crate::snapshot::find_project_root(&requested)
        .or_else(|| requested.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
}

fn lookup_dispatch_registration(file_path: &str, session_id: &str) -> Result<Option<String>> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    sessions::lookup_in(&base_dir, session_id)
}

fn load_dispatch_registry(file_path: &str) -> Result<sessions::SessionRegistry> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    sessions::load_in(&base_dir)
}

fn deregister_dispatch_registration(file_path: &str, session_id: &str) -> Result<bool> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    let registry_path = sessions::registry_path_in(&base_dir);
    let _lock = sessions::RegistryLock::acquire(&registry_path)?;
    let mut registry = sessions::load_in(&base_dir)?;
    let removed_key = registry.iter().find_map(|(key, entry)| {
        ((entry.session_id == session_id) || (entry.session_id.is_empty() && key == session_id))
            .then(|| key.clone())
    });
    let removed = removed_key.and_then(|key| registry.remove(&key)).is_some();
    if removed {
        sessions::save_in(&base_dir, &registry)?;
    }
    Ok(removed)
}

fn register_dispatch_target(
    tmux: &Tmux,
    session_id: &str,
    pane_id: &str,
    file_path: &str,
) -> Result<()> {
    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    let requested_str = requested.to_string_lossy().to_string();
    let base_dir = registry_base_dir_for_dispatch(&requested_str);
    ensure_dispatch_target_can_bind_file(tmux, &base_dir, pane_id, &requested_str)?;
    let window = sessions::pane_window(pane_id).unwrap_or_default();
    let cwd = base_dir.to_string_lossy().to_string();
    sessions::register_full_with_cwd_in(
        &base_dir,
        session_id,
        pane_id,
        &requested_str,
        std::process::id(),
        &window,
        &cwd,
    )
}

fn ensure_dispatch_target_can_bind_file(
    tmux: &Tmux,
    base_dir: &Path,
    pane: &str,
    file_path: &str,
) -> Result<()> {
    let registry = sessions::load_in(base_dir).with_context(|| {
        format!(
            "failed to load route registry before dispatch registration from {}",
            base_dir.display()
        )
    })?;
    if pane_registration_matches_file(&registry, pane, file_path) {
        return Ok(());
    }

    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    if let Some(entry) = registry.values().find(|entry| entry.pane == pane) {
        let registered = canonical_registered_file(entry);
        let registered_is_live_owner = !entry.session_id.is_empty()
            && crate::sync::find_normal_path_owner_pane(tmux, &registered, &entry.session_id)
                .as_deref()
                == Some(pane);
        if !registered_is_live_owner {
            return Ok(());
        }
        anyhow::bail!(
            "route dispatch target {} is registered for {}, not {}; refusing cross-file dispatch",
            pane,
            registered.display(),
            requested.display()
        );
    }

    Ok(())
}

fn pane_registration_matches_file(
    registry: &sessions::SessionRegistry,
    pane: &str,
    file_path: &str,
) -> bool {
    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    registry
        .values()
        .find(|entry| entry.pane == pane)
        .map(|entry| canonical_registered_file(entry) == requested)
        .unwrap_or(false)
}

fn ensure_dispatch_target_matches_file(pane: &str, file_path: &str) -> Result<()> {
    let registry_base_dir = registry_base_dir_for_dispatch(file_path);
    let registry = sessions::load_in(&registry_base_dir).with_context(|| {
        format!(
            "failed to load route registry before dispatch validation from {}",
            registry_base_dir.display()
        )
    })?;
    if pane_registration_matches_file(&registry, pane, file_path) {
        return Ok(());
    }

    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    if let Some(entry) = registry.values().find(|entry| entry.pane == pane) {
        anyhow::bail!(
            "route dispatch target {} is registered for {}, not {}; refusing cross-file dispatch",
            pane,
            canonical_registered_file(entry).display(),
            requested.display()
        );
    }

    anyhow::bail!(
        "route dispatch target {} is not registered for {}; refusing unbound dispatch",
        pane,
        requested.display()
    );
}

fn resolve_fresh_dispatch_target_after_ready_wait(
    tmux: &Tmux,
    session_id: &str,
    pane: &str,
    file_path: &str,
    _startup_miss_handoff_blocked_pane: Option<&str>,
) -> Result<String> {
    let registry_base_dir = registry_base_dir_for_dispatch(file_path);
    let registry = sessions::load_in(&registry_base_dir).with_context(|| {
        format!(
            "failed to load route registry before fresh-dispatch validation from {}",
            registry_base_dir.display()
        )
    })?;
    if pane_registration_matches_file(&registry, pane, file_path) {
        return Ok(pane.to_string());
    }

    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    let handoff_target = registry
        .values()
        .find(|entry| {
            entry.session_id == session_id
                && !entry.pane.is_empty()
                && entry.pane != pane
                && canonical_registered_file(entry) == requested
        })
        .map(|entry| entry.pane.clone());
    if let Some(entry) = registry.values().find(|entry| entry.pane == pane) {
        if let Some(handoff_pane) = handoff_target {
            eprintln!(
                "[route] fresh restart re-bound {} away from pane {} and onto authoritative pane {} before retry",
                file_path, pane, handoff_pane
            );
            return Ok(handoff_pane);
        }
        anyhow::bail!(
            "route dispatch target {} is registered for {}, not {}; refusing cross-file dispatch",
            pane,
            canonical_registered_file(entry).display(),
            requested.display()
        );
    }

    // A fresh route already created `pane` deliberately. If some concurrent
    // sync/layout path rebinds the same document session back to another pane
    // during the ready wait, keep the fresh pane authoritative instead of
    // handing dispatch back to the older pane and making the new pane disposable.
    register_dispatch_target(tmux, session_id, pane, file_path)?;
    Ok(pane.to_string())
}

fn send_command_checked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<CommandDispatchResult> {
    ensure_dispatch_target_matches_file(pane, file_path)?;
    send_command_unchecked(tmux, pane, file_path, harness)
}

fn dispatch_existing_managed_reopen(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<RoutedDispatchStartProof> {
    dispatch_via_supervisor_ipc(tmux, file, pane, session_id, file_path, harness)
}

fn dispatch_routed_reopen(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<RoutedDispatchStartProof> {
    dispatch_routed_reopen_with_mode(tmux, file, pane, file_path, harness, true)
}

fn dispatch_routed_reopen_with_mode(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    print_unproven_progress: bool,
) -> Result<RoutedDispatchStartProof> {
    let tracker =
        build_routed_dispatch_start_tracker(file, file_path, harness, Some(tmux), Some(pane))?;
    let submit_result = send_command_checked(tmux, pane, file_path, harness)?;
    let Some(tracker) = tracker else {
        log_route_latency(
            file,
            "direct_pane_submit",
            submit_result.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(submit_result.status, None),
        );
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    };

    let timeout = routed_dispatch_start_timeout(harness);
    let proof_start = Instant::now();
    if let Some(proof) = wait_for_routed_dispatch_start(tmux, file, &tracker, harness, timeout)? {
        log_route_latency(
            file,
            "direct_pane_submit",
            submit_result.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(submit_result.status, Some(proof)),
        );
        log_route_latency(
            file,
            "dispatch_start_proof",
            proof_start.elapsed(),
            timeout,
            pane,
            harness,
            proof.dispatch_stage_label(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_start_proven file={} pane={} harness={} proof={} timeout_secs={}",
                file.display(),
                pane,
                harness.binary,
                proof.dispatch_stage_label(),
                timeout.as_secs()
            ),
        );
        return Ok(proof);
    }

    log_route_latency(
        file,
        "direct_pane_submit",
        submit_result.elapsed,
        direct_pane_submit_acceptance_budget(),
        pane,
        harness,
        direct_pane_submit_outcome(submit_result.status, None),
    );
    match submit_result.status {
        CommandDispatchStatus::Accepted => {
            log_route_latency(
                file,
                "dispatch_start_proof",
                proof_start.elapsed(),
                timeout,
                pane,
                harness,
                "unproven_but_accepted",
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_start_unproven_but_accepted file={} pane={} harness={} timeout_secs={}",
                    file.display(),
                    pane,
                    harness.binary,
                    timeout.as_secs()
                ),
            );
            if print_unproven_progress {
                eprintln!(
                    "[route] bare {} reopen for {} was accepted in pane {}, but no routed submission proof appeared after {}s",
                    harness.binary,
                    file.display(),
                    pane,
                    timeout.as_secs()
                );
            }
            Ok(RoutedDispatchStartProof::CommandAcceptedOnly)
        }
        CommandDispatchStatus::TimedOut => {
            log_route_latency(
                file,
                "dispatch_start_proof",
                proof_start.elapsed(),
                timeout,
                pane,
                harness,
                "submit_timed_out_without_proof",
            );
            anyhow::bail!(
                "routed {} trigger for {} left the bare reopen drafted in pane {} and still showed no routed submission proof after waiting {}s",
                harness.binary,
                file.display(),
                pane,
                timeout.as_secs()
            )
        }
    }
}

fn routed_trigger_payload(trigger: &str) -> String {
    trigger.to_string()
}

fn apply_plain_trigger_override(harness: &mut HarnessConfig) {
    harness.trigger_command_template = "agent-doc {file}".to_string();
}

fn routed_trigger_submit_payload(payload: &str) -> String {
    crate::supervisor::ipc::normalize_submit_text(payload)
}

fn validate_routed_trigger_payload(
    harness: &HarnessConfig,
    trigger: &str,
    payload: &str,
) -> Result<()> {
    if harness.binary == "codex"
        && (payload != trigger || payload.contains('\n') || payload.contains('\r'))
    {
        anyhow::bail!(
            "internal route bug: Codex reroute payload must stay the bare `agent-doc <FILE>` reopen; refusing to inject {:?}",
            payload
        );
    }
    Ok(())
}

fn existing_pane_ready_timeout() -> Duration {
    crate::flow::routed_reopen::existing_pane_ready_timeout(cfg!(test))
}

fn format_busy_existing_pane_error(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    provenance: &str,
    detail: Option<&str>,
    auto_fix_attempted: bool,
) -> String {
    let detail_clause = detail
        .map(|detail| format!(" ({detail})"))
        .unwrap_or_default();
    if auto_fix_attempted {
        format!(
            "registered pane {} for {} is still not showing an idle {} prompt{} after automatically applying `agent-doc fix {}` once; refusing to inject a routed trigger into a busy session ({})",
            pane,
            file.display(),
            harness.binary,
            detail_clause,
            file.display(),
            provenance
        )
    } else {
        format!(
            "registered pane {} for {} is not showing an idle {} prompt{}; refusing to inject a routed trigger into a busy session ({})",
            pane,
            file.display(),
            harness.binary,
            detail_clause,
            provenance
        )
    }
}

#[cfg(test)]
fn maybe_run_test_busy_auto_fix_hook(tmux: &Tmux, file: &Path, pane: &str) -> Result<bool> {
    let Some(project_root) = snapshot::find_project_root(file)
        .or_else(|| file.parent().map(|parent| parent.to_path_buf()))
    else {
        return Ok(false);
    };
    let hook_path = project_root.join(".agent-doc/route-busy-auto-fix.txt");
    if !hook_path.exists() {
        return Ok(false);
    }
    let command = std::fs::read_to_string(&hook_path)
        .with_context(|| format!("failed to read {}", hook_path.display()))?;
    let command = command.trim();
    if command.is_empty() {
        return Ok(false);
    }
    tmux.raw_cmd(&["respawn-pane", "-k", "-t", pane, command])?;
    Ok(true)
}

#[cfg(not(test))]
fn maybe_run_test_busy_auto_fix_hook(_tmux: &Tmux, _file: &Path, _pane: &str) -> Result<bool> {
    Ok(false)
}

#[cfg(test)]
fn maybe_run_test_busy_interrupt_hook(tmux: &Tmux, file: &Path, pane: &str) -> Result<bool> {
    let Some(project_root) = snapshot::find_project_root(file)
        .or_else(|| file.parent().map(|parent| parent.to_path_buf()))
    else {
        return Ok(false);
    };
    let hook_path = project_root.join(".agent-doc/route-busy-interrupt.txt");
    if !hook_path.exists() {
        return Ok(false);
    }
    let command = std::fs::read_to_string(&hook_path)
        .with_context(|| format!("failed to read {}", hook_path.display()))?;
    let command = command.trim();
    if command.is_empty() {
        return Ok(false);
    }
    tmux.raw_cmd(&["respawn-pane", "-k", "-t", pane, command])?;
    Ok(true)
}

#[cfg(not(test))]
fn maybe_run_test_busy_interrupt_hook(_tmux: &Tmux, _file: &Path, _pane: &str) -> Result<bool> {
    Ok(false)
}

fn attempt_busy_existing_pane_auto_fix(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
) -> Result<BusyPaneAutoFixOutcome> {
    eprintln!(
        "[route] registered pane {} for {} is busy with pending document drift — applying scoped `agent-doc fix {}` once before failing closed",
        pane,
        file_path,
        file.display()
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_auto_fix_started file={} pane={}",
            file_path, pane
        ),
    );
    let test_hook_changed = maybe_run_test_busy_auto_fix_hook(tmux, file, pane)?;
    let fix_outcome = resync::apply_targeted_fix_for_route(tmux, file)?;
    let post_fix_binding = lookup_dispatch_registration(file_path, session_id)?;
    let pane_still_authoritative = post_fix_binding.as_deref() == Some(pane);
    let supervisor_health = Some(query_supervisor_health(file, session_id));
    let mut restarted = false;
    if !test_hook_changed
        && !fix_outcome.made_changes()
        && matches!(supervisor_health, Some(SupervisorHealth::Restartable))
    {
        restarted = restart_via_supervisor(file, session_id);
        if restarted {
            eprintln!(
                "[route] scoped fix left pane {} authoritative for {} — restarted the supervisor once before retrying route",
                pane, file_path
            );
        }
    } else if !test_hook_changed
        && !pane_still_authoritative
        && post_fix_binding.is_none()
        && fix_outcome.fixed_issues > 0
        && fix_outcome.pruned_dead_entries == 0
        && !fix_outcome.reregistered_owner
        && fix_outcome.killed_redundant_stash_panes == 0
        && matches!(supervisor_health, Some(SupervisorHealth::Restartable))
    {
        restarted = restart_via_supervisor(file, session_id);
        if restarted {
            eprintln!(
                "[route] scoped fix deregistered stale pane {} for {}, but the supervisor is still restartable — restarting once to wait for a clean handoff before retrying route",
                pane, file_path
            );
        }
    }
    let outcome = busy_existing_pane_auto_fix_outcome(
        test_hook_changed,
        fix_outcome.made_changes(),
        supervisor_health,
        restarted,
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_auto_fix_finished file={} pane={} pruned_dead_entries={} reregistered_owner={} killed_redundant_stash_panes={} fixed_issues={} restarted_supervisor={} outcome={:?}",
            file_path,
            pane,
            fix_outcome.pruned_dead_entries,
            fix_outcome.reregistered_owner,
            fix_outcome.killed_redundant_stash_panes,
            fix_outcome.fixed_issues,
            restarted,
            outcome
        ),
    );
    Ok(outcome)
}

fn busy_existing_pane_auto_fix_outcome(
    test_hook_changed: bool,
    fix_made_changes: bool,
    supervisor_health: Option<SupervisorHealth>,
    restarted_supervisor: bool,
) -> BusyPaneAutoFixOutcome {
    flow_busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
        test_hook_changed,
        fix_made_changes,
        supervisor_healthy: matches!(supervisor_health, Some(SupervisorHealth::Healthy)),
        restarted_supervisor,
    })
}

/// `#codex-route-busy-ctrl-g-opens-editor`: pure decision for whether the
/// busy-pane reroute may send `C-g`. `C-g` safely aborts a shell
/// `reverse-i-search` / history-search, but in any other Codex state (normal
/// composer, active turn) it opens the external editor. The busy classification
/// already came from [`HarnessConfig::dispatch_blocker_reason`] via
/// [`wait_for_agent_ready_outcome`], so we gate on that authoritative
/// `blocker_reason` rather than re-capturing (which races the wait loop's
/// 2-poll blocker streak). Any non-shell-search reason — including an unknown
/// timeout (`None`) — fails closed to the editor-safe Escape + C-c path.
fn is_codex_shell_search_blocker(blocker_reason: Option<&str>) -> bool {
    matches!(
        blocker_reason,
        Some("interactive shell reverse-i-search") | Some("interactive shell history search")
    )
}

/// Whether the busy-pane reroute may send `C-g`. Fast-path on the authoritative
/// `blocker_reason` from the readiness wait; otherwise re-classify a fresh
/// capture. The wait loop can report a timeout (`blocker_reason == None`) even
/// while the pane is genuinely in reverse-i-search (its 2-poll blocker streak
/// may not have latched), so we re-scan with [`dispatch_only_blocker_reason`],
/// which matches the whole capture rather than only the last few lines —
/// critical here because the shell-search line sits above trailing blank pane
/// rows, out of the window `HarnessConfig::dispatch_blocker_reason` inspects.
fn codex_pane_in_shell_search_state(
    tmux: &Tmux,
    pane: &str,
    harness: &HarnessConfig,
    blocker_reason: Option<&str>,
) -> bool {
    if harness.binary != "codex" {
        return false;
    }
    if is_codex_shell_search_blocker(blocker_reason) {
        return true;
    }
    let Ok(captured) = crate::sessions::capture_pane(tmux, pane) else {
        return false;
    };
    is_codex_shell_search_blocker(dispatch_only_blocker_reason(harness, &captured).as_deref())
}

fn attempt_busy_existing_pane_interrupt_recovery(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    blocker_reason: Option<&str>,
) -> Result<BusyPaneInterruptRecoveryOutcome> {
    if blocker_reason == Some("active permission prompt") {
        return Ok(BusyPaneInterruptRecoveryOutcome::Skipped);
    }

    if harness.binary == "opencode" {
        return attempt_opencode_busy_interrupt_recovery(tmux, file, pane, harness, blocker_reason);
    }

    if harness.binary != "codex" {
        return Ok(BusyPaneInterruptRecoveryOutcome::Skipped);
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_interrupt_started file={} pane={} harness={} blocker={}",
            file.display(),
            pane,
            harness.binary,
            blocker_reason.unwrap_or("timeout")
        ),
    );
    eprintln!(
        "[route] live {} pane {} for {} is still busy after the scoped recovery path — sending one interrupt sequence before the final reroute attempt",
        harness.binary,
        pane,
        file.display()
    );

    // #codex-route-busy-ctrl-g-opens-editor: `C-g` only aborts a shell
    // reverse-i-search / history-search. In the normal Codex composer (or an
    // active turn) `C-g` opens the external editor ($EDITOR/nvim) instead of
    // interrupting — the same root cause as
    // `#codex-interrupt-clear-ctrl-g-opens-editor` in
    // `send_operator_interrupt_sequence`. Only send `C-g` when the live capture
    // proves a shell-search state; otherwise fall straight through to the
    // Escape + C-c interrupt below so a busy active turn is never sent into the
    // editor.
    if codex_pane_in_shell_search_state(tmux, pane, harness, blocker_reason) {
        let _ = tmux.send_keys_raw(pane, "C-g");
        std::thread::sleep(Duration::from_millis(100));
        let ctrl_g_probe = wait_for_agent_ready_outcome(tmux, pane, Duration::from_secs(2), harness);
        if ctrl_g_probe.is_ready() {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_busy_existing_pane_interrupt_finished file={} pane={} harness={} recovered=true outcome=ready stage=ctrl_g_probe",
                    file.display(),
                    pane,
                    harness.binary,
                ),
            );
            return Ok(BusyPaneInterruptRecoveryOutcome::Recovered);
        }
    } else {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_busy_existing_pane_interrupt_skipped_ctrl_g file={} pane={} harness={} reason=not_shell_search",
                file.display(),
                pane,
                harness.binary,
            ),
        );
    }

    let _ = tmux.send_keys_raw(pane, "Escape");
    std::thread::sleep(Duration::from_millis(100));
    let _ = tmux.send_keys_raw(pane, "C-c");
    std::thread::sleep(Duration::from_millis(100));
    let _ = maybe_run_test_busy_interrupt_hook(tmux, file, pane)?;

    let ready = wait_for_agent_ready_outcome(tmux, pane, fresh_route_start_ack_timeout(), harness);
    let recovered = ready.is_ready();
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_interrupt_finished file={} pane={} harness={} recovered={} outcome={} stage=escape_ctrl_c",
            file.display(),
            pane,
            harness.binary,
            recovered,
            match ready {
                AgentReadyWaitOutcome::Ready => "ready",
                AgentReadyWaitOutcome::Blocked { .. } => "blocked",
                AgentReadyWaitOutcome::TimedOut => "timed_out",
            }
        ),
    );
    Ok(match ready {
        AgentReadyWaitOutcome::Ready => BusyPaneInterruptRecoveryOutcome::Recovered,
        AgentReadyWaitOutcome::Blocked { reason } => {
            BusyPaneInterruptRecoveryOutcome::Blocked { reason }
        }
        AgentReadyWaitOutcome::TimedOut => BusyPaneInterruptRecoveryOutcome::TimedOut,
    })
}

fn attempt_opencode_busy_interrupt_recovery(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    blocker_reason: Option<&str>,
) -> Result<BusyPaneInterruptRecoveryOutcome> {
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_opencode_interrupt_started file={} pane={} harness={} blocker={}",
            file.display(),
            pane,
            harness.binary,
            blocker_reason.unwrap_or("timeout")
        ),
    );
    eprintln!(
        "[route] live {} pane {} for {} is still busy after the scoped recovery path — sending Escape to interrupt before the final reroute attempt",
        harness.binary,
        pane,
        file.display()
    );

    let _ = tmux.send_keys_raw(pane, "Escape");
    std::thread::sleep(Duration::from_millis(200));
    let mut ready =
        wait_for_agent_ready_outcome(tmux, pane, fresh_route_start_ack_timeout(), harness);
    if !ready.is_ready() {
        let _ = tmux.send_keys_raw(pane, "Escape");
        std::thread::sleep(Duration::from_millis(100));
        ready = wait_for_agent_ready_outcome(tmux, pane, fresh_route_start_ack_timeout(), harness);
    }
    let recovered = ready.is_ready();
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_opencode_interrupt_finished file={} pane={} harness={} recovered={} outcome={}",
            file.display(),
            pane,
            harness.binary,
            recovered,
            match ready {
                AgentReadyWaitOutcome::Ready => "ready",
                AgentReadyWaitOutcome::Blocked { .. } => "blocked",
                AgentReadyWaitOutcome::TimedOut => "timed_out",
            }
        ),
    );
    Ok(match ready {
        AgentReadyWaitOutcome::Ready => BusyPaneInterruptRecoveryOutcome::Recovered,
        AgentReadyWaitOutcome::Blocked { reason } => {
            BusyPaneInterruptRecoveryOutcome::Blocked { reason }
        }
        AgentReadyWaitOutcome::TimedOut => BusyPaneInterruptRecoveryOutcome::TimedOut,
    })
}

fn ensure_existing_pane_ready_for_dispatch(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    prompt_bearing_marker: Option<&str>,
) -> Result<ExistingPaneDispatchReadiness> {
    let ready_outcome =
        wait_for_agent_ready_outcome(tmux, pane, existing_pane_ready_timeout(), harness);
    if ready_outcome.is_ready() {
        return Ok(ExistingPaneDispatchReadiness::Ready);
    }

    let provenance = pane_route_provenance(tmux, pane);
    let blocker_reason = ready_outcome.blocker_reason().map(str::to_string);
    if prompt_bearing_marker.is_none() {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_existing_pane_already_running file={} pane={} harness={} {}",
                file.display(),
                pane,
                harness.binary,
                provenance
            ),
        );
        if let Err(e) = tmux.select_pane(pane) {
            eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
        }
        eprintln!(
            "[route] registered pane {} for {} is busy but has no pending prompt-bearing drift — focusing the live {} session instead of injecting a duplicate reopen",
            pane,
            file.display(),
            harness.binary
        );
        return Ok(ExistingPaneDispatchReadiness::BusyAlreadyRunning);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "route_existing_pane_not_idle file={} pane={} harness={} blocker={} {}",
            file.display(),
            pane,
            harness.binary,
            blocker_reason.as_deref().unwrap_or("timeout"),
            provenance
        ),
    );
    Ok(ExistingPaneDispatchReadiness::BusyNeedsAutoFix {
        provenance,
        blocker_reason,
    })
}

fn cycle_state_advances_start_ack(
    current: &crate::cycle_state::CycleState,
    baseline: Option<&crate::cycle_state::CycleState>,
) -> bool {
    match baseline {
        None => true,
        Some(previous) if previous.is_open() => {
            current.cycle_id != previous.cycle_id
                || current.updated_at != previous.updated_at
                || current.phase != previous.phase
                || current.last_event != previous.last_event
        }
        Some(previous) => current.cycle_id != previous.cycle_id,
    }
}

fn wait_for_start_ack(
    file: &Path,
    baseline: Option<&crate::cycle_state::CycleState>,
    timeout: Duration,
) -> Option<crate::cycle_state::CycleState> {
    let start = std::time::Instant::now();
    let poll = Duration::from_millis(200);

    while start.elapsed() < timeout {
        if let Ok(Some(state)) = crate::cycle_state::load(file)
            && cycle_state_advances_start_ack(&state, baseline)
        {
            return Some(state);
        }
        std::thread::sleep(poll);
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn retry_routed_cycle_ack_after_fresh_restart(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    baseline: Option<&crate::cycle_state::CycleState>,
    marker: &str,
    ack_timeout: Duration,
) -> Result<Option<String>> {
    if harness.binary != "codex" {
        return Ok(None);
    }
    if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
        return Ok(None);
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "route_cycle_start_retry_after_fresh_restart file={} pane={} harness={} marker={} timeout_secs={}",
            file.display(),
            pane,
            harness.binary,
            marker,
            ack_timeout.as_secs()
        ),
    );
    eprintln!(
        "[route] routed {} trigger for {} never started a new cycle in pane {} — restarting the live session fresh once before failing closed",
        harness.binary,
        file.display(),
        pane
    );

    wait_for_busy_restart_handoff(tmux, file, file_path, session_id, pane);
    let dispatch_pane =
        resolve_fresh_dispatch_target_after_ready_wait(tmux, session_id, pane, file_path, None)?;
    let ready = wait_for_agent_ready_outcome(
        tmux,
        &dispatch_pane,
        fresh_route_start_ack_timeout(),
        harness,
    );
    if !ready.is_ready() {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_cycle_start_retry_fresh_restart_not_ready file={} pane={} harness={} outcome={}",
                file.display(),
                dispatch_pane,
                harness.binary,
                match &ready {
                    AgentReadyWaitOutcome::Ready => "ready",
                    AgentReadyWaitOutcome::Blocked { reason } => reason.as_str(),
                    AgentReadyWaitOutcome::TimedOut => "timed_out",
                }
            ),
        );
        emit_busy_route_diagnostic(tmux, &dispatch_pane, file, harness);
        if should_optimistically_accept_missing_cycle_ack(harness, true) {
            let baseline_id = baseline.map(|b| b.cycle_id.as_str());
            let miss = crate::startup_miss::record(
                file,
                &dispatch_pane,
                session_id,
                &harness.binary,
                crate::startup_miss::StartupMissOrigin::RoutedTrigger,
                baseline_id,
            )?;
            let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_retry_fresh_restart_not_ready_optimistic file={} pane={} harness={} marker={} timeout_secs={} startup_miss_timestamp={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    marker,
                    fresh_route_start_ack_timeout().as_secs(),
                    miss_ts
                ),
            );
            eprintln!(
                "[route] fresh-restart retry for {} never restored a dispatch-ready prompt in pane {} after {}s, but the earlier reopen was already accepted — keeping the reroute optimistic",
                file.display(),
                dispatch_pane,
                fresh_route_start_ack_timeout().as_secs()
            );
            return Ok(Some(dispatch_pane));
        }
        match ready {
            AgentReadyWaitOutcome::Blocked { reason } => anyhow::bail!(
                "routed {} trigger for {} was accepted in pane {}, but the fresh-restart retry never reached a dispatch-ready prompt in pane {}: {}",
                harness.binary,
                file.display(),
                pane,
                dispatch_pane,
                reason
            ),
            AgentReadyWaitOutcome::TimedOut => anyhow::bail!(
                "routed {} trigger for {} was accepted in pane {}, but the fresh-restart retry never reached a dispatch-ready prompt in pane {} after waiting {}s",
                harness.binary,
                file.display(),
                pane,
                dispatch_pane,
                fresh_route_start_ack_timeout().as_secs()
            ),
            AgentReadyWaitOutcome::Ready => unreachable!("checked above"),
        };
    }

    let dispatch_start = match dispatch_existing_managed_reopen(
        tmux,
        file,
        session_id,
        &dispatch_pane,
        file_path,
        harness,
    ) {
        Ok(proof) => proof,
        Err(_) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_retry_fresh_restart_dispatch_not_consumed file={} pane={} harness={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary
                ),
            );
            return Ok(None);
        }
    };

    match wait_for_start_ack(file, baseline, ack_timeout) {
        Some(state) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_acknowledged_after_fresh_restart file={} pane={} harness={} cycle={} phase={} marker={} timeout_secs={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    state.cycle_id,
                    cycle_phase_name(state.phase),
                    marker,
                    ack_timeout.as_secs()
                ),
            );
            let _ = crate::startup_miss::clear(file);
            Ok(Some(dispatch_pane))
        }
        None => {
            if should_optimistically_accept_missing_cycle_ack(harness, true) {
                let baseline_id = baseline.map(|b| b.cycle_id.as_str());
                let miss = crate::startup_miss::record(
                    file,
                    &dispatch_pane,
                    session_id,
                    &harness.binary,
                    crate::startup_miss::StartupMissOrigin::RoutedTrigger,
                    baseline_id,
                )?;
                let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
                emit_startup_miss_diagnostic(
                    tmux,
                    &dispatch_pane,
                    file,
                    &format!(
                        "fresh-restart retry trigger {} but no document cycle started for pending {} after {}s (startup-miss {})",
                        dispatch_start.dispatch_stage_label(),
                        marker,
                        ack_timeout.as_secs(),
                        miss_ts
                    ),
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_cycle_start_missing_after_fresh_restart_optimistic file={} pane={} harness={} marker={} timeout_secs={} startup_miss_timestamp={}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        marker,
                        ack_timeout.as_secs(),
                        miss_ts
                    ),
                );
                eprintln!(
                    "[route] fresh-restart reroute for {} never produced a new cycle ack for pending {} after {}s, but pane {} accepted the reopen — keeping the reroute optimistic",
                    file.display(),
                    marker,
                    ack_timeout.as_secs(),
                    dispatch_pane
                );
                return Ok(Some(dispatch_pane));
            }
            Ok(None)
        }
    }
}

fn cycle_phase_name(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
        crate::cycle_state::CyclePhase::Abandoned => "abandoned",
    }
}

fn fresh_route_start_ack_timeout() -> Duration {
    crate::flow::routed_reopen::fresh_route_start_ack_timeout(cfg!(test))
}

fn routed_cycle_ack_timeout(live_child_for_file: bool) -> Duration {
    crate::flow::routed_reopen::routed_cycle_ack_timeout(live_child_for_file, cfg!(test))
}

fn should_require_routed_cycle_ack(
    baseline: Option<&crate::cycle_state::CycleState>,
    prompt_bearing_marker: Option<&str>,
) -> bool {
    prompt_bearing_marker.is_some() && !baseline.is_some_and(|state| state.is_open())
}

fn should_optimistically_accept_missing_cycle_ack(
    harness: &HarnessConfig,
    live_child_for_file: bool,
) -> bool {
    harness.binary == "codex" && live_child_for_file
}

fn pending_prompt_bearing_context_for_route(
    file: &Path,
    baseline: Option<&crate::cycle_state::CycleState>,
) -> Result<Option<PendingPromptBearingRouteContext>> {
    if baseline.is_some_and(|state| state.is_open()) {
        return Ok(None);
    }
    let Some(change) = crate::session_check::first_unstarted_prompt_bearing_change(file)? else {
        return Ok(None);
    };
    let marker = match change.kind {
        crate::diff::PromptBearingChangeKind::PromptTarget => "prompt_target",
        crate::diff::PromptBearingChangeKind::ContentEdit => "content_edit",
        crate::diff::PromptBearingChangeKind::RecoveryArtifact
        | crate::diff::PromptBearingChangeKind::BoundaryArtifact => return Ok(None),
    };
    let preview = change
        .text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(change.text.as_str())
        .trim();
    let prompt_text = queue_prompt_text_for_route_change(&change.text)
        .unwrap_or_else(|| preview.trim_start_matches('❯').trim().to_string());
    Ok(Some(PendingPromptBearingRouteContext {
        marker: format!("{marker}: {preview}"),
        prompt_text,
    }))
}

#[allow(clippy::too_many_arguments)]
fn require_routed_cycle_ack(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    baseline: Option<&crate::cycle_state::CycleState>,
    prompt_bearing_marker: Option<&str>,
    live_child_for_file: bool,
    dispatch_start: RoutedDispatchStartProof,
) -> Result<Option<String>> {
    if !should_require_routed_cycle_ack(baseline, prompt_bearing_marker) {
        return Ok(None);
    }

    let marker = prompt_bearing_marker.expect("marker checked above");
    let ack_timeout = routed_cycle_ack_timeout(live_child_for_file);
    if live_child_for_file {
        eprintln!(
            "[route] live agent-doc child active in pane {} for {} — waiting up to {}s for a new cycle ack for pending {}",
            pane,
            file.display(),
            ack_timeout.as_secs(),
            marker
        );
    }
    let ack_start = Instant::now();
    match wait_for_start_ack(file, baseline, ack_timeout) {
        Some(state) => {
            log_route_latency(
                file,
                "cycle_start_ack",
                ack_start.elapsed(),
                ack_timeout,
                pane,
                harness,
                "acknowledged",
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_acknowledged file={} pane={} harness={} cycle={} phase={} marker={} timeout_secs={}",
                    file.display(),
                    pane,
                    harness.binary,
                    state.cycle_id,
                    cycle_phase_name(state.phase),
                    marker,
                    ack_timeout.as_secs()
                ),
            );
            let _ = crate::startup_miss::clear(file);
            Ok(None)
        }
        None => {
            log_route_latency(
                file,
                "cycle_start_ack",
                ack_start.elapsed(),
                ack_timeout,
                pane,
                harness,
                "missing",
            );
            let optimistic_allowed =
                should_optimistically_accept_missing_cycle_ack(harness, live_child_for_file);
            if live_child_for_file
                && let Some(dispatch_pane) = retry_routed_cycle_ack_after_fresh_restart(
                    tmux,
                    file,
                    pane,
                    session_id,
                    file_path,
                    harness,
                    baseline,
                    marker,
                    ack_timeout,
                )?
            {
                return Ok(Some(dispatch_pane));
            }
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_missing file={} pane={} harness={} marker={} timeout_secs={}",
                    file.display(),
                    pane,
                    harness.binary,
                    marker,
                    ack_timeout.as_secs()
                ),
            );
            let baseline_id = baseline.map(|b| b.cycle_id.as_str());
            let miss = crate::startup_miss::record(
                file,
                pane,
                session_id,
                &harness.binary,
                crate::startup_miss::StartupMissOrigin::RoutedTrigger,
                baseline_id,
            )?;
            let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
            let dispatch_stage = dispatch_start.dispatch_stage_label();
            emit_startup_miss_diagnostic(
                tmux,
                pane,
                file,
                &format!(
                    "routed trigger {} but no document cycle started for pending {} after {}s (startup-miss {})",
                    dispatch_stage,
                    marker,
                    ack_timeout.as_secs(),
                    miss_ts
                ),
            );
            if optimistic_allowed {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_cycle_start_missing_optimistic file={} pane={} harness={} marker={} timeout_secs={} startup_miss_timestamp={}",
                        file.display(),
                        pane,
                        harness.binary,
                        marker,
                        ack_timeout.as_secs(),
                        miss_ts
                    ),
                );
                eprintln!(
                    "[route] routed {} trigger for {} never produced a new cycle ack for pending {} after {}s, but the correct pane accepted the reopen — keeping the reroute optimistic",
                    harness.binary,
                    file.display(),
                    marker,
                    ack_timeout.as_secs()
                );
                return Ok(Some(pane.to_string()));
            }
            anyhow::bail!(
                "routed {} trigger for {} was {} in pane {}, but no new document cycle started for pending {} after waiting {}s (startup_miss_timestamp={})",
                harness.binary,
                file.display(),
                dispatch_stage,
                pane,
                marker,
                ack_timeout.as_secs(),
                miss_ts
            );
        }
    }
}

fn recent_lines_contain_trigger(content: &str, trigger: &str) -> bool {
    let recent_lines: Vec<String> = content
        .lines()
        .rev()
        .take(8)
        .map(prompt::strip_ansi)
        .collect();
    recent_lines
        .iter()
        .any(|line| line_contains_trigger(line, trigger))
        || recent_lines_contain_wrapped_trigger(&recent_lines, trigger)
}

fn line_contains_trigger(line: &str, trigger: &str) -> bool {
    let mut offset = 0usize;
    while let Some(found) = line[offset..].find(trigger) {
        let start = offset + found;
        let end = start + trigger.len();
        let prev_ok = line[..start]
            .chars()
            .next_back()
            .map(|ch| ch.is_whitespace() || matches!(ch, '>' | '❯' | '⏵'))
            .unwrap_or(true);
        let next_ok = line[end..]
            .chars()
            .next()
            .map(|ch| ch.is_whitespace())
            .unwrap_or(true);
        if prev_ok && next_ok {
            return true;
        }
        offset = start + 1;
    }
    false
}

fn compact_trigger_text(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn strip_leading_prompt_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    for prompt in ["❯", ">", "›", "⏵"] {
        if let Some(rest) = trimmed.strip_prefix(prompt) {
            return rest.trim_start();
        }
    }
    trimmed
}

fn shares_trigger_prefix(fragment: &str, trigger: &str) -> bool {
    let mut frag = fragment.chars();
    let mut trig = trigger.chars();
    loop {
        match (frag.next(), trig.next()) {
            (Some(left), Some(right)) if left == right => {}
            (Some(_), Some(_)) => return false,
            (None, _) | (_, None) => return true,
        }
    }
}

fn recent_lines_contain_wrapped_trigger(recent_lines_rev: &[String], trigger: &str) -> bool {
    let compact_trigger = compact_trigger_text(trigger);
    if compact_trigger.is_empty() {
        return false;
    }
    let lines: Vec<&String> = recent_lines_rev.iter().rev().collect();
    for start in 0..lines.len() {
        let first = compact_trigger_text(strip_leading_prompt_prefix(lines[start]));
        if first.is_empty() || !shares_trigger_prefix(&first, &compact_trigger) {
            continue;
        }
        let mut joined = first;
        if joined.contains(&compact_trigger) {
            return true;
        }
        for next in lines.iter().skip(start + 1).take(3) {
            joined.push_str(&compact_trigger_text(next));
            if joined.contains(&compact_trigger) {
                return true;
            }
            if joined.len() > compact_trigger.len() + 32 {
                break;
            }
        }
    }
    false
}

/// Get the tmux session that owns the caller pane.
fn current_tmux_session(tmux: &Tmux) -> Option<String> {
    tmux.current_session()
}

pub fn resolve_preferred_session(
    tmux: &Tmux,
    context_session: Option<&str>,
    log_prefix: &str,
) -> Option<String> {
    if let Some(ctx) = normalize_context_session(context_session) {
        return Some(ctx.to_string());
    }

    let configured = crate::config::project_tmux_session();
    if configured.as_ref().is_some_and(|s| tmux.session_alive(s)) {
        return configured;
    }

    if let Some(ref stale) = configured {
        eprintln!(
            "{log_prefix} configured tmux_session '{}' is not alive, ignoring stale pin",
            stale
        );
    }

    current_tmux_session(tmux)
}

fn resolve_preferred_session_for_layout(
    tmux: &Tmux,
    context_session: Option<&str>,
    col_args: &[String],
    focus: Option<&Path>,
    log_prefix: &str,
) -> Option<String> {
    if let Some(ctx) = normalize_context_session(context_session) {
        return Some(ctx.to_string());
    }

    let focus_owned = focus.map(|path| path.to_string_lossy().into_owned());
    if let Some(scope_root) = crate::sync::shared_sync_scope_root(col_args, focus_owned.as_deref())
        && let Some(session) = crate::sync::configured_session_for_root(tmux, &scope_root)
    {
        return Some(session);
    }

    resolve_preferred_session(tmux, None, log_prefix)
}

/// Single source of truth for target session resolution.
///
/// Priority:
/// 1. `context_session` if provided (from sync --window)
/// 2. config.toml `tmux_session` if the session is alive (user explicitly pinned via `session set`)
/// 3. Fallback to current tmux session or harness-specific fallback name (auto-detect)
///
/// Session config is never auto-written. Only `agent-doc session set <name>` pins a session.
/// `agent-doc session clear` returns to auto-detect mode.
fn resolve_target_session(
    tmux: &Tmux,
    context_session: Option<&str>,
    col_args: &[String],
    focus: Option<&Path>,
    harness: &HarnessConfig,
) -> String {
    resolve_preferred_session_for_layout(tmux, context_session, col_args, focus, "[route]")
        .unwrap_or_else(|| harness.tmux_session_fallback.clone())
}

fn ensure_auto_start_target_session(
    tmux: &Tmux,
    context_session: Option<&str>,
    session_name: &str,
    harness: &HarnessConfig,
) -> Result<()> {
    if normalize_context_session(context_session).is_some() {
        return Ok(());
    }

    if crate::config::project_tmux_session().as_deref() == Some(session_name)
        && tmux.session_alive(session_name)
    {
        return Ok(());
    }

    if current_tmux_session(tmux).as_deref() == Some(session_name) {
        return Ok(());
    }

    if tmux.session_alive(session_name) {
        return Ok(());
    }

    if session_name == harness.tmux_session_fallback {
        anyhow::bail!(
            "refusing to auto-start in implicit fallback tmux session '{}' without a live explicit target session",
            session_name
        );
    }

    anyhow::bail!(
        "refusing to auto-start in tmux session '{}' because it is not alive",
        session_name
    );
}

fn normalize_context_session(context_session: Option<&str>) -> Option<&str> {
    context_session.and_then(|session| {
        let trimmed = session.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Find an explicit target pane for lazy claiming.
/// Skips panes already claimed by another document in the session registry.
fn find_target_pane(
    tmux: &Tmux,
    explicit_pane: Option<&str>,
    _session_name: &str,
    claimed_panes: &std::collections::HashSet<String>,
) -> Option<String> {
    let target = explicit_pane.map(|p| p.to_string());
    target.filter(|p| tmux.pane_alive(p) && !claimed_panes.contains(p))
}

/// Check if a window with the given name exists in the target tmux session.
fn has_named_window(tmux: &Tmux, session_name: &str, window_name: &str) -> bool {
    let output = tmux
        .cmd()
        .args(["list-windows", "-t", session_name, "-F", "#{window_name}"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines().any(|l| l.trim() == window_name)
        }
        _ => false,
    }
}

fn pane_session_name(tmux: &Tmux, pane_id: &str) -> Option<String> {
    tmux.cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{session_name}"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn pane_window_name(tmux: &Tmux, pane_id: &str) -> Option<String> {
    tmux.pane_window(pane_id).ok().and_then(|window_id| {
        tmux.cmd()
            .args(["display-message", "-t", &window_id, "-p", "#{window_name}"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    })
}

fn is_stash_window_name(window_name: &str) -> bool {
    window_name == "stash" || window_name.starts_with("stash-")
}

fn evict_previous_stash_pane(
    tmux: &Tmux,
    session_id: &str,
    replacement_pane: &str,
    target_session: &str,
    harness: &HarnessConfig,
) {
    let Ok(Some(previous)) = sessions::lookup_entry(session_id) else {
        return;
    };
    evict_previous_stash_pane_entry(
        tmux,
        session_id,
        &previous,
        replacement_pane,
        target_session,
        harness,
    );
}

fn evict_previous_stash_pane_entry(
    tmux: &Tmux,
    session_id: &str,
    previous: &sessions::SessionEntry,
    replacement_pane: &str,
    target_session: &str,
    harness: &HarnessConfig,
) {
    if previous.pane.is_empty()
        || previous.pane == replacement_pane
        || !tmux.pane_alive(&previous.pane)
    {
        return;
    }
    if pane_session_name(tmux, &previous.pane).as_deref() != Some(target_session) {
        return;
    }
    let Some(window_name) = pane_window_name(tmux, &previous.pane) else {
        return;
    };
    if !is_stash_window_name(&window_name) {
        return;
    }

    eprintln!(
        "[route] preserving previous stash pane {} for session {} — automatic stash eviction requires explicit provenance",
        previous.pane,
        &session_id[..std::cmp::min(8, session_id.len())]
    );
    let _ = (replacement_pane, target_session, harness);
}

/// Find a registered agent-doc pane in the target tmux session.
/// Used by auto_start to join alongside an existing agent-doc pane (not any random pane).
fn find_registered_pane_in_session(
    tmux: &Tmux,
    registry_base_dir: &Path,
    session_name: &str,
    exclude_pane: &str,
) -> Option<String> {
    let registry = sessions::load_in(registry_base_dir).ok()?;
    for entry in registry.values() {
        if entry.pane == exclude_pane || entry.pane.is_empty() {
            continue;
        }
        if !tmux.pane_alive(&entry.pane) {
            continue;
        }
        // Check if this pane is in the target session
        if let Ok(output) = tmux
            .cmd()
            .args([
                "display-message",
                "-t",
                &entry.pane,
                "-p",
                "#{session_name}",
            ])
            .output()
        {
            let pane_session = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if pane_session == session_name {
                return Some(entry.pane.clone());
            }
        }
    }
    None
}

fn registry_base_dir_for_file(file: &Path, fallback: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(file)
        .ok()
        .and_then(|path| {
            crate::snapshot::find_project_root(&path)
                .or_else(|| path.parent().map(|parent| parent.to_path_buf()))
        })
        .unwrap_or_else(|| fallback.to_path_buf())
}

/// Auto-start a new agent session in tmux using the default session name.
/// Public so `sync.rs` can call it for unresolved files.
///
/// `context_session` is an optional session override from the calling context
/// (e.g., the sync target session). Used when frontmatter has no `tmux_session`
/// and sync has already resolved a more specific session from editor/window
/// context.
#[allow(dead_code)]
pub fn auto_start(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
) -> Result<String> {
    auto_start_ext(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        false,
        false,
    )
}

/// Rewrite `file_path` to be relative to `cwd` so `agent-doc start <path>` resolves
/// correctly when the spawned pane's cwd is narrowed to a submodule root.
///
/// When `resolve_pane_cwd` narrows to a submodule (e.g. `.../src/session-share`),
/// the caller's super-root-relative `file_path` (e.g. `src/session-share/tasks/foo.md`)
/// does not resolve from inside that cwd. We canonicalize both sides, strip the cwd
/// prefix, and return the cwd-relative remainder. On any failure (canonicalize error,
/// file not under cwd) we fall back to the original `file_path` so non-submodule docs
/// and missing-file cases behave exactly as before.
pub fn rewrite_start_path(file: &Path, cwd: &Path, original: &str) -> String {
    let Ok(abs_file) = std::fs::canonicalize(file) else {
        return original.to_string();
    };
    let Ok(abs_cwd) = std::fs::canonicalize(cwd) else {
        return original.to_string();
    };
    match abs_file.strip_prefix(&abs_cwd) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => original.to_string(),
    }
}

/// **Provisioning** — create a new tmux pane and start Claude asynchronously.
///
/// Called by sync during Reconciliation when a file has a session UUID but no
/// registered pane. Creates the pane immediately but doesn't wait for Claude
/// to initialize (async startup).
pub fn provision_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    col_args: &[String],
) -> Result<String> {
    let split_before = is_first_column(file, col_args);
    auto_start_ext(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        true,
        split_before,
    )
}

fn auto_start_ext(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    skip_wait: bool,
    split_before: bool,
) -> Result<String> {
    let harness = resolve_harness_for_file(file);
    let session_name = resolve_target_session(tmux, context_session, &[], Some(file), &harness);
    ensure_auto_start_target_session(tmux, context_session, &session_name, &harness)?;
    auto_start_in_session(
        tmux,
        file,
        session_id,
        file_path,
        &session_name,
        skip_wait,
        split_before,
        &harness,
        None,
        None,
        false,
    )
}

struct StartupLocks {
    _doc: File,
    _session: File,
}

fn starting_dir_for(file: &Path) -> Option<std::path::PathBuf> {
    let canonical = std::fs::canonicalize(file).ok()?;
    let base = snapshot::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(|p| p.to_path_buf()))?;
    Some(base.join(".agent-doc/starting"))
}

fn session_start_lock_name(session_name: &str) -> String {
    let hash = crate::snapshot::doc_hash_from_str(&format!("session:{session_name}"));
    format!("session-{hash}.lock")
}

fn open_start_lock(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("failed to open startup lock {}", path.display()))
}

fn acquire_startup_locks(file: &Path, session_name: &str) -> Result<Option<StartupLocks>> {
    let Some(starting_dir) = starting_dir_for(file) else {
        return Ok(None);
    };

    let doc_lock_path = if let Ok(hash) = snapshot::doc_hash(file) {
        starting_dir.join(format!("{hash}.lock"))
    } else {
        let fallback = crate::snapshot::doc_hash_from_str(&file.to_string_lossy());
        starting_dir.join(format!("{fallback}.lock"))
    };
    let session_lock_path = starting_dir.join(session_start_lock_name(session_name));

    let doc_lock = open_start_lock(&doc_lock_path)?;
    doc_lock
        .lock_exclusive()
        .with_context(|| format!("failed to acquire startup lock {}", doc_lock_path.display()))?;

    let session_lock = open_start_lock(&session_lock_path)?;
    session_lock.lock_exclusive().with_context(|| {
        format!(
            "failed to acquire session startup lock {}",
            session_lock_path.display()
        )
    })?;

    Ok(Some(StartupLocks {
        _doc: doc_lock,
        _session: session_lock,
    }))
}

/// Resolve HarnessConfig from a file's frontmatter + global config.
fn resolve_harness_for_file(file: &Path) -> HarnessConfig {
    let content = std::fs::read_to_string(file).unwrap_or_default();
    let fm = frontmatter::parse(&content)
        .map(|(f, _)| f)
        .unwrap_or_default();
    let global_config = crate::config::load().unwrap_or_default();
    HarnessConfig::from_context(&fm, &global_config)
}

/// Auto-start a new agent session in a specific tmux session.
///
/// Strategy:
/// 1. Find an existing registered agent-doc pane in the target session
/// 2. If found: `split-window` directly in that pane's window (avoids creating
///    a throwaway window then failing to join due to minimum pane size)
/// 3. If not found: create a new window via `auto_start` (session may not exist yet)
///
/// When `skip_wait` is true, skips `wait_for_agent_ready` and `send_command`.
/// Used by sync which only needs the pane to exist with the agent starting.
#[allow(clippy::too_many_arguments)]
fn auto_start_in_session(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    session_name: &str,
    skip_wait: bool,
    split_before: bool,
    harness: &HarnessConfig,
    startup_miss_handoff_blocked_pane: Option<&str>,
    mut created_panes: Option<&mut Vec<String>>,
    dispatch_only: bool,
) -> Result<String> {
    // Serialize auto-starts for both the document and the target tmux session.
    // This prevents duplicate starts for the same file and split-target races
    // when two different documents provision concurrently into the same window.
    let startup_locks = acquire_startup_locks(file, session_name)?;
    if let Some(existing) = sessions::lookup(session_id)?
        && tmux.pane_alive(&existing)
    {
        eprintln!(
            "[route] startup already provisioned pane {} for {} while waiting on locks",
            existing, file_path
        );
        return Ok(existing);
    }

    // Use the document's own submodule root as the pane cwd when applicable,
    // so `/agent-doc` invocations on submodule-hosted documents spawn panes
    // inside the correct submodule (e.g. `src/session-share`) instead of the
    // agent-loop super root where the command happened to be invoked from.
    let cwd = crate::git::resolve_pane_cwd(file);
    let registry_base_dir = registry_base_dir_for_file(file, &cwd);

    // Resolve the agent-doc binary path (same binary that's currently running)
    let agent_doc_bin = agent_doc_start_bin();

    // Try to split directly in an existing pane.
    // When skip_wait=true (sync path), prefer panes in the target window (agent-doc window)
    // over stash panes — splitting in the stash creates invisible panes.
    let existing_pane = if skip_wait {
        // Sync path: find a pane in the agent-doc window (not stash)
        let window_panes = tmux
            .list_panes_ordered(&format!("{}:agent-doc", session_name))
            .unwrap_or_default();
        let positional = if split_before {
            window_panes.into_iter().next() // leftmost by screen position
        } else {
            window_panes.into_iter().last() // rightmost by screen position
        };
        positional
            .or_else(|| find_registered_pane_in_session(tmux, &registry_base_dir, session_name, ""))
    } else {
        find_registered_pane_in_session(tmux, &registry_base_dir, session_name, "")
    };
    let split_flag = if split_before { "-dbh" } else { "-dh" };
    let new_pane = if let Some(ref target) = existing_pane {
        match tmux.split_window(target, &cwd, split_flag) {
            Ok(pane) => {
                eprintln!(
                    "[route] split-window {} alongside registered pane {} in session '{}' → new pane {}",
                    split_flag, target, session_name, pane
                );
                pane
            }
            Err(e) => {
                anyhow::bail!(
                    "{}",
                    format_duplicate_pane_policy_error(
                        session_name,
                        file_path,
                        Some(target),
                        &format!("split-window failed alongside pane {} ({})", target, e)
                    )
                );
            }
        }
    } else {
        let has_agent_doc_window = has_named_window(tmux, session_name, "agent-doc");
        if has_agent_doc_window {
            anyhow::bail!(
                "{}",
                format_duplicate_pane_policy_error(
                    session_name,
                    file_path,
                    None,
                    "the target session already has an 'agent-doc' window but no safe registered anchor pane was found"
                )
            );
        } else {
            eprintln!(
                "[route] no registered pane found in session '{}', creating new window",
                session_name
            );
            tmux.auto_start(session_name, &cwd)?
        }
    };
    tmux.enable_remain_on_exit(&new_pane)?;
    if let Some(created) = created_panes.as_mut() {
        created.push(new_pane.clone());
    }

    evict_previous_stash_pane(tmux, session_id, &new_pane, session_name, harness);

    // Register immediately so subsequent route calls find this pane
    register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
    drop(startup_locks);

    // Focus the new pane immediately so the user sees Claude starting
    if let Err(e) = tmux.select_pane(&new_pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", new_pane, e);
    }

    // Rewrite file_path to be relative to the spawned pane's cwd.
    // `cwd` may be narrowed to a submodule root (see `resolve_pane_cwd`), in which
    // case a super-root-relative `file_path` like `src/session-share/tasks/foo.md`
    // will not resolve when `agent-doc start` runs from inside the submodule.
    // Fallback: if canonicalize fails or the file is not under `cwd`, use the
    // original `file_path` (preserves behavior for non-submodule docs).
    let start_path = rewrite_start_path(file, &cwd, file_path);

    // Start agent-doc start in the new pane
    let start_cmd = format!("{} start --route-owned {}", agent_doc_bin, start_path);
    crate::input_diag::log_text_submit(
        Some(file),
        "route.auto_start",
        &format!("pane:{new_pane}"),
        &start_cmd,
        Some(&harness.binary),
        "route_owned_start_enter",
        "Enter",
    );
    crate::sessions::send_submitted_text(tmux, &new_pane, &start_cmd)?;

    eprintln!(
        "[route] Started {} for {} in pane {} (session {})",
        harness.binary,
        file_path,
        new_pane,
        &session_id[..std::cmp::min(8, session_id.len())]
    );

    let cycle_baseline = crate::cycle_state::load(file).unwrap_or(None);

    if skip_wait {
        eprintln!(
            "[route] skip_wait=true — pane created, {} starting (sync path)",
            harness.binary
        );
    } else {
        eprintln!("[route] Waiting for {} to initialize...", harness.binary);
        let ready =
            wait_for_agent_ready(tmux, &new_pane, std::time::Duration::from_secs(30), harness);
        // Fresh-start recovery can clear the early geometry-only binding while
        // the harness is still booting. Re-validate the registration before we
        // dispatch, but keep the deliberately created fresh pane authoritative
        // for same-document rebind churn instead of treating it as disposable.
        let dispatch_pane = resolve_fresh_dispatch_target_after_ready_wait(
            tmux,
            session_id,
            &new_pane,
            file_path,
            startup_miss_handoff_blocked_pane,
        )?;
        if dispatch_pane != new_pane {
            eprintln!(
                "[route] fresh start pane {} handed ownership for {} back to existing pane {} during startup; dispatching follow-up there",
                new_pane, file_path, dispatch_pane
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "fresh_route_dispatch_handoff file={} fresh_pane={} dispatch_pane={} harness={}",
                    file.display(),
                    new_pane,
                    dispatch_pane,
                    harness.binary
                ),
            );
            if let Err(e) = tmux.select_pane(&dispatch_pane) {
                eprintln!(
                    "[route] warning: failed to focus handoff pane {}: {}",
                    dispatch_pane, e
                );
            }
        }
        let dispatch_start = if ready {
            eprintln!("[route] {} is ready, sending command", harness.binary);
            if dispatch_only {
                dispatch_only_send_reopen(
                    tmux,
                    file,
                    session_id,
                    &dispatch_pane,
                    file_path,
                    harness,
                    DispatchOnlySendReopenOptions {
                        delivery: DispatchOnlyReopenDelivery::SupervisorIpcOnce,
                        queue_prompt_text: None,
                    },
                )?;
                RoutedDispatchStartProof::CommandAcceptedOnly
            } else {
                dispatch_routed_reopen(tmux, file, &dispatch_pane, file_path, harness)?
            }
        } else {
            eprintln!(
                "[route] Timed out waiting for {} prompt; attempting one fallback trigger injection before failing closed",
                harness.binary
            );
            let dispatch_result = if dispatch_only {
                dispatch_only_send_reopen(
                    tmux,
                    file,
                    session_id,
                    &dispatch_pane,
                    file_path,
                    harness,
                    DispatchOnlySendReopenOptions {
                        delivery: DispatchOnlyReopenDelivery::SupervisorIpcOnce,
                        queue_prompt_text: None,
                    },
                )
                .map(|_| RoutedDispatchStartProof::CommandAcceptedOnly)
            } else {
                dispatch_routed_reopen(tmux, file, &dispatch_pane, file_path, harness)
            };
            match dispatch_result {
                Ok(proof) => {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "fresh_route_trigger_recovered file={} pane={} harness={}",
                            file.display(),
                            dispatch_pane,
                            harness.binary
                        ),
                    );
                    eprintln!(
                        "[route] Fallback trigger injection recovered the fresh {} start for {}",
                        harness.binary, file_path
                    );
                    proof
                }
                Err(err) => {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "fresh_route_trigger_missing file={} pane={} harness={}",
                            file.display(),
                            new_pane,
                            harness.binary
                        ),
                    );
                    return Err(err);
                }
            }
        };

        if dispatch_only {
            crate::ops_log::log_op(
                file,
                &format!(
                    "fresh_route_dispatch_only file={} pane={} harness={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary
                ),
            );
            let _ = dispatch_start;
            return Ok(dispatch_pane);
        }

        let ack_timeout = fresh_route_start_ack_timeout();
        match wait_for_start_ack(file, cycle_baseline.as_ref(), ack_timeout) {
            Some(state) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "fresh_route_start_acknowledged file={} pane={} harness={} cycle={} phase={} timeout_secs={}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        state.cycle_id,
                        match state.phase {
                            crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
                            crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
                            crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
                            crate::cycle_state::CyclePhase::Committed => "committed",
                            crate::cycle_state::CyclePhase::Abandoned => "abandoned",
                        },
                        ack_timeout.as_secs()
                    ),
                );
                let _ = crate::startup_miss::clear(file);
            }
            None => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "fresh_route_start_missing file={} pane={} harness={} timeout_secs={}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        ack_timeout.as_secs()
                    ),
                );
                let baseline_id = cycle_baseline.as_ref().map(|b| b.cycle_id.as_str());
                let _ = crate::startup_miss::record(
                    file,
                    &dispatch_pane,
                    session_id,
                    &harness.binary,
                    crate::startup_miss::StartupMissOrigin::FreshStart,
                    baseline_id,
                );
                emit_startup_miss_diagnostic(
                    tmux,
                    &dispatch_pane,
                    file,
                    &format!(
                        "fresh start: trigger {} but no document cycle started",
                        dispatch_start.dispatch_stage_label()
                    ),
                );
                anyhow::bail!(
                    "fresh {} start for {} never acknowledged with a document cycle after trigger {}",
                    harness.binary,
                    file.display(),
                    dispatch_start.startup_miss_label()
                );
            }
        }
    }

    let final_pane = if skip_wait {
        register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
        new_pane
    } else {
        resolve_fresh_dispatch_target_after_ready_wait(
            tmux,
            session_id,
            &new_pane,
            file_path,
            startup_miss_handoff_blocked_pane,
        )?
    };
    let _ = file; // suppress unused warning
    Ok(final_pane)
}

fn agent_doc_start_bin() -> String {
    if let Ok(override_bin) = std::env::var("AGENT_DOC_ROUTE_BIN")
        && !override_bin.trim().is_empty()
    {
        return override_bin;
    }

    std::env::current_exe()
        .unwrap_or_else(|_| "agent-doc".into())
        .to_string_lossy()
        .to_string()
}

/// Poll a tmux pane until the agent is ready to accept input.
///
/// Uses the harness's prompt patterns for detection.
/// Strips ANSI escape codes before matching. Polls every 500ms up to the given timeout.
fn wait_for_agent_ready(
    tmux: &Tmux,
    pane_id: &str,
    timeout: std::time::Duration,
    harness: &HarnessConfig,
) -> bool {
    wait_for_agent_ready_outcome(tmux, pane_id, timeout, harness).is_ready()
}

fn wait_for_agent_ready_outcome(
    tmux: &Tmux,
    pane_id: &str,
    timeout: std::time::Duration,
    harness: &HarnessConfig,
) -> AgentReadyWaitOutcome {
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(500);
    let mut poll_count = 0u32;
    let mut ready_streak = 0u32;
    let mut last_ready_line: Option<String> = None;
    let mut blocker_streak = 0u32;
    let mut last_blocker: Option<String> = None;

    while start.elapsed() < timeout {
        if let Ok(content) = sessions::capture_pane(tmux, pane_id) {
            if let Some(reason) = harness.dispatch_blocker_reason(&content) {
                ready_streak = 0;
                last_ready_line = None;
                if last_blocker.as_deref() == Some(reason.as_str()) {
                    blocker_streak += 1;
                } else {
                    blocker_streak = 1;
                    last_blocker = Some(reason.clone());
                    if reason == "active permission prompt" {
                        crate::input_diag::log_prompt_detection(
                            None,
                            "route.wait_for_agent_ready",
                            &format!("pane:{pane_id}"),
                            &harness.binary,
                            &reason,
                            "entered",
                        );
                    }
                }
                if blocker_streak >= 2 {
                    eprintln!(
                        "[route] {} blocked after {:.1}s in pane {}: {}",
                        harness.binary,
                        start.elapsed().as_secs_f64(),
                        pane_id,
                        reason
                    );
                    return AgentReadyWaitOutcome::Blocked { reason };
                }
            } else {
                blocker_streak = 0;
                last_blocker = None;
            }

            match ready_prompt_candidate(&content, harness) {
                Some(line) => {
                    if last_ready_line.as_deref() == Some(line.as_str()) {
                        ready_streak += 1;
                    } else {
                        ready_streak = 1;
                        last_ready_line = Some(line);
                    }
                    if ready_streak >= 2 {
                        eprintln!(
                            "[route] {} ready after {:.1}s ({} polls)",
                            harness.binary,
                            start.elapsed().as_secs_f64(),
                            poll_count
                        );
                        return AgentReadyWaitOutcome::Ready;
                    }
                }
                None => {
                    ready_streak = 0;
                    last_ready_line = None;
                }
            }

            poll_count += 1;
            if poll_count.is_multiple_of(10) {
                let last_line = content
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .map(prompt::strip_ansi)
                    .unwrap_or_default();
                eprintln!(
                    "[route] Still waiting for {} ({:.0}s)... last line: {}",
                    harness.binary,
                    start.elapsed().as_secs_f64(),
                    truncate_log_line(&last_line, 60)
                );
            }
        }
        std::thread::sleep(poll_interval);
    }
    AgentReadyWaitOutcome::TimedOut
}

fn ready_prompt_candidate(content: &str, harness: &HarnessConfig) -> Option<String> {
    if harness.has_busy_cue(content) {
        return None;
    }
    if harness.binary == "opencode" && harness.is_idle_chrome_only_output(content) {
        return Some("opencode idle status chrome".to_string());
    }
    harness
        .last_prompt_candidate(content)
        .filter(|line| harness.is_dispatch_ready_prompt_line(line))
}

fn truncate_log_line(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// After a lazy claim, sync tmux layout for all files in the same window.
///
/// This ensures pane arrangement stays consistent when a file is reclaimed
/// to a different pane. Only runs on autoclaim — normal routing skips this.
#[allow(dead_code)]
fn sync_after_claim(tmux: &Tmux, pane_id: &str, col_args: &[String]) {
    let window_id = match tmux.pane_window(pane_id) {
        Ok(w) => w,
        Err(_) => return,
    };

    // Use editor-provided col_args when available (authoritative layout).
    // Only fall back to registry discovery when no col_args given.
    let effective_col_args: Vec<String> = if !col_args.is_empty() {
        col_args.to_vec()
    } else {
        // Load registry and find all files whose panes are in the same window
        let registry = match sessions::load() {
            Ok(r) => r,
            Err(_) => return,
        };

        registry
            .values()
            .filter(|entry| {
                !entry.pane.is_empty()
                    && tmux.pane_alive(&entry.pane)
                    && tmux.pane_window(&entry.pane).ok().as_deref() == Some(&window_id)
                    && !entry.file.is_empty()
            })
            .map(|entry| entry.file.clone())
            .collect()
    };

    if effective_col_args.len() < 2 {
        return; // 0 or 1 files — no layout sync needed
    }

    let file_count = effective_col_args.len();
    // Keep the reconcile scoped to the caller's tmux handle. Falling back to the
    // default server here can mutate an unrelated live agent-doc window during
    // isolated verification runs.
    if let Err(e) = sync::run_with_tmux(&effective_col_args, Some(&window_id), None, tmux) {
        eprintln!("[route] warning: post-claim sync failed: {}", e);
    } else {
        eprintln!(
            "[route] Auto-synced {} files in window {}",
            file_count, window_id
        );
    }
}

/// Wait for the file's mtime and editor typing indicator to settle.
///
/// Polls every 100ms, up to 10× the debounce duration as a safety cap. Route
/// must fail closed instead of proceeding through a visible document mutation
/// while the editor-side typing indicator is still active.
fn await_idle(file: &Path, debounce: Duration) -> Result<()> {
    await_idle_with_max_wait(file, debounce, debounce * 10)
}

fn await_idle_with_max_wait(file: &Path, debounce: Duration, max_wait: Duration) -> Result<()> {
    use std::time::Instant;

    let poll_interval = Duration::from_millis(100);
    let start = Instant::now();
    let debounce_ms = debounce.as_millis().min(u64::MAX as u128) as u64;
    let file_str = file.to_string_lossy();

    loop {
        let mtime = std::fs::metadata(file)
            .and_then(|m| m.modified())
            .with_context(|| format!("failed to stat {}", file.display()))?;
        let elapsed_since_edit = mtime.elapsed().unwrap_or(Duration::ZERO);
        let typing_active = crate::debounce::is_typing_via_file(&file_str, debounce_ms);

        if elapsed_since_edit >= debounce && !typing_active {
            eprintln!(
                "[route] debounce OK — file idle for {:.1}s and typing indicator idle",
                elapsed_since_edit.as_secs_f64(),
            );
            return Ok(());
        }

        if start.elapsed() >= max_wait {
            anyhow::bail!(
                "route deferred for {}: document did not settle within {}ms (mtime_idle_ms={}, typing_active={}); retry after typing stops",
                file.display(),
                max_wait.as_millis(),
                elapsed_since_edit.as_millis(),
                typing_active
            );
        }

        std::thread::sleep(poll_interval);
    }
}

#[cfg(test)]
mod tests;
