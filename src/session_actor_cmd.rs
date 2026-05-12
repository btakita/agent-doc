use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

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
    let ctx = build_context(file)?;
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
    guard_destructive_operator_on_live_busy_pane(&ctx, "session_restart")?;
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
    guard_destructive_operator_on_live_busy_pane(&ctx, "session_clear")?;
    ensure_supervisor_socket(&ctx)?;
    let tmux = Tmux::default_server();
    if let Some((pane, pane_source)) = resolve_direct_submit_pane(&ctx, &tmux) {
        send_clear_to_pane(&tmux, &pane, &ctx.canonical_file)?;
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

fn send_clear_to_pane(tmux: &Tmux, pane: &str, file: &Path) -> Result<()> {
    crate::sessions::send_submitted_text(tmux, pane, "/clear").with_context(|| {
        format!(
            "failed to send `/clear` to authoritative pane {} for {}",
            pane,
            file.display()
        )
    })
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

fn guard_destructive_operator_on_live_busy_pane(ctx: &SessionContext, action: &str) -> Result<()> {
    let tmux = Tmux::default_server();
    let evidence = live_pane_evidence(ctx, &tmux);
    if evidence.state == LivePaneState::AliveBusy {
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
    Ok(())
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
    harness
        .last_prompt_candidate(captured)
        .is_some_and(|line| harness.is_dispatch_ready_prompt_line(&line))
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
    if !crate::agent::codex::managed_capability_contract_required_for_doc(
        &ctx.canonical_file,
        &fm,
        &global_config,
    ) {
        return "not_required".to_string();
    }
    if let Err(err) = crate::startup_miss::session_log_has_event_after_latest_start(
        &ctx.canonical_file,
        &ctx.session_id,
        &format!("{}_capability_proof status=proven", ctx.harness),
    ) {
        return format!("unknown ({err})");
    }
    for status in ["proven", "failed", "pending"] {
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
    fn live_pane_prompt_ready_rejects_active_output_after_prompt() {
        let harness = crate::harness::HarnessConfig::codex();

        assert!(!live_pane_prompt_ready(
            &harness,
            "›\nexploring repository\n"
        ));
    }

    #[test]
    fn last_meaningful_pane_line_trims_ansi_and_blank_lines() {
        assert_eq!(
            last_meaningful_pane_line("\x1b[32mworking\x1b[0m\n\n").as_deref(),
            Some("working")
        );
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

        send_clear_to_pane(&iso, &pane, Path::new("/tmp/doc.md")).unwrap();
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
