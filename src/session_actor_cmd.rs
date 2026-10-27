use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent_doc_controller::actor::{ActorRecord, ActorState};
#[cfg(test)]
use agent_doc_controller::operator_clear::OperatorClearGuardOutcome;
use agent_doc_controller::operator_clear::{OperatorClearInputState, clear_guard_event};
#[cfg(test)]
use agent_doc_sqlite::state_store::SupervisorLeaseStatus;
use agent_doc_sqlite::state_store::{ActorTransitionStatus, SessionOperatorStatus};
use agent_doc_supervisor::ipc_protocol::IpcMethod;
#[cfg(test)]
use agent_doc_supervisor::ipc_protocol::IpcResponse;
use agent_doc_supervisor::startup_miss::{SessionLogStatus, StartupMiss, format_timestamp};
use agent_doc_tmux_commands::tmux_submit_mode_for_harness;
use agent_doc_turn::op_log::OpsLogEvent;
use agent_doc_turn_executor_tmux::context_clear::{
    ABANDONED_PROMPT_SETTLING_EVIDENCE, CONTEXT_CLEAR_CLEARED_STATE_MAX_HISTORY_LINES,
    ContextClearLastFrame, ContextClearSubmitObservation, ContextClearSubmitPollState,
    ContextClearSubmitRetryAction, ContextClearSubmitRetryFacts, ContextClearSubmitRetryProofFacts,
    ContextClearSubmitStatus, DISPATCH_TURN_PENDING_EVIDENCE, InterruptClearTimeoutFacts,
    abandoned_prompt_clear_queued_message, busy_clear_already_deferred_message,
    busy_clear_deferred_message, busy_clear_refusal_message,
    context_clear_capture_shows_queued_input, context_clear_command_visible_in_active_input,
    context_clear_history_proves_cleared_state, context_clear_submit_blocked_line,
    context_clear_submit_blocked_message, context_clear_submit_observation_line,
    context_clear_submit_resubmit_proof_line, context_clear_submit_retry_action,
    interrupt_clear_timeout_message, live_turn_clear_queued_message,
    live_turn_clear_refusal_message, operator_interrupt_key_plan, operator_interrupt_step_delay,
    pending_dispatch_clear_queued_message, protected_clear_refusal_message,
    terminal_editor_command,
};
use tmux_router::{Registry as SessionRegistry, RegistryEntry as SessionEntry, Tmux};

const SUPERVISOR_INJECT_SUBMIT_MODE: &str = "supervisor_normalized_submit";
const CLEAR_DIRECT_SUBMIT_ACCEPTANCE_TIMEOUT: Duration = Duration::from_millis(900);
// Supervisor IPC acceptance proves that the command reached the actor transport,
// not that Codex has consumed it. A real `/clear` observed on 2026-09-29 took
// roughly five seconds to replace the old conversation with a fresh prompt.
const CLEAR_SUPERVISOR_ACCEPTED_SETTLE_TIMEOUT: Duration = Duration::from_secs(10);
const CLEAR_DIRECT_SUBMIT_RENDER_TIMEOUT: Duration = Duration::from_secs(10);
const CLEAR_DIRECT_SUBMIT_ACCEPTANCE_POLL_INTERVAL: Duration = Duration::from_millis(150);
const CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS_DEFAULT: usize = 1;
const CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS_ENV: &str =
    "AGENT_DOC_CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS";
const CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED: &str =
    agent_doc_state_backbone::QUEUE_CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED;
const CONTEXT_CLEAR_SOURCE_OPERATOR_LIVE_TURN_DEFERRED: &str =
    agent_doc_state_backbone::QUEUE_CONTEXT_CLEAR_SOURCE_OPERATOR_LIVE_TURN_DEFERRED;

/// A tmux pane that has passed the `#stale-actor-pane-collision` provenance check and
/// is therefore a safe target for **injected input** (clear / interrupt / resubmit /
/// prompt dispatch). The only ways to obtain one are the provenance-gated constructors
/// below, so any function that requires `&ProvenPane` cannot be called with an
/// unchecked pane. This makes the recurring bug class behind 0.34.82 / 0.34.83 — a
/// code path that submits into a pane without verifying it is still ours (and not a
/// foreign session that reused it) — a *compile error* instead of a runtime hazard.
///
/// Prototype scope: currently threaded through the direct-submit path
/// (`resolve_direct_submit_pane` → `send_clear_to_pane`). Rolling it out to every
/// pane-input site (route dispatch, tmux submit helpers) is the follow-up that fully
/// retires the manual provenance checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProvenPane(String);

impl ProvenPane {
    pub(crate) fn pane_id(&self) -> &str {
        &self.0
    }

    /// Prove a recorded-owner pane (authoritative actor / registry): succeeds only when
    /// `recorded_owner_pane_is_safe_target` holds (reachable supervisor, or the recorded
    /// child/registry pid still lives in the pane's process tree).
    fn from_recorded_owner(
        ctx: &SessionContext,
        tmux: &Tmux,
        pane: &str,
        source: &'static str,
    ) -> Option<ProvenPane> {
        recorded_owner_pane_is_safe_target(ctx, tmux, pane, source)
            .then(|| ProvenPane(pane.to_string()))
    }

    /// A pane already proven the live owner by the sync resolver
    /// (`find_normal_path_owner_pane` verifies process provenance internally), so it is
    /// admitted directly.
    fn from_verified_live_owner(pane: String) -> ProvenPane {
        ProvenPane(pane)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirectSubmitPaneSource {
    AuthoritativeActor,
    LiveOwner,
    Registry,
}

impl DirectSubmitPaneSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::AuthoritativeActor => "authoritative_actor",
            Self::LiveOwner => "live_owner",
            Self::Registry => "registry",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartMode {
    Continue,
    Fresh,
}

impl RestartMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Fresh => "fresh",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperatorAction {
    Clear,
    Restart,
}

impl OperatorAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Clear => "session_clear",
            Self::Restart => "session_restart",
        }
    }

    fn allows_clean_exit_prompt(self) -> bool {
        matches!(self, Self::Restart)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SupervisorHealth {
    Healthy,
    Restartable,
    Halted { restart_count: u32 },
    Unreachable,
    NoSocket,
}

impl SupervisorHealth {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Restartable => "restartable",
            Self::Halted { .. } => "halted",
            Self::Unreachable => "unreachable",
            Self::NoSocket => "no_socket",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SupervisorRuntime {
    health: SupervisorHealth,
    actor_state: Option<ActorState>,
    actor_session_id: Option<String>,
    actor_pane_id: Option<String>,
    actor_generation: Option<u64>,
    supervisor_state: Option<String>,
    restart_count: u32,
    supervisor_pid: Option<u32>,
    supervisor_instance_id: Option<String>,
    child_pid: Option<u32>,
    cwd_source: Option<String>,
    /// The harness binary the supervisor is running in the pane right now
    /// (`current_harness` in its `State` reply; `None` from an older
    /// supervisor or without one).
    current_harness: Option<String>,
}

#[derive(Clone, Debug)]
struct SessionContext {
    canonical_file: PathBuf,
    base_dir: PathBuf,
    session_id: String,
    harness: String,
    actor_record: Option<ActorRecord>,
    operator_status: SessionOperatorStatus,
    registry_entry: Option<SessionEntry>,
    startup_miss: Option<StartupMiss>,
    log_status: Option<SessionLogStatus>,
    supervisor_runtime: SupervisorRuntime,
    supervisor_socket: PathBuf,
    /// The window the bound pane actually lives in *right now*, read from tmux.
    ///
    /// The actor record and the registry both carry a `window` captured at bind
    /// time; neither is re-derived, so a pane moved into `stash` keeps naming the
    /// visible window in both and every record-vs-record check still agrees.
    /// `None` when there is no bound pane or tmux could not answer — which is
    /// never treated as drift.
    live_pane_window: Option<String>,
}

fn manual_clear_cooldown_target(ctx: &SessionContext) -> String {
    ctx.supervisor_runtime
        .actor_pane_id
        .clone()
        .or_else(|| {
            ctx.actor_record
                .as_ref()
                .map(|record| record.pane_id.clone())
        })
        .or_else(|| ctx.registry_entry.as_ref().map(|entry| entry.pane.clone()))
        .filter(|pane| !pane.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn record_manual_clear_cooldown_projection(ctx: &SessionContext) -> Result<()> {
    let target = manual_clear_cooldown_target(ctx);
    agent_doc_controller_io::project_controller::queue_context_clear_manual_cooldown_for_file(
        &ctx.canonical_file,
        &target,
        &ctx.harness,
        harness_clear_command(&ctx.harness),
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LivePaneState {
    AliveIdle,
    AliveBusy,
    AliveUnobservable,
    ClosedClean,
    ProjectionStale,
    Unknown,
}

impl LivePaneState {
    fn as_str(self) -> &'static str {
        match self {
            Self::AliveIdle => "alive-idle",
            Self::AliveBusy => "alive-busy",
            Self::AliveUnobservable => "alive-unobservable",
            Self::ClosedClean => "closed-clean",
            Self::ProjectionStale => "projection-stale",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LivePaneEvidence {
    pane_id: Option<String>,
    source: &'static str,
    state: LivePaneState,
    current_command: Option<String>,
    prompt_ready: Option<bool>,
    tail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestartBusyAction {
    DeferToSupervisorHandoff,
}

fn restart_busy_action(_evidence: &LivePaneEvidence) -> RestartBusyAction {
    RestartBusyAction::DeferToSupervisorHandoff
}

pub fn status(file: &Path) -> Result<()> {
    let mut ctx = build_context(file)?;
    let tmux = agent_doc_tmux_io::configured_tmux();
    if reconcile_idle_projection_from_live_pane(&ctx, &tmux)? {
        ctx = build_context(file)?;
    }
    print_status_summary(&ctx);
    Ok(())
}

pub fn history(file: &Path) -> Result<()> {
    let ctx = build_context(file)?;
    if !ctx.operator_status.transitions.is_empty() {
        for transition in &ctx.operator_status.transitions {
            println!("{}", format_controller_transition(transition));
        }
        return Ok(());
    }

    let path = session_log_path(&ctx.base_dir, &ctx.session_id);
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        println!(
            "No session log recorded for {}",
            ctx.canonical_file.display()
        );
        return Ok(());
    };

    let interesting: Vec<&str> = content
        .lines()
        .filter(|line| {
            let event = line
                .split_once("] ")
                .map(|(_, rest)| rest)
                .unwrap_or(line)
                .trim();
            event.starts_with("ownership_transition ")
                || event.starts_with("session_start ")
                || event.starts_with("session_superseded ")
                || event.starts_with("session_end ")
        })
        .collect();

    if interesting.is_empty() {
        println!(
            "No actor transition history recorded for {}",
            ctx.canonical_file.display()
        );
        return Ok(());
    }

    for line in interesting {
        println!("{line}");
    }
    Ok(())
}

/// `#closeout-recovery-state-machine` — debug state dump for ALL actors in a
/// project, cross-referenced with each document's cycle phase and closeout
/// recovery classification, for investigating state drift. Backed by the
/// `session_actor::load_all_records_in` API. With `file = None`, scopes to the
/// current working directory's project root.
pub fn debug(file: Option<&Path>, json: bool) -> Result<()> {
    use agent_doc_turn::CyclePhase;
    let base_dir = match file {
        Some(f) => {
            let canonical = f.canonicalize().unwrap_or_else(|_| f.to_path_buf());
            agent_doc_fs::find_project_root(&canonical).with_context(|| {
                format!("failed to locate project root for {}", canonical.display())
            })?
        }
        None => {
            let cwd = std::env::current_dir().context("failed to read current directory")?;
            agent_doc_fs::find_project_root(&cwd.join("agent-doc.md")).unwrap_or(cwd)
        }
    };

    let phase_str = |phase: CyclePhase| -> &'static str {
        match phase {
            CyclePhase::PreflightStarted => "preflight_started",
            CyclePhase::ResponseCaptured => "response_captured",
            CyclePhase::WriteApplied => "write_applied",
            CyclePhase::Committed => "committed",
            CyclePhase::Abandoned => "abandoned",
        }
    };

    let store = agent_doc_session_actor_io::load_all_records_in(&base_dir)?;
    let mut rows: Vec<serde_json::Value> = Vec::with_capacity(store.len());
    for (document_id, record) in &store {
        let doc_path = Path::new(document_id);
        let cycle_phase = agent_doc_cycle_state_io::load_with_closeout_projection(doc_path)
            .ok()
            .flatten()
            .map(|s| phase_str(s.phase));
        let recovery_state = agent_doc_flow_io::closeout::classify_closeout_recovery_state_for_file(
            doc_path,
            &agent_doc_closeout_runtime_io::closeout_effects(),
        );
        let recovery_command = agent_doc_flow_io::closeout::closeout_recovery_command_for_file(
            doc_path,
            recovery_state,
        );
        let mut value = serde_json::to_value(record).unwrap_or(serde_json::Value::Null);
        if let serde_json::Value::Object(map) = &mut value {
            map.insert(
                "document_id".into(),
                serde_json::Value::String(document_id.clone()),
            );
            map.insert(
                "cycle_phase".into(),
                cycle_phase
                    .map(|p| serde_json::Value::String(p.to_string()))
                    .unwrap_or(serde_json::Value::Null),
            );
            map.insert(
                "recovery_state".into(),
                serde_json::Value::String(recovery_state.as_str().to_string()),
            );
            map.insert(
                "recovery_command".into(),
                recovery_command
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        rows.push(value);
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    if rows.is_empty() {
        println!("No actors recorded in {}", base_dir.display());
        return Ok(());
    }
    let field = |row: &serde_json::Value, key: &str| -> String {
        row.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string()
    };
    for row in &rows {
        let generation = row.get("generation").and_then(|v| v.as_u64()).unwrap_or(0);
        println!(
            "{}\n  gen={} pane={} harness={} actor={} cycle={} recovery={}",
            field(row, "document_id"),
            generation,
            field(row, "pane_id"),
            field(row, "harness"),
            field(row, "state"),
            field(row, "cycle_phase"),
            field(row, "recovery_state"),
        );
        if let Some(cmd) = row.get("recovery_command").and_then(|v| v.as_str()) {
            println!("  recovery: {cmd}");
        }
    }
    Ok(())
}

pub fn attach(file: &Path, pane: Option<&str>) -> Result<()> {
    let ctx = build_context(file)?;
    let pane_id = resolve_attach_pane(pane)?;
    let tmux = agent_doc_tmux_io::configured_tmux();
    if !tmux.pane_alive(&pane_id) {
        anyhow::bail!("tmux pane {} is not alive", pane_id);
    }
    let window = tmux
        .pane_window(&pane_id)
        .with_context(|| format!("failed to read window for pane {pane_id}"))?;
    let pid = agent_doc_tmux_io::pane_pid(&tmux, &pane_id)
        .with_context(|| format!("failed to read pane PID for {pane_id}"))?;
    agent_doc_controller_io::project_controller::attach_pane(
        &ctx.base_dir,
        agent_doc_controller_io::project_controller::AttachPaneRequest {
            file: ctx.canonical_file.clone(),
            session_id: ctx.session_id.clone(),
            pane_id: pane_id.clone(),
            window_id: window.clone(),
        },
    )?;
    agent_doc_session_registry_io::registration::attach_projection_only_in(
        &ctx.base_dir,
        &ctx.session_id,
        &pane_id,
        &ctx.canonical_file.to_string_lossy(),
        pid,
        &window,
        &ctx.base_dir.to_string_lossy(),
    )?;
    let updated = build_context(&ctx.canonical_file)?;
    let generation = updated
        .actor_record
        .as_ref()
        .map(|record| record.generation.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!(
        "Attached {} to pane {} (generation {}).",
        updated.canonical_file.display(),
        pane_id,
        generation
    );
    Ok(())
}

/// Is the caller running in the pane this restart targets?
///
/// `#restartselfpane`: an agent that runs `session restart-supervisor` for its
/// own document is, by construction, mid-turn at that moment — it is executing
/// the very command that asks.
///
/// This alone is NOT a reason to refuse. `#restartselfdefer`: with a LIVE
/// supervisor the restart is a blue/green drain-and-supersede — it waits for the
/// turn boundary and then `execve`s in place, preserving the child and the pane
/// (`restart_supervisor_drains_then_reexecs_in_place_no_dropped_turn`). A
/// self-request is perfectly safe there: the requesting turn simply finishes
/// first, which is the deferral. Refusing it would break the healthy path.
///
/// It is only fatal when there is NO supervisor to carry the deferral, because
/// the request then escalates to a cold start that must QUIT this pane — and a
/// mid-turn harness ignores the quit keys, so it can only time out. Observed
/// 2026-07-20: a 10s `live_harness_quit_timeout` whose real cause was that the
/// requester WAS the target.
///
/// `$TMUX_PANE` is the caller's own pane, so this is exact rather than heuristic.
fn restart_targets_the_calling_pane(owner_pane: Option<&str>) -> bool {
    let Some(owner_pane) = owner_pane.map(str::trim).filter(|pane| !pane.is_empty()) else {
        return false;
    };
    std::env::var("TMUX_PANE")
        .ok()
        .map(|caller| caller.trim().to_string())
        .filter(|caller| !caller.is_empty())
        .is_some_and(|caller| caller == owner_pane)
}

pub fn restart(file: &Path, mode: RestartMode, force: bool) -> Result<()> {
    restart_with_target(file, mode, force, false)
}

/// Replace the harness child while keeping the session supervisor as the owner.
///
/// The wire prefix is deliberately understood by the supervisor rather than
/// inferred from pane/process state. That keeps the operator's "Restart Agent"
/// intent durable across controller transport and distinct from a controller
/// recycle, including when the running supervisor notices build skew.
pub fn restart_agent(file: &Path, mode: RestartMode, force: bool) -> Result<()> {
    restart_with_target(file, mode, force, true)
}

fn restart_with_target(
    file: &Path,
    mode: RestartMode,
    force: bool,
    restart_agent: bool,
) -> Result<()> {
    let ctx = build_context(file)?;
    let tmux = agent_doc_tmux_io::configured_tmux();
    let owner_pane = ctx
        .actor_record
        .as_ref()
        .map(|record| record.pane_id.clone());
    if restart_targets_the_calling_pane(owner_pane.as_deref()) {
        // `#restartselfdefer`: a live supervisor drains to the turn boundary and
        // supersedes itself in place, so this turn finishing IS the deferral —
        // let it through. Only a dead supervisor forces the cold-start path that
        // has to quit this very pane, which a mid-turn harness cannot honour.
        if ctx.supervisor_runtime.health == SupervisorHealth::Healthy {
            println!(
                "[session] restart requested from this document's own pane ({}); the live supervisor will drain the current turn and supersede itself at the turn boundary — this turn is not interrupted.",
                owner_pane.as_deref().unwrap_or("?"),
            );
        } else {
            anyhow::bail!(
                "refusing to restart {}: this command is running IN that document's own pane ({}) and there is no live supervisor to defer to, so the restart would escalate to a cold start that must quit the session asking for it — and a mid-turn harness ignores the quit keys, so it can only time out. Run it from another pane, or use the editor's Restart Agent action.",
                ctx.canonical_file.display(),
                owner_pane.as_deref().unwrap_or("?"),
            );
        }
    }
    if !force {
        guard_starting_actor_operator_command(&ctx, &tmux, OperatorAction::Restart)?;
    }
    prepare_restart_live_busy_pane(&ctx, &tmux, force)?;
    let replacement_mode = if restart_agent {
        format!("agent:{}", mode.as_str())
    } else {
        mode.as_str().to_string()
    };
    let receipt = agent_doc_controller_io::project_controller::request_supervisor_replacement(
        &ctx.base_dir,
        agent_doc_controller_io::project_controller::SupervisorReplacementRequest {
            file: ctx.canonical_file.clone(),
            mode: replacement_mode,
            force,
        },
    )?;
    let target = if restart_agent { "agent" } else { "supervisor" };
    println!(
        "Requested {} {} replacement for {} via project controller (stage {}, receipt {}, background_started={}).",
        mode.as_str(),
        target,
        ctx.canonical_file.display(),
        receipt.accepted_stage,
        receipt.operator_receipt.receipt_id,
        receipt.background_started
    );
    report_supervisor_replacement_outcome(&ctx.canonical_file, receipt.background_started);
    Ok(())
}

/// `#superviserrsilent`: `background_started=true` only means the worker THREAD
/// spawned — not that a supervisor is live. The worker then runs asynchronously,
/// and when it fails (for example `preserve_pane_blocked`, because the owner pane
/// is a live agent TUI that must not be typed into) the error lands in the ops log
/// long after the operator has been told the request was accepted.
///
/// That silence is expensive rather than cosmetic: with no supervisor there is no
/// idle-queue watch, so a `queue: go` document with drainable heads stops
/// self-draining entirely and only a human can advance it. Observed live —
/// `agent-doc-bugs2.md` logged ZERO idle-watch ticks while six sibling documents
/// logged thousands each, and every replacement attempt on it ended in
/// `background_failed` while reporting success.
///
/// Replacement continues on the controller's reactive lifecycle plane; this
/// command must not poll/sleep while waiting for that asynchronously projected
/// state. The currently-live PID and binary inode are not completion proof: they
/// may describe the pre-request supervisor while the background worker later
/// rejects the replacement.
fn report_supervisor_replacement_outcome(file: &Path, background_started: bool) {
    let (warning, message) = supervisor_replacement_outcome_message(file, background_started);
    if warning {
        eprintln!("{message}");
    } else {
        println!("{message}");
    }
}

fn supervisor_replacement_outcome_message(file: &Path, background_started: bool) -> (bool, String) {
    if !background_started {
        return (
            true,
            format!(
                "[session] warning: no background replacement worker started for {}; the supervisor was NOT replaced.",
                file.display()
            ),
        );
    }
    (
        false,
        "[session] supervisor replacement accepted; completion proof is pending on the controller lifecycle receipt."
            .to_string(),
    )
}

/// "Stop Agent": kill the harness child while keeping the supervisor alive at its
/// restart-or-quit keepalive prompt. Distinct from `restart` (which auto-restarts)
/// and `admin kill-supervisor` (which kills the supervisor process). The operator
/// can restart the agent manually at the keepalive (press Enter).
pub fn stop_agent(file: &Path, reason: Option<String>) -> Result<()> {
    let ctx = build_context(file)?;
    let authorization = agent_doc_controller_io::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_stop_agent",
    )?;
    ensure_supervisor_socket(&ctx)?;
    let response = agent_doc_supervisor_io::ipc::send_command(
        &ctx.supervisor_socket,
        &IpcMethod::StopAgent { reason },
    )
    .with_context(|| {
        format!(
            "failed to contact supervisor for {}",
            ctx.canonical_file.display()
        )
    })?;
    if !response.ok {
        anyhow::bail!(
            "{}",
            response
                .error
                .unwrap_or_else(|| "supervisor stop-agent request failed".to_string())
        );
    }
    println!(
        "Stopped agent for {} (supervisor stays alive at the restart prompt; controller stage {}).",
        ctx.canonical_file.display(),
        authorization.accepted_stage
    );
    Ok(())
}

fn prepare_restart_live_busy_pane(ctx: &SessionContext, tmux: &Tmux, force: bool) -> Result<()> {
    let evidence = live_pane_evidence(ctx, tmux);
    // #hj7s: refuse if a terminal editor (e.g. Claude Code `ctrl+g` edit-in-nvim)
    // owns the pane TTY. The editor is a legitimate operator state, so do not
    // force-quit it or route restart control around it; let the operator close it
    // manually. `--force` must NOT bypass this (it runs before the force branch).
    if let Some(command) = evidence.current_command.as_deref()
        && terminal_editor_command(command)
    {
        let pane = evidence.pane_id.as_deref().unwrap_or("unknown");
        log_restart_evidence_event(ctx, "session_restart_editor_holds_pane_refused", &evidence);
        anyhow::bail!(
            "{}",
            restart_editor_holds_pane_refusal_message(
                &ctx.canonical_file,
                pane,
                evidence.source,
                command,
                evidence.tail.as_deref().unwrap_or("unknown"),
            )
        );
    }
    if !force && evidence.state != LivePaneState::AliveBusy {
        return guard_destructive_operator_on_live_busy_pane(ctx, tmux, "session_restart");
    }
    if evidence.state == LivePaneState::AliveBusy {
        if pane_shows_clean_exit_prompt(ctx, tmux, &evidence) {
            return Ok(());
        }
        match restart_busy_action(&evidence) {
            RestartBusyAction::DeferToSupervisorHandoff => {
                log_restart_evidence_event(
                    ctx,
                    "session_restart_busy_deferred_to_supervisor_handoff",
                    &evidence,
                );
            }
        }
        return Ok(());
    }
    if evidence.state == LivePaneState::AliveIdle {
        reconcile_idle_projection_from_evidence(ctx, &evidence)?;
    }
    Ok(())
}

pub fn clear(file: &Path) -> Result<()> {
    let ctx = build_context(file)?;
    let authorization = agent_doc_controller_io::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_clear",
    )?;
    let tmux = agent_doc_tmux_io::configured_tmux();
    guard_starting_actor_operator_command(&ctx, &tmux, OperatorAction::Clear)?;
    if session_clear_already_satisfied(&ctx, &tmux) {
        agent_doc_ops_log_io::log_op(
            &ctx.canonical_file,
            &format!(
                "session_clear_already_satisfied file={} reason=closed_actor_no_live_delivery_target",
                ctx.canonical_file.display()
            ),
        );
        reclaim_orphaned_cycle_after_run_cancel(
            &ctx.canonical_file,
            "session_clear_already_satisfied",
        );
        clear_durable_context_on_explicit_clear(&ctx)?;
        println!(
            "Cleared session context for {} (already no live session; controller stage {}).",
            ctx.canonical_file.display(),
            authorization.accepted_stage
        );
        return Ok(());
    }
    if reconcile_idle_projection_before_clear(&ctx, &tmux)?
        == ClearPreflightOutcome::DeferredPreempt
    {
        // A non-interrupting clear hit a busy pane driven by an active auto-queue
        // loop (`#autoloop-command-preemption` Phase 2) or a proven-live turn the
        // supervisor will see end (`#clearqueuedliveturn`). The clear is queued
        // for the next idle boundary; do NOT deliver it into the in-flight turn.
        return Ok(());
    }
    let mut restarted_fresh_from_supervisor_prompt = false;
    let mut restart_fresh_outcome = RestartFreshClearOutcome::Armed;
    if supervisor_clear_inject_available(&ctx) {
        let pre_delivery_capture_hash = ctx
            .supervisor_runtime
            .actor_pane_id
            .as_deref()
            .and_then(|pane| capture_context_clear_submit_content_hash(&tmux, pane));
        match send_clear_via_supervisor(&ctx)? {
            SupervisorClearDelivery::RestartedFresh {
                reason,
                deferred_until_cycle_closes,
                captured_cycle_crossed,
            } => {
                restarted_fresh_from_supervisor_prompt = true;
                restart_fresh_outcome = restart_fresh_clear_outcome(
                    deferred_until_cycle_closes,
                    captured_cycle_crossed,
                );
                log_restart_fresh_clear(&ctx.canonical_file, &reason, restart_fresh_outcome);
            }
            SupervisorClearDelivery::Sent => {
                agent_doc_ops_log_io::log_op(
                    &ctx.canonical_file,
                    &format!(
                        "session_clear_sent file={} delivery=supervisor_ipc submit_mode={} pane_source=supervisor_runtime",
                        ctx.canonical_file.display(),
                        SUPERVISOR_INJECT_SUBMIT_MODE
                    ),
                );
                verify_supervisor_clear_submit(
                    &ctx,
                    &tmux,
                    "supervisor_runtime",
                    pre_delivery_capture_hash.as_deref(),
                )?;
            }
            SupervisorClearDelivery::LegacyClearUnsupported { error } => {
                if !send_clear_to_resolved_pane(
                    &ctx,
                    &tmux,
                    Some("legacy_supervisor_clear_ipc_unsupported"),
                )? {
                    anyhow::bail!(
                        "supervisor does not support clear IPC and no live pane is available for direct `/clear` submission for {}: {error}",
                        ctx.canonical_file.display()
                    );
                }
            }
        }
    } else if !send_clear_to_resolved_pane(&ctx, &tmux, None)? {
        let pre_delivery_capture_hash = ctx
            .supervisor_runtime
            .actor_pane_id
            .as_deref()
            .and_then(|pane| capture_context_clear_submit_content_hash(&tmux, pane));
        match send_clear_via_supervisor(&ctx)? {
            SupervisorClearDelivery::RestartedFresh {
                reason,
                deferred_until_cycle_closes,
                captured_cycle_crossed,
            } => {
                restarted_fresh_from_supervisor_prompt = true;
                restart_fresh_outcome = restart_fresh_clear_outcome(
                    deferred_until_cycle_closes,
                    captured_cycle_crossed,
                );
                log_restart_fresh_clear(&ctx.canonical_file, &reason, restart_fresh_outcome);
            }
            SupervisorClearDelivery::Sent => {
                agent_doc_ops_log_io::log_op(
                    &ctx.canonical_file,
                    &format!(
                        "session_clear_sent file={} delivery=supervisor_ipc submit_mode={} pane_source=none",
                        ctx.canonical_file.display(),
                        SUPERVISOR_INJECT_SUBMIT_MODE
                    ),
                );
                verify_supervisor_clear_submit(
                    &ctx,
                    &tmux,
                    "none",
                    pre_delivery_capture_hash.as_deref(),
                )?;
            }
            SupervisorClearDelivery::LegacyClearUnsupported { error } => {
                anyhow::bail!(
                    "supervisor does not support clear IPC and no live pane is available for direct `/clear` submission for {}: {error}",
                    ctx.canonical_file.display()
                );
            }
        }
    }
    if !restarted_fresh_from_supervisor_prompt
        && matches!(ctx.harness.as_str(), "codex" | "opencode")
    {
        agent_doc_codex_hook_io::record_external_prompt_for_file(
            &ctx.canonical_file,
            &ctx.session_id,
            harness_clear_command(&ctx.harness),
        )?;
    }
    match record_manual_clear_cooldown_projection(&ctx) {
        Ok(()) => {
            agent_doc_ops_log_io::log_op(
                &ctx.canonical_file,
                &format!(
                    "session_clear_queue_cooldown file={} harness={} authority=projection",
                    ctx.canonical_file.display(),
                    ctx.harness
                ),
            );
        }
        Err(err) => {
            eprintln!(
                "[clear] warning: failed to record queue clear-cooldown projection for {}: {err:#}",
                ctx.canonical_file.display()
            );
            agent_doc_ops_log_io::log_op(
                &ctx.canonical_file,
                &format!(
                    "session_clear_queue_cooldown_failed file={} authority=projection error={:?}",
                    ctx.canonical_file.display(),
                    err.to_string()
                ),
            );
        }
    }
    // Explicit clear aborts the current turn, so reclaim any orphaned open
    // preflight cycle the cleared run left behind so the next Run Agent Doc
    // starts fresh instead of waiting on a stale open cycle.
    reclaim_orphaned_cycle_after_run_cancel(&ctx.canonical_file, "session_clear");
    // A clear latched only because an EMPTY preflight was still open is
    // released by the supervisor's next Ready tick once the reclaim above
    // abandoned that cycle; report it as armed rather than waiting.
    if restart_fresh_outcome == RestartFreshClearOutcome::DeferredUntilCycleCloses
        && !document_cycle_open(&ctx.canonical_file)
    {
        restart_fresh_outcome = RestartFreshClearOutcome::Armed;
    }
    clear_durable_context_on_explicit_clear(&ctx)?;
    println!(
        "{}",
        restart_fresh_clear_message(
            &ctx.canonical_file.display().to_string(),
            &ctx.harness,
            &authorization.accepted_stage.to_string(),
            restart_fresh_outcome,
        )
    );
    Ok(())
}

/// Whether the document's agent-doc cycle is still open. Fails CLOSED (open)
/// so an unreadable projection never reports a latched clear as applied.
fn document_cycle_open(file: &Path) -> bool {
    match agent_doc_cycle_state_io::load_with_closeout_projection(file) {
        Ok(state) => state.is_some_and(|state| state.is_open()),
        Err(_) => true,
    }
}

/// How a supervisor fresh-restart Clear landed (`#clearcapturedcycle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartFreshClearOutcome {
    /// Typed clear, or a fresh restart armed now.
    Armed,
    /// The fresh restart replaced the child over an open cycle whose response
    /// is durably captured; the supervisor finishes that closeout.
    CrossedCapturedCycle,
    /// Accepted but latched behind the open cycle the child still owns.
    DeferredUntilCycleCloses,
}

fn restart_fresh_clear_outcome(
    deferred_until_cycle_closes: bool,
    captured_cycle_crossed: bool,
) -> RestartFreshClearOutcome {
    if deferred_until_cycle_closes {
        RestartFreshClearOutcome::DeferredUntilCycleCloses
    } else if captured_cycle_crossed {
        RestartFreshClearOutcome::CrossedCapturedCycle
    } else {
        RestartFreshClearOutcome::Armed
    }
}

fn log_restart_fresh_clear(file: &Path, reason: &str, outcome: RestartFreshClearOutcome) {
    let event = match outcome {
        RestartFreshClearOutcome::Armed => format!(
            "session_clear_sent file={} delivery=supervisor_restart_fresh reason={reason}",
            file.display()
        ),
        RestartFreshClearOutcome::CrossedCapturedCycle => format!(
            "session_clear_sent file={} delivery=supervisor_restart_fresh reason={reason} open_cycle=captured_supervisor_closeout (#clearcapturedcycle)",
            file.display()
        ),
        RestartFreshClearOutcome::DeferredUntilCycleCloses => format!(
            "session_clear_deferred file={} delivery=supervisor_restart_fresh reason={reason} deferred_until=agent_doc_cycle_closes (#clearcapturedcycle)",
            file.display()
        ),
    };
    agent_doc_ops_log_io::log_op(file, &event);
}

/// The operator-facing line for an accepted Clear. A latched clear must never
/// read as "Cleared": on 2026-10-16 (`tasks/sdk.md`) two latched clears printed
/// "Cleared session context" and the Codex session was never replaced.
fn restart_fresh_clear_message(
    file: &str,
    harness: &str,
    accepted_stage: &str,
    outcome: RestartFreshClearOutcome,
) -> String {
    match outcome {
        RestartFreshClearOutcome::Armed => {
            format!("Cleared session context for {file} (controller stage {accepted_stage}).")
        }
        RestartFreshClearOutcome::CrossedCapturedCycle => format!(
            "Cleared session context for {file} (controller stage {accepted_stage}). The open \
             cycle's response is already durably captured; the supervisor finishes its closeout \
             without the old {harness} context, and nothing about the document was discarded. \
             The fresh {harness} session waits at its prompt instead of re-running that cycle; \
             queued work and new edits still reach it through the supervisor's idle dispatch."
        ),
        RestartFreshClearOutcome::DeferredUntilCycleCloses => format!(
            "Clear Session Context accepted for {file} but NOT applied yet (controller stage \
             {accepted_stage}): the {harness} session still owns the document's open agent-doc \
             cycle, so the supervisor replaces it automatically when that cycle closes. \
             Pressing Clear Session Context again does not make it sooner; use Interrupt and \
             clear to stop the running turn now."
        ),
    }
}

/// After an explicit clear delivers `/clear` (aborting the current turn),
/// reclaim any orphaned open preflight cycle the cleared run left behind
/// (#clear-stops-running-process, mirroring explicit run-cancel
/// #cancel-orphans-preflight-cycle). An empty `preflight_started` cycle is
/// abandoned so the next Run Agent Doc starts fresh instead of waiting on a
/// stale open cycle; a cycle that already captured a response is protected and
/// left intact. Reclaim failures are non-fatal — the clear itself already
/// delivered — and surface as a warning.
fn reclaim_orphaned_cycle_after_run_cancel(
    file: &Path,
    source: &str,
) -> agent_doc_turn::repair::CancelOutcome {
    match agent_doc_repair_io::cancel_preflight_cycle_after_run_cancel(
        &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
        file,
    ) {
        Ok(outcome) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "run_cancel_cycle_reclaim file={} source={} outcome={outcome:?}",
                    file.display(),
                    source,
                ),
            );
            outcome
        }
        Err(err) => {
            eprintln!(
                "[clear] warning: failed to reclaim orphaned cycle for {}: {err:#}",
                file.display()
            );
            agent_doc_turn::repair::CancelOutcome::NoOpenCycle
        }
    }
}

fn clear_durable_context_on_explicit_clear(ctx: &SessionContext) -> Result<()> {
    let cleared = agent_doc_dynamic_context_io::clear_durable_context_session(
        &ctx.canonical_file,
        &ctx.session_id,
    )
    .with_context(|| {
        format!(
            "clear durable dynamic-context session for {}",
            ctx.canonical_file.display()
        )
    })?;
    agent_doc_ops_log_io::log_op(
        &ctx.canonical_file,
        &format!(
            "session_clear_dynamic_context file={} session={} manifests={} injections={}",
            ctx.canonical_file.display(),
            ctx.session_id,
            cleared.manifests,
            cleared.injections,
        ),
    );
    Ok(())
}

fn supervisor_clear_inject_available(ctx: &SessionContext) -> bool {
    matches!(ctx.supervisor_runtime.health, SupervisorHealth::Healthy)
        && ctx.supervisor_socket.exists()
}

fn session_clear_already_satisfied(ctx: &SessionContext, tmux: &Tmux) -> bool {
    session_clear_already_satisfied_facts(
        ctx.actor_record.as_ref().map(|record| record.state),
        supervisor_clear_inject_available(ctx),
        resolve_direct_submit_pane(ctx, tmux).is_some(),
    )
}

fn session_clear_already_satisfied_facts(
    actor_state: Option<ActorState>,
    supervisor_inject_available: bool,
    direct_submit_pane_available: bool,
) -> bool {
    actor_state == Some(ActorState::Closed)
        && !supervisor_inject_available
        && !direct_submit_pane_available
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SupervisorClearDelivery {
    /// The supervisor replaced the child instead of typing the clear
    /// (`#idleclearrestart`: no live child, `waiting_input`, or a harness whose
    /// clear exits the process). `reason` is the supervisor's restart reason.
    RestartedFresh {
        reason: String,
        /// `#clearcapturedcycle`: the supervisor latched the restart behind an
        /// open cycle the child still owns; nothing was replaced yet.
        deferred_until_cycle_closes: bool,
        /// The restart crossed an open cycle whose response is durably captured.
        captured_cycle_crossed: bool,
    },
    Sent,
    LegacyClearUnsupported {
        error: String,
    },
}

fn send_clear_via_supervisor(ctx: &SessionContext) -> Result<SupervisorClearDelivery> {
    ensure_supervisor_socket(ctx)?;
    // Use the gate-exempt `Clear` control method so an operator can clear a
    // session whose managed-capability proof failed without `kill -9`
    // (#codex-capability-proof-unrecoverable). `Inject` would be refused by the
    // dispatch gate.
    let response = agent_doc_supervisor_io::ipc::send_command(
        &ctx.supervisor_socket,
        &IpcMethod::Clear {
            bytes: agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(
                harness_clear_command(&ctx.harness),
            )
            .to_string(),
        },
    )
    .with_context(|| {
        format!(
            "failed to contact supervisor for {}",
            ctx.canonical_file.display()
        )
    })?;
    if !response.ok {
        let error = response
            .error
            .unwrap_or_else(|| "supervisor inject request failed".to_string());
        if supervisor_clear_legacy_unsupported_error(&error) {
            return Ok(SupervisorClearDelivery::LegacyClearUnsupported { error });
        }
        anyhow::bail!("{error}");
    }
    let data = response.data.as_ref();
    if data
        .and_then(|data| data.get("restart_fresh"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        // Older supervisors answered every idle clear with a restart and the
        // reason `supervisor_idle_prompt` / `supervisor_waiting_input`.
        let reason = data
            .and_then(|data| data.get("reason"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let flag = |key: &str| {
            data.and_then(|data| data.get(key))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        return Ok(SupervisorClearDelivery::RestartedFresh {
            reason,
            deferred_until_cycle_closes: flag("deferred_until_cycle_closes"),
            captured_cycle_crossed: flag("captured_cycle_crossed"),
        });
    }
    Ok(SupervisorClearDelivery::Sent)
}

fn supervisor_clear_legacy_unsupported_error(error: &str) -> bool {
    error.contains("parse error: unknown variant `clear`") && error.contains("expected one of")
}

fn verify_supervisor_clear_submit(
    ctx: &SessionContext,
    tmux: &Tmux,
    pane_source: &str,
    pre_delivery_capture_hash: Option<&str>,
) -> Result<()> {
    let Some(pane) = ctx.supervisor_runtime.actor_pane_id.as_deref() else {
        agent_doc_ops_log_io::log_op(
            &ctx.canonical_file,
            &format!(
                "session_clear_submit_verification_skipped file={} harness={} delivery=supervisor_ipc pane_source={} reason=no_supervisor_actor_pane",
                ctx.canonical_file.display(),
                ctx.harness,
                pane_source
            ),
        );
        return Ok(());
    };
    verify_context_clear_submit_after_delivery(ContextClearSubmitVerification {
        tmux,
        pane,
        file: &ctx.canonical_file,
        harness: &ctx.harness,
        command: harness_clear_command(&ctx.harness),
        initial_phase: "supervisor_ipc_acceptance",
        delivery_evidence: ContextClearDeliveryEvidence::SupervisorAccepted,
        resubmit_source: "session_clear.supervisor_ipc_resubmit",
        pre_delivery_capture_hash,
    })
}

fn send_clear_to_resolved_pane(
    ctx: &SessionContext,
    tmux: &Tmux,
    fallback_reason: Option<&str>,
) -> Result<bool> {
    let Some((pane, pane_source)) = resolve_direct_submit_pane(ctx, tmux) else {
        return Ok(false);
    };
    send_clear_to_pane(tmux, &pane, &ctx.canonical_file, &ctx.harness)?;
    let pane = pane.pane_id();
    let submit_mode = tmux_submit_mode_for_harness(&ctx.harness);
    let fallback_suffix = fallback_reason
        .map(|reason| format!(" fallback_reason={reason}"))
        .unwrap_or_default();
    agent_doc_ops_log_io::log_op(
        &ctx.canonical_file,
        &format!(
            "session_clear_sent file={} pane={} delivery=direct_pane_submit submit_mode={} pane_source={}{}",
            ctx.canonical_file.display(),
            pane,
            submit_mode,
            pane_source.as_str(),
            fallback_suffix
        ),
    );
    Ok(true)
}

/// Outcome of the pre-delivery clear projection reconcile. `Proceed` means the
/// caller should deliver the clear normally; `DeferredPreempt` means a
/// non-interrupting clear hit a busy pane — driven by an active auto-queue loop
/// (`#autoloop-command-preemption` Phase 2), or running a proven-live harness
/// turn its supervisor can observe end (`#clearqueuedliveturn`) — and one clear
/// is queued for the supervisor to deliver at the idle boundary. It must NOT be
/// delivered into the in-flight turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClearPreflightOutcome {
    Proceed,
    DeferredPreempt,
}

fn reconcile_idle_projection_before_clear(
    ctx: &SessionContext,
    tmux: &Tmux,
) -> Result<ClearPreflightOutcome> {
    let mut evidence = resolve_direct_submit_pane(ctx, tmux)
        .map(|(pane, source)| {
            live_pane_evidence_for_pane(ctx, tmux, pane.pane_id().to_string(), source.as_str())
        })
        .unwrap_or_else(|| live_pane_evidence(ctx, tmux));
    // `#cancelturnprompt`: a cancelled routed prompt that Claude Code put back
    // in the composer is agent-doc's own trigger, not operator input. Withdraw
    // it before classifying, so the clear neither types `/clear` onto it nor
    // waits on the abandoned prompt's lease for the settle window.
    if evidence.state == LivePaneState::AliveBusy
        && let Some(pane) = evidence.pane_id.clone()
    {
        let harness = harness_for_evidence(ctx, &evidence);
        if harness.binary == "claude"
            && withdraw_cancelled_trigger(
                ctx,
                tmux,
                &pane,
                &harness,
                Duration::ZERO,
                "session_clear",
            ) == crate::cancelled_trigger::WithdrawOutcome::Withdrawn
        {
            evidence = live_pane_evidence_for_pane(ctx, tmux, pane, evidence.source);
        }
    }
    let protected_reason = if evidence.state == LivePaneState::AliveBusy {
        protected_clear_input_reason(ctx, tmux, &evidence)
    } else {
        None
    };
    let clean_exit_prompt = evidence.state == LivePaneState::AliveBusy
        && pane_shows_clean_exit_prompt(ctx, tmux, &evidence);
    let busy_reason = if evidence.state == LivePaneState::AliveBusy {
        operator_clear_busy_reason(ctx, tmux, &evidence)
    } else {
        None
    };
    // GH #232: consult the supervisor's live-turn
    // evidence BEFORE any input is sent. A composer that renders between tool
    // calls read as `idle_prompt`, the guard then projected the actor `ready`
    // (releasing the dispatch in-flight marker) and typed `/clear` into the
    // busy harness, where it queued behind the turn.
    let harness_turn_live = harness_turn_live_evidence(ctx, &evidence);
    let clear_state = operator_clear_input_state_for_evidence(
        &evidence,
        protected_reason.is_some(),
        clean_exit_prompt,
        busy_reason.is_some(),
        harness_turn_live.is_some(),
    );
    agent_doc_flow_io::log_flow_event(
        &ctx.canonical_file,
        clear_guard_event(clear_state),
        agent_doc_ops_log_io::log_op,
    );

    match clear_state {
        OperatorClearInputState::IdlePrompt => {
            reconcile_idle_projection_from_evidence(ctx, &evidence)?;
        }
        OperatorClearInputState::CleanExit | OperatorClearInputState::NoLivePane => {}
        OperatorClearInputState::ProtectedInput => {
            if let Some(reason) = protected_reason {
                let log_reason = reason.replace(char::is_whitespace, "_");
                agent_doc_ops_log_io::log_op(
                    &ctx.canonical_file,
                    &format!(
                        "{} file={} pane={} source={} reason={} current_command={} tail={:?}",
                        OpsLogEvent::SessionClearProtectedInputGuardRefused,
                        ctx.canonical_file.display(),
                        evidence.pane_id.as_deref().unwrap_or("unknown"),
                        evidence.source,
                        log_reason,
                        evidence.current_command.as_deref().unwrap_or("unknown"),
                        evidence.tail.as_deref().unwrap_or("unknown")
                    ),
                );
                anyhow::bail!(
                    "{}",
                    protected_clear_refusal_message(
                        &ctx.canonical_file,
                        evidence.pane_id.as_deref(),
                        evidence.source,
                        evidence.current_command.as_deref(),
                        evidence.tail.as_deref(),
                        &reason,
                    )
                );
            }
        }
        OperatorClearInputState::Busy | OperatorClearInputState::LiveTurn => {
            let live_turn_evidence = (clear_state == OperatorClearInputState::LiveTurn)
                .then_some(harness_turn_live)
                .flatten();
            let busy_reason = match live_turn_evidence {
                Some(evidence) => format!("harness_turn_live evidence={evidence}"),
                None => busy_reason.unwrap_or_else(|| "busy cue".to_string()),
            };
            // `#autoloop-command-preemption` Phase 2: a non-interrupting clear on
            // a doc whose pane is busy *because an `agent:queue auto` loop keeps
            // dispatching* never finds a quiet window, so the old hard block made
            // the command unrunnable. When such a loop is active, pause it (the
            // durable clear cooldown the idle-queue watch already honors) and
            // defer the clear to the next inter-iteration idle gap instead of
            // killing the in-flight turn. A busy pane with no active loop keeps
            // the existing fail-closed block — there is nothing to preempt.
            let queue_active =
                agent_doc_document_realtime_io::try_resolve_current_document_content(
                    &ctx.canonical_file,
                    "session_actor_busy_clear_queue_continuation",
                )
                .ok()
                .and_then(|content| {
                    agent_doc_queue_io::queue_continuation::detect_for_content(
                        &ctx.canonical_file,
                        &content,
                    )
                    .unwrap_or(None)
                })
                .is_some();
            let pending_deferred_clear =
                agent_doc_controller_io::project_controller::queue_context_clear_deferred_operator_for_file(
                    &ctx.canonical_file,
                )?;
            let deferred_clear_pending = pending_deferred_clear.is_some();
            let pending_is_live_turn_deferral = pending_deferred_clear
                .as_ref()
                .is_some_and(|projection| projection.is_live_turn_deferred_operator_clear());
            // `#clearqueuedliveturn`: a proven-live harness turn ends, and the
            // document's supervisor is the actor that observes that end. When it
            // owns the evidence pane, queue the clear behind the turn instead of
            // refusing; without it nothing would ever deliver a queued clear, so
            // the refusal stays.
            let live_turn_deliverable = live_turn_evidence.is_some()
                && live_turn_clear_deliverable_by_supervisor(
                    supervisor_clear_inject_available(ctx),
                    ctx.supervisor_runtime.actor_pane_id.as_deref(),
                    evidence.pane_id.as_deref(),
                );
            match agent_doc_queue::queue_preemption::plan_busy_clear(
                queue_active,
                deferred_clear_pending,
                pending_is_live_turn_deferral,
                live_turn_deliverable,
            ) {
                outcome @ (agent_doc_queue::queue_preemption::BusyClearOutcome::DeferBehindLiveTurn
                | agent_doc_queue::queue_preemption::BusyClearOutcome::RearmBehindLiveTurn) => {
                    let rearmed = outcome
                        == agent_doc_queue::queue_preemption::BusyClearOutcome::RearmBehindLiveTurn;
                    let pane = evidence.pane_id.as_deref().unwrap_or("unknown");
                    let live_evidence = live_turn_evidence.unwrap_or("unknown");
                    // `#clearctxslow`: a lease whose prompt Claude Code never
                    // recorded is not a running turn; it is waiting out the
                    // abandoned-prompt settle window. Same queue, same wait —
                    // but say so, and say when the clear runs.
                    let settling = settling_abandoned_prompt_for_clear(ctx, pane, live_evidence);
                    let logged_evidence = match &settling {
                        Some(settling) => format!(
                            "{ABANDONED_PROMPT_SETTLING_EVIDENCE} settles_in_secs={}",
                            settling.settles_in_secs
                        ),
                        None => live_evidence.to_string(),
                    };
                    agent_doc_controller_io::project_controller::queue_context_clear_deferred_for_file(
                        &ctx.canonical_file,
                        pane,
                        &ctx.harness,
                        harness_clear_command(&ctx.harness),
                        CONTEXT_CLEAR_SOURCE_OPERATOR_LIVE_TURN_DEFERRED,
                        None,
                    )?;
                    agent_doc_ops_log_io::log_op(
                        &ctx.canonical_file,
                        &format!(
                            "session_clear_live_turn_deferred file={} pane={} source={} evidence={} rearmed={} ui_outcome=queued (#clearqueuedliveturn)",
                            ctx.canonical_file.display(),
                            pane,
                            evidence.source,
                            logged_evidence,
                            rearmed,
                        ),
                    );
                    let message = match settling {
                        Some(settling) => abandoned_prompt_clear_queued_message(
                            &ctx.canonical_file,
                            evidence.pane_id.as_deref(),
                            evidence.source,
                            settling.settles_in_secs,
                            agent_doc_turn::turn_status::ABANDONED_PROMPT_SETTLE_SECS,
                            rearmed,
                        ),
                        // `#clearaftrun`: Run Agent Doc accepted, trigger not yet a
                        // turn. Nothing is running; say so, and say the clear
                        // runs after that run rather than claiming a live turn.
                        None if live_evidence == DISPATCH_TURN_PENDING_EVIDENCE => {
                            pending_dispatch_clear_queued_message(
                                &ctx.canonical_file,
                                evidence.pane_id.as_deref(),
                                evidence.source,
                                live_turn_evidence_age_secs(ctx, pane, live_evidence),
                                agent_doc_sqlite::state_store::DISPATCH_PRE_TURN_GRACE_SECS
                                    .unsigned_abs(),
                                rearmed,
                            )
                        }
                        None => live_turn_clear_queued_message(
                            &ctx.canonical_file,
                            evidence.pane_id.as_deref(),
                            evidence.source,
                            live_evidence,
                            live_turn_evidence_age_secs(ctx, pane, live_evidence),
                            rearmed,
                        ),
                    };
                    eprintln!("{message}");
                    return Ok(ClearPreflightOutcome::DeferredPreempt);
                }
                agent_doc_queue::queue_preemption::BusyClearOutcome::PauseAndDefer => {
                    // Record the deferred clear so the supervisor pauses the loop,
                    // delivers it at the next idle gap, and resumes
                    // (`#autoloop-command-preemption` Phase 2b). The controller
                    // projection is the durable hand-off between this command path
                    // and the supervisor watch thread.
                    agent_doc_controller_io::project_controller::queue_context_clear_deferred_for_file(
                        &ctx.canonical_file,
                        evidence.pane_id.as_deref().unwrap_or("unknown"),
                        &ctx.harness,
                        harness_clear_command(&ctx.harness),
                        CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED,
                        None,
                    )?;
                    agent_doc_ops_log_io::log_op(
                        &ctx.canonical_file,
                        &format!(
                            "session_clear_queue_preempt_deferred file={} pane={} source={} reason={} current_command={} tail={:?}",
                            ctx.canonical_file.display(),
                            evidence.pane_id.as_deref().unwrap_or("unknown"),
                            evidence.source,
                            busy_reason.replace(char::is_whitespace, "_"),
                            evidence.current_command.as_deref().unwrap_or("unknown"),
                            evidence.tail.as_deref().unwrap_or("unknown")
                        ),
                    );
                    eprintln!(
                        "{}",
                        busy_clear_deferred_message(
                            &ctx.canonical_file,
                            evidence.pane_id.as_deref()
                        )
                    );
                    return Ok(ClearPreflightOutcome::DeferredPreempt);
                }
                agent_doc_queue::queue_preemption::BusyClearOutcome::AlreadyDeferred => {
                    agent_doc_ops_log_io::log_op(
                        &ctx.canonical_file,
                        &format!(
                            "session_clear_queue_preempt_already_deferred file={} pane={} source={} reason={} current_command={} tail={:?}",
                            ctx.canonical_file.display(),
                            evidence.pane_id.as_deref().unwrap_or("unknown"),
                            evidence.source,
                            busy_reason.replace(char::is_whitespace, "_"),
                            evidence.current_command.as_deref().unwrap_or("unknown"),
                            evidence.tail.as_deref().unwrap_or("unknown")
                        ),
                    );
                    eprintln!(
                        "{}",
                        busy_clear_already_deferred_message(
                            &ctx.canonical_file,
                            evidence.pane_id.as_deref(),
                            live_turn_evidence,
                        )
                    );
                    return Ok(ClearPreflightOutcome::DeferredPreempt);
                }
                agent_doc_queue::queue_preemption::BusyClearOutcome::HardBlock => {
                    agent_doc_ops_log_io::log_op(
                        &ctx.canonical_file,
                        &format!(
                            "{} file={} pane={} source={} reason={} current_command={} tail={:?}",
                            OpsLogEvent::SessionClearLiveBusyGuardBlocked,
                            ctx.canonical_file.display(),
                            evidence.pane_id.as_deref().unwrap_or("unknown"),
                            evidence.source,
                            busy_reason.replace(char::is_whitespace, "_"),
                            evidence.current_command.as_deref().unwrap_or("unknown"),
                            evidence.tail.as_deref().unwrap_or("unknown")
                        ),
                    );
                    if let Some(live_evidence) = live_turn_evidence {
                        anyhow::bail!(
                            "{}",
                            live_turn_clear_refusal_message(
                                &ctx.canonical_file,
                                evidence.pane_id.as_deref(),
                                evidence.source,
                                live_evidence,
                            )
                        );
                    }
                    anyhow::bail!(
                        "{}",
                        busy_clear_refusal_message(
                            &ctx.canonical_file,
                            evidence.pane_id.as_deref(),
                            evidence.source,
                            evidence.current_command.as_deref(),
                            evidence.tail.as_deref(),
                            &busy_reason,
                        )
                    );
                }
            }
        }
    }
    Ok(ClearPreflightOutcome::Proceed)
}

/// `#clearqueuedliveturn`: can the document's supervisor deliver a clear
/// queued behind the live turn? It must be healthy and reachable, and it must
/// own the very pane the live-turn evidence names — the supervisor watch only
/// ever delivers into its own pane, so a clear queued for any other pane would
/// never run.
fn live_turn_clear_deliverable_by_supervisor(
    supervisor_inject_available: bool,
    supervisor_pane: Option<&str>,
    evidence_pane: Option<&str>,
) -> bool {
    supervisor_inject_available
        && matches!((supervisor_pane, evidence_pane), (Some(owned), Some(live)) if owned == live)
}

/// GH #232: the live-turn evidence the supervisor uses
/// (`harness_turn_live`), read for the operator-clear guard. Returns the kind of
/// evidence that proved the turn live, or `None`.
///
/// - `turn_active_lease`: the harness-authored `UserPromptSubmit` turn-active
///   lease for the evidence pane — the exact read the supervisor's
///   `turn_active_for_owned_pane_with_idle_evidence` makes (including its
///   `#staleharnessturnlive` interrupted-transcript retirement and TTL expiry).
/// - `dispatch_turn_running`: an observed, unsettled dispatch receipt for this
///   document (`#idledispatchstack`), which covers the window between a
///   dispatched trigger going busy and the harness hook writing its lease.
///
/// `#sdkclearwedge`: a lease the harness transcript proves ended (an operator
/// interrupt, or a prompt cancelled before Claude Code persisted it) is retired
/// first, and the dispatch receipt its preflight promoted to `running` is
/// settled with it — a route dispatch never moves the actor, so no `Ready` edge
/// would. Background subagents (`← N agents`) are NOT a live foreground turn:
/// they write sidechain records and no lease, so they never appear here.
///
/// Read-only with respect to actor state: nothing here projects `ready` or
/// releases the dispatch in-flight marker.
fn harness_turn_live_evidence(
    ctx: &SessionContext,
    evidence: &LivePaneEvidence,
) -> Option<&'static str> {
    // `#queuedclearflake`: an unobservable viewport (a blank frame, e.g. while a
    // starting supervisor has not begun relaying its child's screen) is still a
    // live pane, and harness evidence does not depend on what the pane renders.
    if !matches!(
        evidence.state,
        LivePaneState::AliveIdle | LivePaneState::AliveBusy | LivePaneState::AliveUnobservable
    ) {
        return None;
    }
    if let Some(pane) = evidence.pane_id.as_deref() {
        retire_harness_ended_turn_for_clear(ctx, pane);
    }
    if let Some(pane) = evidence.pane_id.as_deref()
        && agent_doc_turn_status_io::turn_active_for_pane(&ctx.base_dir, pane)
    {
        return Some("turn_active_lease");
    }
    if agent_doc_controller_io::project_controller::dispatch_turn_running_for_file(
        &ctx.canonical_file,
    )
    .unwrap_or(false)
    {
        return Some("dispatch_turn_running");
    }
    // `#clearqueuedliveturn`: a dispatch accepted for this document whose turn
    // has not started yet (Run Agent Doc pressed, trigger not yet submitted or
    // not yet past its `UserPromptSubmit` hooks). A clear typed now would race
    // the trigger into the composer; queue it behind that turn instead. Bounded
    // by the pre-turn grace, so a lost trigger cannot hold clears.
    agent_doc_controller_io::project_controller::dispatch_turn_pending_for_file(&ctx.canonical_file)
        .unwrap_or(false)
        .then_some(DISPATCH_TURN_PENDING_EVIDENCE)
}

/// `#clearqueuedliveturn`: how old the live-turn evidence a queued clear waits
/// on is — the turn lease's `UserPromptSubmit` second, or the oldest open
/// dispatch receipt — for the operator notice.
/// `#clearctxslow`: the settling abandoned prompt behind a queued clear's
/// `turn_active_lease` evidence, if that is what the lease is. Read-only: the
/// settle window and the supervisor's delivery are unchanged.
fn settling_abandoned_prompt_for_clear(
    ctx: &SessionContext,
    pane: &str,
    live_evidence: &str,
) -> Option<agent_doc_turn_status_io::SettlingAbandonedPrompt> {
    settling_abandoned_prompt_for_clear_at(
        ctx,
        pane,
        live_evidence,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default(),
    )
}

/// [`settling_abandoned_prompt_for_clear`] evaluated at an explicit `now`.
fn settling_abandoned_prompt_for_clear_at(
    ctx: &SessionContext,
    pane: &str,
    live_evidence: &str,
    now: u64,
) -> Option<agent_doc_turn_status_io::SettlingAbandonedPrompt> {
    (live_evidence == "turn_active_lease")
        .then(|| {
            agent_doc_turn_status_io::settling_abandoned_prompt_for_pane_for_file_at(
                &ctx.canonical_file,
                pane,
                now,
            )
        })
        .flatten()
}

fn live_turn_evidence_age_secs(ctx: &SessionContext, pane: &str, evidence: &str) -> Option<u64> {
    let since = match evidence {
        "turn_active_lease" => agent_doc_turn_status_io::turn_active_marker_for_pane_for_file(
            &ctx.canonical_file,
            pane,
        )
        .map(|marker| marker.written_at),
        _ => agent_doc_controller_io::project_controller::oldest_open_dispatch_turn_at_for_file(
            &ctx.canonical_file,
        )
        .ok()
        .flatten(),
    }?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(now.saturating_sub(since))
}

/// `#sdkclearwedge`: retire `pane`'s turn-active lease when the harness
/// transcript proves the turn ended, and settle that turn's `running` dispatch
/// receipt. Logs the repair; a no-op when no boundary is proven.
fn retire_harness_ended_turn_for_clear(ctx: &SessionContext, pane: &str) {
    let Some(retired) = agent_doc_turn_status_io::reclaim_interrupted_turn_for_pane_for_file(
        &ctx.canonical_file,
        pane,
    ) else {
        return;
    };
    // `#infraclearsession`: the retirement already settled the pane's receipt;
    // this file-scoped settle covers a document with no `documents.pane_id`
    // row yet and is otherwise a no-op.
    let settled = retired.settled_dispatches
        + agent_doc_controller_io::project_controller::settle_dispatch_turn_ended_by_harness_for_file(
            &ctx.canonical_file,
            retired.written_at,
        )
        .unwrap_or(0);
    agent_doc_ops_log_io::log_op(
        &ctx.canonical_file,
        &format!(
            "turn_status_projection_repaired file={} pane={} reason={} source=operator_clear_guard lease_written_at={} interrupted_at={} settled_dispatches={} (#sdkclearwedge)",
            ctx.canonical_file.display(),
            retired.pane,
            retired.reason,
            retired.written_at,
            retired.interrupted_at,
            settled,
        ),
    );
}

fn operator_clear_input_state_for_evidence(
    evidence: &LivePaneEvidence,
    protected_input: bool,
    clean_exit_prompt: bool,
    busy_cue: bool,
    harness_turn_live: bool,
) -> OperatorClearInputState {
    match evidence.state {
        // GH #232: harness-authored live-turn evidence
        // outranks a rendered prompt. Claude Code redraws its composer between
        // tool calls while the turn runs.
        LivePaneState::AliveIdle if harness_turn_live => OperatorClearInputState::LiveTurn,
        LivePaneState::AliveIdle => OperatorClearInputState::IdlePrompt,
        LivePaneState::ClosedClean | LivePaneState::ProjectionStale | LivePaneState::Unknown => {
            OperatorClearInputState::NoLivePane
        }
        // `#queuedclearflake`: a blank or too-small viewport hides the composer,
        // not the turn. Proven live-turn evidence queues the clear behind the
        // turn exactly as for a rendered frame; without it the viewport stays
        // protected. Before this, a clear pressed while a starting supervisor had
        // not yet relayed its child's frame was sent straight into the running
        // dispatched turn.
        LivePaneState::AliveUnobservable if harness_turn_live => OperatorClearInputState::LiveTurn,
        LivePaneState::AliveUnobservable => OperatorClearInputState::ProtectedInput,
        LivePaneState::AliveBusy if protected_input => OperatorClearInputState::ProtectedInput,
        LivePaneState::AliveBusy if clean_exit_prompt => OperatorClearInputState::CleanExit,
        LivePaneState::AliveBusy if harness_turn_live => OperatorClearInputState::LiveTurn,
        LivePaneState::AliveBusy if busy_cue => OperatorClearInputState::Busy,
        // Some harnesses leave a wrapper process (`agent-doc`, `codex`, etc.)
        // as the pane command even when the visible TUI is only idle/status
        // chrome. Do not block clear solely on that process name; protected
        // input and explicit busy cues are handled above.
        LivePaneState::AliveBusy => OperatorClearInputState::IdlePrompt,
    }
}

fn guard_starting_actor_operator_command(
    ctx: &SessionContext,
    tmux: &Tmux,
    action: OperatorAction,
) -> Result<()> {
    if action == OperatorAction::Clear {
        return Ok(());
    }

    if !operator_command_has_starting_actor(ctx) {
        return Ok(());
    }

    let evidence = live_pane_evidence(ctx, tmux);
    let dirty = document_dirty_after_committed_cycle(&ctx.canonical_file)?;
    let clean_exit_prompt = pane_shows_clean_exit_prompt(ctx, tmux, &evidence);
    let dispatch_ready =
        evidence.state == LivePaneState::AliveIdle && evidence.prompt_ready == Some(true);
    if dispatch_ready && !dirty {
        reconcile_idle_projection_from_evidence(ctx, &evidence)?;
        return Ok(());
    }

    if action.allows_clean_exit_prompt() && clean_exit_prompt && !dirty {
        return Ok(());
    }

    let reason = starting_operator_guard_reason(action, dirty, dispatch_ready, clean_exit_prompt);
    agent_doc_ops_log_io::log_op(
        &ctx.canonical_file,
        &format!(
            "session_operator_starting_guard_refused kind={} file={} pane={} source={} reason={} actor_state={} supervisor_state={} lease_state={} prompt_ready={} tail={:?}",
            action.as_str(),
            ctx.canonical_file.display(),
            evidence.pane_id.as_deref().unwrap_or("unknown"),
            evidence.source,
            reason.replace(char::is_whitespace, "_"),
            ctx.actor_record
                .as_ref()
                .map(|record| record.state.as_str())
                .unwrap_or("none"),
            ctx.supervisor_runtime
                .actor_state
                .map(|state| state.as_str())
                .unwrap_or("unknown"),
            ctx.operator_status
                .supervisor_lease
                .as_ref()
                .and_then(|lease| lease.runtime_state.as_deref())
                .unwrap_or("unknown"),
            evidence.prompt_ready.unwrap_or(false),
            evidence.tail.as_deref().unwrap_or("unknown")
        ),
    );
    anyhow::bail!(
        "{}",
        starting_operator_guard_refusal_message(action, &ctx.canonical_file, &reason)
    )
}

fn starting_operator_guard_reason(
    action: OperatorAction,
    dirty: bool,
    dispatch_ready: bool,
    clean_exit_prompt: bool,
) -> String {
    if dirty {
        return "the document changed after the last committed cycle".to_string();
    }
    if clean_exit_prompt && !action.allows_clean_exit_prompt() {
        return "the pane is at a clean-exit restart prompt, not a dispatch-ready composer"
            .to_string();
    }
    if dispatch_ready {
        return "the ready prompt could not be reconciled".to_string();
    }
    "the pane has not reached a dispatch-ready prompt (`prompt_ready=true`)".to_string()
}

fn starting_operator_guard_refusal_message(
    action: OperatorAction,
    file: &Path,
    reason: &str,
) -> String {
    let mut message = format!(
        "{} refused for {} because the authoritative actor is still starting and {}. Wait for a dispatch-ready prompt (`prompt_ready=true`) and retry, or run `agent-doc session status {}` to inspect the pane.",
        action.as_str(),
        file.display(),
        reason,
        file.display()
    );
    if action == OperatorAction::Restart {
        message.push_str(
            " Pass `--force` to bypass the starting-state guard and request a supervisor-mediated restart.",
        );
    }
    message
}

#[cfg(test)]
pub(crate) fn restart_starting_refusal_message(file: &Path, reason: &str) -> String {
    starting_operator_guard_refusal_message(OperatorAction::Restart, file, reason)
}

fn operator_command_has_starting_actor(ctx: &SessionContext) -> bool {
    let Some(record) = ctx.actor_record.as_ref() else {
        return false;
    };
    if supervisor_runtime_applies_to_record(ctx)
        && let Some(state) = ctx.supervisor_runtime.actor_state
    {
        return state == ActorState::Starting;
    }
    if let Some(state) = ctx
        .operator_status
        .supervisor_lease
        .as_ref()
        .filter(|lease| lease.generation == record.generation)
        .and_then(|lease| lease.runtime_state.as_deref())
    {
        return state == ActorState::Starting.as_str();
    }
    record.state == ActorState::Starting
}

fn supervisor_runtime_matches_record(ctx: &SessionContext) -> bool {
    let Some(record) = ctx.actor_record.as_ref() else {
        return false;
    };
    ctx.supervisor_runtime.actor_session_id.as_deref() == Some(record.session_id.as_str())
        && ctx.supervisor_runtime.actor_pane_id.as_deref() == Some(record.pane_id.as_str())
        && ctx.supervisor_runtime.actor_generation == Some(record.generation)
}

fn supervisor_runtime_applies_to_record(ctx: &SessionContext) -> bool {
    if supervisor_runtime_matches_record(ctx) {
        return true;
    }
    ctx.supervisor_runtime.actor_session_id.is_none()
        && ctx.supervisor_runtime.actor_pane_id.is_none()
        && ctx.supervisor_runtime.actor_generation.is_none()
        && matches!(ctx.supervisor_runtime.health, SupervisorHealth::Healthy)
}

fn document_dirty_after_committed_cycle(file: &Path) -> Result<bool> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(false);
    };
    if state.phase != agent_doc_turn::CyclePhase::Committed {
        return Ok(true);
    }
    let Some(hash) = state.file_hash.as_deref() else {
        return Ok(false);
    };
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "session_actor_dirty_check_document",
    )
    .with_context(|| format!("failed to resolve {}", file.display()))?;
    Ok(agent_doc_hash::content_hash(&content) != hash)
}

fn protected_clear_input_reason(
    ctx: &SessionContext,
    tmux: &Tmux,
    evidence: &LivePaneEvidence,
) -> Option<String> {
    let pane = evidence.pane_id.as_deref()?;
    let captured = agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane)
        .or_else(|_| agent_doc_tmux_io::capture_pane(tmux, pane))
        .ok()?;
    let harness = harness_for_evidence(ctx, evidence);
    if let Some(reason) = harness.protected_prompt_input_reason(&captured) {
        return Some(reason);
    }
    // `#cancelturnprompt`: Claude's composer projection reports operator text
    // that `protected_prompt_input_reason` (Codex-shaped) does not. Typing
    // `/clear` onto it would submit one concatenated line, the same hazard the
    // route refuses as `#concatdraftinject`. agent-doc's own restored trigger
    // was already withdrawn above, so anything left is the operator's.
    if harness.binary == "claude" {
        let cursor_y = agent_doc_tmux_io::pane_cursor_y(tmux, pane);
        if let agent_doc_harness::PaneComposerProjection::OperatorDraft { .. } =
            agent_doc_harness::project_pane_composer_at_cursor(&captured, &harness, cursor_y)
        {
            return Some("drafted prompt input".to_string());
        }
    }
    None
}

fn pane_shows_clean_exit_prompt(
    ctx: &SessionContext,
    tmux: &Tmux,
    evidence: &LivePaneEvidence,
) -> bool {
    let Some(pane) = evidence.pane_id.as_deref() else {
        return false;
    };
    let Ok(captured) = agent_doc_tmux_io::capture_pane(tmux, pane) else {
        return false;
    };
    let harness = harness_for_evidence(ctx, evidence);
    harness.dispatch_blocker_reason(&captured).as_deref() == Some("clean-exit restart prompt")
}

fn operator_clear_busy_reason(
    ctx: &SessionContext,
    tmux: &Tmux,
    evidence: &LivePaneEvidence,
) -> Option<String> {
    let pane = evidence.pane_id.as_deref()?;
    let captured = agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane)
        .or_else(|_| agent_doc_tmux_io::capture_pane(tmux, pane))
        .ok()?;
    let harness = harness_for_evidence(ctx, evidence);
    let reason = harness.dispatch_blocker_reason(&captured)?;
    match reason.as_str() {
        "active permission prompt"
        | "queued draft in composer"
        | "interactive shell reverse-i-search"
        | "interactive shell history search"
        | "clean-exit restart prompt" => None,
        _ => Some(reason),
    }
}

fn harness_for_evidence(
    ctx: &SessionContext,
    evidence: &LivePaneEvidence,
) -> agent_doc_harness::HarnessConfig {
    evidence
        .current_command
        .as_deref()
        .and_then(agent_doc_harness::HarnessConfig::from_pane_command)
        .unwrap_or_else(|| agent_doc_harness::HarnessConfig::from_agent_name(&ctx.harness))
}

/// Pure decision for `session cancel-turn`: interrupt the running turn, or
/// no-op when the harness is idle. The whole safety contract lives here — when
/// no turn is active the command must NOT send the harness interrupt sequence,
/// because in Codex/OpenCode a `Ctrl+C` (and in Claude Code a second `Ctrl+C`)
/// at an idle prompt closes the agent. Keeping the active/idle branch in a pure
/// function makes that guard provable without a live PTY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CancelTurnAction {
    /// A turn is in flight; deliver the harness-aware interrupt sequence.
    Interrupt,
    /// The harness is idle; do nothing (sending an interrupt would close it).
    NoOpIdle,
}

impl CancelTurnAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Interrupt => "interrupt_active_turn",
            Self::NoOpIdle => "noop_idle",
        }
    }
}

/// Decide what `cancel_turn` should do from the single fact "is a turn active".
/// `turn_active` is read from the `turn_status` turn-active marker (the same
/// marker `start::turn_active_for_owned_pane` consults). Active → interrupt;
/// idle → no-op.
pub(crate) fn cancel_turn_action(turn_active: bool) -> CancelTurnAction {
    if turn_active {
        CancelTurnAction::Interrupt
    } else {
        CancelTurnAction::NoOpIdle
    }
}

/// True when a `turn_status` turn-active marker is present for `base_dir` and
/// (when a pane can be resolved) belongs to this document's owned pane. Mirrors
/// `start::turn_active_for_owned_pane`: the marker is per-project-root, so we
/// require the marker pane to match the document's authoritative/registered
/// pane when one is known. When no owned pane can be resolved, any non-expired
/// marker counts (the marker itself self-expires via its TTL).
fn document_turn_active(ctx: &SessionContext) -> bool {
    let owned_pane = ctx
        .actor_record
        .as_ref()
        .filter(|record| record.state != ActorState::Closed)
        .map(|record| record.pane_id.clone())
        .or_else(|| ctx.registry_entry.as_ref().map(|entry| entry.pane.clone()));
    match owned_pane {
        Some(pane) if !pane.is_empty() => {
            agent_doc_turn_status_io::turn_active_for_pane(&ctx.base_dir, &pane)
        }
        _ => agent_doc_turn_status_io::turn_active(&ctx.base_dir),
    }
}

/// "Cancel Turn": interrupt the CURRENTLY-RUNNING turn, but stay a no-op when
/// the harness is idle. Sending the harness interrupt while idle would close
/// the agent (Codex/OpenCode `Ctrl+C` at idle, Claude Code's second idle
/// `Ctrl+C`), so the idle branch must never deliver keys. This also makes
/// repeated calls safe: the first call settles the pane to idle, and every
/// later call sees no active turn and no-ops. Unlike `interrupt-clear`, this
/// only interrupts — it does NOT clear context.
pub fn cancel_turn(file: &Path) -> Result<()> {
    let ctx = build_context(file)?;
    agent_doc_controller_io::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_cancel_turn",
    )?;
    let tmux = agent_doc_tmux_io::configured_tmux();
    let turn_active = document_turn_active(&ctx);
    match cancel_turn_action(turn_active) {
        CancelTurnAction::NoOpIdle => {
            agent_doc_ops_log_io::log_op(
                &ctx.canonical_file,
                &format!(
                    "cancel_turn_noop file={} reason=idle_no_active_turn harness={}",
                    ctx.canonical_file.display(),
                    ctx.harness
                ),
            );
            println!(
                "No active turn to cancel for {} (harness is idle; not sending an interrupt).",
                ctx.canonical_file.display()
            );
            Ok(())
        }
        CancelTurnAction::Interrupt => {
            let evidence = live_pane_evidence(&ctx, &tmux);
            let Some(pane) = evidence.pane_id.as_deref() else {
                // The marker says a turn is active but no pane can be resolved —
                // there is nothing to interrupt. Stay safe: do not blind-send keys.
                agent_doc_ops_log_io::log_op(
                    &ctx.canonical_file,
                    &format!(
                        "cancel_turn_noop file={} reason=active_marker_no_live_pane harness={} action={}",
                        ctx.canonical_file.display(),
                        ctx.harness,
                        CancelTurnAction::Interrupt.as_str()
                    ),
                );
                println!(
                    "Turn-active marker present for {} but no live pane to interrupt; nothing sent.",
                    ctx.canonical_file.display()
                );
                return Ok(());
            };
            if !tmux.pane_alive(pane) {
                agent_doc_ops_log_io::log_op(
                    &ctx.canonical_file,
                    &format!(
                        "cancel_turn_noop file={} pane={} reason=pane_already_closed harness={}",
                        ctx.canonical_file.display(),
                        pane,
                        ctx.harness
                    ),
                );
                println!(
                    "Pane {} for {} is already closed; nothing to cancel.",
                    pane,
                    ctx.canonical_file.display()
                );
                return Ok(());
            }
            let codex_shell_search = codex_pane_in_shell_search_state(&ctx, &tmux, &evidence);
            send_operator_interrupt_sequence(&tmux, pane, &ctx.harness, codex_shell_search)?;
            reclaim_orphaned_cycle_after_run_cancel(&ctx.canonical_file, "session_cancel_turn");
            agent_doc_ops_log_io::log_op(
                &ctx.canonical_file,
                &format!(
                    "cancel_turn_performed file={} action={} harness={} pane={} source={}",
                    ctx.canonical_file.display(),
                    CancelTurnAction::Interrupt.as_str(),
                    ctx.harness,
                    pane,
                    evidence.source
                ),
            );
            let harness = harness_for_evidence(&ctx, &evidence);
            if harness.binary == "claude" {
                withdraw_cancelled_trigger(
                    &ctx,
                    &tmux,
                    pane,
                    &harness,
                    crate::cancelled_trigger::RESTORE_WINDOW,
                    "session_cancel_turn",
                );
            }
            println!(
                "Cancelled the active turn for {} (interrupted pane {}; context preserved).",
                ctx.canonical_file.display(),
                pane
            );
            Ok(())
        }
    }
}

/// `#cancelturnprompt`: Claude Code restores a prompt cancelled before it was
/// persisted into the composer. When that prompt is exactly this document's
/// agent-doc trigger, withdraw it (agent-doc typed it; the operator asked to
/// cancel it) and retire the abandoned prompt's turn-active lease as soon as
/// its snapshot proves it. Operator text, a busy pane, a route submit in
/// flight, or an unobservable pane are left untouched.
fn withdraw_cancelled_trigger(
    ctx: &SessionContext,
    tmux: &Tmux,
    pane: &str,
    harness: &agent_doc_harness::HarnessConfig,
    window: Duration,
    source: &'static str,
) -> crate::cancelled_trigger::WithdrawOutcome {
    use crate::cancelled_trigger::{
        SNAPSHOT_WINDOW, TmuxCancelledTriggerPane, WithdrawOutcome, retire_withdrawn_prompt_turn,
        trigger_candidates, withdraw_restored_trigger,
    };
    let triggers = trigger_candidates(harness, &ctx.canonical_file, &ctx.base_dir);
    let trigger_refs = triggers.iter().map(String::as_str).collect::<Vec<_>>();
    let mut tmux_pane = TmuxCancelledTriggerPane {
        tmux,
        pane,
        file: &ctx.canonical_file,
        harness,
        source: "session.withdraw_cancelled_trigger",
    };
    let outcome = withdraw_restored_trigger(&mut tmux_pane, &trigger_refs, window);
    if window.is_zero() && outcome == WithdrawOutcome::NotRestored {
        // The clear guard's single look found nothing restored: the common
        // case, not worth an ops-log line.
        return outcome;
    }
    let detail = match &outcome {
        WithdrawOutcome::Withdrawn => format!(
            " {}",
            retire_withdrawn_prompt_turn(&ctx.canonical_file, pane, SNAPSHOT_WINDOW, &mut |d| {
                std::thread::sleep(d)
            })
        ),
        WithdrawOutcome::OperatorDraftKept { preview } => format!(" draft_preview={preview:?}"),
        WithdrawOutcome::SendFailed { error } => format!(" error={error:?}"),
        _ => String::new(),
    };
    agent_doc_ops_log_io::log_op(
        &ctx.canonical_file,
        &format!(
            "cancelled_trigger_composer file={} pane={} source={} outcome={}{} (#cancelturnprompt)",
            ctx.canonical_file.display(),
            pane,
            source,
            outcome.as_str(),
            detail,
        ),
    );
    if outcome == WithdrawOutcome::Withdrawn {
        println!(
            "Withdrew the cancelled `{}` trigger that Claude Code restored into pane {}'s prompt.",
            ctx.canonical_file.display(),
            pane
        );
    }
    outcome
}

pub fn interrupt_clear(file: &Path, force: bool) -> Result<()> {
    if force {
        return force_interrupt_clear(file);
    }

    let ctx = build_context(file)?;
    agent_doc_controller_io::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_interrupt_clear",
    )?;
    // The explicit interrupt-clear is the destructive path and runs now, so it
    // supersedes any clear the non-interrupting path deferred to the idle gap —
    // settle the deferred-clear projection so the supervisor watch does not ALSO
    // deliver a second clear (`#autoloop-command-preemption` Phase 2b).
    if let Err(err) =
        agent_doc_controller_io::project_controller::clear_queue_context_clear_deferred_for_file(
            &ctx.canonical_file,
        )
    {
        eprintln!(
            "[interrupt-clear] warning: failed to settle deferred-clear projection for {}: {err:#}",
            ctx.canonical_file.display()
        );
    }
    let tmux = agent_doc_tmux_io::configured_tmux();
    let evidence = live_pane_evidence(&ctx, &tmux);
    let pane = evidence
        .pane_id
        .as_deref()
        .with_context(|| format!("no live pane evidence for {}", ctx.canonical_file.display()))?;
    if !tmux.pane_alive(pane) {
        agent_doc_ops_log_io::log_op(
            &ctx.canonical_file,
            &format!(
                "session_interrupt_clear_skip_interrupt file={} pane={} reason=already_closed",
                ctx.canonical_file.display(),
                pane
            ),
        );
        return clear(file);
    }

    match interrupt_clear_initial_action(&evidence) {
        InterruptClearInitialAction::SkipInterruptAlreadyIdle => {
            let _ = reconcile_idle_projection_from_evidence(&ctx, &evidence);
            agent_doc_ops_log_io::log_op(
                &ctx.canonical_file,
                &format!(
                    "session_interrupt_clear_skip_interrupt file={} pane={} reason=already_idle prompt_ready={} tail={:?}",
                    ctx.canonical_file.display(),
                    pane,
                    evidence.prompt_ready.unwrap_or(false),
                    evidence.tail.as_deref().unwrap_or("unknown")
                ),
            );
            return clear(file);
        }
        InterruptClearInitialAction::SendInterrupt => {}
    }

    let codex_shell_search = codex_pane_in_shell_search_state(&ctx, &tmux, &evidence);
    send_operator_interrupt_sequence(&tmux, pane, &ctx.harness, codex_shell_search)?;
    let outcome = wait_for_interrupt_clear_settle(&ctx, &tmux, pane, Duration::from_secs(10));
    agent_doc_ops_log_io::log_op(
        &ctx.canonical_file,
        &format!(
            "session_interrupt_clear_settled file={} pane={} harness={} outcome={} editor_recovery_attempted={} blocking_state={} blocking_source={} prompt_ready={} last_command={} tail={:?}",
            ctx.canonical_file.display(),
            pane,
            ctx.harness,
            outcome.as_str(),
            outcome.editor_recovery_attempted(),
            outcome.blocking_state(),
            outcome.blocking_source(),
            outcome.prompt_ready(),
            outcome.last_command().unwrap_or("unknown"),
            outcome.tail().unwrap_or("none")
        ),
    );
    match outcome {
        InterruptClearSettleOutcome::Idle | InterruptClearSettleOutcome::Closed => clear(file),
        InterruptClearSettleOutcome::TimedOut {
            evidence,
            editor_recovery_attempted,
        } => anyhow::bail!(
            "{}",
            interrupt_clear_timeout_message(InterruptClearTimeoutFacts {
                file: &ctx.canonical_file,
                pane,
                state: evidence.state.as_str(),
                source: evidence.source,
                current_command: evidence.current_command.as_deref(),
                prompt_ready: evidence.prompt_ready,
                tail: evidence.tail.as_deref(),
                editor_recovery_attempted,
            })
        ),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ForceInterruptClearReport {
    actor_closed: bool,
    registry_removed: bool,
    supervisor_signaled: bool,
    child_signaled: bool,
    pane_killed: bool,
    socket_removed: bool,
    cooldown_written: bool,
}

fn force_interrupt_clear(file: &Path) -> Result<()> {
    let ctx = build_context(file)?;
    if let Err(err) = agent_doc_controller_io::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_interrupt_clear",
    ) {
        eprintln!(
            "[interrupt-clear --force] warning: operator authorization failed for {}: {err:#}; continuing with explicit force cleanup",
            ctx.canonical_file.display()
        );
        agent_doc_ops_log_io::log_op(
            &ctx.canonical_file,
            &format!(
                "session_interrupt_clear_force_authorization_failed file={} error={:?}",
                ctx.canonical_file.display(),
                err.to_string()
            ),
        );
    }

    let tmux = agent_doc_tmux_io::configured_tmux();
    let evidence = live_pane_evidence(&ctx, &tmux);
    let pane = force_interrupt_clear_pane(&ctx, &evidence);

    let mut report = ForceInterruptClearReport {
        actor_closed: force_close_actor_record(&ctx),
        registry_removed: force_remove_registry_entry(&ctx)?,
        ..ForceInterruptClearReport::default()
    };

    if let Err(err) =
        agent_doc_controller_io::project_controller::clear_queue_context_clear_deferred_for_file(
            &ctx.canonical_file,
        )
    {
        eprintln!(
            "[interrupt-clear --force] warning: failed to settle deferred-clear projection for {}: {err:#}",
            ctx.canonical_file.display()
        );
    }
    match record_manual_clear_cooldown_projection(&ctx) {
        Ok(()) => report.cooldown_written = true,
        Err(err) => eprintln!(
            "[interrupt-clear --force] warning: failed to record queue clear-cooldown projection for {}: {err:#}",
            ctx.canonical_file.display()
        ),
    }

    let child_pid = ctx.supervisor_runtime.child_pid;
    let supervisor_pid = ctx
        .supervisor_runtime
        .supervisor_pid
        .or_else(|| ctx.registry_entry.as_ref().map(|entry| entry.pid))
        .filter(|pid| *pid > 0);
    report.child_signaled = signal_pid_for_force_clear(&ctx.canonical_file, "child", child_pid);
    report.supervisor_signaled =
        signal_pid_for_force_clear(&ctx.canonical_file, "supervisor", supervisor_pid);
    report.pane_killed = kill_pane_for_force_clear(&ctx.canonical_file, &tmux, pane.as_deref());
    report.socket_removed = remove_supervisor_socket_for_force_clear(&ctx);

    reclaim_orphaned_cycle_after_run_cancel(&ctx.canonical_file, "session_interrupt_clear_force");
    let dynamic_context_clear = clear_durable_context_on_explicit_clear(&ctx);

    agent_doc_ops_log_io::log_op(
        &ctx.canonical_file,
        &format!(
            "session_interrupt_clear_force_cleanup file={} pane={} actor_closed={} registry_removed={} supervisor_pid={} supervisor_signaled={} child_pid={} child_signaled={} pane_killed={} socket_removed={} cooldown_written={}",
            ctx.canonical_file.display(),
            pane.as_deref().unwrap_or("none"),
            report.actor_closed,
            report.registry_removed,
            supervisor_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "none".to_string()),
            report.supervisor_signaled,
            child_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "none".to_string()),
            report.child_signaled,
            report.pane_killed,
            report.socket_removed,
            report.cooldown_written,
        ),
    );
    dynamic_context_clear?;
    println!(
        "{}",
        force_interrupt_clear_summary(&ctx.canonical_file, pane.as_deref(), report)
    );
    Ok(())
}

fn force_interrupt_clear_pane(ctx: &SessionContext, evidence: &LivePaneEvidence) -> Option<String> {
    evidence
        .pane_id
        .clone()
        .or_else(|| {
            ctx.actor_record
                .as_ref()
                .map(|record| record.pane_id.clone())
        })
        .or_else(|| ctx.registry_entry.as_ref().map(|entry| entry.pane.clone()))
}

fn force_close_actor_record(ctx: &SessionContext) -> bool {
    let Some(record) = &ctx.actor_record else {
        return false;
    };
    if record.state == ActorState::Closed {
        return true;
    }
    match agent_doc_controller_io::project_controller::mark_lifecycle(
        &ctx.base_dir,
        agent_doc_controller_io::project_controller::LifecycleRequest {
            file: ctx.canonical_file.clone(),
            session_id: record.session_id.clone(),
            pane_id: record.pane_id.clone(),
            generation: record.generation,
            state: ActorState::Closed,
            caller: "session_interrupt_clear".to_string(),
            reason: "force_interrupt_clear".to_string(),
            expected_revision: None,
        },
    ) {
        Ok(_) => true,
        Err(err) => {
            eprintln!(
                "[interrupt-clear --force] warning: failed to mark actor closed for {}: {err:#}",
                ctx.canonical_file.display()
            );
            agent_doc_ops_log_io::log_op(
                &ctx.canonical_file,
                &format!(
                    "session_interrupt_clear_force_actor_close_failed file={} error={:?}",
                    ctx.canonical_file.display(),
                    err.to_string()
                ),
            );
            false
        }
    }
}

fn force_remove_registry_entry(ctx: &SessionContext) -> Result<bool> {
    let registry_path = agent_doc_session_registry_io::registry_path_in(&ctx.base_dir);
    let _lock = tmux_router::RegistryLock::acquire(&registry_path)?;
    let mut registry = agent_doc_session_registry_io::load_in(&ctx.base_dir)?;
    let canonical = ctx.canonical_file.to_string_lossy().to_string();
    let mut session_ids = BTreeSet::new();
    session_ids.insert(ctx.session_id.clone());
    if let Some(record) = &ctx.actor_record {
        session_ids.insert(record.session_id.clone());
    }
    if let Some(entry) = &ctx.registry_entry {
        session_ids.insert(entry.session_id.clone());
    }
    let remove_keys: Vec<String> = registry
        .iter()
        .filter(|(key, entry)| {
            *key == &canonical || entry.file == canonical || session_ids.contains(&entry.session_id)
        })
        .map(|(key, _)| key.clone())
        .collect();
    let removed = !remove_keys.is_empty();
    for key in remove_keys {
        registry.remove(&key);
    }
    if removed {
        agent_doc_session_registry_io::save_in(&ctx.base_dir, &registry)?;
    }
    Ok(removed)
}

#[cfg(unix)]
fn signal_pid_for_force_clear(file: &Path, kind: &str, pid: Option<u32>) -> bool {
    let Some(pid) = pid.filter(|pid| *pid > 0) else {
        return false;
    };
    let signaled = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) == 0 };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_interrupt_clear_force_signal file={} kind={} pid={} signaled={}",
            file.display(),
            kind,
            pid,
            signaled
        ),
    );
    signaled
}

#[cfg(not(unix))]
fn signal_pid_for_force_clear(file: &Path, kind: &str, pid: Option<u32>) -> bool {
    if let Some(pid) = pid {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "session_interrupt_clear_force_signal file={} kind={} pid={} signaled=false platform=non_unix",
                file.display(),
                kind,
                pid
            ),
        );
    }
    false
}

fn kill_pane_for_force_clear(file: &Path, tmux: &Tmux, pane: Option<&str>) -> bool {
    let Some(pane) = pane else {
        return false;
    };
    if !tmux.pane_alive(pane) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "session_interrupt_clear_force_kill_pane file={} pane={} killed=false reason=already_closed",
                file.display(),
                pane
            ),
        );
        return false;
    }
    let killed = tmux.raw_cmd(&["kill-pane", "-t", pane]).is_ok();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_interrupt_clear_force_kill_pane file={} pane={} killed={}",
            file.display(),
            pane,
            killed
        ),
    );
    killed
}

fn remove_supervisor_socket_for_force_clear(ctx: &SessionContext) -> bool {
    if !ctx.supervisor_socket.exists() {
        return false;
    }
    match std::fs::remove_file(&ctx.supervisor_socket) {
        Ok(()) => true,
        Err(err) => {
            eprintln!(
                "[interrupt-clear --force] warning: failed to remove supervisor socket {}: {err:#}",
                ctx.supervisor_socket.display()
            );
            false
        }
    }
}

fn force_interrupt_clear_summary(
    file: &Path,
    pane: Option<&str>,
    report: ForceInterruptClearReport,
) -> String {
    format!(
        "Force-cleared session for {} (pane={}, actor_closed={}, registry_removed={}, supervisor_signaled={}, child_signaled={}, pane_killed={}, socket_removed={}, cooldown_written={}).",
        file.display(),
        pane.unwrap_or("none"),
        report.actor_closed,
        report.registry_removed,
        report.supervisor_signaled,
        report.child_signaled,
        report.pane_killed,
        report.socket_removed,
        report.cooldown_written,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InterruptClearInitialAction {
    SendInterrupt,
    SkipInterruptAlreadyIdle,
}

fn interrupt_clear_initial_action(evidence: &LivePaneEvidence) -> InterruptClearInitialAction {
    if evidence.state == LivePaneState::AliveIdle && evidence.prompt_ready == Some(true) {
        InterruptClearInitialAction::SkipInterruptAlreadyIdle
    } else {
        InterruptClearInitialAction::SendInterrupt
    }
}

/// The harness-native command that clears session context (starts a fresh
/// conversation). Claude Code and Codex use `/clear`; OpenCode has **no
/// `/clear` command** — its equivalent is `/new` (`session_new`, "Create a new
/// session"). Submitting `/clear` to an OpenCode pane is a no-op, which is why
/// `Clear Session Context` did nothing for OpenCode-backed documents
/// (#opencode-clear-uses-new). Keep this aligned with the harness slash-command
/// surfaces in `harness.rs` and the session/tmux command spec.
fn harness_clear_command(harness: &str) -> &'static str {
    agent_doc_harness::HarnessConfig::from_agent_name(harness).context_clear_command()
}

fn send_clear_to_pane(tmux: &Tmux, pane: &ProvenPane, file: &Path, harness: &str) -> Result<()> {
    // `ProvenPane` guarantees this pane passed the collision provenance check; shadow to
    // the raw id for the existing send/logging below.
    let pane = pane.pane_id();
    let command = harness_clear_command(harness);
    let pre_delivery_capture_hash = capture_context_clear_submit_content_hash(tmux, pane);
    agent_doc_tmux_io::send_submitted_text_for_harness_logged(
        tmux,
        pane,
        command,
        harness,
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
        "sessions.send_submitted_text_for_harness",
    )
    .with_context(|| {
        format!(
            "failed to send `{command}` to authoritative pane {} for {}",
            pane,
            file.display()
        )
    })?;
    verify_context_clear_submit_after_delivery(ContextClearSubmitVerification {
        tmux,
        pane,
        file,
        harness,
        command,
        initial_phase: "direct_pane_acceptance",
        delivery_evidence: ContextClearDeliveryEvidence::DirectPane,
        resubmit_source: "session_clear.direct_pane_resubmit",
        pre_delivery_capture_hash: pre_delivery_capture_hash.as_deref(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextClearDeliveryEvidence {
    /// The supervisor accepted the command for delivery. This is transport
    /// evidence only, so allow the harness enough time to consume `/clear` and
    /// render its replacement prompt before declaring the submit unobserved.
    SupervisorAccepted,
    /// The command was submitted directly to the pane. Preserve the short
    /// stuck-draft detection window used by the direct recovery path.
    DirectPane,
}

impl ContextClearDeliveryEvidence {
    const fn settle_timeout(self) -> Duration {
        match self {
            Self::SupervisorAccepted => CLEAR_SUPERVISOR_ACCEPTED_SETTLE_TIMEOUT,
            Self::DirectPane => CLEAR_DIRECT_SUBMIT_ACCEPTANCE_TIMEOUT,
        }
    }
}

struct ContextClearSubmitVerification<'a> {
    tmux: &'a Tmux,
    pane: &'a str,
    file: &'a Path,
    harness: &'a str,
    command: &'a str,
    initial_phase: &'a str,
    delivery_evidence: ContextClearDeliveryEvidence,
    #[allow(dead_code)]
    resubmit_source: &'a str,
    pre_delivery_capture_hash: Option<&'a str>,
}

fn verify_context_clear_submit_after_delivery(
    ctx: ContextClearSubmitVerification<'_>,
) -> Result<()> {
    let first = poll_context_clear_submit_acceptance(
        &ctx,
        ctx.initial_phase,
        ctx.pre_delivery_capture_hash,
        ctx.delivery_evidence.settle_timeout(),
    );
    let mut final_phase = ctx.initial_phase;
    let mut final_observation = first;
    let profile_allows_pending_draft_enter_resubmit =
        agent_doc_tmux_commands::tmux_submit_profile_for_harness(ctx.harness)
            .pending_draft_enter_resubmit();
    let max_attempts = clear_direct_submit_max_enter_resubmits();
    let mut attempts_sent = 0usize;
    while let Some(retry_action) = context_clear_submit_retry_action(ContextClearSubmitRetryFacts {
        observation: final_observation,
        pending_draft_enter_resubmit: profile_allows_pending_draft_enter_resubmit,
        attempts_sent,
        max_attempts,
    }) {
        attempts_sent += 1;
        let resubmit_phase = match (ctx.initial_phase, retry_action) {
            ("supervisor_ipc_acceptance", ContextClearSubmitRetryAction::SubmitKey) => {
                "supervisor_ipc_resubmit_acceptance"
            }
            ("supervisor_ipc_acceptance", ContextClearSubmitRetryAction::ResendCommand) => {
                "supervisor_ipc_command_repair_acceptance"
            }
            (_, ContextClearSubmitRetryAction::SubmitKey) => "direct_pane_resubmit_acceptance",
            (_, ContextClearSubmitRetryAction::ResendCommand) => {
                "direct_pane_command_repair_acceptance"
            }
        };
        let submit_key = agent_doc_tmux_commands::tmux_submit_key_for_harness(ctx.harness);
        let retry_text = match retry_action {
            ContextClearSubmitRetryAction::SubmitKey => "",
            ContextClearSubmitRetryAction::ResendCommand => ctx.command,
        };
        let retry_mode = match retry_action {
            ContextClearSubmitRetryAction::SubmitKey => "clear_resubmit_submit_key",
            ContextClearSubmitRetryAction::ResendCommand => "clear_resubmit_full_command",
        };
        let pre_resubmit_capture_hash =
            capture_context_clear_submit_content_hash(ctx.tmux, ctx.pane);
        agent_doc_tmux_io::input_diag::log_text_submit(
            agent_doc_tmux_io::input_diag::InputDiagSink::new(
                Some(ctx.file),
                agent_doc_ops_log_io::log_op,
            ),
            ctx.resubmit_source,
            &format!("pane:{}", ctx.pane),
            retry_text,
            Some(ctx.harness),
            retry_mode,
            submit_key,
        );
        if let Err(err) = agent_doc_tmux_io::send_submitted_text_for_harness_logged(
            ctx.tmux,
            ctx.pane,
            retry_text,
            ctx.harness,
            agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
            "sessions.send_submitted_text_for_harness",
        ) {
            eprintln!(
                "[clear] warning: {} clear {} failed for pane {}: {err}",
                ctx.harness,
                retry_action.as_str(),
                ctx.pane
            );
        }
        let second = poll_context_clear_submit_acceptance(
            &ctx,
            resubmit_phase,
            pre_resubmit_capture_hash.as_deref(),
            ContextClearDeliveryEvidence::DirectPane.settle_timeout(),
        );
        agent_doc_ops_log_io::log_op(
            ctx.file,
            &context_clear_submit_resubmit_proof_line(
                ctx.file.display(),
                ctx.pane,
                ctx.harness,
                ContextClearSubmitRetryProofFacts {
                    action: retry_action,
                    submit_key,
                    attempt: attempts_sent,
                    max_attempts,
                    observation: second,
                },
            ),
        );
        final_phase = resubmit_phase;
        final_observation = second;
    }
    require_context_clear_submit_accepted(
        ctx.file,
        ctx.pane,
        ctx.harness,
        ctx.command,
        final_phase,
        final_observation,
    )?;
    Ok(())
}

fn clear_direct_submit_max_enter_resubmits_from_env_value(value: Option<&str>) -> usize {
    value
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS_DEFAULT)
}

fn clear_direct_submit_max_enter_resubmits() -> usize {
    let value = std::env::var(CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS_ENV).ok();
    clear_direct_submit_max_enter_resubmits_from_env_value(value.as_deref())
}

/// (`#clearsubmitunobserved`) Resolve an unobserved clear against the pane's
/// scrollback instead of reporting an unknown.
///
/// Returns [`ContextClearSubmitStatus::AcceptedClearedState`] only when the
/// capture succeeds AND proves the pane holds no conversation. Every other
/// path — capture error, retained scrollback, no settled prompt — keeps the
/// original `Unobserved` verdict, so this can only ever turn a false block into
/// a pass, never a real failure into a pass.
fn upgrade_unobserved_clear_from_pane_history(
    tmux: &Tmux,
    pane: &str,
    file: &Path,
    harness: &str,
    command: &str,
    phase: &str,
) -> ContextClearSubmitStatus {
    let harness_config = agent_doc_harness::HarnessConfig::from_agent_name(harness);
    let history = match agent_doc_tmux_io::capture_pane_history(tmux, pane) {
        Ok(history) => history,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "session_clear_submit_cleared_state_probe file={} pane={pane} harness={harness} phase={phase} result=capture_failed error={err}",
                    file.display()
                ),
            );
            return ContextClearSubmitStatus::Unobserved;
        }
    };
    // Only Codex has a viewport-only proof; Claude Code's lives in the
    // scrollback tail (`#claudeclearfreshchat`), which also covers a one-row
    // stashed pane whose viewport shows a single row.
    let viewport = if harness_config.binary == "codex" {
        agent_doc_tmux_io::capture_pane(tmux, pane).ok()
    } else {
        None
    };
    let verdict =
        unobserved_clear_history_verdict(&harness_config, &history, viewport.as_deref(), command);
    let detail = match verdict.probe {
        UnobservedClearProbe::ClearedStateProven | UnobservedClearProbe::ConversationRetained => {
            format!(
                " retained_lines={} max_lines={}",
                verdict.retained_lines, CONTEXT_CLEAR_CLEARED_STATE_MAX_HISTORY_LINES
            )
        }
        UnobservedClearProbe::FreshChatProven => {
            format!(" retained_lines={}", verdict.retained_lines)
        }
        UnobservedClearProbe::HarnessQueuedInput | UnobservedClearProbe::PaneRestarting => {
            String::new()
        }
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_clear_submit_cleared_state_probe file={} pane={pane} harness={harness} harness_binary={} phase={phase} result={}{detail}",
            file.display(),
            harness_config.binary,
            verdict.probe.as_str(),
        ),
    );
    verdict.status
}

/// What the unobserved-clear probe found in the pane (the `result=` of
/// `session_clear_submit_cleared_state_probe`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnobservedClearProbe {
    HarnessQueuedInput,
    PaneRestarting,
    /// `#clearfreshchatproof` / `#claudeclearfreshchat`: the harness's
    /// fresh-chat shape is the latest thing in the pane.
    FreshChatProven,
    /// The whole scrollback is short enough to hold no conversation.
    ClearedStateProven,
    ConversationRetained,
}

impl UnobservedClearProbe {
    const fn as_str(self) -> &'static str {
        match self {
            Self::HarnessQueuedInput => "harness_queued_input",
            Self::PaneRestarting => "pane_restarting",
            Self::FreshChatProven => "fresh_chat_proven",
            Self::ClearedStateProven => "cleared_state_proven",
            Self::ConversationRetained => "conversation_retained",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UnobservedClearVerdict {
    status: ContextClearSubmitStatus,
    probe: UnobservedClearProbe,
    retained_lines: usize,
}

/// The pure decision behind [`upgrade_unobserved_clear_from_pane_history`]:
/// classify an unobserved clear from the pane's scrollback (and, for Codex,
/// its viewport) using the RESOLVED harness's predicates. Dispatches on
/// `harness_config.binary`, never on the raw harness label, so a session whose
/// label resolved to `default` still gets the Claude Code proofs its
/// `HarnessConfig` already implies (gh-266).
fn unobserved_clear_history_verdict(
    harness_config: &agent_doc_harness::HarnessConfig,
    history: &str,
    viewport: Option<&str>,
    command: &str,
) -> UnobservedClearVerdict {
    let retained_lines = history
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let verdict = |status, probe| UnobservedClearVerdict {
        status,
        probe,
        retained_lines,
    };
    let is_ready = |line: &str| harness_config.is_dispatch_ready_prompt_line(line);
    let is_chrome = |line: &str| harness_config.is_ignorable_output_line(line);
    // `#clearqueuedcomposer`: ask the cheaper, more specific question first. A
    // composer showing `❯ Press up to edit queued messages` proves the harness is
    // mid-turn and QUEUED the command rather than running it. That is a known
    // state with an exact unblocker, not the unknown `Unobserved` reports — and
    // it is why a resend 2s later lands in the same queue instead of clearing.
    // Only the ACTIVE composer counts: stale queued rows in scrollback are not
    // the pane's state (gh-266).
    if context_clear_capture_shows_queued_input(history, is_chrome, |line| {
        harness_config.is_queued_input_placeholder_line(line)
    }) {
        return verdict(
            ContextClearSubmitStatus::HarnessQueuedInput,
            UnobservedClearProbe::HarnessQueuedInput,
        );
    }
    if agent_doc_turn_executor_tmux::context_clear::context_clear_history_shows_pane_restarting(
        history,
        &harness_config.binary,
        is_ready,
    ) {
        return verdict(
            ContextClearSubmitStatus::PaneRestarting,
            UnobservedClearProbe::PaneRestarting,
        );
    }
    // `#clearfreshchatproof`: tmux scrollback keeps the old conversation after
    // `/clear` starts a new chat — for Codex and Claude Code alike — so the
    // history-length proof below can never pass on a long session. The latest
    // chat can: Codex paints a fresh chat's `Context 0% used` footer, or the
    // new-chat banner beside the previous chat's `codex resume` exit summary;
    // Claude Code repaints its banner and echoes `/clear` above an empty
    // composer (`#claudeclearfreshchat`).
    let fresh_chat_visible = (harness_config.binary == "codex"
        && viewport.is_some_and(codex_viewport_shows_fresh_chat))
        || agent_doc_turn_executor_tmux::context_clear::context_clear_history_shows_fresh_chat(
            &harness_config.binary,
            history,
            command,
            is_ready,
            is_chrome,
        );
    if fresh_chat_visible {
        return verdict(
            ContextClearSubmitStatus::AcceptedClearedState,
            UnobservedClearProbe::FreshChatProven,
        );
    }
    if live_pane_prompt_ready_at_cursor(harness_config, history, None)
        && context_clear_history_proves_cleared_state(
            history,
            command,
            CONTEXT_CLEAR_CLEARED_STATE_MAX_HISTORY_LINES,
            is_ready,
        )
    {
        return verdict(
            ContextClearSubmitStatus::AcceptedClearedState,
            UnobservedClearProbe::ClearedStateProven,
        );
    }
    verdict(
        ContextClearSubmitStatus::Unobserved,
        UnobservedClearProbe::ConversationRetained,
    )
}

/// Resolve a blank-viewport (`Unrendered`) clear window against the pane's
/// scrollback. Returns [`ContextClearSubmitStatus::AcceptedClearedState`] only
/// when the history positively proves a fresh Codex or Claude Code chat
/// (`#clearfreshchatproof`, `#claudeclearfreshchat`); a capture error or
/// any retained conversation keeps `Unrendered`, which never resends.
fn upgrade_unrendered_clear_from_pane_history(
    tmux: &Tmux,
    pane: &str,
    file: &Path,
    harness: &str,
    command: &str,
    phase: &str,
) -> ContextClearSubmitStatus {
    let harness_config = agent_doc_harness::HarnessConfig::from_agent_name(harness);
    let history = agent_doc_tmux_io::capture_pane_history(tmux, pane);
    let capture_ok = history.is_ok();
    let status =
        agent_doc_turn_executor_tmux::context_clear::context_clear_unrendered_window_status(
            &harness_config.binary,
            history.as_deref().ok(),
            command,
            |line| harness_config.is_dispatch_ready_prompt_line(line),
            |line| harness_config.is_ignorable_output_line(line),
        );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "session_clear_submit_cleared_state_probe file={} pane={pane} harness={harness} phase={phase} trigger=blank_viewport result={}",
            file.display(),
            if !capture_ok {
                "capture_failed"
            } else if status.is_accepted() {
                "fresh_chat_in_history"
            } else {
                "not_proven"
            }
        ),
    );
    status
}

/// `#clearfreshchatproof`: true when a Codex viewport shows a freshly started
/// chat: the status footer reports `Context 0% used`, or the new-chat banner
/// (`>_ OpenAI Codex`) sits beside the previous chat's exit summary
/// (`To continue this session, run codex resume`), which Codex prints only when
/// a chat ends. src/haiven-dev/tasks/api.md, 2026-09-29: the operator's clear
/// ran, but 12392 lines of retained scrollback made the verifier report it as
/// "not proven submitted".
fn codex_viewport_shows_fresh_chat(viewport: &str) -> bool {
    let lines: Vec<String> = viewport
        .lines()
        .map(|line| {
            agent_doc_turn_executor_tmux::prompt::strip_ansi(line)
                .trim()
                .to_string()
        })
        .collect();
    let zero_context = lines
        .iter()
        .any(|line| line.contains('·') && line.contains("Context 0% used"));
    let new_chat_banner = lines.iter().any(|line| line.starts_with(">_ OpenAI Codex"));
    let prior_chat_closed = lines
        .iter()
        .any(|line| line.starts_with("To continue this session, run codex resume"));
    zero_context || (new_chat_banner && prior_chat_closed)
}

fn poll_context_clear_submit_acceptance(
    ctx: &ContextClearSubmitVerification<'_>,
    phase: &str,
    pre_delivery_capture_hash: Option<&str>,
    acceptance_timeout: Duration,
) -> ContextClearSubmitObservation {
    let (tmux, pane, file, harness, command) =
        (ctx.tmux, ctx.pane, ctx.file, ctx.harness, ctx.command);
    let harness_config = agent_doc_harness::HarnessConfig::from_agent_name(harness);
    let start = Instant::now();
    let mut last_capture: Option<(bool, usize, String)> = None;
    let mut last_frame: Option<ContextClearLastFrame> = None;
    let mut poll_state = ContextClearSubmitPollState::default();
    let mut capture_failed = false;
    // `#cleardoublesend`: the per-frame answer is already computed below to
    // recognize acceptance; remember it for the whole window so a timeout can
    // still say whether input ever reached the pane.
    let mut content_ever_changed_since_delivery = false;
    let mut observation_budget = CLEAR_DIRECT_SUBMIT_RENDER_TIMEOUT;
    while start.elapsed() < observation_budget {
        match agent_doc_tmux_io::capture_pane(tmux, pane) {
            Ok(content) => {
                let command_visible =
                    context_clear_command_visible_in_active_input(&content, command, |line| {
                        harness_config.is_dispatch_ready_prompt_line(line)
                    });
                let capture_hash = short_context_clear_submit_content_hash(&content);
                let capture_len = content.len();
                let content_changed_since_delivery = pre_delivery_capture_hash
                    .map(|pre_hash| pre_hash != capture_hash)
                    .unwrap_or(false);
                content_ever_changed_since_delivery |= content_changed_since_delivery;
                last_capture = Some((command_visible, capture_len, capture_hash));
                last_frame = Some(ContextClearLastFrame {
                    command_visible,
                    blank:
                        agent_doc_turn_executor_tmux::context_clear::context_clear_capture_is_blank(
                            &content,
                        ),
                });
                observation_budget =
                    agent_doc_turn_executor_tmux::context_clear::context_clear_observation_budget(
                        &content,
                        acceptance_timeout,
                        CLEAR_DIRECT_SUBMIT_RENDER_TIMEOUT,
                    );
                let prompt_ready =
                    live_pane_prompt_ready_at_cursor(&harness_config, &content, None);
                if agent_doc_turn_executor_tmux::context_clear::context_clear_submit_frame_status(
                    &mut poll_state,
                    command_visible,
                    content_changed_since_delivery,
                    prompt_ready,
                )
                .is_some()
                {
                    let observation = ContextClearSubmitObservation {
                        status: ContextClearSubmitStatus::Accepted,
                        elapsed: start.elapsed(),
                        command_visible: false,
                        content_changed_since_delivery: content_ever_changed_since_delivery,
                    };
                    log_context_clear_submit_observation(
                        file,
                        pane,
                        harness,
                        phase,
                        observation,
                        Some(capture_len),
                        last_capture.as_ref().map(|(_, _, hash)| hash.as_str()),
                    );
                    return observation;
                }
            }
            Err(_) => {
                capture_failed = true;
            }
        }
        std::thread::sleep(CLEAR_DIRECT_SUBMIT_ACCEPTANCE_POLL_INTERVAL);
    }

    let elapsed = start.elapsed();
    // `#clearsubmitlabel`: report WHICH deadline this was. `StillVisible` means
    // the command sat unconsumed in the composer; `Unobserved` means the window
    // closed with no evidence either way. Both stay non-`Accepted` — the
    // acceptance rule is unchanged, only the label stops lying.
    let status = agent_doc_turn_executor_tmux::context_clear::context_clear_expired_window_status(
        last_frame,
        capture_failed,
    );
    let command_visible = status == ContextClearSubmitStatus::StillVisible;
    // `#clearsubmitunobserved`: the loop above can only observe a transition, and
    // a clear that succeeds on a pane with nothing to clear produces none — the
    // normal case right after a cold start, which is where this was reported.
    // Before reporting the unknown, ask the state question the operator would:
    // does the pane still hold a conversation? Retained scrollback keeps it
    // blocked; an empty pane means the desired end state holds either way.
    // `#clearunrenderedlabel`: `Unrendered` now comes from the last frame
    // itself being blank, not from `observation_budget` equalling the render
    // window — the supervisor settle window has the same length.
    // A blank last frame is not proof the clear failed either. A pane resized
    // to a single row (a layout projection stashing it mid-window) scrolls the
    // whole post-clear repaint into history, so the viewport stays blank while
    // the scrollback shows the fresh chat. Only a positive cleared-state proof
    // upgrades it; otherwise it stays `Unrendered` and never earns a resend.
    let status = match status {
        ContextClearSubmitStatus::Unobserved => {
            upgrade_unobserved_clear_from_pane_history(tmux, pane, file, harness, command, phase)
        }
        ContextClearSubmitStatus::Unrendered => {
            upgrade_unrendered_clear_from_pane_history(tmux, pane, file, harness, command, phase)
        }
        other => other,
    };
    let observation = ContextClearSubmitObservation {
        status,
        elapsed,
        command_visible,
        content_changed_since_delivery: content_ever_changed_since_delivery,
    };
    log_context_clear_submit_observation(
        file,
        pane,
        harness,
        phase,
        observation,
        last_capture.as_ref().map(|(_, len, _)| *len),
        last_capture.as_ref().map(|(_, _, hash)| hash.as_str()),
    );
    observation
}

fn log_context_clear_submit_observation(
    file: &Path,
    pane: &str,
    harness: &str,
    phase: &str,
    observation: ContextClearSubmitObservation,
    capture_len: Option<usize>,
    capture_hash: Option<&str>,
) {
    agent_doc_ops_log_io::log_op(
        file,
        &context_clear_submit_observation_line(
            file.display(),
            pane,
            harness,
            phase,
            observation,
            capture_len,
            capture_hash,
        ),
    );
}

fn require_context_clear_submit_accepted(
    file: &Path,
    pane: &str,
    harness: &str,
    command: &str,
    phase: &str,
    observation: ContextClearSubmitObservation,
) -> Result<()> {
    if observation.status.is_accepted() {
        return Ok(());
    }
    let line = context_clear_submit_blocked_line(
        file.display(),
        pane,
        harness,
        command,
        phase,
        observation,
    );
    agent_doc_ops_log_io::log_op(file, &line);
    anyhow::bail!(
        "{}",
        context_clear_submit_blocked_message(
            file.display(),
            pane,
            harness,
            command,
            phase,
            observation
        )
    );
}

fn capture_context_clear_submit_content_hash(tmux: &Tmux, pane: &str) -> Option<String> {
    agent_doc_tmux_io::capture_pane(tmux, pane)
        .ok()
        .map(|content| short_context_clear_submit_content_hash(&content))
}

fn short_context_clear_submit_content_hash(content: &str) -> String {
    let hash = agent_doc_hash::content_hash(&without_volatile_harness_chrome(content));
    hash[..hash.len().min(12)].to_string()
}

/// `#clearchromechange`: the pane capture with the lines a harness redraws on
/// its own removed: the busy spinner (`esc to interrupt`), rotating tips, the
/// shortcut hint, and the context-usage footer. The clear verifier resends a
/// lost `/clear` only when the capture did not change since delivery
/// (`#cleardoublesend`); hashing the raw capture let this chrome count as
/// change, so src/haiven-dev/tasks/api.md (2026-09-29, pane `%19`) proved the
/// conversation retained and still reported "whether the clear ran is unknown"
/// instead of repairing the lost delivery once.
fn without_volatile_harness_chrome(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start_matches(|c: char| c.is_whitespace() || c == '└');
            let trimmed = trimmed.trim_start();
            !(trimmed.contains("esc to interrupt")
                || trimmed.starts_with("Tip:")
                || trimmed.starts_with("? for shortcuts")
                || (trimmed.contains("Context ")
                    && (trimmed.contains("% used") || trimmed.contains("% left"))))
        })
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn send_operator_interrupt_sequence(
    tmux: &Tmux,
    pane: &str,
    harness: &str,
    codex_shell_search: bool,
) -> Result<()> {
    let plan = operator_interrupt_key_plan(harness, codex_shell_search);
    let delay = operator_interrupt_step_delay(harness);
    for (index, key) in plan.iter().enumerate() {
        if index > 0 {
            std::thread::sleep(delay);
        }
        tmux.send_keys_raw(pane, key)?;
    }
    Ok(())
}

/// True when a live Codex pane is in a shell `reverse-i-search` / history-search
/// state, where sending `C-g` safely aborts the search. Any other state (normal
/// composer, active turn) must not receive `C-g` because it opens the external
/// editor (#codex-interrupt-clear-ctrl-g-opens-editor).
fn codex_pane_in_shell_search_state(
    ctx: &SessionContext,
    tmux: &Tmux,
    evidence: &LivePaneEvidence,
) -> bool {
    let Some(pane) = evidence.pane_id.as_deref() else {
        return false;
    };
    let harness = harness_for_evidence(ctx, evidence);
    if harness.binary != "codex" {
        return false;
    }
    let Ok(captured) = agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane)
        .or_else(|_| agent_doc_tmux_io::capture_pane(tmux, pane))
    else {
        return false;
    };
    matches!(
        harness.dispatch_blocker_reason(&captured).as_deref(),
        Some("interactive shell reverse-i-search") | Some("interactive shell history search")
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InterruptClearSettleOutcome {
    Idle,
    Closed,
    TimedOut {
        evidence: LivePaneEvidence,
        editor_recovery_attempted: bool,
    },
}

impl InterruptClearSettleOutcome {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Closed => "closed",
            Self::TimedOut { .. } => "timed_out",
        }
    }

    fn last_command(&self) -> Option<&str> {
        match self {
            Self::TimedOut { evidence, .. } => evidence.current_command.as_deref(),
            _ => None,
        }
    }

    fn blocking_state(&self) -> &'static str {
        match self {
            Self::TimedOut { evidence, .. } => evidence.state.as_str(),
            _ => "none",
        }
    }

    fn blocking_source(&self) -> &'static str {
        match self {
            Self::TimedOut { evidence, .. } => evidence.source,
            _ => "none",
        }
    }

    fn prompt_ready(&self) -> String {
        match self {
            Self::TimedOut { evidence, .. } => evidence
                .prompt_ready
                .map(|ready| ready.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            _ => "none".to_string(),
        }
    }

    fn tail(&self) -> Option<&str> {
        match self {
            Self::TimedOut { evidence, .. } => evidence.tail.as_deref(),
            _ => None,
        }
    }

    fn editor_recovery_attempted(&self) -> bool {
        matches!(
            self,
            Self::TimedOut {
                editor_recovery_attempted: true,
                ..
            }
        )
    }
}

fn wait_for_interrupt_clear_settle(
    ctx: &SessionContext,
    tmux: &Tmux,
    pane: &str,
    timeout: Duration,
) -> InterruptClearSettleOutcome {
    let deadline = Instant::now() + timeout;
    let mut editor_recovery_attempted = false;
    loop {
        if !tmux.pane_alive(pane) {
            return InterruptClearSettleOutcome::Closed;
        }
        let evidence = live_pane_evidence(ctx, tmux);
        if evidence.state == LivePaneState::AliveIdle {
            let _ = reconcile_idle_projection_from_evidence(ctx, &evidence);
            return InterruptClearSettleOutcome::Idle;
        }
        if !editor_recovery_attempted
            && evidence
                .current_command
                .as_deref()
                .is_some_and(terminal_editor_command)
        {
            editor_recovery_attempted = true;
            let command = evidence.current_command.as_deref().unwrap_or("unknown");
            agent_doc_ops_log_io::log_op(
                &ctx.canonical_file,
                &format!(
                    "session_interrupt_clear_editor_recovery file={} pane={} command={}",
                    ctx.canonical_file.display(),
                    pane,
                    command
                ),
            );
            let _ = tmux.send_keys_raw(pane, "Escape");
            std::thread::sleep(Duration::from_millis(100));
            let _ = tmux.send_keys(pane, ":qa!");
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        if Instant::now() >= deadline {
            return InterruptClearSettleOutcome::TimedOut {
                evidence,
                editor_recovery_attempted,
            };
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn resolve_direct_submit_pane(
    ctx: &SessionContext,
    tmux: &Tmux,
) -> Option<(ProvenPane, DirectSubmitPaneSource)> {
    if let Some(proven) = ctx
        .actor_record
        .as_ref()
        .map(|record| record.pane_id.as_str())
        .and_then(|pane| ProvenPane::from_recorded_owner(ctx, tmux, pane, "authoritative_actor"))
    {
        return Some((proven, DirectSubmitPaneSource::AuthoritativeActor));
    }

    if let Some(pane) = agent_doc_sync_io::sync::find_normal_path_owner_pane(
        tmux,
        &ctx.canonical_file,
        &ctx.session_id,
    ) {
        return Some((
            ProvenPane::from_verified_live_owner(pane),
            DirectSubmitPaneSource::LiveOwner,
        ));
    }

    ctx.registry_entry
        .as_ref()
        .map(|entry| entry.pane.as_str())
        .and_then(|pane| ProvenPane::from_recorded_owner(ctx, tmux, pane, "registry"))
        .map(|proven| (proven, DirectSubmitPaneSource::Registry))
}

fn guard_destructive_operator_on_live_busy_pane(
    ctx: &SessionContext,
    tmux: &Tmux,
    action: &str,
) -> Result<()> {
    let evidence = live_pane_evidence(ctx, tmux);
    if matches!(
        evidence.state,
        LivePaneState::AliveBusy | LivePaneState::AliveUnobservable
    ) {
        if action == "session_restart" && pane_shows_clean_exit_prompt(ctx, tmux, &evidence) {
            return Ok(());
        }
        let pane = evidence.pane_id.as_deref().unwrap_or("unknown");
        let command = evidence.current_command.as_deref().unwrap_or("unknown");
        let tail = evidence.tail.as_deref().unwrap_or("unknown");
        let busy_proof = busy_proof_for_pane(ctx, tmux, &evidence);
        let busy_proof = busy_proof.as_deref();
        if action == "session_restart" {
            anyhow::bail!(
                "{}",
                restart_busy_refusal_message(
                    &ctx.canonical_file,
                    pane,
                    evidence.source,
                    command,
                    busy_proof,
                    tail
                )
            );
        }
        anyhow::bail!(
            "{} refused for {} because pane {} is {} (source={}, current_command={}, busy_proof={:?}, tail={:?}). Run `agent-doc session status {}` and wait for an observable idle prompt, or inspect/stop the pane explicitly before clearing or restarting it.",
            action,
            ctx.canonical_file.display(),
            pane,
            evidence.state.as_str(),
            evidence.source,
            command,
            busy_proof.unwrap_or("none"),
            tail,
            ctx.canonical_file.display()
        );
    }
    if evidence.state == LivePaneState::AliveIdle {
        reconcile_idle_projection_from_evidence(ctx, &evidence)?;
    }
    Ok(())
}

pub(crate) fn restart_busy_refusal_message(
    file: &Path,
    pane: &str,
    source: &str,
    command: &str,
    busy_proof: Option<&str>,
    tail: &str,
) -> String {
    format!(
        "session_restart refused for {} because pane {} is alive-busy (source={}, current_command={}, busy_proof={:?}, tail={:?}). Run `agent-doc session status {}` and wait for an idle prompt, or pass `--force` to bypass this stale busy-state refusal and request a supervisor-mediated restart.",
        file.display(),
        pane,
        source,
        command,
        busy_proof.unwrap_or("none"),
        tail,
        file.display()
    )
}

/// #hj7s: refusal message when a terminal editor owns the pane TTY during a
/// restart. Mirrors `restart_busy_refusal_message`'s field convention
/// (`source=`/`current_command=`/`tail=`) so the JB plugin can parse it. The
/// header (`is held by editor <command>`) is the distinctive marker the editor
/// refusal parser keys off. Applies to both `Restart` and `--force`: the
/// operator must close the editor manually.
pub(crate) fn restart_editor_holds_pane_refusal_message(
    file: &Path,
    pane: &str,
    source: &str,
    command: &str,
    tail: &str,
) -> String {
    format!(
        "session_restart refused for {} because pane {} is held by editor {} (source={}, current_command={}, tail={:?}). Run `agent-doc session status {}` to inspect the pane. A terminal editor (for example Claude Code `ctrl+g` edit-in-nvim) owns the pane TTY, so supervisor control should wait until the editor is closed. Close the editor (for example `:wq` in nvim) and retry.",
        file.display(),
        pane,
        command,
        source,
        command,
        tail,
        file.display()
    )
}

/// Capture the live pane and return the line that proves an active turn (the
/// interrupt/working-spinner cue), so busy-guard refusals can cite concrete
/// busy evidence instead of the ambiguous footer
/// (#session-restart-refusal-shows-busy-proof). Best-effort: returns None when
/// the pane cannot be captured or no proof line is present.
fn busy_proof_for_pane(
    ctx: &SessionContext,
    tmux: &Tmux,
    evidence: &LivePaneEvidence,
) -> Option<String> {
    let pane = evidence.pane_id.as_deref()?;
    let captured = agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane)
        .or_else(|_| agent_doc_tmux_io::capture_pane(tmux, pane))
        .ok()?;
    harness_for_evidence(ctx, evidence).busy_proof_line(&captured)
}

fn log_restart_evidence_event(ctx: &SessionContext, event: &str, evidence: &LivePaneEvidence) {
    agent_doc_ops_log_io::log_op(
        &ctx.canonical_file,
        &format!(
            "{} file={} pane={} source={} state={} current_command={} prompt_ready={} tail={:?}",
            event,
            ctx.canonical_file.display(),
            evidence.pane_id.as_deref().unwrap_or("unknown"),
            evidence.source,
            evidence.state.as_str(),
            evidence.current_command.as_deref().unwrap_or("unknown"),
            evidence
                .prompt_ready
                .map(|ready| ready.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            evidence.tail.as_deref().unwrap_or("unknown")
        ),
    );
}

fn reconcile_idle_projection_from_live_pane(ctx: &SessionContext, tmux: &Tmux) -> Result<bool> {
    let evidence = live_pane_evidence(ctx, tmux);
    if evidence.state != LivePaneState::AliveIdle {
        return Ok(false);
    }
    reconcile_idle_projection_from_evidence(ctx, &evidence)
}

fn reconcile_idle_projection_from_evidence(
    ctx: &SessionContext,
    evidence: &LivePaneEvidence,
) -> Result<bool> {
    if !idle_projection_needs_reconciliation(ctx, evidence) {
        return Ok(false);
    }
    let Some(record) = ctx.actor_record.as_ref() else {
        return Ok(false);
    };
    // GH #232: an idle-looking composer is not proof the
    // turn ended. While the supervisor's live-turn evidence holds, projecting
    // `ready` would release the dispatch in-flight marker under a running turn.
    if let Some(live_evidence) = harness_turn_live_evidence(ctx, evidence) {
        agent_doc_ops_log_io::log_op(
            &ctx.canonical_file,
            &format!(
                "session_operator_idle_projection_reconcile_skipped file={} pane={} source={} reason=harness_turn_live evidence={} prior_actor_state={}",
                ctx.canonical_file.display(),
                record.pane_id,
                evidence.source,
                live_evidence,
                record.state.as_str(),
            ),
        );
        return Ok(false);
    }
    agent_doc_controller_io::project_controller::mark_lifecycle(
        &ctx.base_dir,
        agent_doc_controller_io::project_controller::LifecycleRequest {
            file: ctx.canonical_file.clone(),
            session_id: record.session_id.clone(),
            pane_id: record.pane_id.clone(),
            generation: record.generation,
            state: ActorState::Ready,
            caller: "session_operator".to_string(),
            reason: "live_pane_idle".to_string(),
            expected_revision: None,
        },
    )?;
    if let Some(lease) = ctx.operator_status.supervisor_lease.as_ref() {
        agent_doc_controller_io::project_controller::refresh_supervisor_lease(
            &ctx.base_dir,
            agent_doc_controller_io::project_controller::SupervisorHeartbeatRequest {
                file: ctx.canonical_file.clone(),
                session_id: record.session_id.clone(),
                pane_id: record.pane_id.clone(),
                generation: record.generation,
                supervisor_pid: lease.supervisor_pid,
                supervisor_socket: lease.supervisor_socket.clone(),
                runtime_state: ActorState::Ready.as_str().to_string(),
            },
        )?;
    }
    agent_doc_ops_log_io::log_op(
        &ctx.canonical_file,
        &format!(
            "session_operator_reconciled_idle_projection file={} pane={} source={} prior_actor_state={} prior_supervisor_state={} prior_lease_state={}",
            ctx.canonical_file.display(),
            record.pane_id,
            evidence.source,
            record.state.as_str(),
            ctx.supervisor_runtime
                .actor_state
                .map(|state| state.as_str())
                .unwrap_or("unknown"),
            ctx.operator_status
                .supervisor_lease
                .as_ref()
                .and_then(|lease| lease.runtime_state.as_deref())
                .unwrap_or("unknown")
        ),
    );
    Ok(true)
}

fn idle_projection_needs_reconciliation(ctx: &SessionContext, evidence: &LivePaneEvidence) -> bool {
    if evidence.state != LivePaneState::AliveIdle || evidence.prompt_ready != Some(true) {
        return false;
    }
    let Some(record) = ctx.actor_record.as_ref() else {
        return false;
    };
    if evidence.pane_id.as_deref() != Some(record.pane_id.as_str()) {
        return false;
    }
    matches!(record.state, ActorState::Starting | ActorState::Busy)
        || (supervisor_runtime_matches_record(ctx)
            && matches!(
                ctx.supervisor_runtime.actor_state,
                Some(ActorState::Starting | ActorState::Busy)
            ))
        || ctx
            .operator_status
            .supervisor_lease
            .as_ref()
            .filter(|lease| lease.generation == record.generation)
            .and_then(|lease| lease.runtime_state.as_deref())
            .is_some_and(|state| {
                matches!(
                    state,
                    s if s == ActorState::Starting.as_str() || s == ActorState::Busy.as_str()
                )
            })
}

pub fn doctor(file: &Path, repair: bool) -> Result<()> {
    if repair {
        // Ordered FIRST and reported even when the rest of the repair path fails:
        // a stale pane->window binding permanently disables editor focus sync
        // (`#panewindowbindingrebind`), and it is independent of document health.
        // Behind the document-repair pass it was unreachable on exactly the
        // documents that needed it most.
        let binding_note = match agent_doc_sync_io::sync::repair_pane_window_binding(file) {
            Ok(note) => note,
            Err(error) => {
                eprintln!("[doctor] pane window binding repair failed: {error:#}");
                None
            }
        };
        if let Some(note) = &binding_note {
            println!("{note}");
        }
        let closeout = agent_doc_repair_command_io::repair(file)?;
        let mut repair_notes = agent_doc_sync_io::sync::repair_file_state(file)?;
        let repair_ctx = build_context(file)?;
        if let Some(note) = binding_note {
            repair_notes.push(note);
        }
        if let Some(note) = clear_closed_actor_pane_projection(&repair_ctx)? {
            repair_notes.push(note);
        }
        agent_doc_sync_io::resync::run_fix(Some(file), None)?;
        println!("Applied repair path for {}.", file.display());
        if closeout.repaired() {
            println!("closeout_repair: {}", closeout.doctor_message());
        }
        for note in repair_notes {
            println!("{note}");
        }
    }
    let ctx = build_context(file)?;
    print_status_summary(&ctx);
    let issues = collect_doctor_issues(&ctx);
    if issues.is_empty() {
        println!("doctor: no issues detected");
    } else {
        println!("doctor:");
        for issue in issues {
            println!("- {issue}");
        }
    }
    Ok(())
}

fn clear_closed_actor_pane_projection(ctx: &SessionContext) -> Result<Option<String>> {
    let Some(record) = &ctx.actor_record else {
        return Ok(None);
    };
    if record.state != ActorState::Closed || record.pane_id.is_empty() {
        return Ok(None);
    }
    let old_pane = record.pane_id.clone();
    let old_window = record.window_id.clone();
    let mut cleared = record.clone();
    cleared.pane_id.clear();
    cleared.window_id.clear();
    cleared.last_transition = agent_doc_controller::actor::ActorLastTransition {
        caller: "session_doctor".to_string(),
        reason: format!(
            "cleared_closed_actor_pane old_pane={} old_window={}",
            old_pane,
            empty_or_placeholder(&old_window)
        ),
        timestamp: timestamp_secs(),
        prior_generation: record.generation,
        new_generation: record.generation,
    };
    agent_doc_controller_io::project_controller::store_actor_record(
        &ctx.base_dir,
        Some(record.generation),
        &cleared,
    )?;
    Ok(Some(format!(
        "Cleared stale closed actor pane `{}` for `{}`.",
        old_pane,
        ctx.canonical_file.display()
    )))
}

fn build_context(file: &Path) -> Result<SessionContext> {
    let canonical_file = file
        .canonicalize()
        .unwrap_or_else(|_| agent_doc_git_io::dirs::resolve_absolute_file_path(file));
    // Session commands can run while an editor-owned CRDT projection is newer
    // than disk. Reading the file directly here made `session clear/status/...`
    // falsely report a missing `agent_doc_session` during that convergence gap.
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        &canonical_file,
        "session_actor_build_context",
    )?;
    let session_id = session_id_from_current_content(&canonical_file, &content)?;
    let base_dir = agent_doc_fs::find_project_root(&canonical_file).with_context(|| {
        format!(
            "failed to locate project root for {}",
            canonical_file.display()
        )
    })?;
    let explicit_document_harness = agent_doc_harness::document_harness_from_content(&content);
    let detected_harness = agent_doc_session_actor_io::detect_document_harness_in(
        &base_dir,
        &canonical_file.to_string_lossy(),
    );
    let operator_status = agent_doc_controller_io::project_controller::session_operator_status(
        &base_dir,
        &canonical_file,
    )?;
    let registry_entry = lookup_registry_entry(&base_dir, &session_id, &canonical_file)?;
    let startup_miss = agent_doc_supervisor_io::startup_miss::load_startup_miss(&canonical_file)?;
    let log_status =
        agent_doc_supervisor_io::startup_miss::session_log_status(&canonical_file, &session_id)?;
    let supervisor_socket = agent_doc_supervisor_io::ipc::socket_path(&base_dir, &session_id);
    let supervisor_runtime = query_supervisor_runtime(&supervisor_socket);
    let operator_status = reconcile_controller_lease_with_supervisor_runtime(
        &base_dir,
        &canonical_file,
        &supervisor_socket,
        operator_status,
        &supervisor_runtime,
    )?;
    let harness = status_harness(
        explicit_document_harness,
        operator_status
            .record
            .as_ref()
            .map(|record| record.harness.as_str()),
        supervisor_runtime.current_harness.as_deref(),
        detected_harness,
    );
    let actor_record = operator_status.record.clone();
    let live_pane_window = actor_record
        .as_ref()
        .map(|record| record.pane_id.clone())
        .or_else(|| registry_entry.as_ref().map(|entry| entry.pane.clone()))
        .filter(|pane| !pane.trim().is_empty())
        .and_then(|pane| observe_live_pane_window(&pane));
    Ok(SessionContext {
        canonical_file,
        base_dir,
        session_id,
        harness,
        actor_record,
        operator_status,
        registry_entry,
        startup_miss,
        log_status,
        supervisor_runtime,
        supervisor_socket,
        live_pane_window,
    })
}

fn session_id_from_current_content(file: &Path, content: &str) -> Result<String> {
    agent_doc_frontmatter::frontmatter::session_id_from_content(content)
        .with_context(|| format!("{} has no agent_doc_session", file.display()))
}

/// Report the same harness authority that owns the document actor. An explicit
/// document selection remains highest authority; otherwise the durable actor
/// record must win over ambient harness detection so a Codex shell cannot make
/// a Claude actor look like Codex (or vice versa).
///
/// `default` is not an authority at any layer: it is what ambient detection
/// writes when the caller runs outside every harness (a JetBrains backend
/// command, a plain shell). gh-266: 6 of 11 Clear Session Context probes on a
/// Claude Code pane ran as `harness=default` — the actor record had been bound
/// from such a caller — while the supervisor for the same pane logged
/// `harness=claude`. When no named layer above it resolves, the harness the
/// live supervisor reports running in the pane decides, normalized the way a
/// document's `agent:` value is (`claude` -> `claude-code`).
fn status_harness(
    explicit_document_harness: Option<String>,
    actor_harness: Option<&str>,
    supervisor_harness: Option<&str>,
    ambient_harness: String,
) -> String {
    let resolved = |value: &str| {
        let value = value.trim();
        (!value.is_empty() && value != "default").then(|| value.to_string())
    };
    explicit_document_harness
        .as_deref()
        .and_then(resolved)
        .or_else(|| actor_harness.and_then(resolved))
        .or_else(|| {
            supervisor_harness
                .map(agent_doc_harness::normalize_harness_name)
                .as_deref()
                .and_then(resolved)
        })
        .or_else(|| resolved(&ambient_harness))
        .unwrap_or(ambient_harness)
}

/// Read the window a pane currently lives in. Best-effort: a tmux that cannot
/// answer yields `None`, which `pane_window_binding_drifted` never treats as
/// drift.
fn observe_live_pane_window(pane_id: &str) -> Option<String> {
    agent_doc_tmux_io::configured_tmux()
        .pane_window(pane_id)
        .ok()
        .map(|window| window.trim().to_string())
        .filter(|window| !window.is_empty())
}

fn lookup_registry_entry(
    base_dir: &Path,
    session_id: &str,
    canonical_file: &Path,
) -> Result<Option<SessionEntry>> {
    let registry = agent_doc_session_registry_io::load_in(base_dir)?;
    Ok(find_registry_entry(
        base_dir,
        &registry,
        session_id,
        canonical_file,
    ))
}

fn find_registry_entry(
    base_dir: &Path,
    registry: &SessionRegistry,
    session_id: &str,
    canonical_file: &Path,
) -> Option<SessionEntry> {
    if let Some(entry) = registry.iter().find_map(|(key, entry)| {
        registry_entry_matches_file(base_dir, key, entry, canonical_file).then(|| entry.clone())
    }) {
        return Some(entry);
    }

    // Legacy registries used the session id as the map key and left `file`
    // empty. Keep that fallback, but do not let a session-id collision adopt a
    // registry entry that explicitly names a different document.
    registry.iter().find_map(|(key, entry)| {
        if entry.session_id == session_id
            && entry.file.trim().is_empty()
            && !registry_key_names_foreign_file(base_dir, key, canonical_file)
        {
            Some(entry.clone())
        } else {
            None
        }
    })
}

fn registry_entry_matches_file(
    base_dir: &Path,
    key: &str,
    entry: &SessionEntry,
    canonical_file: &Path,
) -> bool {
    path_text_matches_file(base_dir, key, canonical_file)
        || path_text_matches_file(base_dir, &entry.file, canonical_file)
}

fn registry_key_names_foreign_file(base_dir: &Path, key: &str, canonical_file: &Path) -> bool {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return false;
    }
    let path = Path::new(trimmed);
    let looks_path = path.is_absolute()
        || trimmed.contains(std::path::MAIN_SEPARATOR)
        || trimmed.contains('/')
        || path.extension().is_some();
    looks_path && !path_text_matches_file(base_dir, trimmed, canonical_file)
}

fn path_text_matches_file(base_dir: &Path, raw: &str, canonical_file: &Path) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    let path = Path::new(trimmed);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    absolute.canonicalize().unwrap_or(absolute) == canonical_file
}

fn resolve_attach_pane(pane: Option<&str>) -> Result<String> {
    match pane {
        Some(pane_id) => Ok(pane_id.to_string()),
        None => {
            let tmux = agent_doc_tmux_io::configured_tmux();
            agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux).context(
                "attach requires --pane when no tmux pane is active in the current environment",
            )
        }
    }
}

fn ensure_supervisor_socket(ctx: &SessionContext) -> Result<()> {
    if !ctx.supervisor_socket.exists() {
        anyhow::bail!(
            "no live supervisor socket for {} (expected {})",
            ctx.canonical_file.display(),
            ctx.supervisor_socket.display()
        );
    }
    Ok(())
}

/// `#supdead-coldstart-fallback`: the recovery decision for a dead supervisor.
///
/// `session restart-supervisor` and (the supervisor-socket leg of) `admin recycle`
/// connect to the supervisor socket to deliver an in-place restart/recycle. When
/// the supervisor PROCESS is dead and only a stale socket file remains, that
/// connect fails with a raw `Connection refused (os error 111)` and the command
/// gives up. Those commands restart a *live* supervisor but cannot bootstrap a
/// *dead* one. On the dead case we want to reap the stale socket and cold-start a
/// fresh supervisor through the existing route path — unless cold-starting from
/// this caller's context would be unsafe, in which case we hand back actionable
/// guidance instead of a bare ECONNREFUSED.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub(crate) enum DeadSupervisorRecovery {
    /// The supervisor is live — take the normal in-place restart/recycle path.
    InPlaceLive,
    /// The supervisor is dead — reap the stale socket and cold-start a fresh one
    /// via the route path.
    ColdStart,
    /// The supervisor is dead but a safe cold-start cannot be performed from this
    /// caller (the caller is the dead supervisor's own pane/ancestor, or no
    /// route/controller context is reachable). Surface this actionable message.
    Guidance(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
enum DeadSupervisorColdStartTarget {
    PreserveActorPane(String),
    RouteAutoStart,
}

/// Inputs to [`decide_dead_supervisor_recovery`], kept as a plain struct so the
/// decision is a pure, unit-testable function with no process/socket I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub(crate) struct DeadSupervisorRecoveryInputs {
    /// Socket liveness as probed by `supervisor::ipc::probe_socket`.
    pub(crate) socket_dead: bool,
    /// A live route-owned supervisor PID was found for this doc AND it is this
    /// caller's own process / ancestor. When the socket is dead this is normally
    /// `false` (the process is gone), but a transient stale-socket-with-live-proc
    /// race must still refuse a self-targeting cold-start.
    pub(crate) caller_is_own_ancestor: bool,
    /// Whether a cold-start can resolve a tmux target session from this context.
    /// `false` when the caller is not inside tmux and no session override exists,
    /// so spawning a route-owned pane is impossible.
    pub(crate) can_resolve_tmux_target: bool,
}

#[cfg(test)]
pub(crate) fn decide_dead_supervisor_recovery(
    file: &Path,
    socket: &Path,
    inputs: &DeadSupervisorRecoveryInputs,
) -> DeadSupervisorRecovery {
    if !inputs.socket_dead {
        return DeadSupervisorRecovery::InPlaceLive;
    }
    if inputs.caller_is_own_ancestor {
        return DeadSupervisorRecovery::Guidance(format!(
            "supervisor for {} is dead (stale socket {}), not just unreachable, but its process is this session's own pane/ancestor — refusing an unsafe in-process cold-start. Cold-start a fresh supervisor by running `Run Agent Doc` on the document (or `agent-doc start --route-owned {}`) from a DIFFERENT pane, or restart this pane.",
            file.display(),
            socket.display(),
            file.display()
        ));
    }
    if !inputs.can_resolve_tmux_target {
        return DeadSupervisorRecovery::Guidance(format!(
            "supervisor for {} is dead (stale socket {}), not just unreachable — but no tmux target session is reachable from here to cold-start a replacement. Cold-start a fresh supervisor by running `Run Agent Doc` on the document (or `agent-doc start --route-owned {}`) from inside the editor's tmux session.",
            file.display(),
            socket.display(),
            file.display()
        ));
    }
    DeadSupervisorRecovery::ColdStart
}

#[cfg(test)]
fn dead_supervisor_cold_start_target<F>(
    ctx: &SessionContext,
    mut pane_alive: F,
) -> DeadSupervisorColdStartTarget
where
    F: FnMut(&str) -> bool,
{
    if let Some(record) = ctx.actor_record.as_ref()
        && record.state != ActorState::Closed
        && !record.pane_id.trim().is_empty()
        && pane_alive(&record.pane_id)
    {
        return DeadSupervisorColdStartTarget::PreserveActorPane(record.pane_id.clone());
    }
    DeadSupervisorColdStartTarget::RouteAutoStart
}

#[cfg(test)]
fn shell_quote_arg(raw: &str) -> String {
    if !raw.is_empty()
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '+'))
    {
        return raw.to_string();
    }
    format!("'{}'", raw.replace('\'', "'\\''"))
}

#[cfg(test)]
fn dead_supervisor_start_command(agent_doc_bin: &str, file: &Path) -> String {
    format!(
        "{} start --route-owned {}",
        shell_quote_arg(agent_doc_bin),
        shell_quote_arg(&file.to_string_lossy())
    )
}

fn session_log_path(base_dir: &Path, session_id: &str) -> PathBuf {
    base_dir
        .join(".agent-doc/logs")
        .join(format!("{session_id}.log"))
}

fn query_supervisor_runtime(socket: &Path) -> SupervisorRuntime {
    if !socket.exists() {
        return SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
            actor_session_id: None,
            actor_pane_id: None,
            actor_generation: None,
            supervisor_state: None,
            restart_count: 0,
            supervisor_pid: None,
            supervisor_instance_id: None,
            child_pid: None,
            cwd_source: None,
            current_harness: None,
        };
    }
    match agent_doc_supervisor_io::ipc::send_command(socket, &IpcMethod::State) {
        Ok(response) if response.ok => {
            let data = response.data.unwrap_or_default();
            let running = data
                .get("running")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let supervisor_state = data
                .get("state")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            let restart_count = data
                .get("restart_count")
                .and_then(|value| value.as_u64())
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0);
            let health = match supervisor_state.as_deref() {
                Some("healthy") if running => SupervisorHealth::Healthy,
                Some("halted") => SupervisorHealth::Halted { restart_count },
                Some(_) => SupervisorHealth::Restartable,
                None => SupervisorHealth::Restartable,
            };
            SupervisorRuntime {
                health,
                actor_state: data
                    .get("actor_state")
                    .and_then(|value| value.as_str())
                    .and_then(parse_actor_state),
                actor_session_id: data
                    .get("actor_session_id")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned),
                actor_pane_id: data
                    .get("actor_pane_id")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned),
                actor_generation: data
                    .get("actor_generation")
                    .and_then(|value| value.as_u64()),
                supervisor_state,
                restart_count,
                supervisor_pid: data
                    .get("supervisor_pid")
                    .and_then(|value| value.as_u64())
                    .and_then(|value| u32::try_from(value).ok()),
                supervisor_instance_id: data
                    .get("supervisor_instance_id")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned),
                child_pid: data
                    .get("child_pid")
                    .and_then(|value| value.as_u64())
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|pid| *pid > 0),
                cwd_source: data
                    .get("cwd_source")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned),
                current_harness: data
                    .get("current_harness")
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
            }
        }
        Ok(_) | Err(_) => SupervisorRuntime {
            health: SupervisorHealth::Unreachable,
            actor_state: None,
            actor_session_id: None,
            actor_pane_id: None,
            actor_generation: None,
            supervisor_state: None,
            restart_count: 0,
            supervisor_pid: None,
            supervisor_instance_id: None,
            child_pid: None,
            cwd_source: None,
            current_harness: None,
        },
    }
}

fn live_pane_evidence(ctx: &SessionContext, tmux: &Tmux) -> LivePaneEvidence {
    let (pane_id, source) = live_evidence_target(ctx);
    let Some(pane_id) = pane_id else {
        return LivePaneEvidence {
            pane_id: None,
            source,
            state: if matches!(
                ctx.supervisor_runtime.health,
                SupervisorHealth::NoSocket | SupervisorHealth::Unreachable
            ) {
                LivePaneState::ClosedClean
            } else {
                LivePaneState::Unknown
            },
            current_command: None,
            prompt_ready: None,
            tail: None,
        };
    };

    live_pane_evidence_for_pane(ctx, tmux, pane_id, source)
}

fn live_pane_evidence_for_pane(
    ctx: &SessionContext,
    tmux: &Tmux,
    pane_id: String,
    source: &'static str,
) -> LivePaneEvidence {
    if !tmux.pane_alive(&pane_id) {
        let projected_live = ctx.actor_record.is_some() || ctx.registry_entry.is_some();
        return LivePaneEvidence {
            pane_id: Some(pane_id),
            source,
            state: if projected_live {
                LivePaneState::ProjectionStale
            } else {
                LivePaneState::ClosedClean
            },
            current_command: None,
            prompt_ready: Some(false),
            tail: None,
        };
    }

    // `#stale-actor-pane-collision`: a live pane is NOT proof of OUR live actor. When
    // no reachable supervisor can vouch for the pane (Unreachable/NoSocket) and the
    // evidence pane came from a recorded owner (`authoritative_actor` / `registry`),
    // the pane may have been reused by a DIFFERENT session after our supervised child
    // exited — observed: agent-doc-bugs2.md's supervised claude exited (clean_exit),
    // pane %85 was relaunched as another Claude session, yet the stale actor record
    // still claimed %85, so status/sync/focus reported it `alive-idle` and treated a
    // foreign pane as our live actor. Require the recorded child/registry pid to still
    // live inside the pane's process tree; otherwise the pane is a foreign reuse, not
    // our actor — report a stale projection so resync reclaims it instead of pointing
    // dispatch at someone else's pane.
    let recorded_owner_pid = ctx
        .supervisor_runtime
        .child_pid
        .or_else(|| ctx.registry_entry.as_ref().map(|entry| entry.pid));
    // Only probe the (expensive) pane process tree once the cheap terms already
    // qualify; a healthy/reachable supervisor is authoritative on its own.
    let pane_owned_by_recorded_pid = match (
        evidence_pane_reuse_cheap_gate(source, &ctx.supervisor_runtime.health),
        recorded_owner_pid,
    ) {
        (true, Some(recorded_pid)) => pane_process_tree_contains_pid(tmux, &pane_id, recorded_pid),
        _ => true,
    };
    if evidence_pane_is_foreign_reuse(
        source,
        &ctx.supervisor_runtime.health,
        recorded_owner_pid,
        pane_owned_by_recorded_pid,
    ) {
        return LivePaneEvidence {
            pane_id: Some(pane_id),
            source,
            state: LivePaneState::ProjectionStale,
            current_command: None,
            prompt_ready: Some(false),
            tail: None,
        };
    }

    let harness = agent_doc_harness::HarnessConfig::from_agent_name(&ctx.harness);
    let captured = agent_doc_tmux_io::capture_pane(tmux, &pane_id).unwrap_or_default();
    let cursor_y = agent_doc_tmux_io::pane_cursor_y(tmux, &pane_id);
    let prompt_ready = live_pane_prompt_ready_at_cursor(&harness, &captured, cursor_y);
    let pane_height = pane_display_value(tmux, &pane_id, "#{pane_height}")
        .and_then(|height| height.parse::<usize>().ok());
    let viewport_unobservable =
        live_pane_viewport_unobservable(&harness, &captured, pane_height, prompt_ready);
    LivePaneEvidence {
        pane_id: Some(pane_id.clone()),
        source,
        state: if prompt_ready {
            LivePaneState::AliveIdle
        } else if viewport_unobservable {
            LivePaneState::AliveUnobservable
        } else {
            LivePaneState::AliveBusy
        },
        current_command: pane_display_value(tmux, &pane_id, "#{pane_current_command}"),
        prompt_ready: (!viewport_unobservable).then_some(prompt_ready),
        tail: last_meaningful_pane_line(&captured),
    }
}

/// True if the pane's process tree still contains `recorded_pid` (our supervised
/// child / registry-recorded process). Conservative on inability to resolve the pane
/// pid — returns `true` so we never wrongly downgrade a genuinely-live actor when the
/// tmux query fails. See `#stale-actor-pane-collision`.
fn pane_process_tree_contains_pid(tmux: &Tmux, pane_id: &str, recorded_pid: u32) -> bool {
    let Some(pane_pid) = agent_doc_tmux_io::pane_pid(tmux, pane_id) else {
        return true;
    };
    agent_doc_process_owner_io::process_tree_contains_pid(&pane_pid.to_string(), recorded_pid)
}

/// Cheap (no syscalls) precondition for the `#stale-actor-pane-collision` probe: the
/// evidence pane came from a recorded owner AND no reachable supervisor can vouch for
/// it. Only when this holds is it worth probing the pane's process tree.
fn evidence_pane_reuse_cheap_gate(source: &str, health: &SupervisorHealth) -> bool {
    matches!(source, "authoritative_actor" | "registry")
        && matches!(
            health,
            SupervisorHealth::Unreachable | SupervisorHealth::NoSocket
        )
}

/// Pure decision for `#stale-actor-pane-collision`: a live pane that came from a
/// recorded owner is a FOREIGN reuse (not our live actor) when no reachable supervisor
/// vouches for it, we have a recorded child/registry pid, and that pid no longer lives
/// in the pane's process tree. In that case the pane belongs to a different session
/// that reused it after ours exited, so it must be reported as a stale projection
/// rather than `alive-idle`.
fn evidence_pane_is_foreign_reuse(
    source: &str,
    health: &SupervisorHealth,
    recorded_owner_pid: Option<u32>,
    pane_owned_by_recorded_pid: bool,
) -> bool {
    evidence_pane_reuse_cheap_gate(source, health)
        && recorded_owner_pid.is_some()
        && !pane_owned_by_recorded_pid
}

/// `#stale-actor-pane-collision`: whether a recorded-owner pane (authoritative actor
/// or registry) is a SAFE target for direct input submission (clear / interrupt /
/// resubmit). It must be alive AND not a foreign reuse — sending keystrokes into a
/// pane another session reused after ours exited would interrupt someone else's work.
/// Shares the provenance rule with the live-pane evidence path so status and dispatch
/// agree on which panes are really ours.
fn recorded_owner_pane_is_safe_target(
    ctx: &SessionContext,
    tmux: &Tmux,
    pane: &str,
    source: &'static str,
) -> bool {
    if !tmux.pane_alive(pane) {
        return false;
    }
    let recorded_owner_pid = ctx
        .supervisor_runtime
        .child_pid
        .or_else(|| ctx.registry_entry.as_ref().map(|entry| entry.pid));
    let pane_owned_by_recorded_pid = match (
        evidence_pane_reuse_cheap_gate(source, &ctx.supervisor_runtime.health),
        recorded_owner_pid,
    ) {
        (true, Some(recorded_pid)) => pane_process_tree_contains_pid(tmux, pane, recorded_pid),
        _ => true,
    };
    !evidence_pane_is_foreign_reuse(
        source,
        &ctx.supervisor_runtime.health,
        recorded_owner_pid,
        pane_owned_by_recorded_pid,
    )
}

fn live_evidence_target(ctx: &SessionContext) -> (Option<String>, &'static str) {
    if let Some(record) = &ctx.actor_record
        && record.state != ActorState::Closed
        && !record.pane_id.is_empty()
    {
        return (Some(record.pane_id.clone()), "authoritative_actor");
    }
    if let Some(entry) = &ctx.registry_entry {
        return (Some(entry.pane.clone()), "registry");
    }
    if let Some(pane) = ctx.log_status.as_ref().and_then(|status| {
        status
            .latest_session_open()
            .then(|| status.latest_start_pane.clone())
            .flatten()
    }) {
        return (Some(pane), "session_log");
    }
    (None, "none")
}

#[cfg(test)]
fn live_pane_prompt_ready(harness: &agent_doc_harness::HarnessConfig, captured: &str) -> bool {
    live_pane_prompt_ready_at_cursor(harness, captured, None)
}

fn live_pane_prompt_ready_at_cursor(
    harness: &agent_doc_harness::HarnessConfig,
    captured: &str,
    cursor_y: Option<usize>,
) -> bool {
    // The harness child has exited and the persistent agent-doc wrapper owns
    // the pane. This is an operator-input surface, not a busy harness turn.
    // Recognizing only the bottom-most exact wrapper prompt prevents stale
    // scrollback from making a live turn look idle.
    if agent_doc_restart_prompt_visible(captured) {
        return true;
    }
    if cursor_y.is_some()
        && agent_doc_harness::ready_prompt_candidate_at_cursor(captured, harness, cursor_y)
            .is_some()
    {
        return true;
    }
    let candidate = harness.last_prompt_candidate(captured);
    let latest_dispatch_ready_prompt = candidate
        .as_deref()
        .is_some_and(|line| harness.is_dispatch_ready_prompt_line(line));
    // Claude's busy cue can linger as stale scrollback, so a dispatch-ready
    // composer may outrank it — but ONLY when the composer is a rendered
    // placeholder (e.g. `❯ Press up to edit queued messages`), which itself
    // proves input was accepted. A bare `❯` under a live spinner is an active
    // turn and must fall through to the busy cue, or dispatch clobbers it.
    if harness.binary == "claude"
        && latest_dispatch_ready_prompt
        && candidate
            .as_deref()
            .is_some_and(|line| harness.is_idle_placeholder_prompt_line(line))
    {
        return true;
    }
    if harness.has_busy_cue(captured) {
        return false;
    }
    if latest_dispatch_ready_prompt {
        return true;
    }
    if harness.binary == "opencode" && harness.is_idle_chrome_only_output(captured) {
        return true;
    }
    if harness.is_idle_chrome_only_output(captured)
        || live_pane_bottom_status_is_idle(harness, captured)
    {
        return true;
    }
    if harness.binary == "opencode" && harness.is_bottom_idle_chrome(captured, 12) {
        return true;
    }
    false
}

const MIN_CLASSIFIABLE_PANE_HEIGHT: usize = 4;

/// A hidden stash window may compress a live pane below the number of rows its
/// harness needs to render the composer. Missing prompt chrome in that viewport
/// is absence of evidence, not evidence that the harness is busy. Explicit busy
/// cues and drafted input remain observable blockers even in a short pane.
fn live_pane_viewport_unobservable(
    harness: &agent_doc_harness::HarnessConfig,
    captured: &str,
    pane_height: Option<usize>,
    prompt_ready: bool,
) -> bool {
    let explicit_draft = matches!(
        agent_doc_harness::project_pane_composer_at_cursor(captured, harness, None),
        agent_doc_harness::PaneComposerProjection::OperatorDraft { .. }
    );
    if prompt_ready || explicit_draft || harness.has_busy_cue(captured) {
        return false;
    }
    captured.trim().is_empty()
        || pane_height.is_some_and(|height| height < MIN_CLASSIFIABLE_PANE_HEIGHT)
}

fn agent_doc_restart_prompt_visible(captured: &str) -> bool {
    let Some(line) = captured
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
    else {
        return false;
    };
    matches!(
        line,
        "Press Enter to restart, or 'q' to exit."
            | "Press Enter to restart fresh, or 'q' to exit."
            | "Press Enter to restart and continue this session, or 'q' to exit."
            | "Unrecognized input. Press Enter to restart fresh, or 'q' to exit."
            | "Unrecognized input. Press Enter to restart and continue this session, or 'q' to exit."
    )
}

fn live_pane_bottom_status_is_idle(
    harness: &agent_doc_harness::HarnessConfig,
    captured: &str,
) -> bool {
    if harness.binary != "codex" {
        return false;
    }
    let Some(last_line) = captured
        .lines()
        .rev()
        .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
        .map(|line| line.trim().to_string())
        .find(|line| !line.is_empty())
    else {
        return false;
    };
    if !harness.is_idle_status_line(&last_line) {
        return false;
    }
    if let Some(candidate) = harness.last_prompt_candidate(captured) {
        let stripped = agent_doc_turn_executor_tmux::prompt::strip_ansi(&candidate);
        let trimmed = stripped.trim();
        if matches!(trimmed.chars().next(), Some('>' | '›' | '❯'))
            && !harness.is_dispatch_ready_prompt_line(trimmed)
        {
            return false;
        }
    }
    true
}

fn last_meaningful_pane_line(captured: &str) -> Option<String> {
    captured
        .lines()
        .rev()
        .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
        .map(|line| line.trim().to_string())
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(160).collect())
}

fn pane_display_value(tmux: &Tmux, pane_id: &str, format: &str) -> Option<String> {
    tmux.cmd()
        .args(["display-message", "-t", pane_id, "-p", format])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn reconcile_controller_lease_with_supervisor_runtime(
    base_dir: &Path,
    canonical_file: &Path,
    supervisor_socket: &Path,
    operator_status: SessionOperatorStatus,
    runtime: &SupervisorRuntime,
) -> Result<SessionOperatorStatus> {
    if matches!(
        runtime.health,
        SupervisorHealth::NoSocket | SupervisorHealth::Unreachable
    ) {
        return Ok(operator_status);
    }
    let Some(record) = operator_status.record.as_ref() else {
        return Ok(operator_status);
    };
    let Some(runtime_state) = runtime.actor_state else {
        return Ok(operator_status);
    };
    if runtime.actor_session_id.as_deref() != Some(record.session_id.as_str())
        || runtime.actor_pane_id.as_deref() != Some(record.pane_id.as_str())
        || runtime.actor_generation != Some(record.generation)
    {
        return Ok(operator_status);
    }

    agent_doc_controller_io::project_controller::refresh_supervisor_lease(
        base_dir,
        agent_doc_controller_io::project_controller::SupervisorHeartbeatRequest {
            file: canonical_file.to_path_buf(),
            session_id: record.session_id.clone(),
            pane_id: record.pane_id.clone(),
            generation: record.generation,
            supervisor_pid: runtime.supervisor_pid,
            supervisor_socket: Some(supervisor_socket.to_string_lossy().to_string()),
            runtime_state: runtime_state.as_str().to_string(),
        },
    )?;
    agent_doc_controller_io::project_controller::session_operator_status(base_dir, canonical_file)
}

fn parse_actor_state(raw: &str) -> Option<ActorState> {
    match raw.trim() {
        "starting" => Some(ActorState::Starting),
        "ready" => Some(ActorState::Ready),
        "busy" => Some(ActorState::Busy),
        "waiting_input" => Some(ActorState::WaitingInput),
        "closed" => Some(ActorState::Closed),
        "blocked" => Some(ActorState::Blocked),
        _ => None,
    }
}

fn format_controller_transition(transition: &ActorTransitionStatus) -> String {
    agent_doc_supervisor::format_transition_event(agent_doc_supervisor::OwnershipTransitionEvent {
        caller: &transition.caller,
        reason: &transition.reason,
        prior_generation: transition.prior_generation,
        new_generation: transition.new_generation,
        old_pane: transition.old_pane.as_deref(),
        new_pane: &transition.new_pane,
        old_window: transition.old_window.as_deref(),
        new_window: transition.new_window.as_deref(),
    })
}

fn print_status_summary(ctx: &SessionContext) {
    println!("document: {}", ctx.canonical_file.display());
    println!("session_id: {}", ctx.session_id);
    println!("harness: {}", ctx.harness);
    match &ctx.actor_record {
        Some(record) => {
            println!(
                "actor: generation={} pane={} window={} state={}",
                record.generation,
                record.pane_id,
                record.window_id,
                record.state.as_str()
            );
            println!(
                "actor_last_transition: caller={} reason={} prior_generation={} new_generation={} at={}",
                record.last_transition.caller,
                record.last_transition.reason,
                record.last_transition.prior_generation,
                record.last_transition.new_generation,
                format_timestamp(record.last_transition.timestamp)
            );
        }
        None => println!("actor: missing"),
    }
    match &ctx.registry_entry {
        Some(entry) => {
            println!(
                "registry: pane={} window={} pid={} cwd={} supervisor_instance_id={}",
                entry.pane,
                entry.window,
                entry.pid,
                entry.cwd,
                empty_or_placeholder(&entry.supervisor_instance_id)
            );
        }
        None => println!("registry: missing"),
    }
    let tmux = agent_doc_tmux_io::configured_tmux();
    let evidence = live_pane_evidence(ctx, &tmux);
    println!(
        "live_pane: state={} pane={} source={} current_command={} prompt_ready={} tail={}",
        evidence.state.as_str(),
        evidence.pane_id.as_deref().unwrap_or("none"),
        evidence.source,
        evidence.current_command.as_deref().unwrap_or("unknown"),
        evidence
            .prompt_ready
            .map(|ready| ready.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        evidence.tail.as_deref().unwrap_or("none")
    );
    let pane_blocker = live_pane_recognized_blocker(ctx, &tmux, &evidence);
    if let Some(reason) = pane_blocker.as_deref() {
        println!("{}", live_pane_blocker_status_line(reason));
    }
    println!(
        "supervisor: health={} state={} actor_state={} restart_count={} socket={}",
        ctx.supervisor_runtime.health.as_str(),
        ctx.supervisor_runtime
            .supervisor_state
            .as_deref()
            .unwrap_or("unknown"),
        ctx.supervisor_runtime
            .actor_state
            .map(|state| state.as_str())
            .unwrap_or("unknown"),
        ctx.supervisor_runtime.restart_count,
        if ctx.supervisor_socket.exists() {
            ctx.supervisor_socket.display().to_string()
        } else {
            "missing".to_string()
        }
    );
    println!(
        "supervisor_process: pid={} child_pid={} cwd_source={} instance_id={}",
        ctx.supervisor_runtime
            .supervisor_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        ctx.supervisor_runtime
            .child_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        ctx.supervisor_runtime
            .cwd_source
            .as_deref()
            .unwrap_or("unknown"),
        ctx.supervisor_runtime
            .supervisor_instance_id
            .as_deref()
            .unwrap_or("unknown")
    );
    match &ctx.startup_miss {
        Some(miss) => println!(
            "startup_miss: pane={} origin={:?} at={}",
            miss.pane_id,
            miss.origin,
            format_timestamp(miss.timestamp)
        ),
        None => println!("startup_miss: none"),
    }
    match &ctx.log_status {
        Some(status) => println!(
            "session_log: latest_start_pane={} latest_open={} committed_after_latest_run={} last_event={}",
            status.latest_start_pane.as_deref().unwrap_or("unknown"),
            status.latest_session_open(),
            status.saw_committed_cycle_after_latest_run,
            status.last_event.as_deref().unwrap_or("unknown")
        ),
        None => println!("session_log: missing"),
    }
    if matches!(ctx.harness.as_str(), "codex" | "opencode") {
        println!(
            "{}_capability_proof: {}",
            ctx.harness,
            capability_proof_status(ctx)
        );
    }
    // `#nameelseclaude`: Claude Code refuses a second attach with a bare "This
    // conversation is open in another app", naming nothing. The holders are on
    // disk in `~/.claude/sessions/<pid>.json`, so name them here — including the
    // positive "no other live CLI holds it", which is what tells the operator to
    // stop hunting panes and look outside the terminal.
    // The lock is a property of the Claude Code CLI the operator is RUNNING, not
    // of the harness this document declares, so this must not be gated on
    // `ctx.harness`. It was, and that is precisely how the report went missing in
    // the reported case: `src/haiven-dev/tasks/api.md` and `tasks/frontend.md`
    // both carry `agent: codex`, so `ctx.harness == "codex"` and the whole report
    // was skipped — while the operator, running Claude Code, sat in front of the
    // bare "This conversation is open in another app" lock with nothing named.
    // The operator had already written the cause onto the queue head: "This
    // happened after transitioning from `agent: claude` to `agent: codex`".
    // The document keeps its `resume.claude` id across a harness switch; that
    // id is the conversation a Claude Code client refuses to reopen.
    let claude_conversation =
        std::fs::read_to_string(&ctx.canonical_file)
            .ok()
            .and_then(|content| {
                agent_doc_frontmatter::frontmatter::parse(&content)
                    .ok()
                    .and_then(|(fm, _)| fm.resume_for_harness("claude").map(str::to_string))
            });
    for line in crate::conversation_holders::observe_holders(
        &ctx.base_dir,
        None,
        claude_conversation.as_deref(),
    )
    .status_lines()
    {
        println!("{line}");
    }
    match &ctx.operator_status.supervisor_lease {
        Some(lease) => println!(
            "controller_lease: generation={} pid={} runtime_state={} heartbeat={} socket={}",
            lease.generation,
            lease
                .supervisor_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            lease.runtime_state.as_deref().unwrap_or("unknown"),
            lease
                .last_heartbeat
                .map(format_timestamp)
                .unwrap_or_else(|| "unknown".to_string()),
            lease.supervisor_socket.as_deref().unwrap_or("unknown")
        ),
        None => println!("controller_lease: missing"),
    }
    if let Some(attempt) = ctx.operator_status.dispatch_attempts.last() {
        println!(
            "controller_last_command: kind={} accepted_stage={} failed_stage={} at={}",
            attempt.command_kind,
            attempt.accepted_stage.as_deref().unwrap_or("none"),
            attempt.failed_stage.as_deref().unwrap_or("none"),
            format_timestamp(attempt.timestamp)
        );
    } else {
        println!("controller_last_command: none");
    }
    if !ctx.operator_status.projection_diagnostics.is_empty() {
        println!("controller_projection_diagnostics:");
        for diagnostic in &ctx.operator_status.projection_diagnostics {
            println!(
                "- projection={} generation={} hash={} retry_status={} at={} message={}",
                diagnostic.projection,
                diagnostic
                    .source_generation
                    .map(|generation| generation.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                diagnostic.intended_hash.as_deref().unwrap_or("unknown"),
                diagnostic.retry_status.as_deref().unwrap_or("unknown"),
                format_timestamp(diagnostic.timestamp),
                diagnostic.message
            );
        }
    }
    if owned_pane_ready_busy_conflict(ctx, &evidence)
        && !pane_blocker
            .as_deref()
            .is_some_and(blocker_supersedes_ready_busy_recovery_hint)
    {
        println!(
            "status_warning: owned_pane_ready_busy_conflict actor=ready supervisor_actor=ready controller_lease=ready live_pane=alive-busy prompt_ready=false current_command={}",
            evidence.current_command.as_deref().unwrap_or("unknown")
        );
        println!(
            "recovery_hint: supervisor will treat a stable stale ready/busy probe as recoverable after a bounded re-probe; if this persists, run `agent-doc session status {}` again, then `agent-doc session clear {}` from another pane when no real turn is active.",
            ctx.canonical_file.display(),
            ctx.canonical_file.display()
        );
    }
}

/// `#codexupdateblocker`: name the modal/interactive state holding the live
/// pane, so `alive-busy` is not the operator's only clue.
fn live_pane_recognized_blocker(
    ctx: &SessionContext,
    tmux: &Tmux,
    evidence: &LivePaneEvidence,
) -> Option<String> {
    if evidence.state != LivePaneState::AliveBusy {
        return None;
    }
    let pane = evidence.pane_id.as_deref()?;
    let captured = agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane)
        .or_else(|_| agent_doc_tmux_io::capture_pane(tmux, pane))
        .ok()?;
    let harness = harness_for_evidence(ctx, evidence);
    agent_doc_harness::dispatch_only_blocker_reason(&harness, &captured)
}

/// Blockers only the operator may answer. Suggesting `session clear` for them
/// would type `/clear` into the modal.
fn blocker_supersedes_ready_busy_recovery_hint(reason: &str) -> bool {
    matches!(
        reason,
        agent_doc_harness::CODEX_UPDATE_AVAILABLE_BLOCKER
            | agent_doc_harness::CODEX_CONVERSATION_OPEN_ELSEWHERE_BLOCKER
            | "active permission prompt"
    )
}

fn live_pane_blocker_status_line(reason: &str) -> String {
    let action = match reason {
        agent_doc_harness::CODEX_UPDATE_AVAILABLE_BLOCKER => {
            "operator_action=\"choose Update now or Skip (esc) in the pane; agent-doc never answers this dialog\""
        }
        agent_doc_harness::CODEX_CONVERSATION_OPEN_ELSEWHERE_BLOCKER => {
            "operator_action=\"close the conversation in the other app, then press r in the pane\""
        }
        "active permission prompt" => {
            "operator_action=\"answer the permission prompt in the pane\""
        }
        _ => "operator_action=\"restore an idle prompt\"",
    };
    format!("live_pane_blocker: reason=\"{reason}\" {action}")
}

fn owned_pane_ready_busy_conflict(ctx: &SessionContext, evidence: &LivePaneEvidence) -> bool {
    if evidence.state != LivePaneState::AliveBusy || evidence.prompt_ready != Some(false) {
        return false;
    }
    let Some(record) = ctx.actor_record.as_ref() else {
        return false;
    };
    if record.state != ActorState::Ready {
        return false;
    }
    if ctx.supervisor_runtime.health != SupervisorHealth::Healthy
        || ctx.supervisor_runtime.actor_state != Some(ActorState::Ready)
    {
        return false;
    }
    ctx.operator_status
        .supervisor_lease
        .as_ref()
        .filter(|lease| lease.generation == record.generation)
        .and_then(|lease| lease.runtime_state.as_deref())
        .is_some_and(|state| state == ActorState::Ready.as_str())
}

fn capability_proof_status(ctx: &SessionContext) -> String {
    let content = match std::fs::read_to_string(&ctx.canonical_file) {
        Ok(content) => content,
        Err(err) => return format!("unknown (failed to read document: {err})"),
    };
    let fm = match agent_doc_frontmatter_io::session::parse_for_file(&content, &ctx.canonical_file)
    {
        Ok((fm, _)) => fm,
        Err(err) => return format!("unknown (failed to parse frontmatter: {err})"),
    };
    #[cfg(test)]
    let global_config = agent_doc_config::Config::default();
    #[cfg(not(test))]
    let global_config = agent_doc_project_config_io::apply_project_defaults(
        agent_doc_config::load().unwrap_or_default(),
        &agent_doc_project_config_io::load_project_for_doc(&ctx.canonical_file),
    );
    if !agent_doc_agent_io::agent::codex::managed_capability_contract_required_for_doc_and_harness(
        &ctx.canonical_file,
        &fm,
        &global_config,
        &ctx.harness,
    ) {
        return "not_required".to_string();
    }
    let expected_writable_contract = if ctx.harness == "codex" {
        agent_doc_agent_io::agent::codex::managed_writable_root_contract_id_for_doc(
            &ctx.canonical_file,
            &fm,
            &global_config,
        )
    } else {
        None
    };
    let proven_prefix = format!("{}_capability_proof status=proven", ctx.harness);
    let proven_result = if let Some(contract) = expected_writable_contract.as_deref() {
        agent_doc_supervisor_io::startup_miss::session_log_has_event_after_latest_start_containing(
            &ctx.canonical_file,
            &ctx.session_id,
            &proven_prefix,
            &format!("writable_root_contract={contract}"),
        )
    } else {
        agent_doc_supervisor_io::startup_miss::session_log_has_event_after_latest_start(
            &ctx.canonical_file,
            &ctx.session_id,
            &proven_prefix,
        )
    };
    if let Err(err) = proven_result {
        return format!("unknown ({err})");
    }
    if matches!(proven_result, Ok(true)) {
        return "proven".to_string();
    }
    for status in ["proven", "failed", "pending"] {
        if status == "proven" && expected_writable_contract.is_some() {
            continue;
        }
        match agent_doc_supervisor_io::startup_miss::session_log_has_event_after_latest_start(
            &ctx.canonical_file,
            &ctx.session_id,
            &format!("{}_capability_proof status={}", ctx.harness, status),
        ) {
            Ok(true) => return status.to_string(),
            Ok(false) => {}
            Err(err) => return format!("unknown ({err})"),
        }
    }
    "missing".to_string()
}

fn collect_doctor_issues(ctx: &SessionContext) -> Vec<String> {
    let mut issues = Vec::new();
    // `#pluginactivationprobe`: a registration-INDEPENDENT activation check.
    // `stale_plugin` only fires from live editor registrations, so it stays
    // silent exactly when a hand-recorded (plugin version, IDE pid, start time)
    // premise goes stale — with no editor attached. Comparing the installed jar's
    // mtime against each live IDE's process start time needs no registration and
    // no operator bookkeeping.
    if let Some(issue) =
        crate::plugin_activation::probe(&crate::plugin::jetbrains_plugin_dirs()).doctor_issue()
    {
        issues.push(issue);
    }
    if ctx.actor_record.is_none() {
        issues.push("authoritative actor record is missing".to_string());
    }
    if ctx.registry_entry.is_none() {
        issues.push("durable registry has no live entry for this document session".to_string());
    }
    if matches!(ctx.supervisor_runtime.health, SupervisorHealth::NoSocket) {
        issues.push("no supervisor socket is present for the tracked session".to_string());
    }
    if matches!(ctx.supervisor_runtime.health, SupervisorHealth::Unreachable) {
        issues.push("supervisor socket exists but did not answer the state probe".to_string());
    }
    if let (Some(actor), Some(entry)) = (&ctx.actor_record, &ctx.registry_entry) {
        if actor.pane_id != entry.pane {
            issues.push(format!(
                "actor pane {} disagrees with registry pane {}",
                actor.pane_id, entry.pane
            ));
        }
        if actor.window_id != entry.window {
            issues.push(format!(
                "actor window {} disagrees with registry window {}",
                actor.window_id, entry.window
            ));
        }
    }
    // Record-vs-record above cannot see a pane that MOVED: both records keep the
    // window captured at bind time, so a pane stashed after binding still reads as
    // co-visible and editor focus mirrors onto a pane the operator cannot see.
    // Compare the recorded binding against where the pane actually is.
    if let Some(live_window) = ctx.live_pane_window.as_deref() {
        for (source, pane, recorded_window) in [
            ctx.actor_record
                .as_ref()
                .map(|actor| ("actor", actor.pane_id.as_str(), actor.window_id.as_str())),
            ctx.registry_entry
                .as_ref()
                .map(|entry| ("registry", entry.pane.as_str(), entry.window.as_str())),
        ]
        .into_iter()
        .flatten()
        {
            if agent_doc_controller::pane_layout::pane_window_binding_drifted(
                recorded_window,
                Some(live_window),
            ) {
                issues.push(format!(
                    "{source} window {recorded_window} for pane {pane} is stale — the pane now lives in {live_window} (a pane moved out of the visible window cannot mirror editor focus; repair with `agent-doc session doctor --repair`)"
                ));
            }
        }
    }
    if let Some(status) = &ctx.log_status
        && status.latest_session_open()
        && matches!(ctx.supervisor_runtime.health, SupervisorHealth::NoSocket)
    {
        issues.push(
            "session log still shows an open run, but no live supervisor socket is available"
                .to_string(),
        );
    }
    if let Some(miss) = &ctx.startup_miss {
        issues.push(format!(
            "startup-miss marker is still recorded for pane {}",
            miss.pane_id
        ));
    }
    if let Some(actor) = &ctx.actor_record
        && actor.state == ActorState::Closed
        && !actor.pane_id.is_empty()
    {
        issues.push(format!(
            "closed actor record still references pane {}",
            actor.pane_id
        ));
    }
    for diagnostic in &ctx.operator_status.projection_diagnostics {
        issues.push(format!(
            "controller projection drift in {}: {}",
            diagnostic.projection, diagnostic.message
        ));
    }
    if let Some(attempt) = ctx.operator_status.dispatch_attempts.last()
        && let Some(stage) = attempt.failed_stage.as_deref()
    {
        issues.push(format!(
            "last controller command `{}` failed at stage {}",
            attempt.command_kind, stage
        ));
    }
    issues
}

fn empty_or_placeholder(value: &str) -> &str {
    if value.trim().is_empty() {
        "none"
    } else {
        value
    }
}

fn timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    #[test]
    fn session_id_comes_from_current_content_when_disk_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        let current = "---\nagent_doc_session: live-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n";

        assert_eq!(
            super::session_id_from_current_content(&doc, current).unwrap(),
            "live-session"
        );
        assert_eq!(
            agent_doc_frontmatter_io::session::read_session_id(&doc),
            None,
            "the regression requires disk to remain stale"
        );
    }

    #[test]
    fn status_harness_prefers_document_then_actor_over_ambient_detection() {
        assert_eq!(
            super::status_harness(
                Some("opencode".into()),
                Some("claude"),
                Some("codex"),
                "codex".into()
            ),
            "opencode"
        );
        assert_eq!(
            super::status_harness(None, Some("claude"), Some("codex"), "codex".into()),
            "claude"
        );
        assert_eq!(
            super::status_harness(None, None, None, "codex".into()),
            "codex"
        );
    }

    /// gh-266: an actor record bound from a caller outside every harness
    /// carries `default`, and the clear probe then ran with `harness=default`
    /// on a pane whose supervisor logged `harness=claude`. `default` is never
    /// an authority; the live supervisor's harness resolves it.
    #[test]
    fn status_harness_resolves_default_from_the_live_supervisor_harness() {
        assert_eq!(
            super::status_harness(None, Some("default"), Some("claude"), "default".into()),
            "claude-code"
        );
        assert_eq!(
            super::status_harness(None, None, Some("codex"), "default".into()),
            "codex"
        );
        assert_eq!(
            super::status_harness(
                Some("default".into()),
                Some(""),
                Some("claude"),
                "default".into()
            ),
            "claude-code"
        );
        // A named actor record still outranks the supervisor and ambient
        // detection; ambient only decides when nothing else is named.
        assert_eq!(
            super::status_harness(None, Some("codex"), Some("claude"), "default".into()),
            "codex"
        );
        assert_eq!(
            super::status_harness(None, Some("default"), None, "claude-code".into()),
            "claude-code"
        );
        // Nothing resolves: the ambient label stands.
        assert_eq!(
            super::status_harness(None, Some("default"), Some(" "), "default".into()),
            "default"
        );
    }

    /// `#restartselfpane` / `#restartselfdefer`: detecting that the caller IS the
    /// target is only half the decision. With a live supervisor the restart
    /// drains to the turn boundary and supersedes in place, so a self-request is
    /// safe — the requesting turn finishing IS the deferral. Refusing it there
    /// would break the healthy path. It is fatal only with no supervisor to defer
    /// to, when the request escalates to a cold start that must quit this pane.
    #[test]
    fn restart_detects_when_the_caller_is_the_target_pane() {
        // SAFETY: single-threaded test-local env mutation.
        unsafe { std::env::set_var("TMUX_PANE", "%53") };
        assert!(
            super::restart_targets_the_calling_pane(Some("%53")),
            "a restart aimed at the caller's own pane must be recognised"
        );
        assert!(
            !super::restart_targets_the_calling_pane(Some("%54")),
            "a different pane is an ordinary restart"
        );
        assert!(
            !super::restart_targets_the_calling_pane(None),
            "an unknown owner pane proves nothing"
        );
        assert!(
            !super::restart_targets_the_calling_pane(Some("  ")),
            "a blank owner pane proves nothing"
        );
        unsafe { std::env::remove_var("TMUX_PANE") };
        assert!(
            !super::restart_targets_the_calling_pane(Some("%53")),
            "outside tmux there is no calling pane to detect"
        );
    }

    use super::*;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    fn background_replacement_acceptance_is_not_reported_as_completion_proof() {
        let (warning, message) =
            supervisor_replacement_outcome_message(Path::new("tasks/contracts.md"), true);
        assert!(!warning);
        assert!(message.contains("completion proof is pending"));
        assert!(!message.contains("proven live"));
    }

    // `#stale-actor-pane-collision`: bugs2's supervised claude exited and its pane
    // %85 was reused by another Claude session while the stale actor record still
    // claimed it. With no reachable supervisor to vouch for the pane, a recorded pid
    // that no longer lives in the pane's process tree means a foreign reuse, so the
    // evidence must be a stale projection, not `alive-idle`.
    #[test]
    fn foreign_reuse_when_unreachable_supervisor_and_recorded_pid_absent_from_pane() {
        assert!(evidence_pane_is_foreign_reuse(
            "authoritative_actor",
            &SupervisorHealth::Unreachable,
            Some(605459),
            false, // recorded pid no longer in the pane's process tree
        ));
        // NoSocket + registry-sourced pane is the same collision.
        assert!(evidence_pane_is_foreign_reuse(
            "registry",
            &SupervisorHealth::NoSocket,
            Some(605459),
            false,
        ));
    }

    #[test]
    fn not_foreign_reuse_when_recorded_pid_still_owns_pane() {
        assert!(!evidence_pane_is_foreign_reuse(
            "authoritative_actor",
            &SupervisorHealth::Unreachable,
            Some(605459),
            true, // recorded pid still lives in the pane
        ));
    }

    #[test]
    fn not_foreign_reuse_when_supervisor_reachable() {
        // A healthy/reachable supervisor is authoritative on its own; never downgrade.
        assert!(!evidence_pane_is_foreign_reuse(
            "authoritative_actor",
            &SupervisorHealth::Healthy,
            Some(605459),
            false,
        ));
        assert!(!evidence_pane_reuse_cheap_gate(
            "authoritative_actor",
            &SupervisorHealth::Healthy
        ));
    }

    #[test]
    fn not_foreign_reuse_without_recorded_pid_or_from_live_session_log() {
        // No recorded owner pid to check against → keep prior aliveness evidence.
        assert!(!evidence_pane_is_foreign_reuse(
            "authoritative_actor",
            &SupervisorHealth::Unreachable,
            None,
            true,
        ));
        // A pane resolved from the live session log (not a recorded owner) is not
        // subject to the recorded-owner provenance check.
        assert!(!evidence_pane_reuse_cheap_gate(
            "session_log",
            &SupervisorHealth::Unreachable
        ));
    }

    fn empty_operator_status(
        record: Option<agent_doc_controller::actor::ActorRecord>,
    ) -> SessionOperatorStatus {
        SessionOperatorStatus {
            record,
            transitions: Vec::new(),
            supervisor_lease: None,
            dispatch_attempts: Vec::new(),
            projection_diagnostics: Vec::new(),
        }
    }

    // --- `#supdead-coldstart-fallback` dead-supervisor recovery decision ---

    fn dead_inputs() -> DeadSupervisorRecoveryInputs {
        DeadSupervisorRecoveryInputs {
            socket_dead: true,
            caller_is_own_ancestor: false,
            can_resolve_tmux_target: true,
        }
    }

    #[test]
    fn live_supervisor_takes_in_place_path() {
        let file = Path::new("/tmp/doc.md");
        let sock = Path::new("/tmp/x.sock");
        let inputs = DeadSupervisorRecoveryInputs {
            socket_dead: false,
            ..dead_inputs()
        };
        assert_eq!(
            decide_dead_supervisor_recovery(file, sock, &inputs),
            DeadSupervisorRecovery::InPlaceLive
        );
    }

    #[test]
    fn dead_supervisor_cold_starts_when_safe() {
        let file = Path::new("/tmp/doc.md");
        let sock = Path::new("/tmp/x.sock");
        assert_eq!(
            decide_dead_supervisor_recovery(file, sock, &dead_inputs()),
            DeadSupervisorRecovery::ColdStart
        );
    }

    #[test]
    fn dead_supervisor_cold_start_prefers_alive_authoritative_actor_pane() {
        let record = test_actor_record(ActorState::Ready);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Ready)),
            None,
        );

        let target = dead_supervisor_cold_start_target(&ctx, |pane| pane == "%7");

        assert_eq!(
            target,
            DeadSupervisorColdStartTarget::PreserveActorPane("%7".to_string())
        );
    }

    #[test]
    fn dead_supervisor_cold_start_falls_back_when_actor_pane_is_unavailable() {
        let record = test_actor_record(ActorState::Ready);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Ready)),
            None,
        );

        assert_eq!(
            dead_supervisor_cold_start_target(&ctx, |_| false),
            DeadSupervisorColdStartTarget::RouteAutoStart
        );
    }

    #[test]
    fn dead_supervisor_cold_start_ignores_closed_actor_pane() {
        let record = test_actor_record(ActorState::Closed);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Closed)),
            None,
        );

        assert_eq!(
            dead_supervisor_cold_start_target(&ctx, |pane| pane == "%7"),
            DeadSupervisorColdStartTarget::RouteAutoStart
        );
    }

    #[test]
    fn dead_supervisor_start_command_quotes_binary_and_file() {
        let command = dead_supervisor_start_command(
            "/tmp/agent doc/bin/agent-doc",
            Path::new("/tmp/project/tasks/owner's doc.md"),
        );

        assert_eq!(
            command,
            "'/tmp/agent doc/bin/agent-doc' start --route-owned '/tmp/project/tasks/owner'\\''s doc.md'"
        );
    }

    #[test]
    fn dead_supervisor_own_ancestor_returns_guidance_not_forced_spawn() {
        let file = Path::new("/tmp/doc.md");
        let sock = Path::new("/tmp/x.sock");
        let inputs = DeadSupervisorRecoveryInputs {
            caller_is_own_ancestor: true,
            ..dead_inputs()
        };
        match decide_dead_supervisor_recovery(file, sock, &inputs) {
            DeadSupervisorRecovery::Guidance(msg) => {
                assert!(msg.contains("own pane/ancestor"), "msg: {msg}");
                assert!(msg.contains("dead"), "msg: {msg}");
                assert!(
                    !msg.contains("os error 111") && !msg.contains("Connection refused"),
                    "guidance must not surface raw ECONNREFUSED: {msg}"
                );
            }
            other => panic!("expected Guidance, got {other:?}"),
        }
    }

    #[test]
    fn dead_supervisor_no_tmux_target_returns_guidance() {
        let file = Path::new("/tmp/doc.md");
        let sock = Path::new("/tmp/x.sock");
        let inputs = DeadSupervisorRecoveryInputs {
            can_resolve_tmux_target: false,
            ..dead_inputs()
        };
        match decide_dead_supervisor_recovery(file, sock, &inputs) {
            DeadSupervisorRecovery::Guidance(msg) => {
                assert!(msg.contains("no tmux target session"), "msg: {msg}");
                assert!(
                    !msg.contains("os error 111"),
                    "guidance must not surface raw ECONNREFUSED: {msg}"
                );
            }
            other => panic!("expected Guidance, got {other:?}"),
        }
    }

    #[test]
    fn own_ancestor_gate_precedes_tmux_target_gate() {
        // When BOTH the caller-is-ancestor risk and the no-tmux condition hold,
        // the safety refusal (own-ancestor) must win over the tmux guidance so we
        // never imply a forced spawn is the remedy.
        let file = Path::new("/tmp/doc.md");
        let sock = Path::new("/tmp/x.sock");
        let inputs = DeadSupervisorRecoveryInputs {
            caller_is_own_ancestor: true,
            can_resolve_tmux_target: false,
            ..dead_inputs()
        };
        match decide_dead_supervisor_recovery(file, sock, &inputs) {
            DeadSupervisorRecovery::Guidance(msg) => {
                assert!(msg.contains("own pane/ancestor"), "msg: {msg}")
            }
            other => panic!("expected own-ancestor Guidance, got {other:?}"),
        }
    }

    fn test_actor_record(state: ActorState) -> agent_doc_controller::actor::ActorRecord {
        agent_doc_controller::actor::ActorRecord {
            document_id: "/tmp/doc.md".to_string(),
            session_id: "session-1".to_string(),
            generation: 7,
            pane_id: "%7".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
                caller: "test".to_string(),
                reason: "test".to_string(),
                timestamp: 1,
                prior_generation: 7,
                new_generation: 7,
            },
        }
    }

    fn test_supervisor_runtime(actor_state: Option<ActorState>) -> SupervisorRuntime {
        SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state,
            actor_session_id: Some("session-1".to_string()),
            actor_pane_id: Some("%7".to_string()),
            actor_generation: Some(7),
            supervisor_state: Some("healthy".to_string()),
            restart_count: 0,
            supervisor_pid: Some(100),
            supervisor_instance_id: Some("sup-1".to_string()),
            child_pid: Some(101),
            cwd_source: Some("config".to_string()),
            current_harness: None,
        }
    }

    fn test_session_context(
        record: agent_doc_controller::actor::ActorRecord,
        runtime: SupervisorRuntime,
        lease_state: Option<&str>,
    ) -> SessionContext {
        let lease = lease_state.map(|state| SupervisorLeaseStatus {
            generation: 7,
            supervisor_pid: Some(100),
            supervisor_socket: Some("/tmp/supervisor.sock".to_string()),
            last_heartbeat: Some(1),
            runtime_state: Some(state.to_string()),
        });
        SessionContext {
            canonical_file: PathBuf::from("/tmp/doc.md"),
            base_dir: PathBuf::from("/tmp"),
            session_id: "session-1".to_string(),
            harness: "codex".to_string(),
            actor_record: Some(record.clone()),
            operator_status: SessionOperatorStatus {
                record: Some(record),
                transitions: Vec::new(),
                supervisor_lease: lease,
                dispatch_attempts: Vec::new(),
                projection_diagnostics: Vec::new(),
            },
            registry_entry: None,
            startup_miss: None,
            log_status: None,
            supervisor_runtime: runtime,
            supervisor_socket: PathBuf::from("/tmp/supervisor.sock"),
            live_pane_window: None,
        }
    }

    fn test_registry_entry(session_id: &str, file: &str, pane: &str) -> SessionEntry {
        SessionEntry {
            pane: pane.to_string(),
            pid: 100,
            cwd: "/tmp/project".to_string(),
            started: "now".to_string(),
            session_id: session_id.to_string(),
            file: file.to_string(),
            window: "@1".to_string(),
            supervisor_instance_id: "sup".to_string(),
        }
    }

    #[test]
    fn registry_lookup_rejects_same_session_entry_for_foreign_document_suprestassoc() {
        // #suprestassoc: restart-supervisor must not combine the target document
        // with stale durable registry metadata from a different document that happens
        // to carry the same session id.
        let dir = tempfile::TempDir::new().unwrap();
        let base = dir.path();
        let target = base.join("tasks/sampleportal.md");
        let foreign = base.join("tasks/agent-doc-bugs2.md");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "").unwrap();
        std::fs::write(&foreign, "").unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            foreign.to_string_lossy().to_string(),
            test_registry_entry("shared-session", &foreign.to_string_lossy(), "%12"),
        );

        assert!(
            find_registry_entry(base, &registry, "shared-session", &target).is_none(),
            "foreign document projection must not be adopted by session-id match"
        );
    }

    #[test]
    fn registry_lookup_prefers_exact_document_over_foreign_same_session_suprestassoc() {
        let dir = tempfile::TempDir::new().unwrap();
        let base = dir.path();
        let target = base.join("tasks/sampleportal.md");
        let foreign = base.join("tasks/agent-doc-bugs2.md");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "").unwrap();
        std::fs::write(&foreign, "").unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            foreign.to_string_lossy().to_string(),
            test_registry_entry("shared-session", &foreign.to_string_lossy(), "%12"),
        );
        registry.insert(
            target.to_string_lossy().to_string(),
            test_registry_entry("shared-session", "tasks/sampleportal.md", "%14"),
        );

        let entry = find_registry_entry(base, &registry, "shared-session", &target)
            .expect("target document entry should be found");
        assert_eq!(entry.pane, "%14");
        assert_eq!(entry.file, "tasks/sampleportal.md");
    }

    #[test]
    fn parse_actor_state_handles_known_values() {
        assert_eq!(parse_actor_state("ready"), Some(ActorState::Ready));
        assert_eq!(
            parse_actor_state("waiting_input"),
            Some(ActorState::WaitingInput)
        );
        assert_eq!(parse_actor_state("unknown"), None);
    }

    #[test]
    fn live_pane_prompt_ready_detects_idle_opencode_prompt() {
        let harness = agent_doc_harness::HarnessConfig::opencode();

        assert!(live_pane_prompt_ready(&harness, "work complete\n>\n"));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_opencode_status_chrome_without_proof_output() {
        let harness = agent_doc_harness::HarnessConfig::opencode();

        assert!(live_pane_prompt_ready(
            &harness,
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_opencode_idle_splash_without_prompt_glyph() {
        let harness = agent_doc_harness::HarnessConfig::opencode();

        assert!(live_pane_prompt_ready(
            &harness,
            "\
                                                                                                     ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▄ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀
                                                                                   ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                                   ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                                   ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                                   tab agents  ctrl+p commands
                                                                                        ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_status_chrome_only_output() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_xhigh_status_chrome_only_output() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 41% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_footer_below_prior_output() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "\
### Re: prior turn
The valid choices in that state are wait for the prompt, refresh after it returns idle, or use explicit clear.
gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_codex_drafted_input_above_footer() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(!live_pane_prompt_ready(
            &harness,
            "\
› investigate this issue
gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used
"
        ));
    }

    #[test]
    fn short_blank_stash_viewport_is_unobservable_not_busy() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(live_pane_viewport_unobservable(
            &harness,
            "\n",
            Some(1),
            false
        ));
    }

    #[test]
    fn live_pane_blocker_status_names_codex_update_dialog_without_clear_hint() {
        let line = live_pane_blocker_status_line(agent_doc_harness::CODEX_UPDATE_AVAILABLE_BLOCKER);
        assert!(
            line.contains("reason=\"codex update-available dialog\""),
            "{line}"
        );
        assert!(line.contains("Update now or Skip"), "{line}");
        assert!(line.contains("never answers"), "{line}");
        assert!(blocker_supersedes_ready_busy_recovery_hint(
            agent_doc_harness::CODEX_UPDATE_AVAILABLE_BLOCKER
        ));
        assert!(!blocker_supersedes_ready_busy_recovery_hint(
            "queued draft in composer"
        ));
    }

    #[test]
    fn short_stash_viewport_preserves_explicit_draft_blocker() {
        let harness = agent_doc_harness::HarnessConfig::codex();
        let captured = "\
› investigate this issue
gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used
";

        assert!(matches!(
            agent_doc_harness::project_pane_composer_at_cursor(captured, &harness, None),
            agent_doc_harness::PaneComposerProjection::OperatorDraft { .. }
        ));
        assert!(!live_pane_viewport_unobservable(
            &harness,
            captured,
            Some(1),
            false
        ));
    }

    #[test]
    fn normal_sized_non_prompt_viewport_remains_busy() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(!live_pane_viewport_unobservable(
            &harness,
            "working\n",
            Some(20),
            false
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_default_placeholder() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "\
› Ask Codex to do anything
gpt-5.5 high · ~/work/btakita/agent-loop · Context 55% used
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_write_tests_placeholder() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "\
› Write tests for @filename
gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_codex_working_status_above_placeholder() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(!live_pane_prompt_ready(
            &harness,
            "\
• Working (1m 34s • esc to interrupt)

› Write tests for @filename
gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_active_output_after_prompt() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(!live_pane_prompt_ready(
            &harness,
            "›\nexploring repository\n"
        ));
    }

    // #jb-stale-busy-idle-footer: a genuinely idle Claude pane (composer + status
    // + permissions) must project ready, while a mid-turn pane (spinner) must not.
    #[test]
    fn live_pane_prompt_ready_accepts_idle_claude_composer() {
        let harness = agent_doc_harness::HarnessConfig::claude();
        let idle = concat!(
            "────────────────────\n",
            "❯\n",
            "────────────────────\n",
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@cachyos-x8664\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)\n",
        );
        assert!(live_pane_prompt_ready(&harness, idle));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_idle_claude_with_artifact_attachment_chip() {
        let harness = agent_doc_harness::HarnessConfig::claude();
        let idle = concat!(
            "────────────────────\n",
            "❯\n",
            "────────────────────\n",
            "  Opus 4.8 ctx:24% ~/work/project main brian@host\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents\n",
            "  ⧉  arbitrary-session-artifact-label\n",
        );

        assert!(live_pane_prompt_ready(&harness, idle));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_busy_claude_turn() {
        let harness = agent_doc_harness::HarnessConfig::claude();
        // Mid-turn: spinner above an otherwise-idle-looking composer. The busy cue
        // must win so the live turn is never clobbered by dispatch/clear.
        let busy = concat!(
            "· Roosting… (14s · ↓ 487 tokens · thinking with high effort)\n",
            "────────────────────\n",
            "❯\n",
            "────────────────────\n",
            "  Opus 4.8 (1M context) ctx:23% ~/work/btakita/agent-loop/resume main brian@host\n",
            "  ⏵⏵ bypass permissions on · 1 shell\n",
        );
        assert!(!live_pane_prompt_ready(&harness, busy));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_claude_idle_footer_after_stale_busy_scrollback() {
        let harness = agent_doc_harness::HarnessConfig::claude();
        let idle_after_clear = concat!(
            "✶ Generating… (3s · esc to interrupt)\n",
            "  ❯ /clear\n",
            "────────────────────\n",
            "❯ Press up to edit queued messages\n",
            "────────────────────\n",
            "  Opus 4.8 ctx:10% ~/work/btakita/agent-loop main brian@host\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents\n",
        );

        assert!(live_pane_prompt_ready(&harness, idle_after_clear));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_claude_active_spinner_footer() {
        let harness = agent_doc_harness::HarnessConfig::claude();
        let active = concat!(
            "✶ Generating… (3s · esc to interrupt)\n",
            "❯\n",
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@host\n",
            "  ⏵⏵ bypass permissions on · 1 shell\n",
        );

        assert!(!live_pane_prompt_ready(&harness, active));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_idle_claude_when_status_line_is_last() {
        // Status output is user-configurable. Cursor-scoped proof resolves the
        // composer without interpreting any text rendered below it.
        let harness = agent_doc_harness::HarnessConfig::claude();
        let idle = concat!(
            "❯\n",
            "────────────────────────────────────────────────────────────────\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)\n",
            "  anything the user's status command chooses to print\n",
        );
        assert!(live_pane_prompt_ready_at_cursor(&harness, idle, Some(0)));
    }

    #[test]
    fn live_pane_prompt_ready_opencode_context_bar_idle_hint() {
        let harness = agent_doc_harness::HarnessConfig::opencode();
        let idle = "⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13\n";
        assert!(live_pane_prompt_ready(&harness, idle));
    }

    #[test]
    fn live_pane_prompt_ready_opencode_context_bar_with_scrollback() {
        let harness = agent_doc_harness::HarnessConfig::opencode();
        let idle = concat!(
            "Thought: I need to check the files\n",
            "Click to expand\n",
            "  ~/work/btakita/agent-loop:main                                        1.15.13\n",
            "⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13\n",
        );
        assert!(live_pane_prompt_ready(&harness, idle));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_opencode_active_turn_with_context_bar() {
        let harness = agent_doc_harness::HarnessConfig::opencode();
        let busy = concat!(
            "Working (14s - esc to interrupt)\n",
            "⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13\n",
        );
        assert!(!live_pane_prompt_ready(&harness, busy));
    }

    #[test]
    fn operator_clear_allows_agent_doc_wrapper_without_busy_cue() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 41% used".to_string()),
        };

        let state = operator_clear_input_state_for_evidence(&evidence, false, false, false, false);

        assert_eq!(state, OperatorClearInputState::IdlePrompt);
        assert_eq!(
            agent_doc_controller::operator_clear::clear_guard_outcome(state),
            OperatorClearGuardOutcome::Completed
        );
    }

    #[test]
    fn operator_clear_blocks_explicit_busy_cue() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("Working...".to_string()),
        };

        let state = operator_clear_input_state_for_evidence(&evidence, false, false, true, false);

        assert_eq!(state, OperatorClearInputState::Busy);
        assert_eq!(
            agent_doc_controller::operator_clear::clear_guard_outcome(state),
            OperatorClearGuardOutcome::Blocked
        );
    }

    /// GH #232: the supervisor's live-turn evidence
    /// outranks an idle-looking composer and the unclassified-busy fallback, so
    /// the guard never completes as `idle_prompt` while a turn runs.
    #[test]
    fn operator_clear_live_turn_outranks_idle_prompt_and_busy_fallback() {
        let idle = LivePaneEvidence {
            pane_id: Some("%6".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveIdle,
            current_command: Some("claude".to_string()),
            prompt_ready: Some(true),
            tail: Some("\u{276f}".to_string()),
        };
        assert_eq!(
            operator_clear_input_state_for_evidence(&idle, false, false, false, true),
            OperatorClearInputState::LiveTurn
        );
        assert_eq!(
            operator_clear_input_state_for_evidence(&idle, false, false, false, false),
            OperatorClearInputState::IdlePrompt
        );

        let busy_no_cue = LivePaneEvidence {
            state: LivePaneState::AliveBusy,
            prompt_ready: Some(false),
            ..idle.clone()
        };
        let state =
            operator_clear_input_state_for_evidence(&busy_no_cue, false, false, false, true);
        assert_eq!(state, OperatorClearInputState::LiveTurn);
        assert_eq!(
            agent_doc_controller::operator_clear::clear_guard_outcome(state),
            OperatorClearGuardOutcome::Blocked
        );
        assert_eq!(
            operator_clear_input_state_for_evidence(&busy_no_cue, false, false, true, true),
            OperatorClearInputState::LiveTurn,
            "live-turn evidence is reported in preference to a scraped busy cue"
        );

        // Distinct refusals keep their own reasons: protected input stays
        // fail-closed, and a clean-exit prompt means the harness is gone.
        assert_eq!(
            operator_clear_input_state_for_evidence(&busy_no_cue, true, false, false, true),
            OperatorClearInputState::ProtectedInput
        );
        assert_eq!(
            operator_clear_input_state_for_evidence(&busy_no_cue, false, true, false, true),
            OperatorClearInputState::CleanExit
        );
        // No live pane: live-turn evidence has nothing to protect.
        let closed = LivePaneEvidence {
            state: LivePaneState::ClosedClean,
            ..idle
        };
        assert_eq!(
            operator_clear_input_state_for_evidence(&closed, false, false, false, true),
            OperatorClearInputState::NoLivePane
        );
    }

    /// GH #232 timeline against a real project `state.db`: the actor projects
    /// `busy`, the pane renders an idle composer, and the supervisor's live-turn
    /// evidence is present. The guard reads the same evidence the supervisor's
    /// `harness_turn_live` probe reads, and the idle-projection reconcile refuses
    /// to project `ready` (which would release the dispatch in-flight marker).
    #[test]
    fn clear_guard_reads_supervisor_live_turn_evidence_and_never_projects_ready() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("tasks/doc-a.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: session-1\n---\n\nBody.\n").unwrap();

        let mut ctx = test_session_context(
            test_actor_record(ActorState::Busy),
            test_supervisor_runtime(Some(ActorState::Busy)),
            Some("busy"),
        );
        ctx.canonical_file = doc.clone();
        ctx.base_dir = root.clone();
        let idle = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveIdle,
            current_command: Some("claude".to_string()),
            prompt_ready: Some(true),
            tail: Some(">".to_string()),
        };
        assert!(
            idle_projection_needs_reconciliation(&ctx, &idle),
            "precondition: without live-turn evidence this is the stale-busy repair shape"
        );
        assert_eq!(harness_turn_live_evidence(&ctx, &idle), None);

        // 1. The harness `UserPromptSubmit` hook wrote the pane's turn-active lease.
        agent_doc_turn_status_io::write_turn_active_marker(&root, "%7").unwrap();
        assert!(
            agent_doc_turn_status_io::turn_active_for_pane_for_file(&doc, "%7"),
            "the supervisor's harness_turn_live read sees the lease"
        );
        assert_eq!(
            harness_turn_live_evidence(&ctx, &idle),
            Some("turn_active_lease")
        );
        let state = operator_clear_input_state_for_evidence(&idle, false, false, false, true);
        let event = clear_guard_event(state);
        assert_eq!(event.outcome, agent_doc_flow::types::FlowOutcome::Blocked);
        assert_eq!(event.reason.as_deref(), Some("harness_turn_live"));
        // No controller is running here: had the reconcile tried to project
        // `ready` it would have issued the lifecycle RPC. It returns before that.
        assert!(!reconcile_idle_projection_from_evidence(&ctx, &idle).unwrap());
        let ops = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops.contains("session_operator_idle_projection_reconcile_skipped")
                && ops.contains("reason=harness_turn_live evidence=turn_active_lease"),
            "ops.log must record the skipped ready projection: {ops}"
        );
        agent_doc_turn_status_io::clear_turn_active_marker(&root, "%7").unwrap();
        assert_eq!(harness_turn_live_evidence(&ctx, &idle), None);

        // 2. Between a dispatched trigger going busy and the hook writing its
        // lease, the observed running receipt is the live-turn evidence.
        let document_id =
            agent_doc_session_actor_io::canonical_document_id_in(&root, &doc.to_string_lossy());
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        agent_doc_sqlite::state_store::insert_dispatch_attempt_in_db(
            &conn,
            &agent_doc_sqlite::state_store::DispatchAttemptInsert {
                document_id: &document_id,
                generation: 7,
                command_kind: "auto_trigger",
                accepted_stage: Some("operator_busy"),
                failed_stage: None,
                diagnostic_payload: "{}",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )
        .unwrap();
        // `#clearqueuedliveturn`: an accepted dispatch whose turn has not
        // started is pending evidence — a clear typed now would race the
        // trigger into the composer — but it is not yet a running turn.
        assert_eq!(
            harness_turn_live_evidence(&ctx, &idle),
            Some("dispatch_turn_pending"),
            "a pre-turn receipt is a pending dispatched turn, not a running one"
        );
        assert_eq!(
            agent_doc_sqlite::state_store::mark_open_dispatches_turn_started(&conn, &document_id)
                .unwrap(),
            1
        );
        assert_eq!(
            harness_turn_live_evidence(&ctx, &idle),
            Some("dispatch_turn_running")
        );
        // `#queuedclearflake`: a blank viewport reads the same harness evidence.
        let blank = LivePaneEvidence {
            state: LivePaneState::AliveUnobservable,
            prompt_ready: None,
            tail: None,
            ..idle.clone()
        };
        assert_eq!(
            harness_turn_live_evidence(&ctx, &blank),
            Some("dispatch_turn_running")
        );
        assert!(!reconcile_idle_projection_from_evidence(&ctx, &idle).unwrap());
        assert!(
            agent_doc_sqlite::state_store::has_open_in_flight_dispatch(&conn, &document_id, 7)
                .unwrap(),
            "the dispatch in-flight marker stays held while the turn runs"
        );
    }

    /// `#sdkclearwedge` replay (haiven-dev `tasks/sdk.md`, pane %180,
    /// 2026-10-11): a route `dispatch_only_reopen` submitted `/agent-doc`, the
    /// `UserPromptSubmit` hook wrote the lease and preflight promoted the
    /// receipt to `running`, and the operator cancelled before Claude Code
    /// persisted the prompt. The pane sat idle at its prompt with `← 4 agents`
    /// in the footer, and every clear refused `harness_turn_live
    /// evidence=turn_active_lease`. Once the orphaned snapshot settles, the
    /// guard retires the lease AND the receipt, and the clear proceeds.
    #[test]
    fn clear_guard_allows_clear_after_prompt_cancelled_before_persist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("tasks/sdk.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: session-1\n---\n\nBody.\n").unwrap();
        let mut ctx = test_session_context(
            test_actor_record(ActorState::Ready),
            test_supervisor_runtime(Some(ActorState::Ready)),
            Some("ready"),
        );
        ctx.canonical_file = doc.clone();
        ctx.base_dir = root.clone();
        let idle = LivePaneEvidence {
            pane_id: Some("%180".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveIdle,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(true),
            tail: Some("⏵⏵ bypass permissions on (shift+tab to cycle) · ← 4 agents".to_string()),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let stamp = agent_doc_turn::turn_status::format_rfc3339_utc_secs;
        let transcript = root.join("a07856e8.jsonl");
        let write_incident = |lease_at: u64| {
            let records = [
                serde_json::json!({"type": "assistant", "isSidechain": false, "uuid": "prev-a",
                    "timestamp": stamp(lease_at - 217),
                    "message": {"role": "assistant", "content": [{"type": "text", "text": "The document write didn't complete."}]}}),
                serde_json::json!({"type": "system", "subtype": "stop_hook_summary", "timestamp": stamp(lease_at - 217)}),
                serde_json::json!({"type": "system", "subtype": "turn_duration", "timestamp": stamp(lease_at - 217)}),
                // A background subagent keeps working after the cancel.
                serde_json::json!({"type": "assistant", "isSidechain": true, "timestamp": stamp(lease_at + 1)}),
                serde_json::json!({"type": "file-history-snapshot", "messageId": "7287902a",
                    "snapshot": {"messageId": "7287902a", "trackedFileBackups": {}, "timestamp": stamp(lease_at + 2)},
                    "isSnapshotUpdate": false}),
            ];
            let body: String = records.iter().map(|r| format!("{r}\n")).collect();
            std::fs::write(&transcript, body).unwrap();
            agent_doc_turn_status_io::write_turn_active_marker_with_transcript_at(
                &root,
                "%180",
                lease_at,
                Some(&transcript),
            )
            .unwrap();
        };
        let document_id =
            agent_doc_session_actor_io::canonical_document_id_in(&root, &doc.to_string_lossy());
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        agent_doc_sqlite::state_store::insert_dispatch_attempt_in_db(
            &conn,
            &agent_doc_sqlite::state_store::DispatchAttemptInsert {
                document_id: &document_id,
                generation: 69,
                command_kind: "dispatch_only_reopen",
                accepted_stage: Some("ready"),
                failed_stage: None,
                diagnostic_payload: "{}",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )
        .unwrap();
        assert_eq!(
            agent_doc_sqlite::state_store::mark_open_dispatches_turn_started(&conn, &document_id)
                .unwrap(),
            1
        );

        // Seconds after the cancel the snapshot has not settled: fail closed.
        let lease_at = now - 4;
        conn.execute(
            "UPDATE dispatch_attempts SET timestamp = ?1",
            [i64::try_from(lease_at - 1).unwrap()],
        )
        .unwrap();
        write_incident(lease_at);
        assert_eq!(
            harness_turn_live_evidence(&ctx, &idle),
            Some("turn_active_lease")
        );
        // `#clearctxslow`: the queued clear names the settling abandoned
        // prompt and the seconds left (snapshot 2s old), instead of a live turn.
        let settling =
            settling_abandoned_prompt_for_clear_at(&ctx, "%180", "turn_active_lease", now)
                .expect("an unsettled abandoned prompt behind the lease");
        assert_eq!(settling.snapshot_at, lease_at + 2);
        assert!(
            (agent_doc_turn::turn_status::ABANDONED_PROMPT_SETTLE_SECS - 3
                ..=agent_doc_turn::turn_status::ABANDONED_PROMPT_SETTLE_SECS - 2)
                .contains(&settling.settles_in_secs),
            "{settling:?}"
        );
        assert_eq!(
            settling_abandoned_prompt_for_clear_at(&ctx, "%180", "dispatch_turn_running", now),
            None,
            "only lease evidence can be an abandoned prompt"
        );
        assert_eq!(
            harness_turn_live_evidence(&ctx, &idle),
            Some("turn_active_lease"),
            "naming the wait never shortens it"
        );

        // Settled: the guard retires the lease and its receipt; clear proceeds.
        let lease_at = now - agent_doc_turn::turn_status::ABANDONED_PROMPT_SETTLE_SECS - 10;
        conn.execute(
            "UPDATE dispatch_attempts SET timestamp = ?1",
            [i64::try_from(lease_at - 1).unwrap()],
        )
        .unwrap();
        write_incident(lease_at);
        assert_eq!(
            settling_abandoned_prompt_for_clear_at(&ctx, "%180", "turn_active_lease", now),
            None,
            "a settled snapshot is not waiting on anything"
        );
        assert_eq!(harness_turn_live_evidence(&ctx, &idle), None);
        assert_eq!(
            operator_clear_input_state_for_evidence(&idle, false, false, false, false),
            OperatorClearInputState::IdlePrompt
        );
        assert!(
            !agent_doc_sqlite::state_store::has_running_dispatch_turn_as_of(
                &conn,
                &document_id,
                i64::try_from(now).unwrap()
            )
            .unwrap()
        );
        let receipt = agent_doc_turn_status_io::last_harness_turn_end_for_pane(&root, "%180")
            .expect("abandoned prompt receipt");
        assert_eq!(receipt.reason, "abandoned_prompt");
        let ops = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("turn_status_projection_repaired")
                && ops.contains("reason=abandoned_prompt source=operator_clear_guard")
                && ops.contains("settled_dispatches=1"),
            "{ops}"
        );
    }

    /// `#codexclearwedge` replay (agent-loop `tasks/software/lazily.md`, pane
    /// %121, Codex, 2026-10-12): Run Agent Doc accepted a `dispatch_only_reopen`
    /// at 09:58:08, Codex's `UserPromptSubmit` hook wrote the lease at
    /// 09:58:13, and the operator pressed Ctrl+C at 09:58:14 — before the turn's
    /// preflight (which a Codex turn runs from its first tool call) promoted
    /// the receipt to `running`. The guard retired the lease from the rollout's
    /// `turn_aborted` record (`settled_dispatches=0`), but the still-`accepted`
    /// receipt kept answering `dispatch_turn_pending`, so every Clear press
    /// re-queued (`session_clear_live_turn_deferred
    /// evidence=dispatch_turn_pending rearmed=true`) until the 120s pre-turn
    /// grace expired. Retiring the lease must settle that receipt too.
    #[test]
    fn clear_guard_allows_clear_after_codex_ctrl_c_before_preflight() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("tasks/software/lazily.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: session-1\n---\n\nBody.\n").unwrap();
        let mut ctx = test_session_context(
            test_actor_record(ActorState::Ready),
            test_supervisor_runtime(Some(ActorState::Ready)),
            Some("ready"),
        );
        ctx.canonical_file = doc.clone();
        ctx.base_dir = root.clone();
        let idle = LivePaneEvidence {
            pane_id: Some("%121".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveIdle,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(true),
            tail: Some(
                "■ Conversation interrupted - tell the model what to do differently.".to_string(),
            ),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let stamp = agent_doc_turn::turn_status::format_rfc3339_utc_secs;
        let dispatched = now - 12;
        let lease_at = dispatched + 5;
        let interrupted_at = lease_at + 1;

        let document_id =
            agent_doc_session_actor_io::canonical_document_id_in(&root, &doc.to_string_lossy());
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        agent_doc_sqlite::state_store::insert_dispatch_attempt_in_db(
            &conn,
            &agent_doc_sqlite::state_store::DispatchAttemptInsert {
                document_id: &document_id,
                generation: 300,
                command_kind: "dispatch_only_reopen",
                accepted_stage: Some("ready"),
                failed_stage: None,
                diagnostic_payload: "{}",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE dispatch_attempts SET timestamp = ?1",
            [i64::try_from(dispatched).unwrap()],
        )
        .unwrap();

        // The Codex rollout: prompt submitted, then interrupted. No Stop hook.
        let transcript = root.join("rollout-lazily.jsonl");
        let record = |at: u64, kind: &str, payload: serde_json::Value| serde_json::json!({"timestamp": stamp(at), "type": kind, "payload": payload});
        let records = [
            record(
                lease_at,
                "event_msg",
                serde_json::json!({"type": "task_started"}),
            ),
            record(
                lease_at,
                "response_item",
                serde_json::json!({"type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": "/agent-doc tasks/software/lazily.md"}]}),
            ),
            record(
                lease_at,
                "turn_context",
                serde_json::json!({"cwd": "/repo"}),
            ),
            record(
                interrupted_at,
                "event_msg",
                serde_json::json!({"type": "turn_aborted", "turn_id": "t1",
                    "reason": "interrupted", "duration_ms": 1100}),
            ),
        ];
        let body: String = records.iter().map(|r| format!("{r}\n")).collect();
        std::fs::write(&transcript, body).unwrap();
        agent_doc_turn_status_io::write_turn_active_marker_with_transcript_at(
            &root,
            "%121",
            lease_at,
            Some(&transcript),
        )
        .unwrap();

        // The preflight never ran: the receipt is still pre-turn, not running.
        assert!(
            !agent_doc_sqlite::state_store::has_running_dispatch_turn_as_of(
                &conn,
                &document_id,
                i64::try_from(now).unwrap()
            )
            .unwrap()
        );

        // The guard retires the interrupted lease AND the turn's receipt.
        assert_eq!(harness_turn_live_evidence(&ctx, &idle), None);
        assert_eq!(
            operator_clear_input_state_for_evidence(&idle, false, false, false, false),
            OperatorClearInputState::IdlePrompt
        );
        assert!(
            !agent_doc_controller_io::project_controller::dispatch_turn_pending_for_file(&doc)
                .unwrap(),
            "the interrupted turn's receipt no longer reads as a pending turn"
        );
        let ops = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("turn_status_projection_repaired")
                && ops.contains("source=operator_clear_guard")
                && ops.contains("settled_dispatches=1"),
            "{ops}"
        );

        // A dispatch accepted AFTER the interrupted turn's lease belongs to a
        // newer turn and still holds the clear.
        agent_doc_sqlite::state_store::insert_dispatch_attempt_in_db(
            &conn,
            &agent_doc_sqlite::state_store::DispatchAttemptInsert {
                document_id: &document_id,
                generation: 300,
                command_kind: "dispatch_only_reopen",
                accepted_stage: Some("ready"),
                failed_stage: None,
                diagnostic_payload: "{}",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )
        .unwrap();
        assert_eq!(
            harness_turn_live_evidence(&ctx, &idle),
            Some("dispatch_turn_pending")
        );
    }

    /// GH #317 replay (agent-loop, pane %50, Claude Code, 2026-10-20): Run Agent
    /// Doc's trigger was typed at 01:58:24 but never became a turn (no
    /// `UserPromptSubmit`, no lease, no `dispatch_turn_started`). Clear Session
    /// Context at 01:58:30 withdrew the leftover trigger (`#cancelturnprompt`
    /// `outcome=withdrawn`), re-read the pane — now an idle prompt — and was
    /// still refused as `harness_turn_live evidence=dispatch_turn_pending`, as
    /// was every press after it. The withdrawal settles the never-started
    /// receipt, so that same clear runs at the idle prompt.
    #[test]
    fn clear_runs_after_withdrawing_a_trigger_that_never_became_a_turn() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("doc-a.md");
        std::fs::write(&doc, "---\nagent_doc_session: session-1\n---\n\nBody.\n").unwrap();
        let mut ctx = test_session_context(
            test_actor_record(ActorState::Ready),
            test_supervisor_runtime(Some(ActorState::Ready)),
            Some("ready"),
        );
        ctx.canonical_file = doc.clone();
        ctx.base_dir = root.clone();
        // After the withdrawal the composer is empty: an idle prompt.
        let idle = LivePaneEvidence {
            pane_id: Some("%50".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveIdle,
            current_command: Some("claude".to_string()),
            prompt_ready: Some(true),
            tail: Some("⏵⏵ bypass permissions on (shift+tab to cycle)".to_string()),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let document_id =
            agent_doc_session_actor_io::canonical_document_id_in(&root, &doc.to_string_lossy());
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        agent_doc_sqlite::state_store::insert_dispatch_attempt_in_db(
            &conn,
            &agent_doc_sqlite::state_store::DispatchAttemptInsert {
                document_id: &document_id,
                generation: 6,
                command_kind: "dispatch_only_reopen",
                accepted_stage: Some("ready"),
                failed_stage: None,
                diagnostic_payload: "{}",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE dispatch_attempts SET timestamp = ?1",
            [i64::try_from(now - 7).unwrap()],
        )
        .unwrap();

        // Before the fix: the only evidence is the never-started receipt.
        assert_eq!(
            harness_turn_live_evidence(&ctx, &idle),
            Some(DISPATCH_TURN_PENDING_EVIDENCE)
        );

        // The clear's withdrawal step (virtual clock; no lease ever appears).
        let detail = crate::cancelled_trigger::retire_withdrawn_prompt_turn(
            &doc,
            "%50",
            crate::cancelled_trigger::SNAPSHOT_WINDOW,
            &mut |_| {},
        );
        assert_eq!(
            detail,
            "lease=none reason=trigger_never_submitted settled_pending_dispatches=1"
        );

        // The same clear now sees no live turn and runs at the idle prompt.
        assert_eq!(harness_turn_live_evidence(&ctx, &idle), None);
        assert_eq!(
            operator_clear_input_state_for_evidence(&idle, false, false, false, false),
            OperatorClearInputState::IdlePrompt
        );
        assert!(
            !agent_doc_controller_io::project_controller::dispatch_turn_pending_for_file(&doc)
                .unwrap()
        );

        // A Run Agent Doc accepted after the withdrawal is a new pending turn
        // and still queues the clear behind it.
        agent_doc_sqlite::state_store::insert_dispatch_attempt_in_db(
            &conn,
            &agent_doc_sqlite::state_store::DispatchAttemptInsert {
                document_id: &document_id,
                generation: 6,
                command_kind: "dispatch_only_reopen",
                accepted_stage: Some("ready"),
                failed_stage: None,
                diagnostic_payload: "{}",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )
        .unwrap();
        assert_eq!(
            harness_turn_live_evidence(&ctx, &idle),
            Some(DISPATCH_TURN_PENDING_EVIDENCE)
        );
    }

    /// `#clearaftrun` replay (haiven-dev `tasks/sdk.md`, pane %180,
    /// 2026-10-15): Run Agent Doc was accepted at 19:39:58Z (receipt 3847,
    /// `dispatch_only_reopen`), its route waited ~22s on the previous cycle's
    /// compaction commit before typing the trigger, and Clear Session Context
    /// at 19:40:10Z found only that accepted receipt. The guard queued the
    /// clear (`evidence=dispatch_turn_pending`) and the notice claimed "a turn
    /// is live ... no `/clear` was typed into the running turn" over an idle
    /// pane, offering `interrupt-clear` — which on an idle pane skips the
    /// interrupt and re-enters this same guard. The queue decision stays (a
    /// clear pressed after Run runs after it); the report now says what is
    /// true.
    #[test]
    fn clear_queued_behind_an_accepted_run_reports_no_running_turn() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("tasks/sdk.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: session-1\n---\n\nBody.\n").unwrap();
        let mut ctx = test_session_context(
            test_actor_record(ActorState::Ready),
            test_supervisor_runtime(Some(ActorState::Ready)),
            Some("ready"),
        );
        ctx.canonical_file = doc.clone();
        ctx.base_dir = root.clone();
        let idle = LivePaneEvidence {
            pane_id: Some("%180".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveIdle,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(true),
            tail: Some("⏵⏵ bypass permissions on (shift+tab to cycle) · ← 3 agents".to_string()),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let document_id =
            agent_doc_session_actor_io::canonical_document_id_in(&root, &doc.to_string_lossy());
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        agent_doc_sqlite::state_store::insert_dispatch_attempt_in_db(
            &conn,
            &agent_doc_sqlite::state_store::DispatchAttemptInsert {
                document_id: &document_id,
                generation: 71,
                command_kind: "dispatch_only_reopen",
                accepted_stage: Some("ready"),
                failed_stage: None,
                diagnostic_payload: "{}",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE dispatch_attempts SET timestamp = ?1",
            [i64::try_from(now - 12).unwrap()],
        )
        .unwrap();

        // The guard's only evidence is the accepted, not-yet-started run.
        let live_evidence = harness_turn_live_evidence(&ctx, &idle);
        assert_eq!(live_evidence, Some(DISPATCH_TURN_PENDING_EVIDENCE));
        assert_eq!(
            operator_clear_input_state_for_evidence(&idle, false, false, false, true),
            OperatorClearInputState::LiveTurn,
            "the clear still queues behind the run the operator started first"
        );
        assert_eq!(
            settling_abandoned_prompt_for_clear(&ctx, "%180", DISPATCH_TURN_PENDING_EVIDENCE),
            None
        );
        let age = live_turn_evidence_age_secs(&ctx, "%180", DISPATCH_TURN_PENDING_EVIDENCE)
            .expect("the receipt's acceptance second");
        assert!((12..=14).contains(&age), "{age}");

        let grace = agent_doc_sqlite::state_store::DISPATCH_PRE_TURN_GRACE_SECS.unsigned_abs();
        let message = pending_dispatch_clear_queued_message(
            &doc,
            idle.pane_id.as_deref(),
            idle.source,
            Some(age),
            grace,
            false,
        );
        assert!(message.starts_with(&format!(
            "session_clear queued for {} — Run Agent Doc for pane %180 was accepted {age}s ago and has not started its turn yet",
            doc.display()
        )));
        assert!(message.contains("evidence=dispatch_turn_pending"));
        assert!(message.contains("no turn is running"));
        assert!(!message.contains("a turn is live"));
        assert!(!message.contains("running turn"));
        assert!(message.contains("that run does not drop it"));
        assert!(message.contains(&format!("within about {}s", grace - age)));
        // `interrupt-clear` on this idle pane skips the interrupt and re-enters
        // the same guard, so the notice must not offer it as "clear now".
        assert_eq!(
            interrupt_clear_initial_action(&idle),
            InterruptClearInitialAction::SkipInterruptAlreadyIdle
        );
        assert!(!message.contains("agent-doc session interrupt-clear"));
        assert!(message.contains("ui_outcome=queued ui_outcome_class=queued"));

        // Once the run's turn starts, its receipt is `running`: a real live turn,
        // reported with the live-turn wording.
        assert_eq!(
            agent_doc_sqlite::state_store::mark_open_dispatches_turn_started(&conn, &document_id)
                .unwrap(),
            1
        );
        assert_eq!(
            harness_turn_live_evidence(&ctx, &idle),
            Some("dispatch_turn_running")
        );
    }

    /// `#queuedclearflake`: a blank viewport (a starting supervisor that has not
    /// relayed its child's frame yet) hides the composer, not the turn. With
    /// live-turn evidence the clear is queued behind the turn, never sent.
    #[test]
    fn operator_clear_queues_behind_a_live_turn_in_an_unobservable_viewport() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveUnobservable,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: None,
            tail: None,
        };

        assert_eq!(
            operator_clear_input_state_for_evidence(&evidence, false, false, false, true),
            OperatorClearInputState::LiveTurn
        );
    }

    #[test]
    fn operator_clear_blocks_unobservable_stash_viewport() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveUnobservable,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: None,
            tail: None,
        };

        let state = operator_clear_input_state_for_evidence(&evidence, false, false, false, false);

        assert_eq!(state, OperatorClearInputState::ProtectedInput);
        assert_eq!(
            agent_doc_controller::operator_clear::clear_guard_outcome(state),
            OperatorClearGuardOutcome::FailedClosed
        );
    }

    #[test]
    fn force_interrupt_clear_summary_reports_destructive_cleanup() {
        let message = force_interrupt_clear_summary(
            Path::new("/tmp/doc.md"),
            Some("%7"),
            ForceInterruptClearReport {
                actor_closed: true,
                registry_removed: true,
                supervisor_signaled: true,
                child_signaled: false,
                pane_killed: true,
                socket_removed: true,
                cooldown_written: true,
            },
        );

        assert!(message.contains("Force-cleared session for /tmp/doc.md"));
        assert!(message.contains("pane=%7"));
        assert!(message.contains("actor_closed=true"));
        assert!(message.contains("registry_removed=true"));
        assert!(message.contains("supervisor_signaled=true"));
        assert!(message.contains("pane_killed=true"));
        assert!(message.contains("socket_removed=true"));
        assert!(message.contains("cooldown_written=true"));
    }

    #[test]
    fn restart_busy_refusal_points_to_force() {
        let message = restart_busy_refusal_message(
            Path::new("/tmp/doc.md"),
            "%7",
            "authoritative_actor",
            "agent-doc",
            Some("• Working (7m 47s · esc to interrupt)"),
            "⏵⏵ bypass permissions on (shift+tab to cycle)",
        );

        assert!(message.contains("session_restart refused"));
        assert!(message.contains("pane %7 is alive-busy"));
        assert!(message.contains("source=authoritative_actor"));
        assert!(message.contains("current_command=agent-doc"));
        // Busy-proof line is surfaced so the busy state is self-evident, not the
        // ambiguous permission footer (#session-restart-refusal-shows-busy-proof).
        assert!(message.contains("busy_proof=\"• Working (7m 47s · esc to interrupt)\""));
        assert!(message.contains("bypass permissions"));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
        assert!(message.contains("pass `--force`"));
        assert!(message.contains("request a supervisor-mediated restart"));
    }

    #[test]
    fn restart_busy_current_pane_defers_to_supervisor_handoff() {
        // #restart-current-pane-handoff: when the operator runs
        // `restart-supervisor --force` from the pane that currently owns the
        // session, the CLI must not send interrupt keys into that same pane.
        // It should submit the supervisor restart request and let the supervisor
        // drain/reexec/continue the session.
        let evidence = LivePaneEvidence {
            pane_id: Some("%12".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("node".to_string()),
            prompt_ready: Some(false),
            tail: Some("gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 30% used".to_string()),
        };

        assert_eq!(
            restart_busy_action(&evidence),
            RestartBusyAction::DeferToSupervisorHandoff
        );
    }

    #[test]
    fn restart_busy_foreign_pane_defers_to_supervisor_handoff() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%12".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("node".to_string()),
            prompt_ready: Some(false),
            tail: Some("• Working (2m · esc to interrupt)".to_string()),
        };

        assert_eq!(
            restart_busy_action(&evidence),
            RestartBusyAction::DeferToSupervisorHandoff
        );
    }

    #[test]
    fn restart_editor_holds_pane_refusal_names_editor_and_close_guidance() {
        // #hj7s: the refusal must carry the parseable header + fields the JB plugin
        // keys off, name the editor, and tell the operator to close it manually.
        let message = restart_editor_holds_pane_refusal_message(
            Path::new("/tmp/doc.md"),
            "%7",
            "authoritative_actor",
            "nvim",
            "-- INSERT --",
        );

        assert!(message.contains("session_restart refused for /tmp/doc.md"));
        assert!(message.contains("pane %7 is held by editor nvim"));
        assert!(message.contains("source=authoritative_actor"));
        assert!(message.contains("current_command=nvim"));
        assert!(message.contains("ctrl+g"));
        // UX: close the editor manually — do NOT force-quit or SIGINT around it.
        assert!(message.contains("Close the editor"));
        assert!(message.contains(":wq"));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
        // Must not advise --force: --force does not bypass the editor guard.
        assert!(!message.contains("--force"));
    }

    #[test]
    fn operator_clear_allows_clean_exit_prompt() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("Press Enter to restart...".to_string()),
        };

        let state = operator_clear_input_state_for_evidence(&evidence, false, true, false, false);

        assert_eq!(state, OperatorClearInputState::CleanExit);
        assert_eq!(
            agent_doc_controller::operator_clear::clear_guard_outcome(state),
            OperatorClearGuardOutcome::Completed
        );
    }

    #[test]
    fn interrupt_clear_initial_action_skips_interrupt_keys_for_idle_pane() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveIdle,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(true),
            tail: Some("gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used".to_string()),
        };

        assert_eq!(
            interrupt_clear_initial_action(&evidence),
            InterruptClearInitialAction::SkipInterruptAlreadyIdle
        );
    }

    #[test]
    fn interrupt_clear_initial_action_keeps_interrupt_for_busy_pane() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("Working...".to_string()),
        };

        assert_eq!(
            interrupt_clear_initial_action(&evidence),
            InterruptClearInitialAction::SendInterrupt
        );
    }

    #[test]
    fn cancel_turn_action_interrupts_only_when_turn_active() {
        // The whole safety contract: an active turn → interrupt; idle → no-op.
        // Idle must never resolve to Interrupt, because the harness interrupt
        // sequence at an idle prompt closes the agent.
        assert_eq!(cancel_turn_action(true), CancelTurnAction::Interrupt);
        assert_eq!(cancel_turn_action(false), CancelTurnAction::NoOpIdle);
    }

    #[test]
    fn cancel_turn_action_noop_idle_never_interrupts_on_repeated_calls() {
        // After the first cancel settles the pane to idle, every later call sees
        // an absent turn-active marker (turn_active=false) and stays a no-op —
        // proving repeated Cancel Turn calls never deliver an interrupt that
        // could close the agent.
        for _ in 0..5 {
            assert_eq!(cancel_turn_action(false), CancelTurnAction::NoOpIdle);
        }
    }

    #[test]
    fn interrupt_clear_timeout_outcome_reports_final_blocking_evidence() {
        let outcome = InterruptClearSettleOutcome::TimedOut {
            evidence: LivePaneEvidence {
                pane_id: Some("%7".to_string()),
                source: "authoritative_actor",
                state: LivePaneState::AliveBusy,
                current_command: Some("agent-doc".to_string()),
                prompt_ready: Some(false),
                tail: Some("reverse-i-search".to_string()),
            },
            editor_recovery_attempted: false,
        };

        assert_eq!(outcome.as_str(), "timed_out");
        assert_eq!(outcome.blocking_state(), "alive-busy");
        assert_eq!(outcome.blocking_source(), "authoritative_actor");
        assert_eq!(outcome.prompt_ready(), "false");
        assert_eq!(outcome.last_command(), Some("agent-doc"));
        assert_eq!(outcome.tail(), Some("reverse-i-search"));
    }

    #[test]
    fn operator_starting_guard_sees_supervisor_runtime_starting() {
        let record = test_actor_record(ActorState::Ready);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Starting)),
            None,
        );

        assert!(operator_command_has_starting_actor(&ctx));
    }

    #[test]
    fn live_evidence_target_ignores_closed_actor_pane() {
        let mut record = test_actor_record(ActorState::Closed);
        record.pane_id = "%stale".to_string();
        let mut ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Ready)),
            None,
        );
        ctx.registry_entry = Some(SessionEntry {
            pane: "%registry".to_string(),
            pid: 123,
            cwd: "/tmp".to_string(),
            started: "1".to_string(),
            session_id: "session-1".to_string(),
            file: "/tmp/doc.md".to_string(),
            window: "@1".to_string(),
            supervisor_instance_id: "sup-1".to_string(),
        });

        assert_eq!(
            live_evidence_target(&ctx),
            (Some("%registry".to_string()), "registry")
        );
    }

    #[test]
    fn operator_starting_guard_lets_matching_runtime_ready_override_stale_record() {
        let record = test_actor_record(ActorState::Starting);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Ready)),
            Some("ready"),
        );

        assert!(!operator_command_has_starting_actor(&ctx));
    }

    #[test]
    fn operator_starting_guard_accepts_legacy_session_scoped_runtime_ready() {
        let record = test_actor_record(ActorState::Starting);
        let mut runtime = test_supervisor_runtime(Some(ActorState::Ready));
        runtime.actor_session_id = None;
        runtime.actor_pane_id = None;
        runtime.actor_generation = None;
        let ctx = test_session_context(record, runtime, Some("starting"));

        assert!(!operator_command_has_starting_actor(&ctx));
    }

    #[test]
    fn operator_starting_guard_sees_supervisor_lease_starting() {
        let record = test_actor_record(ActorState::Ready);
        let ctx = test_session_context(record, test_supervisor_runtime(None), Some("starting"));

        assert!(operator_command_has_starting_actor(&ctx));
    }

    #[test]
    fn starting_operator_guard_does_not_gate_clear() {
        let record = test_actor_record(ActorState::Starting);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Starting)),
            Some("starting"),
        );
        let tmux = agent_doc_tmux_io::configured_tmux();

        guard_starting_actor_operator_command(&ctx, &tmux, OperatorAction::Clear)
            .expect("session clear must not be gated by stale starting actor projections");
    }

    #[test]
    fn supervisor_clear_legacy_unsupported_error_matches_old_supervisor_parse_failure() {
        let legacy_error = "parse error: unknown variant `clear`, expected one of `restart`, `inject`, `state`, `pid`, `stop` at line 1 column 17";

        assert!(supervisor_clear_legacy_unsupported_error(legacy_error));
        assert!(!supervisor_clear_legacy_unsupported_error(
            "parse error: unknown variant `nonsense`, expected one of `restart`, `inject`, `state`, `pid`, `stop` at line 1 column 17"
        ));
        assert!(!supervisor_clear_legacy_unsupported_error(
            "supervisor response timeout (2s)"
        ));
    }

    /// `#clearcapturedcycle`: on 2026-10-16 (`tasks/sdk.md`) two Clear presses
    /// latched behind an open cycle each printed "Cleared session context" and
    /// the Codex session was never replaced. A latched clear must say it is
    /// not applied yet; a clear that crossed a captured cycle must say the
    /// closeout continues without the old context.
    #[test]
    fn restart_fresh_clear_message_never_reports_a_latched_clear_as_cleared() {
        assert_eq!(
            restart_fresh_clear_outcome(true, false),
            RestartFreshClearOutcome::DeferredUntilCycleCloses
        );
        // A latch always wins: never report "Cleared" while latched.
        assert_eq!(
            restart_fresh_clear_outcome(true, true),
            RestartFreshClearOutcome::DeferredUntilCycleCloses
        );
        assert_eq!(
            restart_fresh_clear_outcome(false, true),
            RestartFreshClearOutcome::CrossedCapturedCycle
        );
        assert_eq!(
            restart_fresh_clear_outcome(false, false),
            RestartFreshClearOutcome::Armed
        );

        let latched = restart_fresh_clear_message(
            "/repo/tasks/sdk.md",
            "codex",
            "operator_ready",
            RestartFreshClearOutcome::DeferredUntilCycleCloses,
        );
        assert!(!latched.contains("Cleared"), "{latched}");
        assert!(latched.contains("NOT applied yet"), "{latched}");
        assert!(latched.contains("when that cycle closes"), "{latched}");

        let crossed = restart_fresh_clear_message(
            "/repo/tasks/sdk.md",
            "codex",
            "operator_ready",
            RestartFreshClearOutcome::CrossedCapturedCycle,
        );
        assert!(
            crossed.starts_with("Cleared session context for /repo/tasks/sdk.md"),
            "{crossed}"
        );
        assert!(crossed.contains("durably captured"), "{crossed}");
        // `#clearsessioncontext`: the relaunch does not re-run the
        // document over the supervisor-owned closeout.
        assert!(
            crossed.contains("waits at its prompt instead of re-running that cycle"),
            "{crossed}"
        );

        assert_eq!(
            restart_fresh_clear_message(
                "/repo/plan.md",
                "claude",
                "operator_ready",
                RestartFreshClearOutcome::Armed,
            ),
            "Cleared session context for /repo/plan.md (controller stage operator_ready)."
        );
    }

    /// A latched clear is released once its cycle is closed (the reclaim
    /// abandoned an empty preflight); an unreadable or open cycle keeps it
    /// reported as latched.
    #[test]
    fn document_cycle_open_reads_the_closeout_projection() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        std::fs::write(&doc, "# sdk\n").unwrap();
        assert!(!document_cycle_open(&doc), "no cycle recorded");
        agent_doc_cycle_state_io::start_preflight(&doc, Some("# sdk\n"), Some("# sdk\n")).unwrap();
        assert!(document_cycle_open(&doc), "preflight is open");
    }

    #[test]
    fn session_clear_already_satisfied_only_for_closed_session_without_delivery_target() {
        assert!(session_clear_already_satisfied_facts(
            Some(ActorState::Closed),
            false,
            false,
        ));
        assert!(!session_clear_already_satisfied_facts(
            Some(ActorState::Closed),
            true,
            false,
        ));
        assert!(!session_clear_already_satisfied_facts(
            Some(ActorState::Closed),
            false,
            true,
        ));
        assert!(!session_clear_already_satisfied_facts(
            Some(ActorState::Ready),
            false,
            false,
        ));
        assert!(!session_clear_already_satisfied_facts(None, false, false));
    }

    #[test]
    fn session_clear_submit_windows_follow_delivery_evidence() {
        assert_eq!(
            clear_direct_submit_max_enter_resubmits_from_env_value(None),
            CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS_DEFAULT
        );
        assert_eq!(
            clear_direct_submit_max_enter_resubmits_from_env_value(Some("4")),
            4
        );
        assert_eq!(
            clear_direct_submit_max_enter_resubmits_from_env_value(Some("0")),
            CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS_DEFAULT
        );
        assert_eq!(
            clear_direct_submit_max_enter_resubmits_from_env_value(Some("nope")),
            CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS_DEFAULT
        );
        assert!(
            ContextClearDeliveryEvidence::DirectPane.settle_timeout() <= Duration::from_secs(1),
            "direct clear recovery should retain its short stuck-draft proof window"
        );
        assert!(
            ContextClearDeliveryEvidence::SupervisorAccepted.settle_timeout()
                >= Duration::from_secs(6),
            "supervisor acceptance is transport-only evidence and must cover Codex's observed five-second clear transition"
        );
        assert_eq!(
            CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS_DEFAULT
                .cmp(&agent_doc_controller::dispatch::DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT,),
            std::cmp::Ordering::Less,
            "clear must not reuse dispatch's long stuck-draft recovery budget"
        );
    }

    #[test]
    fn starting_operator_guard_reason_blocks_restart_at_clean_exit_prompt() {
        assert_eq!(
            starting_operator_guard_reason(OperatorAction::Restart, false, false, true),
            "the pane has not reached a dispatch-ready prompt (`prompt_ready=true`)"
        );
    }

    #[test]
    fn starting_restart_refusal_points_to_force() {
        let message = starting_operator_guard_refusal_message(
            OperatorAction::Restart,
            Path::new("/tmp/doc.md"),
            "the document changed after the last committed cycle",
        );

        assert!(message.contains("session_restart refused"));
        assert!(message.contains("the authoritative actor is still starting"));
        assert!(message.contains("the document changed after the last committed cycle"));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
        assert!(message.contains("Pass `--force`"));
        assert!(message.contains("request a supervisor-mediated restart"));
    }

    #[test]
    fn document_dirty_after_committed_cycle_detects_post_commit_edit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/doc.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let committed = "---\nagent_doc_session: session-1\n---\n\nDone.\n";
        std::fs::write(&doc, committed).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        assert!(!document_dirty_after_committed_cycle(&doc).unwrap());

        std::fs::write(&doc, format!("{committed}\nnew prompt\n")).unwrap();

        assert!(document_dirty_after_committed_cycle(&doc).unwrap());
    }

    #[test]
    fn document_dirty_after_committed_cycle_uses_terminal_projection() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/doc.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let committed = "---\nagent_doc_session: session-1\n---\n\nDone.\n";
        std::fs::write(&doc, committed).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::Committed
        );

        assert!(!document_dirty_after_committed_cycle(&doc).unwrap());
    }

    #[test]
    fn a_codex_fresh_chat_frame_proves_the_clear_ran() {
        // Post-`/clear` frame from pane `%33` (2026-09-29).
        let cleared = "› Ask Codex to do anything\n\n  GPT-5.6-Sol high · ~/work/btakita/agent-loop · Context 0% used\n  ? for shortcuts\n\n  >_ OpenAI Codex (v0.158.0)\n     ~/work/btakita/agent-loop\n\n  Welcome back. Familiar territory or a fresh adventure?\n\nToken usage: total=308,270 input=266,138 output=42,132\nTo continue this session, run codex resume, then select Review lazily task\n";
        assert!(codex_viewport_shows_fresh_chat(cleared));
        // Only the banner + exit summary (context already moved on) still proves it.
        assert!(codex_viewport_shows_fresh_chat(
            &cleared.replace("Context 0% used", "Context 3% used")
        ));
        // An ongoing chat with context in use is not a cleared frame.
        let busy = "• Working (4m 57s • esc to interrupt)\n› Ask Codex to do anything\n  GPT-5.6-Sol high · ~/work · Context 34% used\n  ? for shortcuts\n";
        assert!(!codex_viewport_shows_fresh_chat(busy));
        // Operator prose that merely mentions the phrase is not the footer.
        assert!(!codex_viewport_shows_fresh_chat(
            "Why does it say Context 0% used?\n"
        ));
    }

    const CLAUDE_POST_CLEAR_TAIL: &str = include_str!(
        "../agent-doc-turn-executor-tmux/src/fixtures/claude_clear_post_clear_tail.txt"
    );

    /// A Claude Code pane's tmux scrollback after `turns` conversation turns,
    /// with the composer-frame residue (including stale queued-message rows)
    /// Claude repaints into history.
    fn claude_pane_history(turns: usize) -> String {
        let mut out = String::from(
            " \u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259b}\u{2588}   Claude Code v2.1.283\n\
             \u{259d}\u{259c}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2580}  Opus 5 \u{b7} Claude Max\n\n",
        );
        for turn in 0..turns {
            out.push_str(&format!(
                "\u{276f} /agent-doc tasks/plan.md turn {turn}\n\n\
                 \u{25cf} Bash(agent-doc plan tasks/plan.md)\n  \u{23bf}  [plan] ok\n\n\
                 \u{25cf} Answer for turn {turn}.\n  More of the answer.\n\n\u{273b} Baked for 41s\n\n"
            ));
            if turn % 40 == 3 {
                out.push_str(
                    "\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} Press up to edit queued messages\n\u{2500}\u{2500}\u{2500}\u{2500}\n  Opus 5 ctx:31% ~/work/example/project main user@host\n  \u{23f5}\u{23f5} bypass permissions on (shift+tab to cycle) \u{b7} \u{2190} 3 agents\n",
                );
            }
        }
        out
    }

    const CLAUDE_IDLE_COMPOSER: &str = "\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n\u{2500}\u{2500}\u{2500}\u{2500}\n  Opus 5 ctx:62% ~/work/example/project main user@host\n  \u{23f5}\u{23f5} bypass permissions on (shift+tab to cycle)\n";

    /// gh-266 end to end through the real `HarnessConfig` predicates, for both
    /// labels the reported probes carried (`claude-code` and the unresolved
    /// `default`): a long Claude Code pane where `/clear` ran is proven, and
    /// stale queued-message residue in its scrollback is not read as a queue.
    #[test]
    fn unobserved_claude_clear_on_a_long_pane_is_proven_for_every_claude_label() {
        let history = format!("{}{CLAUDE_POST_CLEAR_TAIL}", claude_pane_history(800));
        for label in ["claude-code", "claude", "default"] {
            let config = agent_doc_harness::HarnessConfig::from_agent_name(label);
            let verdict = unobserved_clear_history_verdict(&config, &history, None, "/clear");
            assert_eq!(
                verdict.status,
                ContextClearSubmitStatus::AcceptedClearedState,
                "label {label}: {verdict:?}"
            );
            assert_eq!(verdict.probe, UnobservedClearProbe::FreshChatProven);
            assert!(verdict.retained_lines > 3000, "{verdict:?}");
        }
    }

    /// gh-271 end to end through the real `HarnessConfig` predicates: the
    /// captured pane of a Claude Code `/clear` that ran, whose footer carries
    /// an operator-configured `statusLine` and the `✔ Update installed`
    /// notice. Neither row matches a Claude chrome rule, and both sit below
    /// the closed composer frame (`#claudeclearfooter`), so the clear is
    /// proven instead of `conversation_retained`.
    #[test]
    fn unobserved_claude_clear_with_a_custom_statusline_footer_is_proven() {
        let tail = include_str!(
            "../agent-doc-turn-executor-tmux/src/fixtures/claude_clear_custom_statusline_tail.txt"
        );
        let config = agent_doc_harness::HarnessConfig::from_agent_name("claude-code");
        for footer_row in tail
            .lines()
            .filter(|line| line.contains("<(^*)__") || line.contains("Update installed"))
        {
            assert!(
                !config.is_ignorable_output_line(footer_row),
                "fixture row {footer_row:?} must not be per-string chrome"
            );
        }
        let history = format!("{}{tail}", claude_pane_history(800));
        let verdict = unobserved_clear_history_verdict(&config, &history, None, "/clear");
        assert_eq!(
            verdict.status,
            ContextClearSubmitStatus::AcceptedClearedState,
            "{verdict:?}"
        );
        assert_eq!(verdict.probe, UnobservedClearProbe::FreshChatProven);

        // The same footer under the retained conversation is still retained.
        let footer: String = tail
            .lines()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|line| format!("{line}\n"))
            .collect();
        let retained = format!(
            "{}\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n\u{2500}\u{2500}\u{2500}\u{2500}\n{footer}",
            claude_pane_history(800)
        );
        let verdict = unobserved_clear_history_verdict(&config, &retained, None, "/clear");
        assert_eq!(verdict.status, ContextClearSubmitStatus::Unobserved);
        assert_eq!(verdict.probe, UnobservedClearProbe::ConversationRetained);
    }

    /// The fail-closed half: when the old conversation is still the latest
    /// thing in the pane, the verdict stays `Unobserved`; a genuinely queued
    /// clear stays `HarnessQueuedInput`.
    #[test]
    fn unobserved_claude_clear_with_the_conversation_still_visible_stays_blocked() {
        let config = agent_doc_harness::HarnessConfig::from_agent_name("default");
        let retained = format!("{}{CLAUDE_IDLE_COMPOSER}", claude_pane_history(800));
        let verdict = unobserved_clear_history_verdict(&config, &retained, None, "/clear");
        assert_eq!(verdict.status, ContextClearSubmitStatus::Unobserved);
        assert_eq!(verdict.probe, UnobservedClearProbe::ConversationRetained);

        // An earlier `/clear` followed by a new conversation is not this one.
        let earlier_clear = format!(
            "{}{CLAUDE_POST_CLEAR_TAIL}{}{CLAUDE_IDLE_COMPOSER}",
            claude_pane_history(400),
            claude_pane_history(2)
                .lines()
                .skip(3)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let verdict = unobserved_clear_history_verdict(&config, &earlier_clear, None, "/clear");
        assert_eq!(verdict.status, ContextClearSubmitStatus::Unobserved);

        let queued = format!(
            "{}\u{2722} Hatching\u{2026} (6m 18s)\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} Press up to edit queued messages\n\u{2500}\u{2500}\u{2500}\u{2500}\n  Opus 5 ctx:14% ~/work/example/project main user@host\n  \u{23f5}\u{23f5} bypass permissions on (shift+tab to cycle) \u{b7} \u{2190} 3 agents\n",
            claude_pane_history(60)
        );
        let verdict = unobserved_clear_history_verdict(&config, &queued, None, "/clear");
        assert_eq!(verdict.status, ContextClearSubmitStatus::HarnessQueuedInput);
    }

    #[test]
    fn clear_change_detection_ignores_harness_chrome_but_not_the_conversation() {
        let before = "• Ran make check\n  └ ok\n\n• Working (4m 57s • esc to interrupt)\n  └ Tip: Use /statusline to configure which items appear in the status line.\n› Ask Codex to do anything\n  GPT-5.6-Sol high · ~/work · Context 34% used\n  ? for shortcuts        ⚠ 2 warnings · f2 to view\n";
        let chrome_only = "• Ran make check\n  └ ok\n\n• Working (4m 58s • esc to interrupt)\n  └ Tip: Use /review to review your changes.\n› Ask Codex to do anything\n  GPT-5.6-Sol high · ~/work · Context 35% used\n  ? for shortcuts        ⚠ 2 warnings · f2 to view\n";
        assert_eq!(
            short_context_clear_submit_content_hash(before),
            short_context_clear_submit_content_hash(chrome_only),
            "spinner, tip and context-footer redraws are not a change"
        );
        let cleared = "› Ask Codex to do anything\n  GPT-5.6-Sol high · ~/work · Context 0% used\n";
        assert_ne!(
            short_context_clear_submit_content_hash(before),
            short_context_clear_submit_content_hash(cleared),
            "a clear that landed still changes the capture"
        );
    }

    #[test]
    fn last_meaningful_pane_line_trims_ansi_and_blank_lines() {
        assert_eq!(
            last_meaningful_pane_line("\x1b[32mworking\x1b[0m\n\n").as_deref(),
            Some("working")
        );
    }

    #[test]
    fn idle_direct_pane_evidence_supersedes_stale_busy_projection() {
        let record = agent_doc_controller::actor::ActorRecord {
            document_id: "/tmp/doc.md".to_string(),
            session_id: "session-1".to_string(),
            generation: 7,
            pane_id: "%7".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Busy,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
                caller: "supervisor".to_string(),
                reason: "work_started".to_string(),
                timestamp: 1,
                prior_generation: 7,
                new_generation: 7,
            },
        };
        let ctx = SessionContext {
            canonical_file: PathBuf::from("/tmp/doc.md"),
            base_dir: PathBuf::from("/tmp"),
            session_id: "session-1".to_string(),
            harness: "codex".to_string(),
            actor_record: Some(record.clone()),
            operator_status: SessionOperatorStatus {
                record: Some(record),
                transitions: Vec::new(),
                supervisor_lease: Some(SupervisorLeaseStatus {
                    generation: 7,
                    supervisor_pid: Some(100),
                    supervisor_socket: Some("/tmp/supervisor.sock".to_string()),
                    last_heartbeat: Some(1),
                    runtime_state: Some("busy".to_string()),
                }),
                dispatch_attempts: Vec::new(),
                projection_diagnostics: Vec::new(),
            },
            registry_entry: None,
            startup_miss: None,
            log_status: None,
            supervisor_runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(ActorState::Busy),
                actor_session_id: Some("session-1".to_string()),
                actor_pane_id: Some("%7".to_string()),
                actor_generation: Some(7),
                supervisor_state: Some("healthy".to_string()),
                restart_count: 0,
                supervisor_pid: Some(100),
                supervisor_instance_id: Some("sup-1".to_string()),
                child_pid: Some(101),
                cwd_source: Some("config".to_string()),
                current_harness: None,
            },
            supervisor_socket: PathBuf::from("/tmp/supervisor.sock"),
            live_pane_window: None,
        };
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveIdle,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(true),
            tail: Some(">".to_string()),
        };

        assert!(idle_projection_needs_reconciliation(&ctx, &evidence));
        let busy_evidence = LivePaneEvidence {
            state: LivePaneState::AliveBusy,
            prompt_ready: Some(false),
            ..evidence
        };
        assert!(!idle_projection_needs_reconciliation(&ctx, &busy_evidence));
    }

    #[test]
    fn status_flags_owned_pane_ready_busy_conflict() {
        let record = test_actor_record(ActorState::Ready);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Ready)),
            Some("ready"),
        );
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("tab to queue message".to_string()),
        };

        assert!(owned_pane_ready_busy_conflict(&ctx, &evidence));
    }

    #[test]
    fn status_does_not_flag_real_busy_actor_as_ready_busy_conflict() {
        let record = test_actor_record(ActorState::Busy);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Busy)),
            Some("busy"),
        );
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("working".to_string()),
        };

        assert!(!owned_pane_ready_busy_conflict(&ctx, &evidence));
    }

    #[test]
    fn status_does_not_flag_unobservable_stash_viewport_as_ready_busy_conflict() {
        let record = test_actor_record(ActorState::Ready);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Ready)),
            Some("ready"),
        );
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveUnobservable,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: None,
            tail: None,
        };

        assert!(!owned_pane_ready_busy_conflict(&ctx, &evidence));
    }

    #[test]
    fn status_context_refreshes_controller_lease_from_matching_live_supervisor() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/status.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-status\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-status",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-status",
            "%41",
            Some(1),
            ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();
        agent_doc_controller_io::project_controller::refresh_supervisor_lease(
            dir.path(),
            agent_doc_controller_io::project_controller::SupervisorHeartbeatRequest {
                file: doc.clone(),
                session_id: "session-status".to_string(),
                pane_id: "%41".to_string(),
                generation: 1,
                supervisor_pid: Some(999),
                supervisor_socket: Some("/tmp/stale.sock".to_string()),
                runtime_state: "starting".to_string(),
            },
        )
        .unwrap();

        let sock =
            agent_doc_supervisor_io::ipc::SupervisorIpc::start(dir.path(), "session-status", {
                move |method| match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": true,
                        "state": "healthy",
                        "actor_state": "ready",
                        "actor_session_id": "session-status",
                        "actor_pane_id": "%41",
                        "actor_generation": 1,
                        "restart_count": 0,
                        "supervisor_pid": 1001,
                        "supervisor_instance_id": "sup-status",
                        "child_pid": 1002,
                        "cwd_source": "config",
                    })),
                    _ => IpcResponse::ok_empty(),
                }
            })
            .unwrap();

        let ctx = build_context(&doc).unwrap();
        let lease = ctx.operator_status.supervisor_lease.unwrap();
        let expected_socket = sock.path().to_string_lossy().to_string();
        assert_eq!(lease.runtime_state.as_deref(), Some("ready"));
        assert_eq!(lease.supervisor_pid, Some(1001));
        assert_eq!(
            lease.supervisor_socket.as_deref(),
            Some(expected_socket.as_str())
        );
        // `#wsflake2`: assert the invariant, not an exact transition count.
        //
        // This test drives a REAL `SupervisorIpc` socket, so the number of
        // recorded transitions depends on how many state observations the live
        // supervisor happens to service before the assertion runs. Pinning
        // `== 2` made this the last known flake in the workspace suite: the two
        // transitions the test drives are always recorded, but an extra
        // observation under load pushed the count to 3 and failed a run that
        // was otherwise correct.
        //
        // What the test actually protects is that the lease was refreshed from
        // the matching live supervisor — already asserted above by
        // runtime_state / supervisor_pid / supervisor_socket — plus the fact
        // that transitions were recorded at all. Timing cannot produce FEWER
        // than the two driven here, so a lower bound is the honest assertion.
        assert!(
            ctx.operator_status.transitions.len() >= 2,
            "expected at least the two driven transitions, got {}",
            ctx.operator_status.transitions.len()
        );
    }

    #[test]
    fn doctor_flags_missing_actor_and_registry() {
        let runtime = SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
            actor_session_id: None,
            actor_pane_id: None,
            actor_generation: None,
            supervisor_state: None,
            restart_count: 0,
            supervisor_pid: None,
            supervisor_instance_id: None,
            child_pid: None,
            cwd_source: None,
            current_harness: None,
        };
        let ctx = SessionContext {
            canonical_file: PathBuf::from("/tmp/doc.md"),
            base_dir: PathBuf::from("/tmp"),
            session_id: "session-1".to_string(),
            harness: "codex".to_string(),
            actor_record: None,
            operator_status: empty_operator_status(None),
            registry_entry: None,
            startup_miss: None,
            log_status: None,
            supervisor_runtime: runtime,
            supervisor_socket: PathBuf::from("/tmp/missing.sock"),
            live_pane_window: None,
        };
        let issues = collect_doctor_issues(&ctx);
        assert!(issues.iter().any(|issue| issue.contains("actor record")));
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("durable registry"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("supervisor socket"))
        );
    }

    #[test]
    fn doctor_stale_window_issue_names_the_command_that_actually_repairs_it() {
        // `agent-doc sync` and `resync --fix` re-place panes but never rebind the
        // recorded window, so naming them here sent operators in a loop while the
        // drift survived. Only the explicit repair path runs
        // `repair_pane_window_binding` (`#panewindowbindingrebind`).
        let record = test_actor_record(ActorState::Ready);
        let mut ctx = test_session_context(record, test_supervisor_runtime(None), None);
        ctx.live_pane_window = Some("@4".to_string());

        let issues = collect_doctor_issues(&ctx);

        let stale = issues
            .iter()
            .find(|issue| issue.contains("window @1 for pane %7 is stale"))
            .expect("a pane whose live window differs from the record must be flagged");
        assert!(
            stale.contains("agent-doc session doctor --repair"),
            "remedy must name the path that rebinds the record: {stale}"
        );
    }

    #[test]
    fn doctor_flags_closed_actor_with_stale_pane() {
        let record = test_actor_record(ActorState::Closed);
        let ctx = test_session_context(record, test_supervisor_runtime(None), None);

        let issues = collect_doctor_issues(&ctx);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("closed actor record still references pane %7"))
        );
    }

    #[test]
    fn harness_clear_command_maps_opencode_to_new() {
        // OpenCode has no `/clear` command; its clear-context equivalent is
        // `/new` (#opencode-clear-uses-new). Claude/Codex keep `/clear`.
        assert_eq!(harness_clear_command("opencode"), "/new");
        assert_eq!(harness_clear_command("claude"), "/clear");
        assert_eq!(harness_clear_command("codex"), "/clear");
        assert_eq!(harness_clear_command("unknown"), "/clear");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn send_clear_to_pane_submits_clear_command() {
        let dir = tempfile::tempdir().unwrap();
        let iso = agent_doc_tmux_io::isolated_test_tmux("session-clear-direct-pane");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let output_path = dir.path().join("clear.txt");
        let ready_path = dir.path().join("clear.ready");
        let done_path = dir.path().join("clear.done");
        // Start the reader as the pane command. Typing its setup command into
        // a just-created interactive shell races that shell's initialization.
        iso.raw_cmd(&[
            "respawn-pane",
            "-k",
            "-t",
            &pane,
            &format!(
                "sh -c 'touch \"{}\"; IFS= read -r line; printf \"%s\" \"$line\" > \"{}\"; touch \"{}\"; printf \"\\033[2J\\033[H\"; sleep 1.2; printf \"❯ \"; while IFS= read -r line; do :; done'",
                ready_path.display(),
                output_path.display(),
                done_path.display()
            ),
        ])
        .unwrap();
        let ready_deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < ready_deadline && !ready_path.exists() {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            ready_path.exists(),
            "fixture must prove the line-reader is ready before sending `/clear`",
        );

        // The fixture repaints after the ordinary 900ms budget. Neither its
        // blank screen nor the echoed command may count as accepted submission.
        let started = Instant::now();
        send_clear_to_pane(
            &iso,
            &ProvenPane::from_verified_live_owner(pane.clone()),
            Path::new("/tmp/doc.md"),
            "claude",
        )
        .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(1200));
        for _ in 0..40 {
            if done_path.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            done_path.exists(),
            "expected `/clear` to submit through pane input"
        );
        assert_eq!(std::fs::read_to_string(&output_path).unwrap(), "/clear");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn clear_falls_back_to_supervisor_inject_when_authoritative_pane_is_not_on_default_tmux() {
        let dir = tempfile::tempdir().unwrap();
        let session_id = format!(
            "session-clear-{}",
            dir.path()
                .file_name()
                .expect("tempdir should have a basename")
                .to_string_lossy()
        );
        let iso = agent_doc_tmux_io::isolated_test_tmux("session-clear-direct-pane");
        let default_tmux = agent_doc_tmux_io::configured_tmux();
        let mut pane = iso.new_session("test", dir.path()).unwrap();
        for _ in 0..64 {
            if !default_tmux.pane_alive(&pane) {
                break;
            }
            pane = iso.new_window("test", dir.path()).unwrap();
        }
        assert!(
            !default_tmux.pane_alive(&pane),
            "fixture requires an isolated pane id that is absent from the operator's default tmux server"
        );
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            format!("---\nagent_doc_session: {session_id}\nagent: codex\n---\n"),
        )
        .unwrap();
        let captured = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured_for_ipc = captured.clone();
        let sock = agent_doc_supervisor_io::ipc::SupervisorIpc::start(dir.path(), &session_id, {
            move |method| match method {
                IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                    captured_for_ipc.lock().push(bytes);
                    IpcResponse::ok_empty()
                }
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": true,
                    "state": "healthy",
                    "actor_state": "ready",
                    "restart_count": 0,
                })),
                _ => IpcResponse::ok_empty(),
            }
        })
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let pane_window = iso.pane_window(&pane).unwrap();
        // Register under the fixture root, never the process cwd's project
        // registry (#testisolationleaks).
        agent_doc_session_registry_io::registration::register_full_in(
            dir.path(),
            &session_id,
            &pane,
            &doc.to_string_lossy(),
            std::process::id(),
            &pane_window,
        )
        .unwrap();
        // The fixture-root registration already committed a generation; start
        // the session at the next one so the actor CAS sees its true prior.
        let generation = agent_doc_session_actor_io::next_generation(&doc, &session_id)
            .unwrap()
            .new_generation;
        agent_doc_session_actor_io::record_session_start(
            &doc,
            &session_id,
            &pane,
            &pane_window,
            generation,
        )
        .unwrap();
        clear(&doc).unwrap();
        let latest = agent_doc_codex_hook_io::load_latest_prompt_for_file(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(latest, "/clear");
        assert_eq!(
            captured.lock().as_slice(),
            &[
                agent_doc_tmux_commands::submitted_text_without_trailing_line_endings("/clear")
                    .to_string()
            ]
        );
        drop(sock);
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_direct_submit_pane_prefers_authoritative_actor() {
        let dir = tempfile::tempdir().unwrap();
        let iso = agent_doc_tmux_io::isolated_test_tmux("session-clear-pane-select-actor");
        let actor_pane = iso.new_session("test", dir.path()).unwrap();
        let registry_pane = iso.new_window("test", dir.path()).unwrap();
        let actor_record = agent_doc_controller::actor::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session-clear".to_string(),
            generation: 3,
            pane_id: actor_pane.clone(),
            window_id: iso.pane_window(&actor_pane).unwrap(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
                caller: "test".to_string(),
                reason: "actor".to_string(),
                timestamp: 1,
                prior_generation: 2,
                new_generation: 3,
            },
        };
        let ctx = SessionContext {
            canonical_file: dir.path().join("doc.md"),
            base_dir: dir.path().to_path_buf(),
            session_id: "session-clear".to_string(),
            harness: "codex".to_string(),
            actor_record: Some(actor_record.clone()),
            operator_status: empty_operator_status(Some(actor_record)),
            registry_entry: Some(SessionEntry {
                pane: registry_pane.clone(),
                pid: 1,
                cwd: dir.path().display().to_string(),
                started: "now".to_string(),
                session_id: "session-clear".to_string(),
                file: dir.path().join("doc.md").display().to_string(),
                window: iso.pane_window(&registry_pane).unwrap(),
                supervisor_instance_id: "sup".to_string(),
            }),
            startup_miss: None,
            log_status: None,
            supervisor_runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(ActorState::Ready),
                actor_session_id: None,
                actor_pane_id: None,
                actor_generation: None,
                supervisor_state: Some("healthy".to_string()),
                restart_count: 0,
                supervisor_pid: Some(1),
                supervisor_instance_id: Some("sup".to_string()),
                child_pid: Some(2),
                cwd_source: Some("config".to_string()),
                current_harness: None,
            },
            supervisor_socket: dir.path().join("session-clear.sock"),
            live_pane_window: None,
        };

        assert_eq!(
            resolve_direct_submit_pane(&ctx, &iso),
            Some((
                ProvenPane::from_verified_live_owner(actor_pane),
                DirectSubmitPaneSource::AuthoritativeActor
            ))
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_direct_submit_pane_falls_back_to_registry() {
        let dir = tempfile::tempdir().unwrap();
        let iso = agent_doc_tmux_io::isolated_test_tmux("session-clear-pane-select-registry");
        let registry_pane = iso.new_session("test", dir.path()).unwrap();
        let actor_record = agent_doc_controller::actor::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session-clear".to_string(),
            generation: 3,
            pane_id: "%9999".to_string(),
            window_id: "@9999".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
                caller: "test".to_string(),
                reason: "actor".to_string(),
                timestamp: 1,
                prior_generation: 2,
                new_generation: 3,
            },
        };
        let ctx = SessionContext {
            canonical_file: dir.path().join("doc.md"),
            base_dir: dir.path().to_path_buf(),
            session_id: "session-clear".to_string(),
            harness: "codex".to_string(),
            actor_record: Some(actor_record.clone()),
            operator_status: empty_operator_status(Some(actor_record)),
            registry_entry: Some(SessionEntry {
                pane: registry_pane.clone(),
                pid: 1,
                cwd: dir.path().display().to_string(),
                started: "now".to_string(),
                session_id: "session-clear".to_string(),
                file: dir.path().join("doc.md").display().to_string(),
                window: iso.pane_window(&registry_pane).unwrap(),
                supervisor_instance_id: "sup".to_string(),
            }),
            startup_miss: None,
            log_status: None,
            supervisor_runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(ActorState::Ready),
                actor_session_id: None,
                actor_pane_id: None,
                actor_generation: None,
                supervisor_state: Some("healthy".to_string()),
                restart_count: 0,
                supervisor_pid: Some(1),
                supervisor_instance_id: Some("sup".to_string()),
                child_pid: Some(2),
                cwd_source: Some("config".to_string()),
                current_harness: None,
            },
            supervisor_socket: dir.path().join("session-clear.sock"),
            live_pane_window: None,
        };

        assert_eq!(
            resolve_direct_submit_pane(&ctx, &iso),
            Some((
                ProvenPane::from_verified_live_owner(registry_pane),
                DirectSubmitPaneSource::Registry
            ))
        );
    }

    fn clear_reclaim_project() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "# Doc\n\n## User\n\nDo the thing\n").unwrap();
        (dir, doc)
    }

    #[test]
    fn clear_reclaims_orphaned_empty_preflight_cycle() {
        let (_dir, doc) = clear_reclaim_project();
        let content = std::fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&content), Some(&content)).unwrap();

        // The clear path reclaims the orphaned cycle so the next Run Agent Doc
        // is not wedged by a stale open cycle.
        assert_eq!(
            reclaim_orphaned_cycle_after_run_cancel(&doc, "test_clear"),
            agent_doc_turn::repair::CancelOutcome::Abandoned
        );
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Abandoned);
    }

    #[test]
    fn clear_protects_cycle_that_already_captured_a_response() {
        let (_dir, doc) = clear_reclaim_project();
        let content = std::fs::read_to_string(&doc).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        agent_doc_capture_io::capture_response(&doc, "### Re: do — opus-4-8\n\nDone.\n").unwrap();

        // A cycle that owns a captured response must not be discarded by clear.
        assert_eq!(
            reclaim_orphaned_cycle_after_run_cancel(&doc, "test_clear"),
            agent_doc_turn::repair::CancelOutcome::Protected
        );
        assert!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .is_open(),
            "clear must protect a cycle that already captured a response"
        );
    }

    #[test]
    fn clear_reclaim_is_noop_without_an_open_cycle() {
        let (_dir, doc) = clear_reclaim_project();
        assert_eq!(
            reclaim_orphaned_cycle_after_run_cancel(&doc, "test_clear"),
            agent_doc_turn::repair::CancelOutcome::NoOpenCycle
        );
    }

    #[test]
    fn live_pane_prompt_ready_accepts_agent_doc_restart_control_prompt() {
        let harness = agent_doc_harness::HarnessConfig::codex();
        let captured = concat!(
            "To continue this session, run codex resume abc123\n",
            "Press Enter to restart fresh, or 'q' to exit.\n",
        );

        assert!(live_pane_prompt_ready(&harness, captured));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_stale_agent_doc_restart_prompt() {
        let harness = agent_doc_harness::HarnessConfig::codex();
        let captured = concat!(
            "Press Enter to restart fresh, or 'q' to exit.\n",
            "• Working (3s • esc to interrupt)\n",
        );

        assert!(!agent_doc_restart_prompt_visible(captured));
        assert!(!live_pane_prompt_ready(&harness, captured));
    }

    /// `#clearqueuedliveturn`: a clear is queued behind a live turn only when
    /// the supervisor that will observe the turn end owns that exact pane.
    #[test]
    fn live_turn_clear_is_deliverable_only_by_the_supervisor_owning_the_live_pane() {
        assert!(live_turn_clear_deliverable_by_supervisor(
            true,
            Some("%149"),
            Some("%149")
        ));
        assert!(
            !live_turn_clear_deliverable_by_supervisor(false, Some("%149"), Some("%149")),
            "no reachable supervisor: nothing would deliver the queued clear"
        );
        assert!(
            !live_turn_clear_deliverable_by_supervisor(true, Some("%7"), Some("%149")),
            "the supervisor only delivers into its own pane"
        );
        assert!(!live_turn_clear_deliverable_by_supervisor(
            true,
            None,
            Some("%149")
        ));
        assert!(!live_turn_clear_deliverable_by_supervisor(
            true,
            Some("%149"),
            None
        ));
    }
}
