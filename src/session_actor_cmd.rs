use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent_doc_orchestration::flow::operator_clear::OperatorClearInputState;
use agent_doc_orchestration::session_actor::{ActorRecord, ActorState};
use agent_doc_orchestration::sessions::{SessionEntry, SessionRegistry, Tmux};
use agent_doc_orchestration::startup_miss::{SessionLogStatus, StartupMiss};
use agent_doc_orchestration::supervisor::ipc::IpcMethod;

const TMUX_DIRECT_SUBMIT_MODE: &str = "tmux_literal_enter_delayed";
const SUPERVISOR_INJECT_SUBMIT_MODE: &str = "supervisor_normalized_submit";

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
    operator_status: agent_doc_orchestration::project_controller::SessionOperatorStatus,
    registry_entry: Option<SessionEntry>,
    startup_miss: Option<StartupMiss>,
    log_status: Option<SessionLogStatus>,
    supervisor_runtime: SupervisorRuntime,
    supervisor_socket: PathBuf,
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
    let Some(content) = agent_doc_orchestration::fs_util::read_optional_text(&path)? else {
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
    use agent_doc_orchestration::cycle_state::CyclePhase;
    let base_dir = match file {
        Some(f) => {
            let canonical = f.canonicalize().unwrap_or_else(|_| f.to_path_buf());
            agent_doc_orchestration::snapshot::find_project_root(&canonical).with_context(|| {
                format!("failed to locate project root for {}", canonical.display())
            })?
        }
        None => {
            let cwd = std::env::current_dir().context("failed to read current directory")?;
            agent_doc_orchestration::snapshot::find_project_root(&cwd.join("agent-doc.md"))
                .unwrap_or(cwd)
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

    let store = agent_doc_orchestration::session_actor::load_all_records_in(&base_dir)?;
    let mut rows: Vec<serde_json::Value> = Vec::with_capacity(store.len());
    for (document_id, record) in &store {
        let doc_path = Path::new(document_id);
        let cycle_phase = agent_doc_orchestration::cycle_state::load(doc_path)
            .ok()
            .flatten()
            .map(|s| phase_str(s.phase));
        let recovery_state =
            agent_doc_orchestration::flow::closeout::classify_closeout_recovery_state(doc_path);
        let recovery_command = recovery_state.recovery_command(doc_path);
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
    let pid = agent_doc_orchestration::sessions::pane_pid(&pane_id)
        .with_context(|| format!("failed to read pane PID for {pane_id}"))?;
    agent_doc_orchestration::project_controller::attach_pane(
        &ctx.base_dir,
        agent_doc_orchestration::project_controller::AttachPaneRequest {
            file: ctx.canonical_file.clone(),
            session_id: ctx.session_id.clone(),
            pane_id: pane_id.clone(),
            window_id: window.clone(),
        },
    )?;
    agent_doc_orchestration::sessions::attach_projection_only_in(
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

pub fn restart(file: &Path, mode: RestartMode, force: bool) -> Result<()> {
    let ctx = build_context(file)?;
    let authorization = agent_doc_orchestration::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_restart",
    )?;
    let tmux = Tmux::default_server();
    if !force {
        guard_starting_actor_operator_command(&ctx, &tmux, OperatorAction::Restart)?;
    }
    prepare_restart_live_busy_pane(&ctx, &tmux, force)?;
    ensure_supervisor_socket(&ctx)?;
    let response = agent_doc_orchestration::supervisor::ipc::send_command(
        &ctx.supervisor_socket,
        &IpcMethod::Restart {
            mode: mode.as_str().to_string(),
        },
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
                .unwrap_or_else(|| "supervisor restart request failed".to_string())
        );
    }
    println!(
        "Requested {} restart for {} (controller stage {}).",
        mode.as_str(),
        ctx.canonical_file.display(),
        authorization.accepted_stage
    );
    Ok(())
}

fn prepare_restart_live_busy_pane(ctx: &SessionContext, tmux: &Tmux, force: bool) -> Result<()> {
    let evidence = live_pane_evidence(ctx, tmux);
    // #hj7s: refuse if a terminal editor (e.g. Claude Code `ctrl+g` edit-in-nvim)
    // owns the pane TTY. A raw `C-c` is swallowed by the editor and the follow-up
    // restart-command keystrokes would land in the editor buffer. The editor is a
    // legitimate operator state, so do not force-quit it or SIGINT around it —
    // refuse and let the operator close it manually. `--force` must NOT bypass this
    // (it runs before the force branch).
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
    if !force {
        return guard_destructive_operator_on_live_busy_pane(ctx, tmux, "session_restart");
    }
    if evidence.state == LivePaneState::AliveBusy {
        if pane_shows_clean_exit_prompt(ctx, tmux, &evidence) {
            return Ok(());
        }
        force_restart_live_busy_pane(ctx, tmux, &evidence)?;
        return Ok(());
    }
    if evidence.state == LivePaneState::AliveIdle {
        reconcile_idle_projection_from_evidence(ctx, &evidence)?;
    }
    Ok(())
}

fn force_restart_live_busy_pane(
    ctx: &SessionContext,
    tmux: &Tmux,
    evidence: &LivePaneEvidence,
) -> Result<()> {
    let pane = evidence.pane_id.as_deref().unwrap_or("unknown");
    log_restart_evidence_event(ctx, "session_restart_force_used", evidence);
    let codex_shell_search = codex_pane_in_shell_search_state(ctx, tmux, evidence);
    if let Err(err) = send_operator_interrupt_sequence(tmux, pane, &ctx.harness, codex_shell_search)
    {
        agent_doc_orchestration::ops_log::log_op(
            &ctx.canonical_file,
            &format!(
                "session_restart_force_interrupt_failed file={} pane={} source={} error={:?}",
                ctx.canonical_file.display(),
                pane,
                evidence.source,
                err.to_string()
            ),
        );
        log_restart_evidence_event(ctx, "session_restart_busy_force_killed", evidence);
        return Ok(());
    }

    match wait_for_restart_interrupt_settle(ctx, tmux, pane, Duration::from_secs(2)) {
        RestartInterruptSettleOutcome::Idle { evidence } => {
            let _ = reconcile_idle_projection_from_evidence(ctx, &evidence);
            log_restart_evidence_event(ctx, "session_restart_busy_pre_interrupt_idle", &evidence);
        }
        RestartInterruptSettleOutcome::Closed { evidence } => {
            log_restart_evidence_event(ctx, "session_restart_busy_force_killed", &evidence);
        }
        RestartInterruptSettleOutcome::TimedOut { evidence } => {
            log_restart_evidence_event(ctx, "session_restart_busy_force_killed", &evidence);
        }
    }
    Ok(())
}

pub fn clear(file: &Path) -> Result<()> {
    let ctx = build_context(file)?;
    let authorization = agent_doc_orchestration::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_clear",
    )?;
    let tmux = Tmux::default_server();
    guard_starting_actor_operator_command(&ctx, &tmux, OperatorAction::Clear)?;
    if reconcile_idle_projection_before_clear(&ctx, &tmux)?
        == ClearPreflightOutcome::DeferredPreempt
    {
        // A non-interrupting clear hit a busy pane driven by an active auto-queue
        // loop. The loop is paused (clear cooldown) and the clear is deferred to
        // the next idle gap; do NOT deliver it into the in-flight turn.
        // (`#autoloop-command-preemption` Phase 2.)
        return Ok(());
    }
    if supervisor_clear_inject_available(&ctx) {
        match send_clear_via_supervisor(&ctx)? {
            SupervisorClearDelivery::Sent => {
                agent_doc_orchestration::ops_log::log_op(
                    &ctx.canonical_file,
                    &format!(
                        "session_clear_sent file={} delivery=supervisor_ipc submit_mode={} pane_source=supervisor_runtime",
                        ctx.canonical_file.display(),
                        SUPERVISOR_INJECT_SUBMIT_MODE
                    ),
                );
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
        match send_clear_via_supervisor(&ctx)? {
            SupervisorClearDelivery::Sent => {
                agent_doc_orchestration::ops_log::log_op(
                    &ctx.canonical_file,
                    &format!(
                        "session_clear_sent file={} delivery=supervisor_ipc submit_mode={} pane_source=none",
                        ctx.canonical_file.display(),
                        SUPERVISOR_INJECT_SUBMIT_MODE
                    ),
                );
            }
            SupervisorClearDelivery::LegacyClearUnsupported { error } => {
                anyhow::bail!(
                    "supervisor does not support clear IPC and no live pane is available for direct `/clear` submission for {}: {error}",
                    ctx.canonical_file.display()
                );
            }
        }
    }
    if matches!(ctx.harness.as_str(), "codex" | "opencode") {
        agent_doc_orchestration::codex_hook::record_external_prompt_for_file(
            &ctx.canonical_file,
            &ctx.session_id,
            harness_clear_command(&ctx.harness),
        )?;
    }
    match agent_doc_orchestration::queue_continuation::write_clear_cooldown(&ctx.canonical_file) {
        Ok(()) => {
            agent_doc_orchestration::ops_log::log_op(
                &ctx.canonical_file,
                &format!(
                    "session_clear_queue_cooldown file={} harness={}",
                    ctx.canonical_file.display(),
                    ctx.harness
                ),
            );
        }
        Err(err) => {
            eprintln!(
                "[clear] warning: failed to write queue cooldown marker for {}: {err:#}",
                ctx.canonical_file.display()
            );
            agent_doc_orchestration::ops_log::log_op(
                &ctx.canonical_file,
                &format!(
                    "session_clear_queue_cooldown_failed file={} error={:?}",
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
fn reclaim_orphaned_cycle_on_clear(file: &Path) -> agent_doc_orchestration::repair::CancelOutcome {
    match agent_doc_orchestration::repair::cancel_preflight_cycle(file) {
        Ok(outcome) => {
            agent_doc_orchestration::ops_log::log_op(
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
            agent_doc_orchestration::repair::CancelOutcome::NoOpenCycle
        }
    }
}

fn supervisor_clear_inject_available(ctx: &SessionContext) -> bool {
    matches!(ctx.supervisor_runtime.health, SupervisorHealth::Healthy)
        && ctx.supervisor_socket.exists()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SupervisorClearDelivery {
    Sent,
    LegacyClearUnsupported { error: String },
}

fn send_clear_via_supervisor(ctx: &SessionContext) -> Result<SupervisorClearDelivery> {
    ensure_supervisor_socket(ctx)?;
    // Use the gate-exempt `Clear` control method so an operator can clear a
    // session whose managed-capability proof failed without `kill -9`
    // (#codex-capability-proof-unrecoverable). `Inject` would be refused by the
    // dispatch gate.
    let response = agent_doc_orchestration::supervisor::ipc::send_command(
        &ctx.supervisor_socket,
        &IpcMethod::Clear {
            bytes: agent_doc_orchestration::supervisor::ipc::normalize_submit_text(
                harness_clear_command(&ctx.harness),
            ),
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
    Ok(SupervisorClearDelivery::Sent)
}

fn supervisor_clear_legacy_unsupported_error(error: &str) -> bool {
    error.contains("parse error: unknown variant `clear`") && error.contains("expected one of")
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
    let fallback_suffix = fallback_reason
        .map(|reason| format!(" fallback_reason={reason}"))
        .unwrap_or_default();
    agent_doc_orchestration::ops_log::log_op(
        &ctx.canonical_file,
        &format!(
            "session_clear_sent file={} pane={} delivery=direct_pane_submit submit_mode={} pane_source={}{}",
            ctx.canonical_file.display(),
            pane,
            TMUX_DIRECT_SUBMIT_MODE,
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
        .map(|(pane, source)| live_pane_evidence_for_pane(ctx, tmux, pane, source.as_str()))
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
    agent_doc_orchestration::flow::operator_clear::log_clear_guard_event(
        &ctx.canonical_file,
        clear_state,
    );

    match clear_state {
        OperatorClearInputState::IdlePrompt => {
            reconcile_idle_projection_from_evidence(ctx, &evidence)?;
        }
        OperatorClearInputState::CleanExit | OperatorClearInputState::NoLivePane => {}
        OperatorClearInputState::ProtectedInput => {
            if let Some(reason) = protected_reason {
                let log_reason = reason.replace(char::is_whitespace, "_");
                agent_doc_orchestration::ops_log::log_op(
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
                    protected_clear_refusal_message(&ctx.canonical_file, &evidence, &reason)
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
                agent_doc_orchestration::queue_continuation::detect(&ctx.canonical_file)
                    .unwrap_or(None)
                    .is_some();
            match agent_doc_orchestration::queue_preemption::plan_busy_clear(queue_active) {
                agent_doc_orchestration::queue_preemption::BusyClearOutcome::PauseAndDefer => {
                    // Pause the loop (the idle-queue watch honors this cooldown)
                    // AND record the deferred clear so the supervisor delivers it
                    // at the next idle gap and resumes (`#autoloop-command-preemption`
                    // Phase 2b). The two markers are the durable hand-off between
                    // this command path and the supervisor watch thread.
                    agent_doc_orchestration::queue_continuation::write_clear_cooldown(
                        &ctx.canonical_file,
                    )?;
                    agent_doc_orchestration::queue_continuation::write_deferred_operator_clear(
                        &ctx.canonical_file,
                        harness_clear_command(&ctx.harness),
                    )?;
                    agent_doc_orchestration::ops_log::log_op(
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
                        busy_clear_deferred_message(&ctx.canonical_file, &evidence)
                    );
                    return Ok(ClearPreflightOutcome::DeferredPreempt);
                }
                agent_doc_orchestration::queue_preemption::BusyClearOutcome::HardBlock => {
                    agent_doc_orchestration::ops_log::log_op(
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
                        busy_clear_refusal_message(&ctx.canonical_file, &evidence, &busy_reason)
                    );
                }
            }
        }
    }
    Ok(ClearPreflightOutcome::Proceed)
}

/// User-facing message when a non-interrupting clear pauses an active auto-loop
/// and defers (`#autoloop-command-preemption` Phase 2). The loop is paused via
/// the clear cooldown so the pane reaches an idle gap; the operator can re-run
/// `session clear` there, and the destructive path stays explicit.
fn busy_clear_deferred_message(file: &Path, evidence: &LivePaneEvidence) -> String {
    let pane = evidence.pane_id.as_deref().unwrap_or("unknown");
    format!(
        "session_clear deferred for {} — pane {} is alive-busy under an active `agent:queue auto` loop, so the clear cannot run mid-turn without discarding in-flight work. Paused the loop (clear cooldown); retry `agent-doc session clear {}` once the pane reaches an idle prompt, or run `agent-doc session interrupt-clear {}` to interrupt the turn and clear now.",
        file.display(),
        pane,
        file.display(),
        file.display()
    )
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
    agent_doc_orchestration::ops_log::log_op(
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
        message.push_str(" Pass `--force` to interrupt the running turn and restart anyway.");
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
    let Some(state) = agent_doc_orchestration::cycle_state::load(file)? else {
        return Ok(false);
    };
    if state.phase != agent_doc_orchestration::cycle_state::CyclePhase::Committed {
        return Ok(true);
    }
    let Some(hash) = state.file_hash.as_deref() else {
        return Ok(false);
    };
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    Ok(agent_doc_orchestration::ops_log::content_hash(&content) != hash)
}

fn protected_clear_input_reason(
    ctx: &SessionContext,
    tmux: &Tmux,
    evidence: &LivePaneEvidence,
) -> Option<String> {
    let pane = evidence.pane_id.as_deref()?;
    let captured = agent_doc_orchestration::sessions::capture_pane_with_ansi(tmux, pane)
        .or_else(|_| agent_doc_orchestration::sessions::capture_pane(tmux, pane))
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
    let Ok(captured) = agent_doc_orchestration::sessions::capture_pane(tmux, pane) else {
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
    let captured = agent_doc_orchestration::sessions::capture_pane_with_ansi(tmux, pane)
        .or_else(|_| agent_doc_orchestration::sessions::capture_pane(tmux, pane))
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
) -> agent_doc_orchestration::harness::HarnessConfig {
    evidence
        .current_command
        .as_deref()
        .and_then(agent_doc_orchestration::harness::HarnessConfig::from_pane_command)
        .unwrap_or_else(|| {
            agent_doc_orchestration::harness::HarnessConfig::from_agent_name(&ctx.harness)
        })
}

fn protected_clear_refusal_message(
    file: &Path,
    evidence: &LivePaneEvidence,
    reason: &str,
) -> String {
    let pane = evidence.pane_id.as_deref().unwrap_or("unknown");
    let command = evidence.current_command.as_deref().unwrap_or("unknown");
    let tail = evidence.tail.as_deref().unwrap_or("unknown");
    format!(
        "session_clear refused for {} because pane {} contains protected prompt input (reason={}, source={}, current_command={}, tail={:?}). Clear the prompt input manually, or run `agent-doc session interrupt-clear {}` to intentionally interrupt the pane and clear context.",
        file.display(),
        pane,
        reason,
        evidence.source,
        command,
        tail,
        file.display()
    )
}

fn busy_clear_refusal_message(file: &Path, evidence: &LivePaneEvidence, reason: &str) -> String {
    let pane = evidence.pane_id.as_deref().unwrap_or("unknown");
    let command = evidence.current_command.as_deref().unwrap_or("unknown");
    let tail = evidence.tail.as_deref().unwrap_or("unknown");
    format!(
        "session_clear refused for {} because pane {} is alive-busy (reason={}, source={}, current_command={}, tail={:?}). Wait for an idle prompt, or run `agent-doc session interrupt-clear {}` to intentionally interrupt the pane and clear context.",
        file.display(),
        pane,
        reason,
        evidence.source,
        command,
        tail,
        file.display()
    )
}

pub fn interrupt_clear(file: &Path, force: bool) -> Result<()> {
    if force {
        return force_interrupt_clear(file);
    }

    let ctx = build_context(file)?;
    agent_doc_orchestration::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_interrupt_clear",
    )?;
    // The explicit interrupt-clear is the destructive path and runs now, so it
    // supersedes any clear the non-interrupting path deferred to the idle gap —
    // drop the deferred-clear marker so the supervisor watch does not ALSO
    // deliver a second clear (`#autoloop-command-preemption` Phase 2b).
    if let Err(err) =
        agent_doc_orchestration::queue_continuation::clear_deferred_operator_clear_marker(
            &ctx.canonical_file,
        )
    {
        eprintln!(
            "[interrupt-clear] warning: failed to drop deferred-clear marker for {}: {err:#}",
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
        agent_doc_orchestration::ops_log::log_op(
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
            agent_doc_orchestration::ops_log::log_op(
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
    agent_doc_orchestration::ops_log::log_op(
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
            interrupt_clear_timeout_message(
                &ctx.canonical_file,
                pane,
                &evidence,
                editor_recovery_attempted
            )
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
    if let Err(err) = agent_doc_orchestration::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_interrupt_clear",
    ) {
        eprintln!(
            "[interrupt-clear --force] warning: operator authorization failed for {}: {err:#}; continuing with explicit force cleanup",
            ctx.canonical_file.display()
        );
        agent_doc_orchestration::ops_log::log_op(
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
        registry_removed: force_remove_registry_projection(&ctx)?,
        ..ForceInterruptClearReport::default()
    };

    if let Err(err) =
        agent_doc_orchestration::queue_continuation::clear_deferred_operator_clear_marker(
            &ctx.canonical_file,
        )
    {
        eprintln!(
            "[interrupt-clear --force] warning: failed to drop deferred-clear marker for {}: {err:#}",
            ctx.canonical_file.display()
        );
    }
    match agent_doc_orchestration::queue_continuation::write_clear_cooldown(&ctx.canonical_file) {
        Ok(()) => report.cooldown_written = true,
        Err(err) => eprintln!(
            "[interrupt-clear --force] warning: failed to write queue cooldown marker for {}: {err:#}",
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

    agent_doc_orchestration::ops_log::log_op(
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
    match agent_doc_orchestration::project_controller::mark_lifecycle(
        &ctx.base_dir,
        agent_doc_orchestration::project_controller::LifecycleRequest {
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
            agent_doc_orchestration::ops_log::log_op(
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

fn force_remove_registry_projection(ctx: &SessionContext) -> Result<bool> {
    let registry_path = agent_doc_orchestration::sessions::registry_path_in(&ctx.base_dir);
    let _lock = agent_doc_orchestration::sessions::RegistryLock::acquire(&registry_path)?;
    let mut registry = agent_doc_orchestration::sessions::load_in(&ctx.base_dir)?;
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
        agent_doc_orchestration::sessions::save_in(&ctx.base_dir, &registry)?;
    }
    Ok(removed)
}

#[cfg(unix)]
fn signal_pid_for_force_clear(file: &Path, kind: &str, pid: Option<u32>) -> bool {
    let Some(pid) = pid.filter(|pid| *pid > 0) else {
        return false;
    };
    let signaled = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) == 0 };
    agent_doc_orchestration::ops_log::log_op(
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
        agent_doc_orchestration::ops_log::log_op(
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
        agent_doc_orchestration::ops_log::log_op(
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
    agent_doc_orchestration::ops_log::log_op(
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

fn interrupt_clear_timeout_message(
    file: &Path,
    pane: &str,
    evidence: &LivePaneEvidence,
    editor_recovery_attempted: bool,
) -> String {
    let command = evidence.current_command.as_deref().unwrap_or("unknown");
    let tail = evidence.tail.as_deref().unwrap_or("unknown");
    if editor_recovery_attempted {
        return format!(
            "session_interrupt_clear timed out for {} because pane {} stayed {} after interrupt and forced editor recovery (source={}, current_command={}, prompt_ready={}, tail={:?}). Inspect the pane, exit any editor prompt with `:qa!`, then run `agent-doc session status {}` before retrying.",
            file.display(),
            pane,
            evidence.state.as_str(),
            evidence.source,
            command,
            evidence
                .prompt_ready
                .map(|ready| ready.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            tail,
            file.display()
        );
    }
    format!(
        "session_interrupt_clear timed out for {} because pane {} stayed {} after interrupt (source={}, current_command={}, prompt_ready={}, tail={:?}). Run `agent-doc session status {}` before retrying.",
        file.display(),
        pane,
        evidence.state.as_str(),
        evidence.source,
        command,
        evidence
            .prompt_ready
            .map(|ready| ready.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        tail,
        file.display()
    )
}

fn terminal_editor_command(command: &str) -> bool {
    let name = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
        .trim();
    matches!(
        name,
        "vi" | "view" | "vim" | "vim.basic" | "vimdiff" | "nvim" | "nvimdiff"
    )
}

/// The harness-native command that clears session context (starts a fresh
/// conversation). Claude Code and Codex use `/clear`; OpenCode has **no
/// `/clear` command** — its equivalent is `/new` (`session_new`, "Create a new
/// session"). Submitting `/clear` to an OpenCode pane is a no-op, which is why
/// `Clear Session Context` did nothing for OpenCode-backed documents
/// (#opencode-clear-uses-new). Keep this aligned with the harness slash-command
/// surfaces in `harness.rs` and the session/tmux command spec.
fn harness_clear_command(harness: &str) -> &'static str {
    agent_doc_orchestration::harness::HarnessConfig::from_agent_name(harness)
        .context_clear_command()
}

fn send_clear_to_pane(tmux: &Tmux, pane: &str, file: &Path, harness: &str) -> Result<()> {
    let command = harness_clear_command(harness);
    agent_doc_orchestration::sessions::send_submitted_text_for_harness(tmux, pane, command, harness)
        .with_context(|| {
            format!(
                "failed to send `{command}` to authoritative pane {} for {}",
                pane,
                file.display()
            )
        })
}

/// Ordered interrupt keys for an operator interrupt-clear / force-restart on a
/// live pane. `codex_shell_search` is true only when the Codex pane is in a
/// shell `reverse-i-search` / history-search state — the one place `C-g` is
/// safe and useful (it aborts the search). In the normal Codex TUI composer
/// `C-g` opens the external editor (`$EDITOR`, e.g. nvim), so it must be omitted
/// there and the interrupt falls through to `Escape` + `C-c`
/// (#codex-interrupt-clear-ctrl-g-opens-editor).
fn operator_interrupt_key_plan(harness: &str, codex_shell_search: bool) -> Vec<&'static str> {
    match harness {
        "opencode" => vec!["Escape", "Escape"],
        "codex" if codex_shell_search => vec!["C-g", "Escape", "C-c"],
        "codex" => vec!["Escape", "C-c"],
        _ => vec!["C-c"],
    }
}

fn operator_interrupt_step_delay(harness: &str) -> Duration {
    match harness {
        "opencode" => Duration::from_millis(200),
        _ => Duration::from_millis(100),
    }
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
    let Ok(captured) = agent_doc_orchestration::sessions::capture_pane_with_ansi(tmux, pane)
        .or_else(|_| agent_doc_orchestration::sessions::capture_pane(tmux, pane))
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
            agent_doc_orchestration::ops_log::log_op(
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum RestartInterruptSettleOutcome {
    Idle { evidence: LivePaneEvidence },
    Closed { evidence: LivePaneEvidence },
    TimedOut { evidence: LivePaneEvidence },
}

fn wait_for_restart_interrupt_settle(
    ctx: &SessionContext,
    tmux: &Tmux,
    pane: &str,
    timeout: Duration,
) -> RestartInterruptSettleOutcome {
    let deadline = Instant::now() + timeout;
    loop {
        if !tmux.pane_alive(pane) {
            return RestartInterruptSettleOutcome::Closed {
                evidence: live_pane_evidence(ctx, tmux),
            };
        }
        let evidence = live_pane_evidence(ctx, tmux);
        if evidence.state == LivePaneState::AliveIdle {
            return RestartInterruptSettleOutcome::Idle { evidence };
        }
        if Instant::now() >= deadline {
            return RestartInterruptSettleOutcome::TimedOut { evidence };
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn resolve_direct_submit_pane(
    ctx: &SessionContext,
    tmux: &Tmux,
) -> Option<(String, DirectSubmitPaneSource)> {
    if let Some(pane) = ctx
        .actor_record
        .as_ref()
        .map(|record| record.pane_id.as_str())
        .filter(|pane| tmux.pane_alive(pane))
    {
        return Some((pane.to_string(), DirectSubmitPaneSource::AuthoritativeActor));
    }

    if let Some(pane) = agent_doc_orchestration::sync::find_normal_path_owner_pane(
        tmux,
        &ctx.canonical_file,
        &ctx.session_id,
    ) {
        return Some((pane, DirectSubmitPaneSource::LiveOwner));
    }

    ctx.registry_entry
        .as_ref()
        .map(|entry| entry.pane.as_str())
        .filter(|pane| tmux.pane_alive(pane))
        .map(|pane| (pane.to_string(), DirectSubmitPaneSource::Registry))
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
        "session_restart refused for {} because pane {} is alive-busy (source={}, current_command={}, busy_proof={:?}, tail={:?}). Run `agent-doc session status {}` and wait for an idle prompt, or pass `--force` to interrupt the running turn and restart anyway.",
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
/// refusal parser keys off. Applies to both `Restart` and `--force`
/// `Interrupt and Restart`: the operator must close the editor manually.
pub(crate) fn restart_editor_holds_pane_refusal_message(
    file: &Path,
    pane: &str,
    source: &str,
    command: &str,
    tail: &str,
) -> String {
    format!(
        "session_restart refused for {} because pane {} is held by editor {} (source={}, current_command={}, tail={:?}). Run `agent-doc session status {}` to inspect the pane. A terminal editor (for example Claude Code `ctrl+g` edit-in-nvim) owns the pane, so the restart interrupt is swallowed and the restart command would type into the editor buffer. Close the editor (for example `:wq` in nvim) and retry.",
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
    let captured = agent_doc_orchestration::sessions::capture_pane_with_ansi(tmux, pane)
        .or_else(|_| agent_doc_orchestration::sessions::capture_pane(tmux, pane))
        .ok()?;
    harness_for_evidence(ctx, evidence).busy_proof_line(&captured)
}

fn log_restart_evidence_event(ctx: &SessionContext, event: &str, evidence: &LivePaneEvidence) {
    agent_doc_orchestration::ops_log::log_op(
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
    agent_doc_orchestration::project_controller::mark_lifecycle(
        &ctx.base_dir,
        agent_doc_orchestration::project_controller::LifecycleRequest {
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
        agent_doc_orchestration::project_controller::refresh_supervisor_lease(
            &ctx.base_dir,
            agent_doc_orchestration::project_controller::SupervisorHeartbeatRequest {
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
    agent_doc_orchestration::ops_log::log_op(
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
        let closeout = agent_doc_orchestration::repair::repair(file)?;
        let mut repair_notes = agent_doc_orchestration::sync::repair_file_state(file)?;
        let repair_ctx = build_context(file)?;
        if let Some(note) = clear_closed_actor_pane_projection(&repair_ctx)? {
            repair_notes.push(note);
        }
        agent_doc_orchestration::resync::run_fix(Some(file), None)?;
        println!("Applied repair path for {}.", file.display());
        if closeout.repaired() {
            println!(
                "closeout_repair: {}",
                match closeout {
                    agent_doc_orchestration::repair::RepairOutcome::ReplayedResponse => {
                        "replayed a captured response through the normal closeout path"
                    }
                    agent_doc_orchestration::repair::RepairOutcome::AlreadyApplied => {
                        "completed a pending commit boundary for an already-applied response"
                    }
                    agent_doc_orchestration::repair::RepairOutcome::ManualTailRemovalRespected => {
                        "respected a manual assistant-tail removal while closing the cycle"
                    }
                    agent_doc_orchestration::repair::RepairOutcome::StaleCaptureRetired => {
                        "retired a wedged write-applied capture and rebuilt sidecars from the current document"
                    }
                    agent_doc_orchestration::repair::RepairOutcome::StalePreflightLockRepaired => {
                        "closed a stale preflight-started cycle"
                    }
                    agent_doc_orchestration::repair::RepairOutcome::StalePreflightCycleAbandoned => {
                        "abandoned a stale empty preflight-started cycle"
                    }
                    agent_doc_orchestration::repair::RepairOutcome::CommitBoundaryRecovered => {
                        "recovered a missing commit boundary"
                    }
                    agent_doc_orchestration::repair::RepairOutcome::TemplateNormalized => {
                        "normalized template drift before closeout"
                    }
                    agent_doc_orchestration::repair::RepairOutcome::CompletedBacklogReaped => {
                        "reaped a stale completed backlog item during recovery"
                    }
                    agent_doc_orchestration::repair::RepairOutcome::Noop => unreachable!(),
                }
            );
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
    cleared.last_transition = agent_doc_orchestration::session_actor::ActorLastTransition {
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
    agent_doc_orchestration::project_controller::store_actor_record(
        &ctx.base_dir,
        Some(record.generation),
        &cleared,
    )?;
    agent_doc_orchestration::project_controller::project_sessions_projection_for_actor(
        &ctx.base_dir,
        &cleared.document_id,
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
        .unwrap_or_else(|_| agent_doc_orchestration::git::resolve_absolute_file_path(file));
    let content = std::fs::read_to_string(&canonical_file)
        .with_context(|| format!("failed to read {}", canonical_file.display()))?;
    let session_id = crate::frontmatter::read_session_id(&canonical_file)
        .or_else(|| {
            crate::frontmatter::parse(&content)
                .ok()
                .and_then(|(fm, _)| fm.session)
        })
        .with_context(|| format!("{} has no agent_doc_session", canonical_file.display()))?;
    let base_dir = agent_doc_orchestration::snapshot::find_project_root(&canonical_file)
        .with_context(|| {
            format!(
                "failed to locate project root for {}",
                canonical_file.display()
            )
        })?;
    let harness = agent_doc_orchestration::session_actor::detect_document_harness_in(
        &base_dir,
        &canonical_file.to_string_lossy(),
    );
    let operator_status = agent_doc_orchestration::project_controller::session_operator_status(
        &base_dir,
        &canonical_file,
    )?;
    let registry_entry = lookup_registry_entry(&base_dir, &session_id, &canonical_file)?;
    let startup_miss = agent_doc_orchestration::startup_miss::load(&canonical_file)?;
    let log_status =
        agent_doc_orchestration::startup_miss::session_log_status(&canonical_file, &session_id)?;
    let supervisor_socket =
        agent_doc_orchestration::supervisor::ipc::socket_path(&base_dir, &session_id);
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
    let registry = agent_doc_orchestration::sessions::load_in(base_dir)?;
    Ok(find_registry_entry(&registry, session_id, canonical_file))
}

fn find_registry_entry(
    registry: &SessionRegistry,
    session_id: &str,
    canonical_file: &Path,
) -> Option<SessionEntry> {
    let canonical = canonical_file.to_string_lossy();
    registry.iter().find_map(|(key, entry)| {
        if entry.session_id == session_id || key == canonical.as_ref() || entry.file == canonical {
            Some(entry.clone())
        } else {
            None
        }
    })
}

fn resolve_attach_pane(pane: Option<&str>) -> Result<String> {
    match pane {
        Some(pane_id) => Ok(pane_id.to_string()),
        None => agent_doc_orchestration::sessions::current_pane().context(
            "attach requires --pane when no tmux pane is active in the current environment",
        ),
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
    match agent_doc_orchestration::supervisor::ipc::send_command(socket, &IpcMethod::State) {
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

    let harness = agent_doc_orchestration::harness::HarnessConfig::from_agent_name(&ctx.harness);
    let captured =
        agent_doc_orchestration::sessions::capture_pane(tmux, &pane_id).unwrap_or_default();
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

fn live_pane_prompt_ready(
    harness: &agent_doc_orchestration::harness::HarnessConfig,
    captured: &str,
) -> bool {
    let latest_dispatch_ready_prompt = harness
        .last_prompt_candidate(captured)
        .is_some_and(|line| harness.is_dispatch_ready_prompt_line(&line));
    if harness.binary == "claude" && latest_dispatch_ready_prompt {
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

fn live_pane_bottom_status_is_idle(
    harness: &agent_doc_orchestration::harness::HarnessConfig,
    captured: &str,
) -> bool {
    if harness.binary != "codex" {
        return false;
    }
    let Some(last_line) = captured
        .lines()
        .rev()
        .map(agent_doc_orchestration::prompt::strip_ansi)
        .map(|line| line.trim().to_string())
        .find(|line| !line.is_empty())
    else {
        return false;
    };
    if !harness.is_idle_status_line(&last_line) {
        return false;
    }
    if let Some(candidate) = harness.last_prompt_candidate(captured) {
        let stripped = agent_doc_orchestration::prompt::strip_ansi(&candidate);
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
        .map(agent_doc_orchestration::prompt::strip_ansi)
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
    operator_status: agent_doc_orchestration::project_controller::SessionOperatorStatus,
    runtime: &SupervisorRuntime,
) -> Result<agent_doc_orchestration::project_controller::SessionOperatorStatus> {
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

    agent_doc_orchestration::project_controller::refresh_supervisor_lease(
        base_dir,
        agent_doc_orchestration::project_controller::SupervisorHeartbeatRequest {
            file: canonical_file.to_path_buf(),
            session_id: record.session_id.clone(),
            pane_id: record.pane_id.clone(),
            generation: record.generation,
            supervisor_pid: runtime.supervisor_pid,
            supervisor_socket: Some(supervisor_socket.to_string_lossy().to_string()),
            runtime_state: runtime_state.as_str().to_string(),
        },
    )?;
    agent_doc_orchestration::project_controller::session_operator_status(base_dir, canonical_file)
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

fn format_controller_transition(
    transition: &agent_doc_orchestration::project_controller::ActorTransitionStatus,
) -> String {
    agent_doc_orchestration::session_actor::format_transition_event(
        agent_doc_orchestration::session_actor::OwnershipTransitionEvent {
            caller: &transition.caller,
            reason: &transition.reason,
            prior_generation: transition.prior_generation,
            new_generation: transition.new_generation,
            old_pane: transition.old_pane.as_deref(),
            new_pane: &transition.new_pane,
            old_window: transition.old_window.as_deref(),
            new_window: transition.new_window.as_deref(),
        },
    )
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
                agent_doc_orchestration::startup_miss::format_timestamp(
                    record.last_transition.timestamp
                )
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
            agent_doc_orchestration::startup_miss::format_timestamp(miss.timestamp)
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
                .map(agent_doc_orchestration::startup_miss::format_timestamp)
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
            agent_doc_orchestration::startup_miss::format_timestamp(attempt.timestamp)
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
                agent_doc_orchestration::startup_miss::format_timestamp(diagnostic.timestamp),
                diagnostic.message
            );
        }
    }
}

fn capability_proof_status(ctx: &SessionContext) -> String {
    let content = match std::fs::read_to_string(&ctx.canonical_file) {
        Ok(content) => content,
        Err(err) => return format!("unknown (failed to read document: {err})"),
    };
    let fm = match crate::frontmatter::parse_for_file(&content, &ctx.canonical_file) {
        Ok((fm, _)) => fm,
        Err(err) => return format!("unknown (failed to parse frontmatter: {err})"),
    };
    #[cfg(test)]
    let global_config = agent_doc_orchestration::config::Config::default();
    #[cfg(not(test))]
    let global_config = agent_doc_orchestration::config::load().unwrap_or_default();
    if !agent_doc_orchestration::agent::codex::managed_capability_contract_required_for_doc_and_harness(
        &ctx.canonical_file,
        &fm,
        &global_config,
        &ctx.harness,
    ) {
        return "not_required".to_string();
    }
    let expected_writable_contract = if ctx.harness == "codex" {
        agent_doc_orchestration::agent::codex::managed_writable_root_contract_id_for_doc(
            &ctx.canonical_file,
            &fm,
            &global_config,
        )
    } else {
        None
    };
    let proven_prefix = format!("{}_capability_proof status=proven", ctx.harness);
    let proven_result = if let Some(contract) = expected_writable_contract.as_deref() {
        agent_doc_orchestration::startup_miss::session_log_has_event_after_latest_start_containing(
            &ctx.canonical_file,
            &ctx.session_id,
            &proven_prefix,
            &format!("writable_root_contract={contract}"),
        )
    } else {
        agent_doc_orchestration::startup_miss::session_log_has_event_after_latest_start(
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
        match agent_doc_orchestration::startup_miss::session_log_has_event_after_latest_start(
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
        issues
            .push("sessions.json has no live registry entry for this document session".to_string());
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
    use super::*;
    use std::sync::{Arc, Mutex};

    fn empty_operator_status(
        record: Option<agent_doc_orchestration::session_actor::ActorRecord>,
    ) -> agent_doc_orchestration::project_controller::SessionOperatorStatus {
        agent_doc_orchestration::project_controller::SessionOperatorStatus {
            record,
            transitions: Vec::new(),
            supervisor_lease: None,
            dispatch_attempts: Vec::new(),
            projection_diagnostics: Vec::new(),
        }
    }

    fn test_actor_record(state: ActorState) -> agent_doc_orchestration::session_actor::ActorRecord {
        agent_doc_orchestration::session_actor::ActorRecord {
            document_id: "/tmp/doc.md".to_string(),
            session_id: "session-1".to_string(),
            generation: 7,
            pane_id: "%7".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state,
            last_transition: agent_doc_orchestration::session_actor::ActorLastTransition {
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
        record: agent_doc_orchestration::session_actor::ActorRecord,
        runtime: SupervisorRuntime,
        lease_state: Option<&str>,
    ) -> SessionContext {
        let lease = lease_state.map(|state| {
            agent_doc_orchestration::project_controller::SupervisorLeaseStatus {
                generation: 7,
                supervisor_pid: Some(100),
                supervisor_socket: Some("/tmp/supervisor.sock".to_string()),
                last_heartbeat: Some(1),
                runtime_state: Some(state.to_string()),
            }
        });
        SessionContext {
            canonical_file: PathBuf::from("/tmp/doc.md"),
            base_dir: PathBuf::from("/tmp"),
            session_id: "session-1".to_string(),
            harness: "codex".to_string(),
            actor_record: Some(record.clone()),
            operator_status: agent_doc_orchestration::project_controller::SessionOperatorStatus {
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
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();

        assert!(live_pane_prompt_ready(&harness, "work complete\n>\n"));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_opencode_status_chrome_without_proof_output() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();

        assert!(live_pane_prompt_ready(
            &harness,
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_opencode_idle_splash_without_prompt_glyph() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();

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
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_xhigh_status_chrome_only_output() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 41% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_footer_below_prior_output() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

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
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

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
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

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
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

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
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

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
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(!live_pane_prompt_ready(
            &harness,
            "›\nexploring repository\n"
        ));
    }

    // #jb-stale-busy-idle-footer: a genuinely idle Claude pane (composer + status
    // + permissions) must project ready, while a mid-turn pane (spinner) must not.
    #[test]
    fn live_pane_prompt_ready_accepts_idle_claude_composer() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::claude();
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
    fn live_pane_prompt_ready_rejects_busy_claude_turn() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::claude();
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
        let harness = agent_doc_orchestration::harness::HarnessConfig::claude();
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
        let harness = agent_doc_orchestration::harness::HarnessConfig::claude();
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
        let harness = agent_doc_orchestration::harness::HarnessConfig::claude();
        let idle = concat!(
            "❯\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)\n",
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@cachyos-x8664\n",
        );
        assert!(live_pane_prompt_ready(&harness, idle));
    }

    #[test]
    fn live_pane_prompt_ready_opencode_context_bar_idle_hint() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();
        let idle = "⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13\n";
        assert!(live_pane_prompt_ready(&harness, idle));
    }

    #[test]
    fn live_pane_prompt_ready_opencode_context_bar_with_scrollback() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();
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
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();
        let busy = concat!(
            "Working (14s - esc to interrupt)\n",
            "⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13\n",
        );
        assert!(!live_pane_prompt_ready(&harness, busy));
    }

    #[test]
    fn protected_clear_refusal_points_to_interrupt_clear() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("gpt-5.5 high · ~/work/btakita/agent-loop · Context 85% used".to_string()),
        };

        let message = protected_clear_refusal_message(
            Path::new("/tmp/doc.md"),
            &evidence,
            "drafted prompt input",
        );

        assert!(message.contains("session_clear refused"));
        assert!(message.contains("pane %7 contains protected prompt input"));
        assert!(message.contains("reason=drafted prompt input"));
        assert!(message.contains("agent-doc session interrupt-clear /tmp/doc.md"));
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
            agent_doc_orchestration::flow::operator_clear::clear_guard_outcome(state),
            agent_doc_orchestration::flow::types::FlowOutcome::Completed
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
            agent_doc_orchestration::flow::operator_clear::clear_guard_outcome(state),
            agent_doc_orchestration::flow::types::FlowOutcome::Blocked
        );
        let message =
            busy_clear_refusal_message(Path::new("/tmp/doc.md"), &evidence, "active codex turn");
        assert!(message.contains("session_clear refused"));
        assert!(message.contains("pane %7 is alive-busy"));
        assert!(message.contains("reason=active codex turn"));
        assert!(message.contains("agent-doc session interrupt-clear /tmp/doc.md"));
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
        assert!(message.contains("interrupt the running turn and restart anyway"));
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
            agent_doc_orchestration::flow::operator_clear::clear_guard_outcome(state),
            agent_doc_orchestration::flow::types::FlowOutcome::Completed
        );
    }

    #[test]
    fn terminal_editor_command_detects_vim_family_processes() {
        for command in [
            "vi",
            "view",
            "vim",
            "vim.basic",
            "vimdiff",
            "nvim",
            "nvimdiff",
        ] {
            assert!(
                terminal_editor_command(command),
                "{command} should trigger interrupt-clear editor recovery"
            );
        }
        assert!(!terminal_editor_command("codex"));
        assert!(!terminal_editor_command("agent-doc"));
        assert!(!terminal_editor_command("vim-addon-manager"));
    }

    #[test]
    fn operator_interrupt_key_plan_omits_ctrl_g_for_codex_composer() {
        // #codex-interrupt-clear-ctrl-g-opens-editor: C-g opens the external
        // editor (nvim) in the Codex composer, so the normal interrupt path must
        // not send it — Escape + C-c is the safe interrupt.
        assert_eq!(
            operator_interrupt_key_plan("codex", false),
            vec!["Escape", "C-c"]
        );
        assert!(!operator_interrupt_key_plan("codex", false).contains(&"C-g"));
    }

    #[test]
    fn operator_interrupt_key_plan_sends_ctrl_g_only_for_codex_shell_search() {
        // C-g is safe (aborts the search) only when the Codex pane is in a shell
        // reverse-i-search / history-search state.
        assert_eq!(
            operator_interrupt_key_plan("codex", true),
            vec!["C-g", "Escape", "C-c"]
        );
    }

    #[test]
    fn operator_interrupt_key_plan_unchanged_for_other_harnesses() {
        // The codex_shell_search flag is codex-scoped and must not perturb other
        // harnesses' interrupt sequences.
        assert_eq!(
            operator_interrupt_key_plan("opencode", false),
            vec!["Escape", "Escape"]
        );
        assert_eq!(
            operator_interrupt_key_plan("opencode", true),
            vec!["Escape", "Escape"]
        );
        assert_eq!(operator_interrupt_key_plan("claude", false), vec!["C-c"]);
        assert_eq!(operator_interrupt_key_plan("claude", true), vec!["C-c"]);
    }

    #[test]
    fn interrupt_clear_timeout_message_reports_editor_recovery() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("vim".to_string()),
            prompt_ready: Some(false),
            tail: Some("-- INSERT --".to_string()),
        };
        let message =
            interrupt_clear_timeout_message(Path::new("/tmp/doc.md"), "%7", &evidence, true);

        assert!(message.contains("forced editor recovery"));
        assert!(message.contains("stayed alive-busy"));
        assert!(message.contains("source=authoritative_actor"));
        assert!(message.contains("current_command=vim"));
        assert!(message.contains("prompt_ready=false"));
        assert!(message.contains("tail=\"-- INSERT --\""));
        assert!(message.contains(":qa!"));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
    }

    #[test]
    fn interrupt_clear_timeout_message_reports_last_command_without_editor_recovery() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("codex".to_string()),
            prompt_ready: Some(false),
            tail: Some("⏵⏵ bypass permissions on".to_string()),
        };
        let message =
            interrupt_clear_timeout_message(Path::new("/tmp/doc.md"), "%7", &evidence, false);

        assert!(!message.contains("forced editor recovery"));
        assert!(message.contains("stayed alive-busy"));
        assert!(message.contains("source=authoritative_actor"));
        assert!(message.contains("current_command=codex"));
        assert!(message.contains("prompt_ready=false"));
        assert!(message.contains("tail=\"⏵⏵ bypass permissions on\""));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
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
        assert!(message.contains("interrupt the running turn and restart anyway"));
    }

    #[test]
    fn document_dirty_after_committed_cycle_detects_post_commit_edit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/doc.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let committed = "---\nagent_doc_session: session-1\n---\n\nDone.\n";
        std::fs::write(&doc, committed).unwrap();
        agent_doc_orchestration::cycle_state::mark_committed(
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
    fn last_meaningful_pane_line_trims_ansi_and_blank_lines() {
        assert_eq!(
            last_meaningful_pane_line("\x1b[32mworking\x1b[0m\n\n").as_deref(),
            Some("working")
        );
    }

    #[test]
    fn idle_direct_pane_evidence_supersedes_stale_busy_projection() {
        let record = agent_doc_orchestration::session_actor::ActorRecord {
            document_id: "/tmp/doc.md".to_string(),
            session_id: "session-1".to_string(),
            generation: 7,
            pane_id: "%7".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Busy,
            last_transition: agent_doc_orchestration::session_actor::ActorLastTransition {
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
            operator_status: agent_doc_orchestration::project_controller::SessionOperatorStatus {
                record: Some(record),
                transitions: Vec::new(),
                supervisor_lease: Some(
                    agent_doc_orchestration::project_controller::SupervisorLeaseStatus {
                        generation: 7,
                        supervisor_pid: Some(100),
                        supervisor_socket: Some("/tmp/supervisor.sock".to_string()),
                        last_heartbeat: Some(1),
                        runtime_state: Some("busy".to_string()),
                    },
                ),
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
        agent_doc_orchestration::session_actor::record_session_start_direct(
            &doc,
            "session-status",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_orchestration::session_actor::transition_state_direct(
            &doc,
            "session-status",
            "%41",
            Some(1),
            ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();
        agent_doc_orchestration::project_controller::refresh_supervisor_lease(
            dir.path(),
            agent_doc_orchestration::project_controller::SupervisorHeartbeatRequest {
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

        let sock = agent_doc_orchestration::supervisor::ipc::SupervisorIpc::start(
            dir.path(),
            "session-status",
            {
                move |method| match method {
                    IpcMethod::State => agent_doc_orchestration::supervisor::ipc::IpcResponse::ok(
                        serde_json::json!({
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
                        }),
                    ),
                    _ => agent_doc_orchestration::supervisor::ipc::IpcResponse::ok_empty(),
                }
            },
        )
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
        assert_eq!(ctx.operator_status.transitions.len(), 2);
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
        assert!(issues.iter().any(|issue| issue.contains("registry entry")));
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
        std::thread::sleep(std::time::Duration::from_millis(150));
        let output_path = dir.path().join("clear.txt");
        let done_path = dir.path().join("clear.done");
        iso.send_keys(
            &pane,
            &format!(
                "sh -lc 'IFS= read -r line; printf \"%s\" \"$line\" > \"{}\"; touch \"{}\"'",
                output_path.display(),
                done_path.display()
            ),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));

        send_clear_to_pane(&iso, &pane, Path::new("/tmp/doc.md"), "claude").unwrap();
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
        let sock = agent_doc_orchestration::supervisor::ipc::SupervisorIpc::start(
            dir.path(),
            "session-clear",
            {
                move |method| match method {
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        captured_for_ipc.lock().unwrap().push(bytes);
                        agent_doc_orchestration::supervisor::ipc::IpcResponse::ok_empty()
                    }
                    IpcMethod::State => agent_doc_orchestration::supervisor::ipc::IpcResponse::ok(
                        serde_json::json!({
                            "running": true,
                            "state": "healthy",
                            "actor_state": "ready",
                            "restart_count": 0,
                        }),
                    ),
                    _ => agent_doc_orchestration::supervisor::ipc::IpcResponse::ok_empty(),
                }
            },
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let pane_window = iso.pane_window(&pane).unwrap();
        agent_doc_orchestration::sessions::register("session-clear", &pane, &doc.to_string_lossy())
            .unwrap();
        agent_doc_orchestration::session_actor::record_session_start(
            &doc,
            "session-clear",
            &pane,
            &pane_window,
            1,
        )
        .unwrap();
        clear(&doc).unwrap();
        let latest = agent_doc_orchestration::codex_hook::load_latest_prompt_for_file(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(latest, "/clear");
        assert_eq!(
            captured.lock().unwrap().as_slice(),
            &[agent_doc_orchestration::supervisor::ipc::normalize_submit_text("/clear")]
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
        let actor_record = agent_doc_orchestration::session_actor::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session-clear".to_string(),
            generation: 3,
            pane_id: actor_pane.clone(),
            window_id: iso.pane_window(&actor_pane).unwrap(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: agent_doc_orchestration::session_actor::ActorLastTransition {
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
            Some((actor_pane, DirectSubmitPaneSource::AuthoritativeActor))
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_direct_submit_pane_falls_back_to_registry() {
        let dir = tempfile::tempdir().unwrap();
        let iso = tmux_router::IsolatedTmux::new("session-clear-pane-select-registry");
        let registry_pane = iso.new_session("test", dir.path()).unwrap();
        let actor_record = agent_doc_orchestration::session_actor::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session-clear".to_string(),
            generation: 3,
            pane_id: "%9999".to_string(),
            window_id: "@9999".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: agent_doc_orchestration::session_actor::ActorLastTransition {
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
            Some((registry_pane, DirectSubmitPaneSource::Registry))
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
        agent_doc_orchestration::cycle_state::start_preflight(&doc, Some(&content), Some(&content))
            .unwrap();

        // The clear path reclaims the orphaned cycle so the next Run Agent Doc
        // is not wedged by a stale open cycle.
        assert_eq!(
            reclaim_orphaned_cycle_on_clear(&doc),
            agent_doc_orchestration::repair::CancelOutcome::Abandoned
        );
        let state = agent_doc_orchestration::cycle_state::load(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            state.phase,
            agent_doc_orchestration::cycle_state::CyclePhase::Abandoned
        );
    }

    #[test]
    fn clear_protects_cycle_that_already_captured_a_response() {
        let (_dir, doc) = clear_reclaim_project();
        let content = std::fs::read_to_string(&doc).unwrap();
        agent_doc_orchestration::cycle_state::start_preflight(&doc, Some(&content), Some(&content))
            .unwrap();
        agent_doc_orchestration::capture::capture_response(
            &doc,
            "### Re: do — opus-4-8\n\nDone.\n",
        )
        .unwrap();

        // A cycle that owns a captured response must not be discarded by clear.
        assert_eq!(
            reclaim_orphaned_cycle_on_clear(&doc),
            agent_doc_orchestration::repair::CancelOutcome::Protected
        );
        assert!(
            agent_doc_orchestration::cycle_state::load(&doc)
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
            agent_doc_orchestration::repair::CancelOutcome::NoOpenCycle
        );
    }
}
