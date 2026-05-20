use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::session_actor::{ActorRecord, ActorState};
use crate::sessions::{SessionEntry, SessionRegistry, Tmux};
use crate::startup_miss::{SessionLogStatus, StartupMiss};
use crate::supervisor::ipc::IpcMethod;

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
    operator_status: crate::project_controller::SessionOperatorStatus,
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
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
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
    let pid = crate::sessions::pane_pid(&pane_id)
        .with_context(|| format!("failed to read pane PID for {pane_id}"))?;
    crate::project_controller::attach_pane(
        &ctx.base_dir,
        crate::project_controller::AttachPaneRequest {
            file: ctx.canonical_file.clone(),
            session_id: ctx.session_id.clone(),
            pane_id: pane_id.clone(),
            window_id: window.clone(),
        },
    )?;
    crate::sessions::attach_projection_only_in(
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

pub fn restart(file: &Path, mode: RestartMode) -> Result<()> {
    let ctx = build_context(file)?;
    let authorization = crate::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_restart",
    )?;
    let tmux = Tmux::default_server();
    guard_starting_actor_operator_command(&ctx, &tmux, OperatorAction::Restart)?;
    guard_destructive_operator_on_live_busy_pane(&ctx, &tmux, "session_restart")?;
    ensure_supervisor_socket(&ctx)?;
    let response = crate::supervisor::ipc::send_command(
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

pub fn clear(file: &Path) -> Result<()> {
    let ctx = build_context(file)?;
    let authorization = crate::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_clear",
    )?;
    let tmux = Tmux::default_server();
    guard_starting_actor_operator_command(&ctx, &tmux, OperatorAction::Clear)?;
    reconcile_idle_projection_before_clear(&ctx, &tmux)?;
    if let Some((pane, pane_source)) = resolve_direct_submit_pane(&ctx, &tmux) {
        send_clear_to_pane(&tmux, &pane, &ctx.canonical_file, &ctx.harness)?;
        crate::ops_log::log_op(
            &ctx.canonical_file,
            &format!(
                "session_clear_sent file={} pane={} delivery=direct_pane_submit submit_mode={} pane_source={}",
                ctx.canonical_file.display(),
                pane,
                TMUX_DIRECT_SUBMIT_MODE,
                pane_source.as_str()
            ),
        );
    } else {
        ensure_supervisor_socket(&ctx)?;
        let response = crate::supervisor::ipc::send_command(
            &ctx.supervisor_socket,
            &IpcMethod::Inject {
                bytes: crate::supervisor::ipc::normalize_submit_text("/clear"),
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
                    .unwrap_or_else(|| "supervisor inject request failed".to_string())
            );
        }
        crate::ops_log::log_op(
            &ctx.canonical_file,
            &format!(
                "session_clear_sent file={} delivery=supervisor_ipc submit_mode={} pane_source=none",
                ctx.canonical_file.display(),
                SUPERVISOR_INJECT_SUBMIT_MODE
            ),
        );
    }
    if matches!(ctx.harness.as_str(), "codex" | "opencode") {
        crate::codex_hook::record_external_prompt_for_file(
            &ctx.canonical_file,
            &ctx.session_id,
            "/clear",
        )?;
    }
    println!(
        "Cleared session context for {} (controller stage {}).",
        ctx.canonical_file.display(),
        authorization.accepted_stage
    );
    Ok(())
}

fn reconcile_idle_projection_before_clear(ctx: &SessionContext, tmux: &Tmux) -> Result<()> {
    let evidence = live_pane_evidence(ctx, tmux);
    if evidence.state == LivePaneState::AliveIdle {
        reconcile_idle_projection_from_evidence(ctx, &evidence)?;
    } else if evidence.state == LivePaneState::AliveBusy {
        if let Some(reason) = protected_clear_input_reason(ctx, tmux, &evidence) {
            let log_reason = reason.replace(char::is_whitespace, "_");
            crate::ops_log::log_op(
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
        crate::ops_log::log_op(
            &ctx.canonical_file,
            &format!(
                "session_clear_active_pane_allowed file={} pane={} source={} current_command={} tail={:?}",
                ctx.canonical_file.display(),
                evidence.pane_id.as_deref().unwrap_or("unknown"),
                evidence.source,
                evidence.current_command.as_deref().unwrap_or("unknown"),
                evidence.tail.as_deref().unwrap_or("unknown")
            ),
        );
    }
    Ok(())
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
    crate::ops_log::log_op(
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
        "{} refused for {} because the authoritative actor is still starting and {}. Wait for a dispatch-ready prompt (`prompt_ready=true`) and retry, or run `agent-doc session status {}` to inspect the pane.",
        action.as_str(),
        ctx.canonical_file.display(),
        reason,
        ctx.canonical_file.display()
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
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(false);
    };
    if state.phase != crate::cycle_state::CyclePhase::Committed {
        return Ok(true);
    }
    let Some(hash) = state.file_hash.as_deref() else {
        return Ok(false);
    };
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    Ok(crate::ops_log::content_hash(&content) != hash)
}

fn protected_clear_input_reason(
    ctx: &SessionContext,
    tmux: &Tmux,
    evidence: &LivePaneEvidence,
) -> Option<String> {
    let pane = evidence.pane_id.as_deref()?;
    let captured = crate::sessions::capture_pane_with_ansi(tmux, pane)
        .or_else(|_| crate::sessions::capture_pane(tmux, pane))
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
    let Ok(captured) = crate::sessions::capture_pane(tmux, pane) else {
        return false;
    };
    let harness = harness_for_evidence(ctx, evidence);
    harness.dispatch_blocker_reason(&captured).as_deref() == Some("clean-exit restart prompt")
}

fn harness_for_evidence(
    ctx: &SessionContext,
    evidence: &LivePaneEvidence,
) -> crate::harness::HarnessConfig {
    evidence
        .current_command
        .as_deref()
        .and_then(crate::harness::HarnessConfig::from_pane_command)
        .unwrap_or_else(|| crate::harness::HarnessConfig::from_agent_name(&ctx.harness))
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

pub fn interrupt_clear(file: &Path) -> Result<()> {
    let ctx = build_context(file)?;
    crate::project_controller::authorize_operator_command(
        &ctx.base_dir,
        &ctx.canonical_file,
        "session_interrupt_clear",
    )?;
    let tmux = Tmux::default_server();
    let evidence = live_pane_evidence(&ctx, &tmux);
    let pane = evidence
        .pane_id
        .as_deref()
        .with_context(|| format!("no live pane evidence for {}", ctx.canonical_file.display()))?;
    if !tmux.pane_alive(pane) {
        crate::ops_log::log_op(
            &ctx.canonical_file,
            &format!(
                "session_interrupt_clear_skip_interrupt file={} pane={} reason=already_closed",
                ctx.canonical_file.display(),
                pane
            ),
        );
        return clear(file);
    }

    send_operator_interrupt_sequence(&tmux, pane, &ctx.harness)?;
    let outcome = wait_for_interrupt_clear_settle(&ctx, &tmux, pane, Duration::from_secs(10));
    crate::ops_log::log_op(
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

fn send_clear_to_pane(tmux: &Tmux, pane: &str, file: &Path, harness: &str) -> Result<()> {
    crate::sessions::send_submitted_text_for_harness(tmux, pane, "/clear", harness).with_context(
        || {
            format!(
                "failed to send `/clear` to authoritative pane {} for {}",
                pane,
                file.display()
            )
        },
    )
}

fn send_operator_interrupt_sequence(tmux: &Tmux, pane: &str, harness: &str) -> Result<()> {
    match harness {
        "opencode" => {
            tmux.send_keys_raw(pane, "Escape")?;
            std::thread::sleep(Duration::from_millis(200));
            tmux.send_keys_raw(pane, "Escape")?;
        }
        "codex" => {
            tmux.send_keys_raw(pane, "C-g")?;
            std::thread::sleep(Duration::from_millis(100));
            tmux.send_keys_raw(pane, "Escape")?;
            std::thread::sleep(Duration::from_millis(100));
            tmux.send_keys_raw(pane, "C-c")?;
        }
        _ => {
            tmux.send_keys_raw(pane, "C-c")?;
        }
    }
    Ok(())
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
            crate::ops_log::log_op(
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
) -> Option<(String, DirectSubmitPaneSource)> {
    if let Some(pane) = ctx
        .actor_record
        .as_ref()
        .map(|record| record.pane_id.as_str())
        .filter(|pane| tmux.pane_alive(pane))
    {
        return Some((pane.to_string(), DirectSubmitPaneSource::AuthoritativeActor));
    }

    if let Some(pane) =
        crate::sync::find_normal_path_owner_pane(tmux, &ctx.canonical_file, &ctx.session_id)
    {
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
        anyhow::bail!(
            "{} refused for {} because pane {} is alive-busy (source={}, current_command={}, tail={:?}). Run `agent-doc session status {}` and wait for an idle prompt, or inspect/stop the pane explicitly before clearing or restarting it.",
            action,
            ctx.canonical_file.display(),
            pane,
            evidence.source,
            command,
            tail,
            ctx.canonical_file.display()
        );
    }
    if evidence.state == LivePaneState::AliveIdle {
        reconcile_idle_projection_from_evidence(ctx, &evidence)?;
    }
    Ok(())
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
    crate::project_controller::mark_lifecycle(
        &ctx.base_dir,
        crate::project_controller::LifecycleRequest {
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
        crate::project_controller::refresh_supervisor_lease(
            &ctx.base_dir,
            crate::project_controller::SupervisorHeartbeatRequest {
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
    crate::ops_log::log_op(
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
        let closeout = crate::repair::repair(file)?;
        let repair_notes = crate::sync::repair_file_state(file)?;
        crate::resync::run_fix(Some(file), None)?;
        println!("Applied repair path for {}.", file.display());
        if closeout.repaired() {
            println!(
                "closeout_repair: {}",
                match closeout {
                    crate::repair::RepairOutcome::ReplayedResponse => {
                        "replayed a captured response through the normal closeout path"
                    }
                    crate::repair::RepairOutcome::AlreadyApplied => {
                        "completed a pending commit boundary for an already-applied response"
                    }
                    crate::repair::RepairOutcome::ManualTailRemovalRespected => {
                        "respected a manual assistant-tail removal while closing the cycle"
                    }
                    crate::repair::RepairOutcome::StalePreflightLockRepaired => {
                        "closed a stale preflight-started cycle"
                    }
                    crate::repair::RepairOutcome::StalePreflightCycleAbandoned => {
                        "abandoned a stale empty preflight-started cycle"
                    }
                    crate::repair::RepairOutcome::CommitBoundaryRecovered => {
                        "recovered a missing commit boundary"
                    }
                    crate::repair::RepairOutcome::TemplateNormalized => {
                        "normalized template drift before closeout"
                    }
                    crate::repair::RepairOutcome::CompletedBacklogReaped => {
                        "reaped a stale completed backlog item during recovery"
                    }
                    crate::repair::RepairOutcome::Noop => unreachable!(),
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

fn build_context(file: &Path) -> Result<SessionContext> {
    let canonical_file = file
        .canonicalize()
        .unwrap_or_else(|_| crate::git::resolve_absolute_file_path(file));
    let content = std::fs::read_to_string(&canonical_file)
        .with_context(|| format!("failed to read {}", canonical_file.display()))?;
    let session_id = crate::frontmatter::read_session_id(&canonical_file)
        .or_else(|| {
            crate::frontmatter::parse(&content)
                .ok()
                .and_then(|(fm, _)| fm.session)
        })
        .with_context(|| format!("{} has no agent_doc_session", canonical_file.display()))?;
    let base_dir = crate::snapshot::find_project_root(&canonical_file).with_context(|| {
        format!(
            "failed to locate project root for {}",
            canonical_file.display()
        )
    })?;
    let harness = crate::session_actor::detect_document_harness_in(
        &base_dir,
        &canonical_file.to_string_lossy(),
    );
    let operator_status =
        crate::project_controller::session_operator_status(&base_dir, &canonical_file)?;
    let registry_entry = lookup_registry_entry(&base_dir, &session_id, &canonical_file)?;
    let startup_miss = crate::startup_miss::load(&canonical_file)?;
    let log_status = crate::startup_miss::session_log_status(&canonical_file, &session_id)?;
    let supervisor_socket = crate::supervisor::ipc::socket_path(&base_dir, &session_id);
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
    let registry = crate::sessions::load_in(base_dir)?;
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
        None => crate::sessions::current_pane().context(
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
    match crate::supervisor::ipc::send_command(socket, &IpcMethod::State) {
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

    let harness = crate::harness::HarnessConfig::from_agent_name(&ctx.harness);
    let captured = crate::sessions::capture_pane(tmux, &pane_id).unwrap_or_default();
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
    if let Some(record) = &ctx.actor_record {
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

fn live_pane_prompt_ready(harness: &crate::harness::HarnessConfig, captured: &str) -> bool {
    if harness.has_busy_cue(captured) {
        return false;
    }
    if harness
        .last_prompt_candidate(captured)
        .is_some_and(|line| harness.is_dispatch_ready_prompt_line(&line))
    {
        return true;
    }
    if harness.binary == "opencode" && harness.is_idle_chrome_only_output(captured) {
        return true;
    }
    harness.is_idle_chrome_only_output(captured)
        || live_pane_bottom_status_is_idle(harness, captured)
}

fn live_pane_bottom_status_is_idle(
    harness: &crate::harness::HarnessConfig,
    captured: &str,
) -> bool {
    if harness.binary != "codex" {
        return false;
    }
    let Some(last_line) = captured
        .lines()
        .rev()
        .map(crate::prompt::strip_ansi)
        .map(|line| line.trim().to_string())
        .find(|line| !line.is_empty())
    else {
        return false;
    };
    if !harness.is_idle_status_line(&last_line) {
        return false;
    }
    if let Some(candidate) = harness.last_prompt_candidate(captured) {
        let stripped = crate::prompt::strip_ansi(&candidate);
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
        .map(crate::prompt::strip_ansi)
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
    operator_status: crate::project_controller::SessionOperatorStatus,
    runtime: &SupervisorRuntime,
) -> Result<crate::project_controller::SessionOperatorStatus> {
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

    crate::project_controller::refresh_supervisor_lease(
        base_dir,
        crate::project_controller::SupervisorHeartbeatRequest {
            file: canonical_file.to_path_buf(),
            session_id: record.session_id.clone(),
            pane_id: record.pane_id.clone(),
            generation: record.generation,
            supervisor_pid: runtime.supervisor_pid,
            supervisor_socket: Some(supervisor_socket.to_string_lossy().to_string()),
            runtime_state: runtime_state.as_str().to_string(),
        },
    )?;
    crate::project_controller::session_operator_status(base_dir, canonical_file)
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
    transition: &crate::project_controller::ActorTransitionStatus,
) -> String {
    crate::session_actor::format_transition_event(crate::session_actor::OwnershipTransitionEvent {
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
                crate::startup_miss::format_timestamp(record.last_transition.timestamp)
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
            crate::startup_miss::format_timestamp(miss.timestamp)
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
                .map(crate::startup_miss::format_timestamp)
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
            crate::startup_miss::format_timestamp(attempt.timestamp)
        );
    } else {
        println!("controller_last_command: none");
    }
    if !ctx.operator_status.projection_diagnostics.is_empty() {
        println!("controller_projection_diagnostics:");
        for diagnostic in &ctx.operator_status.projection_diagnostics {
            println!(
                "- projection={} at={} message={}",
                diagnostic.projection,
                crate::startup_miss::format_timestamp(diagnostic.timestamp),
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
    let global_config = crate::config::Config::default();
    #[cfg(not(test))]
    let global_config = crate::config::load().unwrap_or_default();
    if !crate::agent::codex::managed_capability_contract_required_for_doc_and_harness(
        &ctx.canonical_file,
        &fm,
        &global_config,
        &ctx.harness,
    ) {
        return "not_required".to_string();
    }
    let expected_writable_contract = if ctx.harness == "codex" {
        crate::agent::codex::managed_writable_root_contract_id_for_doc(
            &ctx.canonical_file,
            &fm,
            &global_config,
        )
    } else {
        None
    };
    let proven_prefix = format!("{}_capability_proof status=proven", ctx.harness);
    let proven_result = if let Some(contract) = expected_writable_contract.as_deref() {
        crate::startup_miss::session_log_has_event_after_latest_start_containing(
            &ctx.canonical_file,
            &ctx.session_id,
            &proven_prefix,
            &format!("writable_root_contract={contract}"),
        )
    } else {
        crate::startup_miss::session_log_has_event_after_latest_start(
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
        match crate::startup_miss::session_log_has_event_after_latest_start(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn empty_operator_status(
        record: Option<crate::session_actor::ActorRecord>,
    ) -> crate::project_controller::SessionOperatorStatus {
        crate::project_controller::SessionOperatorStatus {
            record,
            transitions: Vec::new(),
            supervisor_lease: None,
            dispatch_attempts: Vec::new(),
            projection_diagnostics: Vec::new(),
        }
    }

    fn test_actor_record(state: ActorState) -> crate::session_actor::ActorRecord {
        crate::session_actor::ActorRecord {
            document_id: "/tmp/doc.md".to_string(),
            session_id: "session-1".to_string(),
            generation: 7,
            pane_id: "%7".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state,
            last_transition: crate::session_actor::ActorLastTransition {
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
        record: crate::session_actor::ActorRecord,
        runtime: SupervisorRuntime,
        lease_state: Option<&str>,
    ) -> SessionContext {
        let lease = lease_state.map(|state| crate::project_controller::SupervisorLeaseStatus {
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
            operator_status: crate::project_controller::SessionOperatorStatus {
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
        let harness = crate::harness::HarnessConfig::opencode();

        assert!(live_pane_prompt_ready(&harness, "work complete\n>\n"));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_opencode_status_chrome_without_proof_output() {
        let harness = crate::harness::HarnessConfig::opencode();

        assert!(live_pane_prompt_ready(
            &harness,
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_opencode_idle_splash_without_prompt_glyph() {
        let harness = crate::harness::HarnessConfig::opencode();

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
        let harness = crate::harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_footer_below_prior_output() {
        let harness = crate::harness::HarnessConfig::codex();

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
        let harness = crate::harness::HarnessConfig::codex();

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
        let harness = crate::harness::HarnessConfig::codex();

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
        let harness = crate::harness::HarnessConfig::codex();

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
        let harness = crate::harness::HarnessConfig::codex();

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
        let harness = crate::harness::HarnessConfig::codex();

        assert!(!live_pane_prompt_ready(
            &harness,
            "›\nexploring repository\n"
        ));
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
    fn starting_operator_guard_reason_blocks_restart_at_clean_exit_prompt() {
        assert_eq!(
            starting_operator_guard_reason(OperatorAction::Restart, false, false, true),
            "the pane has not reached a dispatch-ready prompt (`prompt_ready=true`)"
        );
    }

    #[test]
    fn document_dirty_after_committed_cycle_detects_post_commit_edit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/doc.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let committed = "---\nagent_doc_session: session-1\n---\n\nDone.\n";
        std::fs::write(&doc, committed).unwrap();
        crate::cycle_state::mark_committed(
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
        let record = crate::session_actor::ActorRecord {
            document_id: "/tmp/doc.md".to_string(),
            session_id: "session-1".to_string(),
            generation: 7,
            pane_id: "%7".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Busy,
            last_transition: crate::session_actor::ActorLastTransition {
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
            operator_status: crate::project_controller::SessionOperatorStatus {
                record: Some(record),
                transitions: Vec::new(),
                supervisor_lease: Some(crate::project_controller::SupervisorLeaseStatus {
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
        crate::session_actor::record_session_start_direct(&doc, "session-status", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-status",
            "%41",
            Some(1),
            ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();
        crate::project_controller::refresh_supervisor_lease(
            dir.path(),
            crate::project_controller::SupervisorHeartbeatRequest {
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

        let sock = crate::supervisor::ipc::SupervisorIpc::start(dir.path(), "session-status", {
            move |method| match method {
                IpcMethod::State => crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({
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
                _ => crate::supervisor::ipc::IpcResponse::ok_empty(),
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn send_clear_to_pane_submits_clear_command() {
        let dir = tempfile::tempdir().unwrap();
        let socket = format!("session-clear-direct-pane-{}", uuid::Uuid::new_v4());
        let iso = crate::sessions::IsolatedTmux::new(&socket);
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
        let iso = crate::sessions::IsolatedTmux::new("session-clear-direct-pane");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-clear\nagent: codex\n---\n",
        )
        .unwrap();
        let captured = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured_for_ipc = captured.clone();
        let sock = crate::supervisor::ipc::SupervisorIpc::start(dir.path(), "session-clear", {
            move |method| match method {
                IpcMethod::Inject { bytes } => {
                    captured_for_ipc.lock().unwrap().push(bytes);
                    crate::supervisor::ipc::IpcResponse::ok_empty()
                }
                IpcMethod::State => crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({
                    "running": true,
                    "state": "healthy",
                    "actor_state": "ready",
                    "restart_count": 0,
                })),
                _ => crate::supervisor::ipc::IpcResponse::ok_empty(),
            }
        })
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let pane_window = iso.pane_window(&pane).unwrap();
        crate::sessions::register("session-clear", &pane, &doc.to_string_lossy()).unwrap();
        crate::session_actor::record_session_start(&doc, "session-clear", &pane, &pane_window, 1)
            .unwrap();
        clear(&doc).unwrap();
        let latest = crate::codex_hook::load_latest_prompt_for_file(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(latest, "/clear");
        assert_eq!(
            captured.lock().unwrap().as_slice(),
            &[crate::supervisor::ipc::normalize_submit_text("/clear")]
        );
        drop(sock);
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_direct_submit_pane_prefers_authoritative_actor() {
        let dir = tempfile::tempdir().unwrap();
        let iso = crate::sessions::IsolatedTmux::new("session-clear-pane-select-actor");
        let actor_pane = iso.new_session("test", dir.path()).unwrap();
        let registry_pane = iso.new_window("test", dir.path()).unwrap();
        let actor_record = crate::session_actor::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session-clear".to_string(),
            generation: 3,
            pane_id: actor_pane.clone(),
            window_id: iso.pane_window(&actor_pane).unwrap(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: crate::session_actor::ActorLastTransition {
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
        let iso = crate::sessions::IsolatedTmux::new("session-clear-pane-select-registry");
        let registry_pane = iso.new_session("test", dir.path()).unwrap();
        let actor_record = crate::session_actor::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session-clear".to_string(),
            generation: 3,
            pane_id: "%9999".to_string(),
            window_id: "@9999".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: crate::session_actor::ActorLastTransition {
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
}
