use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use agent_doc_controller::operator_clear::OperatorClearGuardOutcome;
use agent_doc_controller::operator_clear::{OperatorClearInputState, clear_guard_event};
#[cfg(test)]
use agent_doc_sqlite::state_store::SupervisorLeaseStatus;
use agent_doc_sqlite::state_store::{
    ActorRecord, ActorState, ActorTransitionStatus, SessionOperatorStatus,
};
use agent_doc_supervisor::ipc_protocol::IpcMethod;
#[cfg(test)]
use agent_doc_supervisor::ipc_protocol::IpcResponse;
use agent_doc_supervisor::startup_miss::{SessionLogStatus, StartupMiss, format_timestamp};
use agent_doc_tmux_commands::tmux_submit_mode_for_harness;
use agent_doc_turn_executor_tmux::context_clear::{
    ContextClearSubmitObservation, ContextClearSubmitPollState, ContextClearSubmitRetryFacts,
    ContextClearSubmitStatus, InterruptClearTimeoutFacts, busy_clear_already_deferred_message,
    busy_clear_deferred_message, busy_clear_refusal_message,
    context_clear_command_visible_in_active_input, context_clear_submit_blocked_line,
    context_clear_submit_blocked_message, context_clear_submit_can_enter_resubmit,
    context_clear_submit_observation_line, context_clear_submit_poll_status,
    context_clear_submit_resubmit_proof_line, interrupt_clear_timeout_message,
    operator_interrupt_key_plan, operator_interrupt_step_delay, protected_clear_refusal_message,
    terminal_editor_command,
};
use tmux_router::{Registry as SessionRegistry, RegistryEntry as SessionEntry, Tmux};

const SUPERVISOR_INJECT_SUBMIT_MODE: &str = "supervisor_normalized_submit";
const CLEAR_DIRECT_SUBMIT_ACCEPTANCE_TIMEOUT: Duration = Duration::from_millis(900);
const CLEAR_DIRECT_SUBMIT_ACCEPTANCE_POLL_INTERVAL: Duration = Duration::from_millis(150);
const CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS_DEFAULT: usize = 1;
const CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS_ENV: &str =
    "AGENT_DOC_CLEAR_DIRECT_SUBMIT_MAX_ENTER_RESUBMITS";
const CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED: &str =
    agent_doc_state_backbone::QUEUE_CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED;

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
    ClosedClean,
    ProjectionStale,
    Unknown,
}

impl LivePaneState {
    fn as_str(self) -> &'static str {
        match self {
            Self::AliveIdle => "alive-idle",
            Self::AliveBusy => "alive-busy",
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
    let tmux = Tmux::default_server();
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
    let tmux = Tmux::default_server();
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
    let ctx = build_context(file)?;
    let tmux = Tmux::default_server();
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
    let receipt = agent_doc_controller_io::project_controller::request_supervisor_replacement(
        &ctx.base_dir,
        agent_doc_controller_io::project_controller::SupervisorReplacementRequest {
            file: ctx.canonical_file.clone(),
            mode: mode.as_str().to_string(),
            force,
        },
    )?;
    println!(
        "Requested {} supervisor replacement for {} via project controller (stage {}, receipt {}, background_started={}).",
        mode.as_str(),
        ctx.canonical_file.display(),
        receipt.accepted_stage,
        receipt.operator_receipt.receipt_id,
        receipt.background_started
    );
    report_supervisor_replacement_outcome(&ctx.canonical_file, receipt.background_started);
    Ok(())
}

/// How long to wait for the background replacement worker to produce a live
/// supervisor before reporting the request as unfulfilled.
const SUPERVISOR_REPLACEMENT_PROOF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

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
/// Wait briefly for proof of a live supervisor and say plainly what it means for
/// the queue when there is none.
fn report_supervisor_replacement_outcome(file: &Path, background_started: bool) {
    if !background_started {
        eprintln!(
            "[session] warning: no background replacement worker started for {}; the supervisor was NOT replaced.",
            file.display()
        );
        return;
    }
    let deadline = std::time::Instant::now() + SUPERVISOR_REPLACEMENT_PROOF_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if let Some(pid) = agent_doc_supervisor_io::process::supervisor_pid_for_doc(file) {
            println!("[session] supervisor replacement proven live (pid {pid}).");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    eprintln!(
        "[session] warning: no live supervisor for {} after {}s — the replacement request was accepted but did not produce one.",
        file.display(),
        SUPERVISOR_REPLACEMENT_PROOF_TIMEOUT.as_secs()
    );
    eprintln!(
        "[session] warning: without a supervisor there is NO idle-queue watch, so an active `queue: go` document will not self-drain and needs a human to advance it."
    );
    eprintln!(
        // `#restartlivepane`: "run it from a different pane" was wrong advice —
        // the refusal is about the TARGET pane, not the calling one, so retrying
        // elsewhere reproduces it exactly. A pane running the document's own
        // harness now auto-restarts; anything else needs the pane freed.
        "[session] hint: check `controller_supervisor_replacement_background_failed` in .agent-doc/logs/ops.log for the reason; if it is `preserve_pane_blocked`, the owner pane is running a program that is not this document's harness — quit it, or point the document at another pane with `agent-doc claim {}`.",
        file.display()
    );
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
    let tmux = Tmux::default_server();
    guard_starting_actor_operator_command(&ctx, &tmux, OperatorAction::Clear)?;
    if session_clear_already_satisfied(&ctx, &tmux) {
        agent_doc_ops_log_io::log_op(
            &ctx.canonical_file,
            &format!(
                "session_clear_already_satisfied file={} reason=closed_actor_no_live_delivery_target",
                ctx.canonical_file.display()
            ),
        );
        reclaim_orphaned_cycle_on_clear(&ctx.canonical_file);
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
        // loop. The loop is paused (clear cooldown) and the clear is deferred to
        // the next idle gap; do NOT deliver it into the in-flight turn.
        // (`#autoloop-command-preemption` Phase 2.)
        return Ok(());
    }
    let mut restarted_fresh_from_supervisor_prompt = false;
    if supervisor_clear_inject_available(&ctx) {
        let pre_delivery_capture_hash = ctx
            .supervisor_runtime
            .actor_pane_id
            .as_deref()
            .and_then(|pane| capture_context_clear_submit_content_hash(&tmux, pane));
        match send_clear_via_supervisor(&ctx)? {
            SupervisorClearDelivery::RestartedFresh => {
                restarted_fresh_from_supervisor_prompt = true;
                agent_doc_ops_log_io::log_op(
                    &ctx.canonical_file,
                    &format!(
                        "session_clear_sent file={} delivery=supervisor_restart_fresh reason=waiting_input",
                        ctx.canonical_file.display()
                    ),
                );
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
            SupervisorClearDelivery::RestartedFresh => {
                restarted_fresh_from_supervisor_prompt = true;
                agent_doc_ops_log_io::log_op(
                    &ctx.canonical_file,
                    &format!(
                        "session_clear_sent file={} delivery=supervisor_restart_fresh reason=waiting_input",
                        ctx.canonical_file.display()
                    ),
                );
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
    reclaim_orphaned_cycle_on_clear(&ctx.canonical_file);
    println!(
        "Cleared session context for {} (controller stage {}).",
        ctx.canonical_file.display(),
        authorization.accepted_stage
    );
    Ok(())
}

/// After an explicit clear delivers `/clear` (aborting the current turn),
/// reclaim any orphaned open preflight cycle the cleared run left behind
/// (#clear-stops-running-process, mirroring explicit run-cancel
/// #cancel-orphans-preflight-cycle). An empty `preflight_started` cycle is
/// abandoned so the next Run Agent Doc starts fresh instead of waiting on a
/// stale open cycle; a cycle that already captured a response is protected and
/// left intact. Reclaim failures are non-fatal — the clear itself already
/// delivered — and surface as a warning.
fn reclaim_orphaned_cycle_on_clear(file: &Path) -> agent_doc_turn::repair::CancelOutcome {
    match agent_doc_repair_io::cancel_preflight_cycle(
        &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
        file,
    ) {
        Ok(outcome) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "session_clear_cycle_reclaim file={} outcome={outcome:?}",
                    file.display()
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
    RestartedFresh,
    Sent,
    LegacyClearUnsupported { error: String },
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
    if response
        .data
        .as_ref()
        .and_then(|data| data.get("restart_fresh"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(SupervisorClearDelivery::RestartedFresh);
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
/// non-interrupting clear hit a busy pane driven by an active auto-queue loop,
/// the loop has been paused, and the clear must NOT be delivered into the
/// in-flight turn (`#autoloop-command-preemption` Phase 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClearPreflightOutcome {
    Proceed,
    DeferredPreempt,
}

fn reconcile_idle_projection_before_clear(
    ctx: &SessionContext,
    tmux: &Tmux,
) -> Result<ClearPreflightOutcome> {
    let evidence = resolve_direct_submit_pane(ctx, tmux)
        .map(|(pane, source)| {
            live_pane_evidence_for_pane(ctx, tmux, pane.pane_id().to_string(), source.as_str())
        })
        .unwrap_or_else(|| live_pane_evidence(ctx, tmux));
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
    let clear_state = operator_clear_input_state_for_evidence(
        &evidence,
        protected_reason.is_some(),
        clean_exit_prompt,
        busy_reason.is_some(),
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
                        "session_clear_protected_input_guard_refused file={} pane={} source={} reason={} current_command={} tail={:?}",
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
        OperatorClearInputState::Busy => {
            let busy_reason = busy_reason.unwrap_or_else(|| "busy cue".to_string());
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
            let deferred_clear_pending =
                agent_doc_controller_io::project_controller::queue_context_clear_deferred_operator_for_file(
                    &ctx.canonical_file,
                )?
                .is_some();
            match agent_doc_queue::queue_preemption::plan_busy_clear(
                queue_active,
                deferred_clear_pending,
            ) {
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
                            evidence.pane_id.as_deref()
                        )
                    );
                    return Ok(ClearPreflightOutcome::DeferredPreempt);
                }
                agent_doc_queue::queue_preemption::BusyClearOutcome::HardBlock => {
                    agent_doc_ops_log_io::log_op(
                        &ctx.canonical_file,
                        &format!(
                            "session_clear_live_busy_guard_blocked file={} pane={} source={} reason={} current_command={} tail={:?}",
                            ctx.canonical_file.display(),
                            evidence.pane_id.as_deref().unwrap_or("unknown"),
                            evidence.source,
                            busy_reason.replace(char::is_whitespace, "_"),
                            evidence.current_command.as_deref().unwrap_or("unknown"),
                            evidence.tail.as_deref().unwrap_or("unknown")
                        ),
                    );
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

fn operator_clear_input_state_for_evidence(
    evidence: &LivePaneEvidence,
    protected_input: bool,
    clean_exit_prompt: bool,
    busy_cue: bool,
) -> OperatorClearInputState {
    match evidence.state {
        LivePaneState::AliveIdle => OperatorClearInputState::IdlePrompt,
        LivePaneState::ClosedClean | LivePaneState::ProjectionStale | LivePaneState::Unknown => {
            OperatorClearInputState::NoLivePane
        }
        LivePaneState::AliveBusy if protected_input => OperatorClearInputState::ProtectedInput,
        LivePaneState::AliveBusy if clean_exit_prompt => OperatorClearInputState::CleanExit,
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
    harness.protected_prompt_input_reason(&captured)
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
    let Some(marker) = agent_doc_turn_status_io::read_turn_active_marker(&ctx.base_dir) else {
        return false;
    };
    let owned_pane = ctx
        .actor_record
        .as_ref()
        .filter(|record| record.state != ActorState::Closed)
        .map(|record| record.pane_id.clone())
        .or_else(|| ctx.registry_entry.as_ref().map(|entry| entry.pane.clone()));
    match owned_pane {
        Some(pane) if !pane.is_empty() => marker.pane == pane,
        _ => true,
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
    let tmux = Tmux::default_server();
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
            println!(
                "Cancelled the active turn for {} (interrupted pane {}; context preserved).",
                ctx.canonical_file.display(),
                pane
            );
            Ok(())
        }
    }
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
    let tmux = Tmux::default_server();
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

    let tmux = Tmux::default_server();
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

    reclaim_orphaned_cycle_on_clear(&ctx.canonical_file);

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
        resubmit_source: "session_clear.direct_pane_resubmit",
        pre_delivery_capture_hash: pre_delivery_capture_hash.as_deref(),
    })
}

struct ContextClearSubmitVerification<'a> {
    tmux: &'a Tmux,
    pane: &'a str,
    file: &'a Path,
    harness: &'a str,
    command: &'a str,
    initial_phase: &'a str,
    #[allow(dead_code)]
    resubmit_source: &'a str,
    pre_delivery_capture_hash: Option<&'a str>,
}

fn verify_context_clear_submit_after_delivery(
    ctx: ContextClearSubmitVerification<'_>,
) -> Result<()> {
    let first = poll_context_clear_submit_acceptance(
        ctx.tmux,
        ctx.pane,
        ctx.file,
        ctx.harness,
        ctx.command,
        ctx.initial_phase,
        ctx.pre_delivery_capture_hash,
    );
    let mut final_phase = ctx.initial_phase;
    let mut final_observation = first;
    let profile_allows_pending_draft_enter_resubmit =
        agent_doc_tmux_commands::tmux_submit_profile_for_harness(ctx.harness)
            .pending_draft_enter_resubmit();
    let max_attempts = clear_direct_submit_max_enter_resubmits();
    let mut attempts_sent = 0usize;
    while context_clear_submit_can_enter_resubmit(ContextClearSubmitRetryFacts {
        observation: final_observation,
        pending_draft_enter_resubmit: profile_allows_pending_draft_enter_resubmit,
        attempts_sent,
        max_attempts,
    }) {
        attempts_sent += 1;
        let resubmit_phase = match ctx.initial_phase {
            "supervisor_ipc_acceptance" => "supervisor_ipc_resubmit_acceptance",
            _ => "direct_pane_resubmit_acceptance",
        };
        let submit_key = agent_doc_tmux_commands::tmux_submit_key_for_harness(ctx.harness);
        let pre_resubmit_capture_hash =
            capture_context_clear_submit_content_hash(ctx.tmux, ctx.pane);
        agent_doc_tmux_io::input_diag::log_text_submit(
            agent_doc_tmux_io::input_diag::InputDiagSink::new(
                Some(ctx.file),
                agent_doc_ops_log_io::log_op,
            ),
            ctx.resubmit_source,
            &format!("pane:{}", ctx.pane),
            "",
            Some(ctx.harness),
            "clear_resubmit_submit_key",
            submit_key,
        );
        if let Err(err) = agent_doc_tmux_io::send_submitted_text_for_harness_logged(
            ctx.tmux,
            ctx.pane,
            "",
            ctx.harness,
            agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
            "sessions.send_submitted_text_for_harness",
        ) {
            eprintln!(
                "[clear] warning: {} clear resubmit {submit_key} failed for pane {}: {err}",
                ctx.harness, ctx.pane
            );
        }
        let second = poll_context_clear_submit_acceptance(
            ctx.tmux,
            ctx.pane,
            ctx.file,
            ctx.harness,
            ctx.command,
            resubmit_phase,
            pre_resubmit_capture_hash.as_deref(),
        );
        agent_doc_ops_log_io::log_op(
            ctx.file,
            &context_clear_submit_resubmit_proof_line(
                ctx.file.display(),
                ctx.pane,
                ctx.harness,
                submit_key,
                attempts_sent,
                max_attempts,
                second,
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

fn poll_context_clear_submit_acceptance(
    tmux: &Tmux,
    pane: &str,
    file: &Path,
    harness: &str,
    command: &str,
    phase: &str,
    pre_delivery_capture_hash: Option<&str>,
) -> ContextClearSubmitObservation {
    let harness_config = agent_doc_harness::HarnessConfig::from_agent_name(harness);
    let start = Instant::now();
    let mut last_capture: Option<(bool, usize, String)> = None;
    let mut poll_state = ContextClearSubmitPollState::default();
    let mut capture_failed = false;
    while start.elapsed() < CLEAR_DIRECT_SUBMIT_ACCEPTANCE_TIMEOUT {
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
                last_capture = Some((command_visible, capture_len, capture_hash));
                if context_clear_submit_poll_status(
                    &mut poll_state,
                    command_visible,
                    content_changed_since_delivery,
                )
                .is_some()
                {
                    let observation = ContextClearSubmitObservation {
                        status: ContextClearSubmitStatus::Accepted,
                        elapsed: start.elapsed(),
                        command_visible: false,
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
    let (status, command_visible) = if let Some((visible, _, _)) = last_capture.as_ref() {
        if *visible {
            (ContextClearSubmitStatus::TimedOut, true)
        } else if poll_state.saw_submission_evidence() {
            (ContextClearSubmitStatus::Accepted, false)
        } else {
            (ContextClearSubmitStatus::TimedOut, false)
        }
    } else if capture_failed {
        (ContextClearSubmitStatus::CaptureFailed, false)
    } else {
        (ContextClearSubmitStatus::TimedOut, false)
    };
    let observation = ContextClearSubmitObservation {
        status,
        elapsed,
        command_visible,
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
    if observation.status == ContextClearSubmitStatus::Accepted {
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
    let hash = agent_doc_hash::content_hash(content);
    hash[..hash.len().min(12)].to_string()
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
    if evidence.state == LivePaneState::AliveBusy {
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
            "{} refused for {} because pane {} is alive-busy (source={}, current_command={}, busy_proof={:?}, tail={:?}). Run `agent-doc session status {}` and wait for an idle prompt, or inspect/stop the pane explicitly before clearing or restarting it.",
            action,
            ctx.canonical_file.display(),
            pane,
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
        let closeout = agent_doc_repair_command_io::repair(file)?;
        let mut repair_notes = agent_doc_sync_io::sync::repair_file_state(file)?;
        let repair_ctx = build_context(file)?;
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
    cleared.last_transition = agent_doc_sqlite::state_store::ActorLastTransition {
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
    let content = std::fs::read_to_string(&canonical_file)
        .with_context(|| format!("failed to read {}", canonical_file.display()))?;
    let session_id = agent_doc_frontmatter_io::session::read_session_id(&canonical_file)
        .or_else(|| {
            agent_doc_frontmatter::frontmatter::parse(&content)
                .ok()
                .and_then(|(fm, _)| fm.session)
        })
        .with_context(|| format!("{} has no agent_doc_session", canonical_file.display()))?;
    let base_dir = agent_doc_fs::find_project_root(&canonical_file).with_context(|| {
        format!(
            "failed to locate project root for {}",
            canonical_file.display()
        )
    })?;
    let harness = agent_doc_session_actor_io::detect_document_harness_in(
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
    let actor_record = operator_status.record.clone();
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
    })
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
            let tmux = Tmux::default_server();
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
    let prompt_ready = live_pane_prompt_ready(&harness, &captured);
    LivePaneEvidence {
        pane_id: Some(pane_id.clone()),
        source,
        state: if prompt_ready {
            LivePaneState::AliveIdle
        } else {
            LivePaneState::AliveBusy
        },
        current_command: pane_display_value(tmux, &pane_id, "#{pane_current_command}"),
        prompt_ready: Some(prompt_ready),
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

fn live_pane_prompt_ready(harness: &agent_doc_harness::HarnessConfig, captured: &str) -> bool {
    // The harness child has exited and the persistent agent-doc wrapper owns
    // the pane. This is an operator-input surface, not a busy harness turn.
    // Recognizing only the bottom-most exact wrapper prompt prevents stale
    // scrollback from making a live turn look idle.
    if agent_doc_restart_prompt_visible(captured) {
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
            | "Unrecognized input. Press Enter to restart fresh, or 'q' to exit."
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
    let tmux = Tmux::default_server();
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
    if owned_pane_ready_busy_conflict(ctx, &evidence) {
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
    let global_config = agent_doc_config::load().unwrap_or_default();
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
        record: Option<agent_doc_sqlite::state_store::ActorRecord>,
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

    fn test_actor_record(state: ActorState) -> agent_doc_sqlite::state_store::ActorRecord {
        agent_doc_sqlite::state_store::ActorRecord {
            document_id: "/tmp/doc.md".to_string(),
            session_id: "session-1".to_string(),
            generation: 7,
            pane_id: "%7".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
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
        }
    }

    fn test_session_context(
        record: agent_doc_sqlite::state_store::ActorRecord,
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
        // The plan's question-1 state: no trailing `⏵⏵` line, status line last.
        // With the status line ignorable and no busy cue, the `⏵⏵` composer above
        // it becomes the candidate → ready.
        let harness = agent_doc_harness::HarnessConfig::claude();
        let idle = concat!(
            "❯\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)\n",
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@cachyos-x8664\n",
        );
        assert!(live_pane_prompt_ready(&harness, idle));
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

        let state = operator_clear_input_state_for_evidence(&evidence, false, false, false);

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

        let state = operator_clear_input_state_for_evidence(&evidence, false, false, true);

        assert_eq!(state, OperatorClearInputState::Busy);
        assert_eq!(
            agent_doc_controller::operator_clear::clear_guard_outcome(state),
            OperatorClearGuardOutcome::Blocked
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

        let state = operator_clear_input_state_for_evidence(&evidence, false, true, false);

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
        let tmux = Tmux::default_server();

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
    fn session_clear_submit_retry_budget_is_fast_and_independent_from_dispatch() {
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
            CLEAR_DIRECT_SUBMIT_ACCEPTANCE_TIMEOUT <= Duration::from_secs(1),
            "clear should fail fast instead of holding JB Run Agent Doc behind a long submit proof window"
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
    fn last_meaningful_pane_line_trims_ansi_and_blank_lines() {
        assert_eq!(
            last_meaningful_pane_line("\x1b[32mworking\x1b[0m\n\n").as_deref(),
            Some("working")
        );
    }

    #[test]
    fn idle_direct_pane_evidence_supersedes_stale_busy_projection() {
        let record = agent_doc_sqlite::state_store::ActorRecord {
            document_id: "/tmp/doc.md".to_string(),
            session_id: "session-1".to_string(),
            generation: 7,
            pane_id: "%7".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Busy,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
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
            },
            supervisor_socket: PathBuf::from("/tmp/supervisor.sock"),
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
        let socket = format!("session-clear-direct-pane-{}", uuid::Uuid::new_v4());
        let iso = tmux_router::IsolatedTmux::new(&socket);
        let pane = iso.new_session("test", dir.path()).unwrap();
        let output_path = dir.path().join("clear.txt");
        let ready_path = dir.path().join("clear.ready");
        let done_path = dir.path().join("clear.done");
        iso.send_keys(
            &pane,
            &format!(
                "sh -lc 'touch \"{}\"; IFS= read -r line; printf \"%s\" \"$line\" > \"{}\"; touch \"{}\"'",
                ready_path.display(),
                output_path.display(),
                done_path.display()
            ),
        )
        .unwrap();
        let ready_deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < ready_deadline && !ready_path.exists() {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            ready_path.exists(),
            "fixture must prove the line-reader is ready before sending `/clear`",
        );

        send_clear_to_pane(
            &iso,
            &ProvenPane::from_verified_live_owner(pane.clone()),
            Path::new("/tmp/doc.md"),
            "claude",
        )
        .unwrap();
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
        let iso = tmux_router::IsolatedTmux::new("session-clear-direct-pane");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-clear\nagent: codex\n---\n",
        )
        .unwrap();
        let captured = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured_for_ipc = captured.clone();
        let sock =
            agent_doc_supervisor_io::ipc::SupervisorIpc::start(dir.path(), "session-clear", {
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
        agent_doc_session_registry_io::registration::register(
            "session-clear",
            &pane,
            &doc.to_string_lossy(),
        )
        .unwrap();
        agent_doc_session_actor_io::record_session_start(
            &doc,
            "session-clear",
            &pane,
            &pane_window,
            1,
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
        let iso = tmux_router::IsolatedTmux::new("session-clear-pane-select-actor");
        let actor_pane = iso.new_session("test", dir.path()).unwrap();
        let registry_pane = iso.new_window("test", dir.path()).unwrap();
        let actor_record = agent_doc_sqlite::state_store::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session-clear".to_string(),
            generation: 3,
            pane_id: actor_pane.clone(),
            window_id: iso.pane_window(&actor_pane).unwrap(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
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
            },
            supervisor_socket: dir.path().join("session-clear.sock"),
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
        let iso = tmux_router::IsolatedTmux::new("session-clear-pane-select-registry");
        let registry_pane = iso.new_session("test", dir.path()).unwrap();
        let actor_record = agent_doc_sqlite::state_store::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session-clear".to_string(),
            generation: 3,
            pane_id: "%9999".to_string(),
            window_id: "@9999".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
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
            },
            supervisor_socket: dir.path().join("session-clear.sock"),
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
            reclaim_orphaned_cycle_on_clear(&doc),
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
            reclaim_orphaned_cycle_on_clear(&doc),
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
            reclaim_orphaned_cycle_on_clear(&doc),
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
}
