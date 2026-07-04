//! Start command runtime I/O for agent-doc.

use agent_doc_controller::status::LaunchMode;
use agent_doc_frontmatter::frontmatter;
use agent_doc_session_registry_io::registration as sessions;
use agent_doc_supervisor::{
    lifecycle::start_session_retryable_during_recycle,
    session_owner::{
        ExistingPaneConflictFacts, ExistingSessionPaneAction,
        format_existing_pane_conflict_error as format_existing_pane_conflict_error_from_facts,
    },
};
use agent_doc_supervisor_process_io::{SupervisorLaunchLog, SupervisorStderrRedirect};
use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct StartRuntime {
    pub session_id: String,
    pub fm: frontmatter::Frontmatter,
    pub global_config: agent_doc_config::Config,
    pub canonical: PathBuf,
    pub project_root: PathBuf,
    pub session_log: Option<std::fs::File>,
    pub stderr_redirect: SupervisorStderrRedirect,
    pub harness: agent_doc_harness::HarnessConfig,
    pub pane_id: String,
    pub supervisor_instance_id: String,
    pub actor_record: agent_doc_sqlite::state_store::ActorRecord,
}

pub fn log_event(log: &mut Option<std::fs::File>, msg: &str) {
    if let Some(f) = log {
        let _ = writeln!(f, "[{}] {}", timestamp(), msg);
    }
}

pub fn start_console_status(
    session_log: &mut Option<std::fs::File>,
    route_owned: bool,
    message: impl AsRef<str>,
) {
    let message = message.as_ref();
    let printed = !route_owned || agent_doc_tmux_commands::input_diag::verbose_enabled();
    log_event(
        session_log,
        &format!(
            "start_console_status route_owned={} printed={} message={:?}",
            route_owned, printed, message
        ),
    );
    if printed {
        eprintln!("{message}");
    }
}

pub fn open_session_log(file: &Path, session_id: &str) -> Option<std::fs::File> {
    let path = agent_doc_supervisor_io::startup_miss::supervisor_session_log_path(file, session_id)
        .ok()
        .flatten()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    agent_doc_log_time::format_log_timestamp(now)
}

struct StartAdmissionLaunchLog<'a> {
    session_log: &'a mut Option<std::fs::File>,
    route_owned: bool,
}

impl SupervisorLaunchLog for StartAdmissionLaunchLog<'_> {
    fn log_event(&mut self, msg: &str) {
        log_event(self.session_log, msg);
    }

    fn start_console_status(&mut self, message: &str) {
        start_console_status(self.session_log, self.route_owned, message);
    }
}

pub fn prepare_start_runtime(file: &Path, force: bool, route_owned: bool) -> Result<StartRuntime> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let _ = agent_doc_run_io::repair_document_frontmatter_on_disk(file);
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    agent_doc_frontmatter_io::session::require_agent_doc_document(&content, file)?;
    let (updated_content, session_id) =
        agent_doc_frontmatter_io::session::ensure_session_for_file(&content, file)?;
    let generated_session_uuid = updated_content != content;
    if updated_content != content {
        std::fs::write(file, &updated_content)
            .with_context(|| format!("failed to write {}", file.display()))?;
    }

    let updated_content = replay_missing_operator_queue_items(file, updated_content);
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
    let (fm, _body) = agent_doc_frontmatter_io::session::parse_for_file_with_context(
        &updated_content,
        file,
        &rc.ssh_context(),
    )?;
    let global_config = agent_doc_config::load().unwrap_or_default();
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .ok()
                .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf())
        });
    let mut session_log = open_session_log(&canonical, &session_id);
    if generated_session_uuid {
        start_console_status(
            &mut session_log,
            route_owned,
            format!("Generated session UUID: {session_id}"),
        );
    }

    close_stale_start_actors(&project_root, &mut session_log, route_owned);

    let harness = agent_doc_harness::HarnessConfig::from_context(&fm, &global_config);
    let stderr_redirect = {
        let mut launch_log = StartAdmissionLaunchLog {
            session_log: &mut session_log,
            route_owned,
        };
        SupervisorStderrRedirect::maybe_start(&project_root, &harness, route_owned, &mut launch_log)
    };
    report_harness_resolution(&fm, &global_config, &harness, &mut session_log, route_owned);

    ensure_inside_tmux()?;
    let tmux = tmux_router::Tmux::default_server();
    let pane_id = agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux)
        .context("failed to query current tmux pane")?;

    if let Some(diagnostic) = agent_doc_run_io::recursive_codex_start_invocation_diagnostic(
        file,
        &session_id,
        &harness.binary,
    ) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "start_recursive_self_owned_pane_refused file={} pane={} session_id={}",
                file.display(),
                pane_id,
                session_id
            ),
        );
        anyhow::bail!("{}", diagnostic);
    }

    clear_superseded_startup_miss(file, &mut session_log, route_owned)?;
    let unresolved_startup_miss = agent_doc_supervisor_io::startup_miss::load_startup_miss(file)
        .ok()
        .flatten();

    if !force {
        if let Some(action) = existing_session_pane_action(&tmux, &session_id, file, &pane_id)? {
            match action {
                ExistingSessionPaneAction::Refuse(conflicting_pane) => {
                    if let Some(miss) = unresolved_startup_miss.as_ref()
                        && miss.pane_id == conflicting_pane
                    {
                        let miss_ts =
                            agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
                        anyhow::bail!(
                            "startup-miss from {} still belongs to alive pane {} for {}.\n\n{}",
                            miss_ts,
                            conflicting_pane,
                            file.display(),
                            format_existing_pane_conflict_error(
                                &tmux,
                                file,
                                &pane_id,
                                &conflicting_pane
                            )
                        );
                    }
                    anyhow::bail!(
                        "{}",
                        format_existing_pane_conflict_error(
                            &tmux,
                            file,
                            &pane_id,
                            &conflicting_pane
                        )
                    );
                }
            }
        }
    } else {
        start_console_status(
            &mut session_log,
            route_owned,
            format!(
                "[start] --force: bypassing existing session pane reuse for {}",
                file.display()
            ),
        );
    }

    if let Some(expected_session) = agent_doc_project_config_io::project_tmux_session()
        && !relocate_if_wrong_session(&tmux, &pane_id, &expected_session)
    {
        rebind_project_tmux_session_if_expected_dead(&tmux, &pane_id, &expected_session);
    }

    let supervisor_instance_id = uuid::Uuid::new_v4().to_string();
    let prior_entry = agent_doc_session_registry_io::lookup_entry(&session_id)?;
    let pane_window = agent_doc_tmux_io::target_window_id(&tmux, &pane_id).unwrap_or_default();
    let file_str = file.to_string_lossy();
    sessions::register_supervisor(
        &session_id,
        &pane_id,
        &file_str,
        std::process::id(),
        &supervisor_instance_id,
    )?;
    start_console_status(
        &mut session_log,
        route_owned,
        format!(
            "Registered session {} -> pane {}",
            &session_id[..8],
            pane_id
        ),
    );

    let start_generation = {
        let generations = agent_doc_session_actor_io::next_generation(&canonical, &session_id)
            .unwrap_or(agent_doc_supervisor::OwnershipGeneration {
                prior_generation: 0,
                new_generation: 1,
            });
        log_event(
            &mut session_log,
            &agent_doc_supervisor::format_transition_event(
                agent_doc_supervisor::OwnershipTransitionEvent {
                    caller: "start",
                    reason: "session_start",
                    prior_generation: generations.prior_generation,
                    new_generation: generations.new_generation,
                    old_pane: prior_entry.as_ref().map(|entry| entry.pane.as_str()),
                    new_pane: &pane_id,
                    old_window: prior_entry.as_ref().and_then(|entry| {
                        (!entry.window.is_empty()).then_some(entry.window.as_str())
                    }),
                    new_window: Some(pane_window.as_str()),
                },
            ),
        );
        generations.new_generation
    };
    log_event(
        &mut session_log,
        &format!(
            "session_start file={} pane={} session={} generation={}",
            file.display(),
            pane_id,
            &session_id[..8],
            start_generation
        ),
    );
    let actor_record = start_controller_session(StartControllerSessionInput {
        file,
        canonical: &canonical,
        project_root: &project_root,
        session_id: &session_id,
        pane_id: &pane_id,
        pane_window: &pane_window,
        start_generation,
        session_log: &mut session_log,
    })?;
    log_event(
        &mut session_log,
        &format!(
            "controller_session_start generation={} state={}",
            actor_record.generation,
            actor_record.state.as_str()
        ),
    );

    fire_session_start_hooks(file, &session_id, &fm, &global_config, &harness);

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "supervisor_host_gate file={} hosting=in-process",
            file.display()
        ),
    );
    log_event(&mut session_log, "supervisor_host_gate hosting=in-process");

    Ok(StartRuntime {
        session_id,
        fm,
        global_config,
        canonical,
        project_root,
        session_log,
        stderr_redirect,
        harness,
        pane_id,
        supervisor_instance_id,
        actor_record,
    })
}

fn replay_missing_operator_queue_items(file: &Path, updated_content: String) -> String {
    if let Err(err) = agent_doc_queue_io::queue_journal::record_live_buffer(file) {
        eprintln!(
            "[agent-doc] queue_journal: live-buffer record failed for {} ({err:#}) — continuing",
            file.display()
        );
    }
    let durable_content = agent_doc_snapshot_io::load(file).ok().flatten();
    let missing = agent_doc_queue_io::queue_journal::replay_missing(
        file,
        &updated_content,
        durable_content.as_deref(),
    );
    match agent_doc_queue::queue_journal::merge_missing_into_content(&missing, &updated_content) {
        Ok(Some(merged)) => {
            if let Err(err) = std::fs::write(file, &merged) {
                eprintln!(
                    "[agent-doc] queue_journal: failed to write replayed queue items to {} ({err:#})",
                    file.display()
                );
                updated_content
            } else {
                eprintln!(
                    "[agent-doc] queue_journal: replayed {} operator queue item(s) lost to a crash+restart for {}",
                    missing.len(),
                    file.display()
                );
                merged
            }
        }
        Ok(None) => updated_content,
        Err(err) => {
            eprintln!(
                "[agent-doc] queue_journal: replay merge failed for {} ({err:#}) — continuing without replay",
                file.display()
            );
            updated_content
        }
    }
}

fn close_stale_start_actors(
    project_root: &Path,
    session_log: &mut Option<std::fs::File>,
    route_owned: bool,
) {
    match agent_doc_controller_io::project_controller::close_stale_starting_actors_for_caller(
        project_root,
        Duration::from_secs(3600),
        false,
        "start",
    ) {
        Ok((closed, kept)) if closed > 0 => start_console_status(
            session_log,
            route_owned,
            format!("[start] actors: {closed} stale starting closed, {kept} still active"),
        ),
        Ok(_) => {}
        Err(e) => start_console_status(
            session_log,
            route_owned,
            format!("[start] actor gc warning: {e}"),
        ),
    }
    match agent_doc_controller_io::project_controller::close_stale_dead_pane_actors_with_tmux_for_caller(
        project_root,
        false,
        "start",
        "stale_dead_pane_actor",
    ) {
        Ok((closed, kept)) if closed > 0 => start_console_status(
            session_log,
            route_owned,
            format!("[start] actors: {closed} stale dead-pane closed, {kept} still active"),
        ),
        Ok(_) => {}
        Err(e) => start_console_status(
            session_log,
            route_owned,
            format!("[start] dead-pane actor gc warning: {e}"),
        ),
    }
}

fn report_harness_resolution(
    fm: &frontmatter::Frontmatter,
    global_config: &agent_doc_config::Config,
    harness: &agent_doc_harness::HarnessConfig,
    session_log: &mut Option<std::fs::File>,
    route_owned: bool,
) {
    let (source, _resolved_name) = if fm.agent.is_some() {
        ("frontmatter", fm.agent.as_deref().unwrap_or("?"))
    } else if global_config.default_agent.is_some() {
        (
            "config",
            global_config.default_agent.as_deref().unwrap_or("?"),
        )
    } else {
        ("fallback", "claude")
    };
    let env_harness = agent_doc_model_tier::detect_harness();
    start_console_status(
        session_log,
        route_owned,
        format!(
            "[start] harness resolved: binary={} source={} env={}",
            harness.binary, source, env_harness
        ),
    );
    if env_harness != "default" && env_harness != harness.binary {
        start_console_status(
            session_log,
            route_owned,
            format!(
                "[start] WARNING: harness mismatch - from_context resolved {} (via {}) but env detect_harness returned {}",
                harness.binary, source, env_harness
            ),
        );
    }
}

fn ensure_inside_tmux() -> Result<()> {
    if agent_doc_tmux_io::in_tmux() {
        return Ok(());
    }
    let tmux_installed = std::process::Command::new("which")
        .arg("tmux")
        .output()
        .is_ok_and(|o| o.status.success());
    if !tmux_installed {
        let hint = if cfg!(target_os = "macos") {
            "brew install tmux"
        } else if cfg!(target_os = "linux") {
            "sudo apt install tmux  # or: sudo pacman -S tmux / sudo dnf install tmux"
        } else {
            "Install WSL first, then: sudo apt install tmux"
        };
        anyhow::bail!(
            "tmux is not installed.\n\n  Install it:\n    {}\n\n  Then start a tmux session:\n    tmux new-session -s dev",
            hint
        );
    }
    anyhow::bail!(
        "not running inside tmux — start a tmux session first:\n    tmux new-session -s dev"
    )
}

fn clear_superseded_startup_miss(
    file: &Path,
    session_log: &mut Option<std::fs::File>,
    route_owned: bool,
) -> Result<()> {
    if let Some((miss, supersession)) =
        agent_doc_supervisor_io::startup_miss::take_superseded_startup_miss(
            agent_doc_supervisor_io::startup_miss::session_registry_lookup(),
            file,
        )?
    {
        let miss_ts = agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
        start_console_status(
            session_log,
            route_owned,
            format!(
                "[start] clearing stale startup-miss on pane {} from {} for {} because newer registered owner {} already took over",
                miss.pane_id,
                miss_ts,
                file.display(),
                supersession.registered_pane
            ),
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "start_startup_miss_cleared_superseded file={} stale_pane={} registered_pane={} miss_timestamp={} latest_start_timestamp={}",
                file.display(),
                miss.pane_id,
                supersession.registered_pane,
                miss_ts,
                supersession.latest_start_timestamp
            ),
        );
    }
    Ok(())
}

pub fn existing_session_pane_action(
    tmux: &tmux_router::Tmux,
    session_id: &str,
    file: &Path,
    current_pane: &str,
) -> Result<Option<ExistingSessionPaneAction>> {
    let entry = agent_doc_session_registry_io::lookup_entry(session_id)?;
    let live_owner = agent_doc_sync_io::sync::find_normal_path_owner_pane_excluding_quiet(
        tmux,
        file,
        session_id,
        Some(current_pane),
    );
    Ok(existing_session_pane_action_from_entry(
        tmux,
        current_pane,
        entry.as_ref(),
        live_owner.as_deref(),
    ))
}

pub fn existing_session_pane_action_from_entry(
    tmux: &tmux_router::Tmux,
    current_pane: &str,
    entry: Option<&tmux_router::RegistryEntry>,
    live_owner: Option<&str>,
) -> Option<ExistingSessionPaneAction> {
    let registry_pane = entry.map(|entry| entry.pane.as_str());
    let registry_pane_alive = registry_pane
        .map(|pane| tmux.pane_alive(pane))
        .unwrap_or(false);
    agent_doc_supervisor::session_owner::existing_session_pane_action(
        current_pane,
        registry_pane,
        registry_pane_alive,
        live_owner,
    )
}

pub fn format_existing_pane_conflict_error(
    tmux: &tmux_router::Tmux,
    file: &Path,
    current_pane: &str,
    conflicting_pane: &str,
) -> String {
    let conflict_session = tmux.pane_session(conflicting_pane).unwrap_or_default();
    let conflict_window =
        agent_doc_tmux_io::target_window_id(tmux, conflicting_pane).unwrap_or_default();
    let current_session = tmux.pane_session(current_pane).unwrap_or_default();
    let current_window =
        agent_doc_tmux_io::target_window_id(tmux, current_pane).unwrap_or_default();
    let document = file.display().to_string();
    format_existing_pane_conflict_error_from_facts(&ExistingPaneConflictFacts {
        document: &document,
        current_pane,
        conflicting_pane,
        conflict_session: &conflict_session,
        conflict_window: &conflict_window,
        current_session: &current_session,
        current_window: &current_window,
    })
}

pub fn relocate_if_wrong_session(
    tmux: &tmux_router::Tmux,
    pane_id: &str,
    expected_session: &str,
) -> bool {
    let actual_session = match tmux.pane_session(pane_id) {
        Ok(s) => s,
        Err(_) => return true,
    };
    if actual_session == expected_session {
        return true;
    }
    eprintln!(
        "[start] pane {} is in session '{}', expected '{}' — auto-relocating to project session",
        pane_id, actual_session, expected_session
    );
    if let Some(anchor) = tmux.active_pane(expected_session) {
        match tmux_router::PaneMoveOp::new(tmux, pane_id, &anchor)
            .allow_cross_session("auto-relocate to project session on start")
            .join("-dh")
        {
            Ok(()) => {
                eprintln!(
                    "[start] relocated pane {} → session '{}'",
                    pane_id, expected_session
                );
                true
            }
            Err(e) => {
                eprintln!(
                    "[start] WARNING: relocation failed ({}); pane {} will register in session '{}'",
                    e, pane_id, actual_session
                );
                false
            }
        }
    } else {
        eprintln!(
            "[start] WARNING: no active pane found in session '{}'; \
             pane {} will register in session '{}'",
            expected_session, pane_id, actual_session
        );
        false
    }
}

pub fn rebind_project_tmux_session_if_expected_dead(
    tmux: &tmux_router::Tmux,
    pane_id: &str,
    expected_session: &str,
) {
    let actual_session = match tmux.pane_session(pane_id) {
        Ok(session) => session,
        Err(_) => return,
    };
    if actual_session == expected_session || tmux.session_alive(expected_session) {
        return;
    }
    match agent_doc_project_config_io::update_project_tmux_session(&actual_session) {
        Ok(()) => eprintln!(
            "[start] configured project session '{}' is dead — rebound tmux_session to '{}'",
            expected_session, actual_session
        ),
        Err(e) => eprintln!(
            "[start] WARNING: configured project session '{}' is dead but failed to persist tmux_session '{}': {}",
            expected_session, actual_session, e
        ),
    }
}

struct StartControllerSessionInput<'a> {
    file: &'a Path,
    canonical: &'a Path,
    project_root: &'a Path,
    session_id: &'a str,
    pane_id: &'a str,
    pane_window: &'a str,
    start_generation: u64,
    session_log: &'a mut Option<std::fs::File>,
}

fn start_controller_session(
    input: StartControllerSessionInput<'_>,
) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
    let StartControllerSessionInput {
        file,
        canonical,
        project_root,
        session_id,
        pane_id,
        pane_window,
        start_generation,
        session_log,
    } = input;
    agent_doc_controller_io::project_controller::ensure_controller_running(
        project_root,
        LaunchMode::Lazy,
    )?;
    let start_request = agent_doc_controller_io::project_controller::StartSessionRequest {
        file: canonical.to_path_buf(),
        session_id: session_id.to_string(),
        pane_id: pane_id.to_string(),
        window_id: pane_window.to_string(),
        generation: start_generation,
    };
    let mut attempts_used = 0usize;
    const MAX_START_SESSION_RECYCLE_RETRIES: usize = 2;
    loop {
        match agent_doc_controller_io::project_controller::start_session(
            project_root,
            start_request.clone(),
        ) {
            Ok(record) => break Ok(record),
            Err(err) => {
                let recycle_status =
                    agent_doc_controller_io::project_controller::supervisor_recycle_status_for_file(
                        file,
                    )
                    .unwrap_or_default();
                let recycle_pending = matches!(
                    recycle_status.phase,
                    agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
                );
                if !start_session_retryable_during_recycle(
                    recycle_pending,
                    attempts_used,
                    MAX_START_SESSION_RECYCLE_RETRIES,
                ) {
                    break Err(err);
                }
                attempts_used += 1;
                let reason = recycle_status
                    .reason
                    .unwrap_or_else(|| "unknown".to_string());
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "start_session_recycle_retry file={} pane={} attempt={} reason={} err={}",
                        file.display(),
                        pane_id,
                        attempts_used,
                        reason,
                        err
                    ),
                );
                log_event(
                    session_log,
                    &format!("start_session_recycle_retry attempt={attempts_used} reason={reason}"),
                );
                let settled = agent_doc_controller_io::project_controller::
                    wait_for_supervisor_recycle_settle_for_file(file)
                        .map(|projection| {
                            matches!(
                                projection.phase,
                                agent_doc_state_backbone::SupervisorRecyclePhase::Settled
                            )
                        })
                        .unwrap_or(false);
                if !settled {
                    break Err(err);
                }
                agent_doc_controller_io::project_controller::ensure_controller_running(
                    project_root,
                    LaunchMode::Lazy,
                )?;
            }
        }
    }
}

fn fire_session_start_hooks(
    file: &Path,
    session_id: &str,
    fm: &frontmatter::Frontmatter,
    global_config: &agent_doc_config::Config,
    harness: &agent_doc_harness::HarnessConfig,
) {
    let harness_name = agent_doc_model_tier::harness_key_for_agent_name(&harness.binary);
    let resolved_model = fm.resolve_harness_model(&harness_name).map(|s| {
        agent_doc_model_tier::canonical_model_name(s, &harness_name, &global_config.model)
    });
    agent_doc_hooks_io::fire_doc_hooks(
        &fm.hooks,
        "session_start",
        file,
        session_id,
        &fm.agent,
        &resolved_model,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_console_status_suppresses_route_owned_stderr_by_default() {
        let mut log = tempfile::tempfile().unwrap();
        let mut cloned = Some(log.try_clone().unwrap());
        start_console_status(&mut cloned, true, "[start] harness resolved: binary=codex");
        drop(cloned);

        use std::io::{Read, Seek, SeekFrom};
        log.seek(SeekFrom::Start(0)).unwrap();
        let mut content = String::new();
        log.read_to_string(&mut content).unwrap();
        assert!(
            content.contains("start_console_status route_owned=true printed=false"),
            "{content}"
        );
        assert!(content.contains("[start] harness resolved: binary=codex"));
    }
}
