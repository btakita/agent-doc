//! Direct-run prompt, diff, and typed auto-queue IO graph.

use agent_doc_diff as diff;
use agent_doc_document::queue_projection::strip_in_progress_marker;
use agent_doc_element::element;
use agent_doc_frontmatter::frontmatter;
use agent_doc_prompt_cache::PromptCacheBlocks;
use agent_doc_queue_io::queue_consume;
use agent_doc_session_accretion::SessionAccretionReport;
use agent_doc_turn::owner_pane_recursion::{
    recursive_direct_invocation_message, recursive_start_invocation_message,
};
use agent_doc_workflow::owner_pane_self_invocation::{
    OwnedPaneSelfInvocation, OwnedPaneSelfInvocationInput, OwnedPaneSelfInvocationKind,
    OwnedPaneSelfInvocationOptions, build_owned_pane_self_invocation,
};
use anyhow::{Context, Result};
#[cfg(unix)]
use std::fs::OpenOptions;
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const AGENT_DOC_RUN_HEARTBEAT_SECS_ENV: &str = "AGENT_DOC_RUN_HEARTBEAT_SECS";
const DEFAULT_RUN_HEARTBEAT_SECS: u64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    Append,
    Template,
}

impl RunMode {
    pub fn from_frontmatter(fm: &frontmatter::Frontmatter) -> Self {
        if fm.resolve_mode().is_template() {
            Self::Template
        } else {
            Self::Append
        }
    }

    fn cache_label(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Template => "template",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCycleOutcome {
    pub dispatched: bool,
    pub queue_synthetic_diff: bool,
    pub queue_consumption: Option<queue_consume::QueueConsumptionOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoQueueContinuation {
    Stop,
    Continue { force_fresh_agent_session: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveQueuePromptState {
    Ready {
        prompt: String,
    },
    Inactive,
    StopFence {
        next_prompt: Option<String>,
    },
    TimeGate {
        start_at: String,
        next_prompt: Option<String>,
    },
    ItemModified {
        snapshot_head: Option<String>,
        document_head: Option<String>,
    },
    Unproven {
        reason: String,
        document_head: Option<String>,
    },
    Empty,
}

pub fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(unix)]
pub struct RunStderrRedirect {
    saved_stderr: Option<OwnedFd>,
}

#[cfg(unix)]
impl RunStderrRedirect {
    pub fn inactive() -> Self {
        Self { saved_stderr: None }
    }

    pub fn maybe_start(file: &Path) -> Self {
        if !run_stderr_redirect_needed() {
            return Self::inactive();
        }
        match Self::start(file) {
            Ok(guard) => guard,
            Err(err) => {
                eprintln!("[run] warning: could not redirect stderr for managed TUI: {err:#}");
                Self::inactive()
            }
        }
    }

    fn start(file: &Path) -> Result<Self> {
        let canonical = file
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", file.display()))?;
        let project_root = agent_doc_project_root_io::project_root_containing(&canonical)
            .with_context(|| format!("failed to resolve project root for {}", file.display()))?;
        let logs_dir = project_root.join(".agent-doc").join("logs");
        std::fs::create_dir_all(&logs_dir)
            .with_context(|| format!("failed to create {}", logs_dir.display()))?;
        let stderr_path = logs_dir.join("run-stderr.log");
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stderr_path)
            .with_context(|| format!("failed to open {}", stderr_path.display()))?;
        let saved_fd = unsafe { libc::dup(libc::STDERR_FILENO) };
        if saved_fd < 0 {
            anyhow::bail!("dup(stderr) failed: {}", std::io::Error::last_os_error());
        }
        let saved_stderr = unsafe { OwnedFd::from_raw_fd(saved_fd) };
        let redirected = unsafe { libc::dup2(log_file.as_raw_fd(), libc::STDERR_FILENO) };
        if redirected < 0 {
            anyhow::bail!("dup2(stderr) failed: {}", std::io::Error::last_os_error());
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "run_stderr_redirect harness={} tmux_pane={} target={}",
                agent_doc_model_tier::detect_harness(),
                std::env::var("TMUX_PANE").unwrap_or_else(|_| "<unset>".to_string()),
                stderr_path.display()
            ),
        );
        eprintln!(
            "[run] stderr redirected to {} for managed TUI",
            stderr_path.display()
        );
        Ok(Self {
            saved_stderr: Some(saved_stderr),
        })
    }
}

#[cfg(unix)]
impl Drop for RunStderrRedirect {
    fn drop(&mut self) {
        let Some(saved_stderr) = self.saved_stderr.take() else {
            return;
        };
        let restored = unsafe { libc::dup2(saved_stderr.as_raw_fd(), libc::STDERR_FILENO) };
        if restored < 0 {
            let msg = b"[run] warning: failed to restore stderr after managed TUI redirect\n";
            unsafe {
                libc::write(
                    saved_stderr.as_raw_fd(),
                    msg.as_ptr().cast::<libc::c_void>(),
                    msg.len(),
                );
            }
        }
    }
}

#[cfg(not(unix))]
pub struct RunStderrRedirect;

#[cfg(not(unix))]
impl RunStderrRedirect {
    pub fn inactive() -> Self {
        Self
    }

    pub fn maybe_start(_file: &Path) -> Self {
        Self
    }
}

fn run_stderr_redirect_needed() -> bool {
    if agent_doc_tmux_commands::input_diag::verbose_enabled()
        || std::env::var_os("TMUX_PANE").is_none()
    {
        return false;
    }
    if std::env::var_os("AGENT_DOC_FORCE_RUN_STDERR_REDIRECT").is_none()
        && !std::io::stderr().is_terminal()
    {
        return false;
    }
    run_stderr_redirect_harness(agent_doc_model_tier::detect_harness().as_str())
}

pub fn run_stderr_redirect_harness(harness: &str) -> bool {
    matches!(harness, "claude" | "codex" | "opencode")
}

pub struct RunHeartbeat {
    stop: mpsc::Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl RunHeartbeat {
    pub fn start(
        file: &Path,
        phase: &'static str,
        agent_name: &str,
        timeout: Option<Duration>,
    ) -> Self {
        let file = file.to_path_buf();
        let agent_name = agent_name.to_string();
        let (stop, stop_rx) = mpsc::channel();
        let interval = run_heartbeat_interval();
        let handle = std::thread::spawn(move || {
            let started = Instant::now();
            loop {
                if stop_rx.recv_timeout(interval).is_ok() {
                    break;
                }
                let elapsed = started.elapsed();
                let timeout_detail = timeout
                    .map(|timeout| format!(" timeout_s={}", timeout.as_secs()))
                    .unwrap_or_default();
                let event = format!(
                    "run_heartbeat phase={} agent={} elapsed_s={}{}",
                    phase,
                    agent_name,
                    elapsed.as_secs(),
                    timeout_detail
                );
                let state = agent_doc_cycle_state_io::record_open_cycle_progress(&file, &event)
                    .ok()
                    .flatten();
                let (cycle_id, cycle_phase, last_event_age) = state
                    .as_ref()
                    .map(|state| {
                        (
                            state.cycle_id.as_str(),
                            state.phase.as_str(),
                            state.updated_at.saturating_sub(state.started_at),
                        )
                    })
                    .unwrap_or(("<unknown>", "<unknown>", 0));
                eprintln!(
                    "[run] heartbeat phase={} elapsed_s={}{} cycle_id={} cycle_phase={} cycle_age_s={}",
                    phase,
                    elapsed.as_secs(),
                    timeout_detail,
                    cycle_id,
                    cycle_phase,
                    last_event_age
                );
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for RunHeartbeat {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_heartbeat_interval() -> Duration {
    let secs = std::env::var(AGENT_DOC_RUN_HEARTBEAT_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_RUN_HEARTBEAT_SECS);
    Duration::from_secs(secs.max(1))
}

pub fn record_run_progress(file: &Path, phase: &str, agent_name: &str, timeout: Option<Duration>) {
    let timeout_detail = timeout
        .map(|timeout| format!(" timeout_s={}", timeout.as_secs()))
        .unwrap_or_default();
    let event = format!("run_progress phase={phase} agent={agent_name}{timeout_detail}");
    let _ = agent_doc_cycle_state_io::record_open_cycle_progress(file, &event);
    eprintln!("[run] progress phase={phase}{timeout_detail}");
}

pub fn mark_run_write_applied(file: &Path, event: &str) -> Result<()> {
    let file_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} after run write", file.display()))?;
    let snapshot_content = agent_doc_snapshot_io::load(file)?;
    agent_doc_cycle_state_io::mark_write_applied(
        file,
        event,
        snapshot_content.as_deref(),
        Some(&file_content),
    )?;
    Ok(())
}

pub fn start_run_cycle(file: &Path) -> Result<()> {
    agent_doc_cycle_state_io::admit_with_current_resolver(
        file,
        |file, disk| agent_doc_document_realtime_io::resolve_current_doc(file, disk).content,
        agent_doc_snapshot_io::load,
        agent_doc_ops_log_io::log_op,
    )?;
    Ok(())
}

pub fn record_run_preflight_timeout(file: &Path, event: &str, diagnostic: &str) -> Result<()> {
    let compact = diagnostic.split_whitespace().collect::<Vec<_>>().join(" ");
    let event = format!("{event} {}", compact.chars().take(700).collect::<String>());
    agent_doc_cycle_state_io::mark_recoverable_preflight_timeout(file, &event)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "run_preflight_timeout file={} event={} diagnostic={}",
            file.display(),
            event.split_whitespace().next().unwrap_or(event.as_str()),
            compact
        ),
    );
    Ok(())
}

pub fn abandon_run_recursive_cycle(
    effects: &impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects,
    file: &Path,
    event: &str,
    diagnostic: &str,
) -> Result<()> {
    let compact = diagnostic.split_whitespace().collect::<Vec<_>>().join(" ");
    let event = format!("{event} {}", compact.chars().take(700).collect::<String>());
    let snapshot_content = agent_doc_snapshot_io::load(file)?;
    let file_content = std::fs::read_to_string(file).ok();
    agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(
        effects,
        file,
        &event,
        snapshot_content.as_deref(),
        file_content.as_deref(),
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "run_recursive_direct_invocation_abandoned file={} diagnostic={}",
            file.display(),
            compact
        ),
    );
    Ok(())
}

pub fn owned_pane_self_invocation_detail(
    file: &Path,
    session_id: &str,
    agent_name: &str,
) -> Option<String> {
    if agent_name != "codex" || agent_doc_model_tier::detect_harness() != "codex" {
        return None;
    }
    let tmux = tmux_router::Tmux::default_server();
    let current_pane = agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux)?;
    let registry_match = agent_doc_session_registry_io::lookup_entry(session_id)
        .ok()
        .flatten()
        .filter(|entry| entry.pane == current_pane);
    let actor = actor_record_for_file(file).ok().flatten();
    let actor_match = actor
        .as_ref()
        .is_some_and(|record| record.pane_id == current_pane);
    if registry_match.is_none() && !actor_match {
        return None;
    }
    let actor_detail = actor
        .as_ref()
        .map(|record| {
            format!(
                "actor_generation={} actor_state={} actor_pane={}",
                record.generation,
                record.state.as_str(),
                record.pane_id
            )
        })
        .unwrap_or_else(|| {
            "actor_generation=<unknown> actor_state=<unknown> actor_pane=<unknown>".to_string()
        });
    Some(format!(
        "current_pane={} session_id={} {}",
        current_pane, session_id, actor_detail
    ))
}

pub fn detect_owned_pane_self_invocation(
    file: &Path,
    session_id: &str,
    agent_name: &str,
    unresolved_prompt: Option<String>,
) -> Result<Option<OwnedPaneSelfInvocation>> {
    detect_owned_pane_self_invocation_with_options(
        file,
        session_id,
        agent_name,
        unresolved_prompt,
        OwnedPaneSelfInvocationOptions::default(),
    )
}

pub fn detect_owned_pane_self_invocation_with_options(
    file: &Path,
    session_id: &str,
    agent_name: &str,
    unresolved_prompt: Option<String>,
    options: OwnedPaneSelfInvocationOptions,
) -> Result<Option<OwnedPaneSelfInvocation>> {
    if owned_pane_self_invocation_detail(file, session_id, agent_name).is_none() {
        return Ok(None);
    }
    let tmux = tmux_router::Tmux::default_server();
    let current_pane =
        agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux).unwrap_or_default();
    let actor = actor_record_for_file(file).ok().flatten();
    let actor_generation = actor.as_ref().map(|record| record.generation);
    let actor_state = actor
        .as_ref()
        .map(|record| record.state.as_str().to_string());
    let file_display = file.display().to_string();
    if let Some(unresolved) = unresolved_prompt.filter(|p| !p.trim().is_empty()) {
        return Ok(Some(build_owned_pane_self_invocation(
            OwnedPaneSelfInvocationInput {
                file: &file_display,
                current_pane: &current_pane,
                session_id,
                actor_generation,
                actor_state: actor_state.as_deref(),
                kind: OwnedPaneSelfInvocationKind::UnresolvedPrompt,
                work: &unresolved,
                head_id: None,
            },
        )));
    }
    if !options.suppress_active_queue_head
        && let Some(continuation) = agent_doc_queue_io::queue_continuation::detect(file)?
    {
        return Ok(Some(build_owned_pane_self_invocation(
            OwnedPaneSelfInvocationInput {
                file: &file_display,
                current_pane: &current_pane,
                session_id,
                actor_generation,
                actor_state: actor_state.as_deref(),
                kind: OwnedPaneSelfInvocationKind::ActiveQueueHead,
                work: &continuation.head_prompt,
                head_id: continuation.head_id.as_deref(),
            },
        )));
    }
    Ok(None)
}

pub fn owner_pane_queue_edit_should_defer_until_closeout(
    file: &Path,
    diff_text: &str,
    current_content: &str,
) -> bool {
    let open_cycle = agent_doc_cycle_state_io::load(file)
        .ok()
        .flatten()
        .is_some_and(|state| state.is_open());
    if !open_cycle {
        return false;
    }
    let Some(scope) = agent_doc_turn_scope_io::load(file) else {
        return false;
    };
    let prompt_bearing_changes = diff::classify_prompt_bearing_changes(diff_text);
    if prompt_bearing_changes.is_empty() {
        return false;
    }
    let Some(previous) = agent_doc_snapshot_io::load(file).ok().flatten() else {
        return false;
    };
    let Some(summary) = agent_doc_diff::semantic::semantic_diff_summary(
        &previous,
        current_content,
        &prompt_bearing_changes,
    ) else {
        return false;
    };
    let document_path = file.to_string_lossy().to_string();
    let ops =
        agent_doc_turn::op_log::build_ops_from_semantic_diff(&document_path, None, "", &summary);
    let affectedness = agent_doc_turn::turn_scope::classify_cycle(&ops, &scope);
    !affectedness.turn_affected
}

pub fn owner_pane_queue_edit_deferred_outcome(
    file: &Path,
    queue_synthetic_diff: bool,
    detail: &str,
    continuation: &agent_doc_queue::queue_continuation::QueueContinuation,
) -> RunCycleOutcome {
    eprintln!(
        "[run] owner-pane queue edit deferred until current closeout for {} (head_id={} {})",
        file.display(),
        continuation.head_id.as_deref().unwrap_or("<none>"),
        detail
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "run_owned_pane_queue_edit_deferred file={} head_id={} {}",
            file.display(),
            continuation.head_id.as_deref().unwrap_or("<none>"),
            detail
        ),
    );
    RunCycleOutcome {
        dispatched: false,
        queue_synthetic_diff,
        queue_consumption: None,
    }
}

pub fn recursive_codex_direct_invocation_diagnostic(
    file: &Path,
    session_id: &str,
    agent_name: &str,
) -> Option<String> {
    let detail = owned_pane_self_invocation_detail(file, session_id, agent_name)?;
    Some(recursive_direct_invocation_message(
        &file.display().to_string(),
        &detail,
    ))
}

pub fn recursive_codex_start_invocation_diagnostic(
    file: &Path,
    session_id: &str,
    agent_name: &str,
) -> Option<String> {
    let detail = owned_pane_self_invocation_detail(file, session_id, agent_name)?;
    Some(recursive_start_invocation_message(
        &file.display().to_string(),
        &detail,
    ))
}

pub fn run_dispatch_timeout_diagnostic(file: &Path, agent_name: &str) -> String {
    let state = agent_doc_cycle_state_io::load(file).ok().flatten();
    let actor = actor_record_for_file(file).ok().flatten();
    let tmux = tmux_router::Tmux::default_server();
    let current_pane = agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux);
    let (cycle_id, phase, last_event) = state
        .as_ref()
        .map(|state| {
            (
                state.cycle_id.as_str(),
                state.phase.as_str(),
                state.last_event.as_str(),
            )
        })
        .unwrap_or(("<unknown>", "<unknown>", "<unknown>"));
    let actor_detail = actor
        .as_ref()
        .map(|record| {
            format!(
                "actor_generation={} actor_state={} actor_pane={}",
                record.generation,
                record.state.as_str(),
                record.pane_id
            )
        })
        .unwrap_or_else(|| {
            "actor_generation=<unknown> actor_state=<unknown> actor_pane=<unknown>".to_string()
        });
    format!(
        "direct `agent-doc {}` invocation timed out after waiting {}s for {} to return after preflight. cycle_id={} phase={} last_event={} current_pane={} {}. The cycle is recoverable; inspect with `agent-doc session-check {}` or restart the managed owner with `agent-doc start {}`.",
        file.display(),
        agent_doc_agent_io::agent::run_agent_timeout().as_secs(),
        agent_name,
        cycle_id,
        phase,
        last_event,
        current_pane.as_deref().unwrap_or("<unknown>"),
        actor_detail,
        file.display(),
        file.display()
    )
}

fn actor_record_for_file(
    file: &Path,
) -> Result<Option<agent_doc_sqlite::state_store::ActorRecord>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(None);
    };
    let file_arg = canonical.to_string_lossy();
    agent_doc_session_actor_io::load_record_in(&project_root, &file_arg)
}

pub fn compute_run_diff(file: &Path) -> Result<Option<(String, bool)>> {
    if let Some(d) = agent_doc_diff_io::compute(
        &agent_doc_snapshot_io::DiffSnapshotStore::new(agent_doc_ops_log_io::log_op),
        file,
    )? {
        eprintln!("[run] diff computed ({} bytes)", d.len());
        return Ok(Some((d, false)));
    }

    if let Some(d) = active_queue_prompt_diff(file)? {
        eprintln!("[run] active queue head synthesized as prompt diff");
        return Ok(Some((d, true)));
    }

    Ok(None)
}

pub fn active_queue_prompt_diff(file: &Path) -> Result<Option<String>> {
    let ActiveQueuePromptState::Ready { prompt } = active_queue_prompt_state(file)? else {
        return Ok(None);
    };
    if let Some(command) = agent_doc_queue::queue_command::slash_command_text(&prompt) {
        eprintln!(
            "[run] active queue head is slash command {command:?}; leaving it for the managed supervisor to submit after the owner pane is idle"
        );
        return Ok(None);
    }
    Ok(Some(diff::synthetic_added_lines_diff(&prompt, "queue")))
}

pub fn active_queue_prompt_state(file: &Path) -> Result<ActiveQueuePromptState> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
    let (fm, _) = agent_doc_frontmatter_io::session::parse_for_file_with_context(
        &content,
        file,
        &rc.ssh_context(),
    )?;
    if fm.queue_active != Some(true) {
        return Ok(ActiveQueuePromptState::Inactive);
    }

    let components = element::parse(&content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(ActiveQueuePromptState::Inactive);
    };
    if !agent_doc_queue::control_binding::explicit_queue_go_mode(
        &queue_component.attrs,
        fm.queue.as_deref(),
    ) {
        return Ok(ActiveQueuePromptState::Inactive);
    }
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = agent_doc_queue::document_queue::parse(body)
        .context("run queue resume: failed to parse document queue")?;
    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&queue_component.attrs);
    let activation =
        agent_doc_queue::document_queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active {
        return Ok(ActiveQueuePromptState::Inactive);
    }
    let document_head = agent_doc_queue::document_queue::first_prompt(&activation.entries_after)
        .map(|prompt| strip_in_progress_marker(&prompt.text));
    if document_head.is_none() {
        return Ok(ActiveQueuePromptState::Empty);
    };

    if let Some(state) = typed_queue_prompt_state(file, &content) {
        return Ok(state);
    }

    eprintln!(
        "[run] active queue has no current typed selected/deferred head; refusing markdown fallback"
    );
    Ok(ActiveQueuePromptState::Unproven {
        reason: "missing_or_stale_typed_queue_head".to_string(),
        document_head,
    })
}

pub fn typed_queue_prompt_state(file: &Path, content: &str) -> Option<ActiveQueuePromptState> {
    let canonical = file.canonicalize().ok()?;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let document_hash = agent_doc_fs::document_state_hash(&canonical).ok()?;
    let ledger =
        agent_doc_controller_io::project_controller::load_state_event_ledger(&project_root).ok()?;
    let projection = ledger.project_document(&document_hash)?;
    let current_nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue").ok()?;
    let current_head = current_nodes.iter().find(|node| !node.item.struck)?;
    let head = projection.queue.heads.get(&current_head.node_key)?;
    match head.phase {
        agent_doc_state_backbone::QueueHeadPhase::Selected => {
            if projection.queue.active_head.as_deref() != Some(current_head.node_key.as_str()) {
                return None;
            }
            let prompt = head.prompt_text.clone()?;
            Some(ActiveQueuePromptState::Ready { prompt })
        }
        agent_doc_state_backbone::QueueHeadPhase::Deferred => {
            let reason = head.defer_reason.as_deref()?;
            if reason == "stop_fence" {
                eprintln!("[run] active queue halted by typed stop-fence state");
                return Some(ActiveQueuePromptState::StopFence {
                    next_prompt: head.prompt_text.clone(),
                });
            }
            if let Some(start_at) = reason.strip_prefix("time_gate:") {
                eprintln!("[run] active queue deferred by typed time-gate state: {start_at}");
                return Some(ActiveQueuePromptState::TimeGate {
                    start_at: start_at.to_string(),
                    next_prompt: head.prompt_text.clone(),
                });
            }
            if reason == "item_modified" {
                eprintln!("[run] active queue halted by typed item-modified state");
                return Some(ActiveQueuePromptState::ItemModified {
                    snapshot_head: None,
                    document_head: head.prompt_text.clone(),
                });
            }
            None
        }
        _ => None,
    }
}

pub fn should_continue_auto_queue(
    file: &Path,
    outcome: &RunCycleOutcome,
    completed_queue_items: usize,
    no_git: bool,
    last_context_clear_at: Option<u64>,
) -> Result<AutoQueueContinuation> {
    if no_git || !outcome.queue_synthetic_diff {
        return Ok(AutoQueueContinuation::Stop);
    }
    let Some(queue) = outcome.queue_consumption.as_ref() else {
        return Ok(AutoQueueContinuation::Stop);
    };
    // `auto` and `start` are start triggers only. Continuation is driven by
    // typed active queue state plus explicit `go` mode; a persisted
    // `queue_active: true` plain queue stays inert. The
    // `active_queue_prompt_state` re-check below still halts on typed stop fence
    // / time gate / head-modified / inactive / empty, and refuses markdown-only
    // fallback.
    if queue.drained || queue.remaining == 0 {
        return Ok(AutoQueueContinuation::Stop);
    }

    match active_queue_prompt_state(file)? {
        ActiveQueuePromptState::Ready { prompt } => {
            let force_fresh_agent_session =
                match agent_doc_session_accretion_io::queue_context_reset_reason_if_opted_in(
                    file,
                    last_context_clear_at,
                ) {
                    Ok(Some(reason)) => {
                        eprintln!(
                            "[queue] queue continuation will start a fresh agent session before next prompt: {}",
                            reason
                        );
                        true
                    }
                    Ok(None) => false,
                    Err(err) => {
                        eprintln!(
                            "[queue] warning: failed to inspect queue context reset policy for {}: {}",
                            file.display(),
                            err
                        );
                        false
                    }
                };
            eprintln!(
                "[queue] queue continuation: completed {} item(s); launching next prompt: {:?}",
                completed_queue_items, prompt
            );
            Ok(AutoQueueContinuation::Continue {
                force_fresh_agent_session,
            })
        }
        ActiveQueuePromptState::StopFence { next_prompt } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): stop_fence before next prompt {:?}",
                completed_queue_items, next_prompt
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::TimeGate {
            start_at,
            next_prompt,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): time_gate {} before next prompt {:?}",
                completed_queue_items, start_at, next_prompt
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::ItemModified {
            snapshot_head,
            document_head,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): item_modified snapshot_head={:?} document_head={:?}",
                completed_queue_items, snapshot_head, document_head
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::Unproven {
            reason,
            document_head,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): unproven_typed_queue_state reason={} document_head={:?}",
                completed_queue_items, reason, document_head
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::Inactive => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): queue_inactive",
                completed_queue_items
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::Empty => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): no_remaining_prompt",
                completed_queue_items
            );
            Ok(AutoQueueContinuation::Stop)
        }
    }
}

pub fn build_prompt(
    file: &Path,
    run_mode: RunMode,
    fm: &frontmatter::Frontmatter,
    the_diff: &str,
    content: &str,
    session_accretion: Option<&SessionAccretionReport>,
) -> String {
    let stable_prefix = build_prompt_stable_prefix(run_mode);
    let volatile_suffix =
        build_prompt_volatile_suffix(file, run_mode, fm, the_diff, content, session_accretion);
    PromptCacheBlocks::new(stable_prefix, volatile_suffix).render()
}

pub fn prompt_cache_routing_affinity(
    run_mode: RunMode,
    agent_name: &str,
    resolved_model: Option<&str>,
) -> String {
    format!(
        "agent_doc_run:v1;agent={agent_name};model={};mode={}",
        resolved_model.unwrap_or("<default>"),
        run_mode.cache_label()
    )
}

fn build_prompt_stable_prefix(run_mode: RunMode) -> String {
    let response_format = match run_mode {
        RunMode::Template => {
            "Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->"
        }
        RunMode::Append => {
            "Write your response in markdown.\n\
             Do not include a ## Assistant heading - it will be added automatically.\n\
             If the volatile payload contains inline prompt-bearing edits, classify them as prompt targets vs content edits before responding."
        }
    };
    format!(
        "<agent_doc_prompt_stable_prefix>\n\
         You are responding inside an agent-doc markdown session.\n\n\
         <response_contract>\n{}\n\
         </response_contract>\n\n\
         <turn_payload_contract>\n\
         Read the volatile turn payload after the cache boundary before acting. Queue heads, status advisories, compaction/accretion diagnostics, diffs, and document excerpts in that payload are current for this turn.\n\
         </turn_payload_contract>\n\
         </agent_doc_prompt_stable_prefix>",
        response_format
    )
}

fn build_prompt_volatile_suffix(
    file: &Path,
    run_mode: RunMode,
    fm: &frontmatter::Frontmatter,
    the_diff: &str,
    content: &str,
    session_accretion: Option<&SessionAccretionReport>,
) -> String {
    let prompt_bearing_changes = diff::format_prompt_bearing_changes(the_diff)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();
    let active_format_requirements =
        agent_doc_prompt_context::format_active_format_requirements(content)
            .map(|section| format!("\n\n{}\n", section))
            .unwrap_or_default();
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
    let ssh_context = rc.ssh_context();
    let document_section = agent_doc_prompt_context_io::build_document_section_with_ssh_context(
        file,
        the_diff,
        content,
        session_accretion,
        &ssh_context,
    );
    match (run_mode, fm.resume.is_some()) {
        (RunMode::Template, true) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content.\n\
             </agent_doc_prompt_volatile_suffix>",
            the_diff, prompt_bearing_changes, active_format_requirements, document_section
        ),
        (RunMode::Template, false) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user is starting a session document. Here is the full document:\n\n\
             {}\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content.\n\
             </agent_doc_prompt_volatile_suffix>",
            active_format_requirements, content
        ),
        (RunMode::Append, true) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content.\n\
             </agent_doc_prompt_volatile_suffix>",
            the_diff, prompt_bearing_changes, active_format_requirements, document_section
        ),
        (RunMode::Append, false) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user is starting a session document. Here is the full document:\n\n\
             {}\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. If the user asked questions or prompt-bearing edits inline (e.g., in blockquotes or prior responses), address those too.\n\
             </agent_doc_prompt_volatile_suffix>",
            active_format_requirements, content
        ),
    }
}
