//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_start_io::{log_event, prepare_start_runtime, start_console_status};
use agent_doc_supervisor::{
    agent_change::harness_change_forces_fresh_spawn,
    lifecycle::{BootResumeAction, boot_resume_action},
    run_loop::{PostChildExitAction, child_launch_plan, post_child_exit_action},
};
use agent_doc_supervisor_process::{
    REEXEC_CHILD_PID_ENV, REEXEC_MASTER_FD_ENV, ReexecState,
    io_threads::{spawn_reader_thread, spawn_writer_thread},
    resize,
};
use agent_doc_supervisor_process_io::{
    HarnessLaunchSpec, SupervisorLaunchLog, build_harness_launch_spec,
};
#[cfg(test)]
use agent_doc_supervisor_process_io::{
    supervisor_stderr_redirect_needed, supervisor_stderr_redirect_path,
};

pub fn run(file: &Path, force: bool, route_owned: bool) -> Result<()> {
    run_with_reap_policy(file, force, route_owned, RouteOwnedReapPolicy::Auto)
}

struct StartRunLaunchLog<'a> {
    session_log: &'a mut Option<std::fs::File>,
    route_owned: bool,
}

impl SupervisorLaunchLog for StartRunLaunchLog<'_> {
    fn log_event(&mut self, msg: &str) {
        log_event(self.session_log, msg);
    }

    fn start_console_status(&mut self, message: &str) {
        start_console_status(self.session_log, self.route_owned, message);
    }
}

fn configure_managed_capability_proof_for_spec(
    shared: &Arc<SupervisorShared>,
    spec: &HarnessLaunchSpec,
    fm: &frontmatter::Frontmatter,
    global_config: &agent_doc_config::Config,
    session_log: &mut Option<std::fs::File>,
) -> Option<std::thread::JoinHandle<()>> {
    let proof_epoch = shared.next_capability_proof_epoch();
    if !spec.capability_proof_required {
        shared.set_capability_proof_gate(CapabilityProofGate::NotRequired, None);
        return None;
    }

    shared.set_capability_proof_gate(CapabilityProofGate::Pending, None);
    log_event(
        session_log,
        &format!("{}_capability_proof status=pending", spec.harness.binary),
    );
    Some(spawn_managed_capability_proof_thread(
        shared.clone(),
        ManagedCapabilityProofTask {
            proof_epoch,
            harness_binary: spec.harness.binary.clone(),
            args: spec.base_args.clone(),
            env: spec.resolved_env.clone(),
            frontmatter: fm.clone(),
            global_config: global_config.clone(),
            session_log: session_log.as_ref().and_then(|f| f.try_clone().ok()),
        },
    ))
}

pub fn run_with_reap_policy(
    file: &Path,
    force: bool,
    route_owned: bool,
    route_owned_reap_policy: RouteOwnedReapPolicy,
) -> Result<()> {
    let agent_doc_start_io::StartRuntime {
        session_id,
        fm,
        global_config,
        canonical,
        project_root,
        mut session_log,
        stderr_redirect,
        harness: _harness,
        pane_id,
        supervisor_instance_id,
        actor_record,
    } = prepare_start_runtime(file, force, route_owned)?;
    let _stderr_redirect = stderr_redirect;

    // --- Snapshot integrity validation ---
    // If file was moved (JB plugin respawn after rename), the old path hash
    // won't match — migrate state files or bootstrap a fresh snapshot before
    // the IPC listener starts. Prevents CRDT corruption from stale state.
    match agent_doc_workflow_io::document_init::ensure_initialized(
        file,
        crate::git::commit,
        agent_doc_ops_log_io::log_op,
    ) {
        Ok(true) => {
            log_event(&mut session_log, "snapshot_validated action=initialized");
            start_console_status(
                &mut session_log,
                route_owned,
                "[start] snapshot integrity validated (initialized)",
            );
        }
        Ok(false) => {
            log_event(&mut session_log, "snapshot_validated action=already_valid");
        }
        Err(e) => {
            log_event(
                &mut session_log,
                &format!("snapshot_validation_failed error={}", e),
            );
            start_console_status(
                &mut session_log,
                route_owned,
                format!("[start] warning: snapshot validation failed: {e}"),
            );
        }
    }

    // --- Supervisor setup ---

    // Resolve CWD deterministically
    let resolved_cwd = cwd::resolve(None, fm.cwd.as_deref(), &canonical)?;
    log_event(
        &mut session_log,
        &format!(
            "cwd_resolved path={} source={}",
            resolved_cwd.path.display(),
            resolved_cwd.source.as_str()
        ),
    );

    // `#agentreloadrestart` Phase 1b — assemble the harness launch spec from
    // current frontmatter. Built once here; re-built at the top of a restart
    // iteration to bring up a freshly-resolved harness on an `agent:` change.
    // `harness` was already resolved above for the early recursive-guard / shared
    // state; the spec re-resolves it (identical inputs ⇒ identical harness) and
    // also carries `base_args`/`resolved_env`/`capability_proof_required`.
    let initial_launch_spec = {
        let mut launch_log = StartRunLaunchLog {
            session_log: &mut session_log,
            route_owned,
        };
        build_harness_launch_spec(&fm, &global_config, &canonical, &mut launch_log)?
    };
    let mut harness = initial_launch_spec.harness.clone();
    let mut base_args = initial_launch_spec.base_args.clone();
    let mut resolved_env = initial_launch_spec.resolved_env.clone();
    let mut capability_proof_frontmatter = fm.clone();

    // Query initial terminal size
    let initial_size = {
        #[cfg(unix)]
        {
            resize::query_terminal_size(libc::STDIN_FILENO)
                .map(|(rows, cols)| PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap_or(PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                })
        }
        #[cfg(not(unix))]
        {
            PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            }
        }
    };

    let actor_runtime = SessionActorRuntime {
        project_root: project_root.clone(),
        file: canonical.clone(),
        session_id: session_id.clone(),
        pane_id: pane_id.clone(),
        generation: actor_record.generation,
    };

    // Create shared state for IPC handler
    let shared = Arc::new(SupervisorShared::with_actor_runtime(
        resolved_cwd.source.as_str(),
        supervisor_instance_id,
        &harness.binary,
        Some(actor_runtime),
        Some(agent_doc_sqlite::state_store::ActorState::Starting),
        Some(pane_id.clone()),
    ));
    let mut capability_proof_thread = configure_managed_capability_proof_for_spec(
        &shared,
        &initial_launch_spec,
        &capability_proof_frontmatter,
        &global_config,
        &mut session_log,
    );

    // Start IPC listener
    let shared_for_ipc = shared.clone();
    let mut ipc = SupervisorIpc::start(&project_root, &session_id, move |method| {
        agent_doc_supervisor_io::ipc::handle_supervisor_ipc(method, shared_for_ipc.as_ref())
    })?;
    log_event(
        &mut session_log,
        &format!("ipc_started project_root={}", project_root.display()),
    );
    let supervisor_socket = agent_doc_supervisor_io::ipc::socket_path(&project_root, &session_id)
        .to_string_lossy()
        .to_string();
    agent_doc_controller_io::project_controller::register_supervisor(
        &project_root,
        agent_doc_controller_io::project_controller::SupervisorRegistration {
            file: canonical.clone(),
            session_id: session_id.clone(),
            pane_id: pane_id.clone(),
            generation: actor_record.generation,
            supervisor_pid: std::process::id(),
            supervisor_socket,
            runtime_state: agent_doc_sqlite::state_store::ActorState::Starting
                .as_str()
                .to_string(),
        },
    )?;
    log_event(
        &mut session_log,
        "controller_supervisor_registered state=starting",
    );

    // Crash policy state machine
    let mut policy = CrashPolicy::new();
    let route_owned_cycle_baseline = if route_owned {
        agent_doc_cycle_state_io::load(file).unwrap_or(None)
    } else {
        None
    };
    let route_owned_completion = Arc::new(AtomicBool::new(false));
    let route_owned_completion_stop = Arc::new(AtomicBool::new(false));
    let route_owned_completion_thread = if route_owned {
        log_event(&mut session_log, "route_owned_start enabled=true");
        Some(spawn_route_owned_completion_thread(
            shared.clone(),
            RouteOwnedCompletionConfig::new(
                canonical.clone(),
                route_owned_cycle_baseline,
                route_owned_reap_policy,
                harness.clone(),
            ),
            route_owned_completion.clone(),
            route_owned_completion_stop.clone(),
            session_log.as_ref().and_then(|f| f.try_clone().ok()),
            log_event,
        ))
    } else {
        None
    };

    // Put stdin into raw mode so the outer pty's line discipline doesn't
    // mangle input bytes (e.g. ICRNL converting \r→\n). Claude Code sets
    // the inner pty slave to raw mode and expects \r for Enter — without
    // this, the outer pty's cooked mode silently converts \r to \n before
    // we even read it, breaking Enter for Claude Code's TUI.
    let raw_mode = RawMode::enable();

    // --- Supervisor restart loop ---
    // `#ctlrecycle` R3 — if this process was launched by a stale supervisor's
    // self-`execve`, adopt the preserved harness child on the first iteration instead
    // of spawning a new one. Consume the handoff env immediately so a later in-process
    // (continue) restart never re-adopts a now-dead fd.
    let mut pending_adopt = ReexecState::from_env();
    if pending_adopt.is_some() {
        unsafe {
            std::env::remove_var(REEXEC_CHILD_PID_ENV);
            std::env::remove_var(REEXEC_MASTER_FD_ENV);
        }
        log_event(&mut session_log, "supervisor_reexec_reentry detected");
    }
    let mut first_run = true;
    let mut auto_trigger_next_launch = false;

    // `#midturn-recycle-resume` Phase B — actively resume a turn that was genuinely
    // INTERRUPTED across the recycle. Phase A makes a mid-cycle `execve` impossible
    // in the steady state, but a child can still die across the recycle window (it
    // crashed/was killed, or the escalation forced the recycle over a never-closing
    // wedged cycle). In that case the surviving-child resume never happens, so the
    // fresh image must re-dispatch the interrupted turn from the `#durablerecycle`
    // checkpoint keyed off `queue_task_id` / `prompt_targets`. When the child DID
    // survive (the common case) we do NOT re-dispatch — the adopted child is still
    // running the turn (idempotency). A committed/abandoned (closed) checkpoint, an
    // already-consumed checkpoint, or a non-recycle boot all resume nothing.
    {
        let is_recycle_boot = pending_adopt.is_some();
        let cycle_open = agent_doc_cycle_state_io::load(file)
            .ok()
            .flatten()
            .map(|state| state.is_open())
            .unwrap_or(false);
        let already_consumed = agent_doc_cycle_state_io::load(file)
            .ok()
            .flatten()
            .map(|state| state.recycle_resume_consumed)
            .unwrap_or(false);
        let child_survived = pending_adopt
            .map(|state| state.child_survived())
            .unwrap_or(false);
        let resume_action = boot_resume_action(
            is_recycle_boot,
            cycle_open,
            child_survived,
            already_consumed,
        );
        match resume_action {
            BootResumeAction::RedispatchInterruptedTurn => {
                // The harness child died across the recycle — the interrupted turn
                // has no surviving owner. Spawn a FRESH child (drop the dead-pid
                // adopt) and re-trigger the same turn on the first iteration. Mark the
                // checkpoint consumed so a second boot reading the same still-open
                // checkpoint cannot re-dispatch the turn again (idempotency).
                let checkpoint = agent_doc_cycle_state_io::load(file).ok().flatten();
                let target = checkpoint
                    .as_ref()
                    .and_then(|s| {
                        s.queue_task_id
                            .as_deref()
                            .or_else(|| s.prompt_targets.first().map(String::as_str))
                    })
                    .unwrap_or("<none>")
                    .to_string();
                let cycle_id = checkpoint
                    .as_ref()
                    .map(|s| s.cycle_id.clone())
                    .unwrap_or_default();
                pending_adopt = None;
                auto_trigger_next_launch = true;
                if let Err(err) = agent_doc_cycle_state_io::mark_recycle_resume_consumed(file) {
                    eprintln!(
                        "[agent-doc] warning: failed to mark #durablerecycle checkpoint consumed for {} ({err:#}) — continuing; the re-dispatch may repeat on a second boot",
                        file.display()
                    );
                }
                log_event(
                    &mut session_log,
                    &format!(
                        "supervisor_recycle_resume_redispatch cycle={cycle_id} target={target} reason=harness_child_died_across_recycle"
                    ),
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "supervisor_recycle_resume_redispatch file={} cycle={cycle_id} target={target} reason=harness_child_died_across_recycle action=spawn_fresh_and_retrigger (#midturn-recycle-resume)",
                        file.display(),
                    ),
                );
            }
            BootResumeAction::AdoptSurvivingChild => {
                // The common Phase-A steady state: the adopted child is still running
                // the interrupted turn. Adopt it as-is (the existing `#ctlrecycle`
                // path below) without re-triggering — re-dispatching would double-run
                // the turn.
                log_event(
                    &mut session_log,
                    "supervisor_recycle_resume_adopt_surviving_child reason=child_alive_owns_resume",
                );
            }
            BootResumeAction::None => {}
        }
    }
    let mut restart_count: u32 = 0;
    let mut resize_watcher: Option<resize::ResizeWatcher> = None;
    let mut failed_resume_tracker = FailedResumeTracker::default();
    let mut suppress_stale_ctrl_d_until_prompt = false;
    let mut child_launch_count: u32 = 0;
    let _actor_context = agent_doc_run_context_io::ActorContext::new(canonical.clone());
    let supervisor_exit_reason = loop {
        if child_launch_count > 0 {
            let restart_reason = if first_run {
                "restart_fresh_spawn"
            } else {
                "restart_continue_spawn"
            };
            shared.transition_actor_state(
                agent_doc_sqlite::state_store::ActorState::Busy,
                "supervisor",
                restart_reason,
            );

            // `#agentreloadrestart` Phase 1b — this iteration is serving a restart.
            // Re-read CURRENT frontmatter and re-resolve the harness launch spec. If
            // the operator changed `agent:` (e.g. claude→opencode) the resolved
            // binary now DIFFERS from the running one: swap in the new spec and force
            // a FRESH spawn (a harness change must never adopt the old child).
            //
            // INERT for an unchanged `agent:`: the re-resolved binary matches, so we
            // skip the swap entirely and the same-harness restart path is byte-for-
            // byte unchanged (no spec swap, `pending_adopt` untouched, no marker).
            let restart_fm = std::fs::read_to_string(file)
                .ok()
                .and_then(|content| frontmatter::parse(&content).ok().map(|(fm, _)| fm));
            match restart_fm {
                Some(restart_fm) => {
                    let mut launch_log = StartRunLaunchLog {
                        session_log: &mut session_log,
                        route_owned,
                    };
                    match build_harness_launch_spec(
                        &restart_fm,
                        &global_config,
                        &canonical,
                        &mut launch_log,
                    ) {
                        Ok(restart_spec)
                            if harness_change_forces_fresh_spawn(
                                &harness.binary,
                                &restart_spec.harness.binary,
                            ) =>
                        {
                            let old_harness = harness.binary.clone();
                            let new_harness = restart_spec.harness.binary.clone();
                            harness = restart_spec.harness.clone();
                            base_args = restart_spec.base_args.clone();
                            resolved_env = restart_spec.resolved_env.clone();
                            capability_proof_frontmatter = restart_fm.clone();
                            // Retire the old harness proof before the restart marker lands in
                            // the session log, so stale proof events cannot satisfy post-restart
                            // route checks.
                            let _ = shared.next_capability_proof_epoch();
                            shared.set_capability_proof_gate(
                                if restart_spec.capability_proof_required {
                                    CapabilityProofGate::Pending
                                } else {
                                    CapabilityProofGate::NotRequired
                                },
                                None,
                            );
                            // A harness change must spawn the NEW harness fresh — never
                            // adopt the OLD harness child preserved across a reexec.
                            pending_adopt = None;
                            agent_doc_ops_log_io::log_op(
                                file,
                                &format!(
                                    "agent_restart_performed file={} old_harness={} new_harness={} action=spawn_fresh_harness",
                                    file.display(),
                                    old_harness,
                                    new_harness
                                ),
                            );
                            log_event(
                                &mut session_log,
                                &format!(
                                    "agent_restart_performed old_harness={} new_harness={} action=spawn_fresh_harness",
                                    old_harness, new_harness
                                ),
                            );
                            if let Some(handle) = capability_proof_thread.take()
                                && handle.is_finished()
                            {
                                let _ = handle.join();
                            }
                            capability_proof_thread = configure_managed_capability_proof_for_spec(
                                &shared,
                                &restart_spec,
                                &capability_proof_frontmatter,
                                &global_config,
                                &mut session_log,
                            );
                        }
                        // Unchanged harness (the common case) — INERT, no swap.
                        Ok(_) => {}
                        Err(e) => {
                            log_event(
                                &mut session_log,
                                &format!(
                                    "agent_restart_respec_failed error={} note=keeping_running_harness",
                                    e
                                ),
                            );
                        }
                    }
                }
                None => {
                    log_event(
                        &mut session_log,
                        "agent_restart_respec_skipped reason=frontmatter_unreadable note=keeping_running_harness",
                    );
                }
            }
        }
        // Build args for this iteration. A restart can be "fresh" (base args,
        // no resume/continue flags) while still needing the document trigger
        // re-submitted after the new child prompt appears.
        let launch_plan = child_launch_plan(first_run, auto_trigger_next_launch);
        auto_trigger_next_launch = false;
        let auto_trigger = launch_plan.auto_trigger;
        let args = if launch_plan.use_continue_args {
            let restart_args = harness.restart_args(&base_args)?;
            start_console_status(
                &mut session_log,
                route_owned,
                format!("Restarting {} (continue)...", harness.binary),
            );
            log_event(
                &mut session_log,
                &format!(
                    "{}_restart mode=continue restart_count={}",
                    harness.binary, restart_count
                ),
            );
            restart_args
        } else {
            let args = base_args.clone();
            start_console_status(
                &mut session_log,
                route_owned,
                format!("Starting {}...", harness.binary),
            );
            log_event(
                &mut session_log,
                &format!(
                    "{}_start mode={} restart_count={}",
                    harness.binary,
                    if restart_count == 0 {
                        "fresh"
                    } else {
                        "fresh_restart"
                    },
                    restart_count
                ),
            );
            args
        };

        // Build PtySpawnConfig and spawn child under pty
        let cfg = PtySpawnConfig {
            program: harness.binary.clone(),
            args,
            cwd: resolved_cwd.path.clone(),
            env: resolved_env.clone(),
            size: initial_size,
        };
        child_launch_count += 1;
        let mut session = if let Some(state) = pending_adopt.take() {
            // `#ctlrecycle` R3 re-entry: adopt the harness child preserved across the
            // supervisor's self-`execve` rather than spawning a fresh one. `first_run`
            // is still true here, so `auto_trigger` is false — the adopted child is
            // mid-session and must not be re-triggered.
            #[cfg(unix)]
            {
                log_event(
                    &mut session_log,
                    &format!(
                        "supervisor_reexec_adopted_child pid={} master_fd={}",
                        state.child_pid, state.master_fd
                    ),
                );
                agent_doc_supervisor_process::pty::PtySession::adopt(
                    state.master_fd,
                    state.child_pid,
                )
                .with_context(|| "failed to adopt harness child across supervisor reexec")?
            }

            #[cfg(not(unix))]
            {
                let _ = state;
                log_event(
                    &mut session_log,
                    "supervisor_reexec_adopt_skipped reason=unsupported_platform",
                );
                agent_doc_supervisor_process::pty::PtySession::spawn(cfg)
                    .with_context(|| format!("failed to spawn {}", harness.binary))?
            }
        } else {
            agent_doc_supervisor_process::pty::PtySession::spawn(cfg)
                .with_context(|| format!("failed to spawn {}", harness.binary))?
        };

        // Extract writer and reader for shared I/O
        #[cfg(unix)]
        let pty_write_fd = session.dup_write_fd()?;
        let pty_writer = session.take_writer()?;
        let pty_reader = session.clone_reader()?;
        #[cfg(unix)]
        let writer_arc = Arc::new(Mutex::new(SharedPtyWriter::with_raw_fd(
            pty_writer,
            pty_write_fd,
        )));
        #[cfg(not(unix))]
        let writer_arc = Arc::new(Mutex::new(SharedPtyWriter::new(pty_writer)));

        // Update shared state
        *shared.inject_writer.lock().unwrap() = Some(writer_arc.clone());
        shared
            .child_pid
            .store(session.process_id().unwrap_or(0), Ordering::Relaxed);
        // `#ctlrecycle` R3 — publish a dedicated dup of the live master fd so the
        // idle-watch thread can hand it to a self-`execve`. Close the previous
        // generation's dup so restarts do not leak fds.
        #[cfg(unix)]
        {
            let master_dup = session.dup_write_fd().unwrap_or(-1);
            let prev = shared.master_fd.swap(master_dup, Ordering::Relaxed);
            if prev >= 0 {
                unsafe { libc::close(prev) };
            }
        }
        shared.running.store(true, Ordering::Relaxed);
        shared.restart_count.store(restart_count, Ordering::Relaxed);
        shared.restart_requested.store(false, Ordering::Relaxed);
        shared.restart_reexec.store(false, Ordering::Relaxed);
        shared.stop_requested.store(false, Ordering::Relaxed);
        shared.stop_agent_requested.store(false, Ordering::Relaxed);
        shared.ctrl_d_forwarded.store(false, Ordering::Relaxed);
        shared.ctrl_c_forwarded.store(false, Ordering::Relaxed);
        shared
            .auto_trigger_outcome
            .store(AutoTriggerOutcome::NotNeeded as u8, Ordering::Relaxed);
        shared.prompt_visible_once.store(false, Ordering::Relaxed);
        shared
            .suppress_stale_ctrl_d_until_prompt
            .store(suppress_stale_ctrl_d_until_prompt, Ordering::Relaxed);
        shared.output.clear_recent_output();
        reset_terminal_screen(&shared, initial_size);

        // Spawn I/O forwarding threads
        let process_io_observer = Arc::new(
            agent_doc_supervisor_process_io::SupervisorProcessIoObserver::new(shared.clone()),
        );
        let reader_thread =
            spawn_reader_thread(process_io_observer.clone(), harness.clone(), pty_reader);
        let writer_stop = StopSignal::new().context("failed to create writer stop signal")?;
        let writer_stop_flag = Arc::new(AtomicBool::new(false));
        let ctrl_c_flag = Arc::new(AtomicBool::new(false));
        let ctrl_d_flag = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let writer_thread = spawn_writer_thread(
            process_io_observer.clone(),
            harness.clone(),
            writer_arc.clone(),
            writer_stop.read_fd(),
            writer_stop_flag.clone(),
            Some(ctrl_c_flag.clone()),
            Some(ctrl_d_flag.clone()),
        );
        #[cfg(not(unix))]
        let writer_thread = spawn_writer_thread(
            process_io_observer.clone(),
            harness.clone(),
            writer_arc.clone(),
            (),
            writer_stop_flag.clone(),
            Some(ctrl_c_flag.clone()),
            Some(ctrl_d_flag.clone()),
        );

        // Start resize watcher (stop previous one first)
        if let Some(mut rw) = resize_watcher.take() {
            rw.stop();
        }
        let resize_handle = session.resize_handle()?;
        let resize_shared = shared.clone();
        resize_watcher = resize::ResizeWatcher::spawn(move |size| {
            resize_shared.output.resize_terminal_screen(size);
            if let Err(e) = resize_handle.resize(size) {
                eprintln!("[supervisor::resize] resize error: {e}");
            }
        })
        .ok();

        // For restarts, poll for agent prompt then re-send trigger command
        let mut auto_trigger_thread: Option<(Arc<AtomicBool>, std::thread::JoinHandle<()>)> = None;
        if auto_trigger {
            shared
                .auto_trigger_outcome
                .store(AutoTriggerOutcome::Pending as u8, Ordering::Relaxed);
            let trigger_stop = Arc::new(AtomicBool::new(false));
            let trigger_log = session_log.as_ref().and_then(|f| f.try_clone().ok());
            let handle = spawn_auto_trigger_thread(
                shared.clone(),
                trigger_stop.clone(),
                file.to_string_lossy().to_string(),
                harness.clone(),
                trigger_log,
            );
            auto_trigger_thread = Some((trigger_stop, handle));
        }

        // Idle-queue watch (#jb-run-agent-doc-busy-queue-dispatch-deadlock):
        // a long-lived sibling of the one-shot auto-trigger that drains a live
        // go-mode `agent:queue` head on each busy→idle transition, so the queued
        // prompt is never stranded waiting for a harness-delegated drain that
        // never comes.
        let idle_watch_stop = Arc::new(AtomicBool::new(false));
        let idle_watch_thread = {
            let watch_log = session_log.as_ref().and_then(|f| f.try_clone().ok());
            idle_watch::spawn_idle_queue_watch_thread(
                shared.clone(),
                idle_watch_stop.clone(),
                file.to_string_lossy().to_string(),
                harness.clone(),
                watch_log,
            )
        };

        // Block until child exits.
        //
        // 08b end state: hand the child to the in-process supervisor adapter
        // (`PtySession::take_child`) and drive its tick loop for non-blocking exit
        // reaping + heartbeat + crash policy. `start.rs` keeps the
        // reader/writer/resize/auto-trigger/idle-watch plumbing (set up above) and
        // the Unix-socket IPC boundary; only *who reaps the child* lives in the
        // adapter. The outer restart loop still owns respawn (it rebuilds the I/O
        // plumbing per generation), so the adapter's factory refuses an in-adapter
        // respawn. The old out-of-process `session.wait()` host path was removed
        // at the removal rung.
        let status = {
            let child_pid = session.process_id();
            let pty_child = PtySupervisedChild::monitor(
                session
                    .take_child()
                    .context("take child for in-process supervisor hosting")?,
                child_pid,
            );
            let mut sup = InProcessSupervisor::adopt(Box::new(pty_child));
            log_event(
                &mut session_log,
                &format!(
                    "supervisor_host_inprocess_attach pane={} harness={} pid={} generation={}",
                    pane_id,
                    harness.binary,
                    child_pid.unwrap_or(0),
                    sup.generation()
                ),
            );
            let poll = Duration::from_millis(40);
            // Honor an external stop/restart/route-complete request by killing the
            // child once, so the next tick observes its exit (the out-of-process
            // host path is killed via the IPC handler's `libc::kill` by PID).
            let mut kill_requested = false;
            let exit_code = loop {
                // `#supkill-bg` — a stale restart routed to the idle-watch in-place
                // reexec (`restart_reexec`) must NOT have its child killed here: the
                // reexec preserves the live child across `execve`, so the host loop
                // defers to the idle watch and only kills for stop / fresh-binary
                // restart / route-complete.
                if !kill_requested
                    && (shared.stop_requested.load(Ordering::Relaxed)
                        || shared.stop_agent_requested.load(Ordering::Relaxed)
                        || (shared.restart_requested.load(Ordering::Relaxed)
                            && !shared.restart_reexec.load(Ordering::Relaxed))
                        || route_owned_completion.load(Ordering::Relaxed))
                {
                    kill_requested = true;
                    if let Err(e) = sup.kill_child() {
                        eprintln!(
                            "[supervisor::in_process] kill on stop/restart request failed: {e}"
                        );
                    }
                }
                match sup.tick() {
                    TickOutcome::Running => std::thread::sleep(poll),
                    TickOutcome::PromptOperator { exit_code }
                    | TickOutcome::Halted { exit_code }
                    | TickOutcome::RestartFailed { exit_code, .. }
                    | TickOutcome::Restarted { exit_code, .. } => break exit_code,
                    TickOutcome::Stopped => break 0,
                }
            };
            log_event(
                &mut session_log,
                &format!(
                    "supervisor_host_inprocess_exit pane={} harness={} exit_code={} heartbeat={}",
                    pane_id,
                    harness.binary,
                    exit_code,
                    sup.heartbeat()
                ),
            );
            portable_pty::ExitStatus::with_exit_code(exit_code as u32)
        };
        first_run = false;

        if let Some((stop, _)) = auto_trigger_thread.as_ref() {
            stop.store(true, Ordering::Relaxed);
        }
        idle_watch_stop.store(true, Ordering::Relaxed);
        writer_stop_flag.store(true, Ordering::Relaxed);

        // Stop the stdin→pty writer thread so stdin is free for the restart
        // prompt (or for the next iteration's fresh writer thread).
        writer_stop.signal();
        let _ = writer_thread.join();
        if let Some((_, handle)) = auto_trigger_thread.take() {
            let _ = handle.join();
        }
        let _ = idle_watch_thread.join();
        if ctrl_d_flag.load(Ordering::Relaxed) {
            shared.ctrl_d_forwarded.store(true, Ordering::Relaxed);
        }
        if ctrl_c_flag.load(Ordering::Relaxed) {
            shared.ctrl_c_forwarded.store(true, Ordering::Relaxed);
        }

        // Clean up shared state (must happen before dropping session so the
        // inject_writer Arc is released before the pty master closes).
        shared.running.store(false, Ordering::Relaxed);
        *shared.inject_writer.lock().unwrap() = None;
        shared.child_pid.store(0, Ordering::Relaxed);

        // Drop the session to close the pty master. The reader thread holds a
        // cloned reader fd — closing the master causes its read() to return
        // EOF so the thread can exit cleanly.
        drop(session);
        let _ = reader_thread.join();

        // Flush any stale stdin bytes that the writer thread consumed from the
        // kernel but couldn't forward (e.g., user pressed Enter during the
        // tiny race window between session.wait() and writer_stop.signal()).
        #[cfg(unix)]
        unsafe {
            libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
        }

        // Post-child-exit flag dispatch. The priority order (route-owned →
        // stop → stop-agent → restart → normal classification) is modeled by the
        // pure `post_child_exit_action` so the "Stop Agent" keepalive contract is
        // unit-testable without a live PTY.
        match post_child_exit_action(
            route_owned_completion.load(Ordering::Relaxed),
            shared.stop_requested.load(Ordering::Relaxed),
            shared.stop_agent_requested.load(Ordering::Relaxed),
            shared.restart_requested.load(Ordering::Relaxed),
        ) {
            PostChildExitAction::RouteOwnedComplete => {
                log_event(&mut session_log, "route_owned_cycle_complete_stop");
                break "route_owned_cycle_complete";
            }
            PostChildExitAction::ExitSupervisor => {
                log_event(&mut session_log, "ipc_stop");
                break "ipc_stop";
            }
            PostChildExitAction::StopAgentKeepalive => {
                // "Stop Agent": the harness child was killed, but the supervisor must
                // STAY alive at the restart-or-quit keepalive prompt — never exit
                // (unlike `stop_requested`) and never auto-restart (unlike
                // `restart_requested` or the normal clean-exit classification, which
                // would auto-restart codex/opencode whose `clean_exit_behavior` is
                // RestartContinue). The operator presses Enter to restart manually.
                //
                // Clear the flag so a later natural child exit re-enters normal handling.
                shared.stop_agent_requested.store(false, Ordering::Relaxed);
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "stop_agent_performed file={} action=kill_child_keep_supervisor",
                        file.display()
                    ),
                );
                log_event(
                    &mut session_log,
                    &format!("ipc_stop_agent restart_count={}", restart_count),
                );
                shared.transition_actor_state(
                    agent_doc_sqlite::state_store::ActorState::WaitingInput,
                    "supervisor",
                    "stop_agent_prompt",
                );
                raw_mode.suspend();
                eprintln!("\nAgent stopped. Supervisor is still running.");
                match prompt_for_restart_or_quit(
                    &mut session_log,
                    "stop_agent",
                    "Press Enter to restart the agent, or 'q' to exit.",
                    "user_quit_after_stop_agent",
                    PromptEofPolicy::Quit,
                ) {
                    PromptOutcome::Quit => {
                        break "user_quit_after_stop_agent";
                    }
                    PromptOutcome::RestartFresh => {
                        raw_mode.resume();
                        first_run = true;
                        auto_trigger_next_launch = auto_trigger;
                        restart_count += 1;
                        suppress_stale_ctrl_d_until_prompt = false;
                        continue;
                    }
                }
            }
            PostChildExitAction::AutoRestart => {
                let mode = shared.restart_mode.lock().unwrap().clone();
                first_run = mode == "fresh";
                auto_trigger_next_launch = true;
                restart_count += 1;
                log_event(
                    &mut session_log,
                    &format!("ipc_restart mode={} restart_count={}", mode, restart_count),
                );
                continue;
            }
            PostChildExitAction::NormalExitClassification => {}
        }

        // Normal exit classification via CrashPolicy
        let code = status.exit_code() as i32;
        let exit_provenance = exit_provenance_fields(&status);
        log_event(
            &mut session_log,
            &format!(
                "{}_exit code={} restart_count={} {}",
                harness.binary, code, restart_count, exit_provenance
            ),
        );
        let auto_trigger_outcome =
            AutoTriggerOutcome::from_u8(shared.auto_trigger_outcome.load(Ordering::Relaxed));
        let prompt_visible_once = shared.prompt_visible_once.load(Ordering::Relaxed);

        let ctrl_c_forwarded_interrupt = is_forwarded_ctrl_c_interrupt_exit(
            &status,
            shared.ctrl_c_forwarded.load(Ordering::Relaxed),
        );
        let ctrl_d_forwarded = shared.ctrl_d_forwarded.load(Ordering::Relaxed);
        let failed_resume = supervisor_resume_handoff_failed(
            auto_trigger,
            ctrl_d_forwarded,
            matches!(
                auto_trigger_outcome,
                AutoTriggerOutcome::Pending
                    | AutoTriggerOutcome::Timeout
                    | AutoTriggerOutcome::SendFailed
                    | AutoTriggerOutcome::Cancelled
            ),
        );
        let clean_exit_before_prompt =
            supervisor_clean_exit_before_prompt_seen(auto_trigger, prompt_visible_once);
        if matches!(
            auto_trigger_outcome,
            AutoTriggerOutcome::Sent | AutoTriggerOutcome::NotNeeded
        ) {
            failed_resume_tracker.reset();
        }

        // Forwarded operator Ctrl+C is an intentional shutdown request, not a
        // supervisor crash signal, so keep the policy state on the clean-exit
        // path and surface the same restart/quit prompt as Ctrl+D.
        let policy_exit_code = supervisor_policy_exit_code(code, ctrl_c_forwarded_interrupt);
        let action = policy.on_exit(policy_exit_code);
        *shared.supervisor_state.lock().unwrap() = policy.state;
        let action_name = match &action {
            RestartAction::PromptUser => "prompt_user",
            RestartAction::RestartAfter { .. } => "restart_after",
            RestartAction::Halt => "halt",
        };
        log_event(
            &mut session_log,
            &format!(
                "restart_eval pane={} harness={} exit_code={} {} auto_trigger_outcome={} ctrl_d={} state={} action={}",
                pane_id,
                harness.binary,
                code,
                exit_provenance,
                auto_trigger_outcome.as_str(),
                ctrl_d_forwarded,
                policy.state.as_str(),
                action_name
            ),
        );

        match action {
            RestartAction::PromptUser => {
                match supervisor_clean_exit_resolution(
                    matches!(
                        harness.clean_exit_behavior,
                        agent_doc_harness::CleanExitBehavior::RestartContinue
                    ),
                    route_owned,
                ) {
                    SupervisorCleanExitResolution::PromptUser => {
                        shared.transition_actor_state(
                            agent_doc_sqlite::state_store::ActorState::WaitingInput,
                            "supervisor",
                            "clean_exit_prompt",
                        );
                        // Temporarily restore cooked mode so read_line() works with
                        // normal line editing (echo, backspace, etc.)
                        raw_mode.suspend();
                        eprintln!("\n{} exited cleanly.", harness.binary);
                        match prompt_for_restart_or_quit(
                            &mut session_log,
                            "clean_exit",
                            "Press Enter to restart, or 'q' to exit.",
                            "user_quit",
                            PromptEofPolicy::Quit,
                        ) {
                            PromptOutcome::Quit => {
                                break "user_quit_clean_exit";
                            }
                            PromptOutcome::RestartFresh => {
                                raw_mode.resume();
                                first_run = true;
                                restart_count += 1;
                                suppress_stale_ctrl_d_until_prompt = false;
                            }
                        }
                    }
                    SupervisorCleanExitResolution::RestartContinue => {
                        let recent_failures = if failed_resume {
                            let now = Instant::now();
                            let recent_failures = failed_resume_tracker.record(now);
                            log_event(
                                &mut session_log,
                                &format!(
                                    "resume_restart_failed pane={} harness={} outcome={} recent_failures={} window_secs={} restart_count={}",
                                    pane_id,
                                    harness.binary,
                                    auto_trigger_outcome.as_str(),
                                    recent_failures,
                                    FAILED_RESUME_WINDOW.as_secs(),
                                    restart_count
                                ),
                            );
                            recent_failures
                        } else {
                            0
                        };

                        match restart_continue_exit_strategy(
                            ctrl_c_forwarded_interrupt,
                            failed_resume,
                            ctrl_d_forwarded,
                            recent_failures,
                            clean_exit_before_prompt,
                        ) {
                            SupervisorRestartContinueExitStrategy::CtrlCPromptUser => {
                                shared.transition_actor_state(
                                    agent_doc_sqlite::state_store::ActorState::WaitingInput,
                                    "supervisor",
                                    "ctrl_c_prompt",
                                );
                                raw_mode.suspend();
                                eprintln!("\n{} exited after stdin Ctrl+C.", harness.binary);
                                log_event(
                                    &mut session_log,
                                    &format!("ctrl_c_prompt_user restart_count={}", restart_count),
                                );
                                match prompt_for_restart_or_quit(
                                    &mut session_log,
                                    "ctrl_c",
                                    "Press Enter to restart fresh, or 'q' to exit.",
                                    "user_quit_after_ctrl_c",
                                    PromptEofPolicy::Quit,
                                ) {
                                    PromptOutcome::Quit => {
                                        break "user_quit_after_ctrl_c";
                                    }
                                    PromptOutcome::RestartFresh => {
                                        raw_mode.resume();
                                        first_run = true;
                                        restart_count += 1;
                                        suppress_stale_ctrl_d_until_prompt = false;
                                    }
                                }
                            }
                            SupervisorRestartContinueExitStrategy::CtrlDPromptUser => {
                                shared.transition_actor_state(
                                    agent_doc_sqlite::state_store::ActorState::WaitingInput,
                                    "supervisor",
                                    "ctrl_d_prompt",
                                );
                                raw_mode.suspend();
                                eprintln!("\n{} exited after stdin EOF/Ctrl-D.", harness.binary);
                                log_event(
                                    &mut session_log,
                                    &format!("ctrl_d_prompt_user restart_count={}", restart_count),
                                );
                                match prompt_for_restart_or_quit(
                                    &mut session_log,
                                    "ctrl_d",
                                    "Press Enter to restart fresh, or 'q' to exit.",
                                    "user_quit_after_ctrl_d",
                                    PromptEofPolicy::Quit,
                                ) {
                                    PromptOutcome::Quit => {
                                        break "user_quit_after_ctrl_d";
                                    }
                                    PromptOutcome::RestartFresh => {
                                        raw_mode.resume();
                                        first_run = true;
                                        restart_count += 1;
                                        suppress_stale_ctrl_d_until_prompt = false;
                                    }
                                }
                            }
                            SupervisorRestartContinueExitStrategy::PromptUser => {
                                shared.transition_actor_state(
                                    agent_doc_sqlite::state_store::ActorState::WaitingInput,
                                    "supervisor",
                                    "resume_failure_prompt",
                                );
                                raw_mode.suspend();
                                eprintln!(
                                    "\n{} failed to re-establish a prompt after resume {} times in the last {}s.",
                                    harness.binary,
                                    recent_failures,
                                    FAILED_RESUME_WINDOW.as_secs()
                                );
                                match prompt_for_restart_or_quit(
                                    &mut session_log,
                                    "resume_failure",
                                    "Press Enter to restart fresh, or 'q' to exit.",
                                    "user_quit_after_resume_failure",
                                    PromptEofPolicy::RestartFresh,
                                ) {
                                    PromptOutcome::Quit => {
                                        break "user_quit_after_resume_failure";
                                    }
                                    PromptOutcome::RestartFresh => {
                                        raw_mode.resume();
                                        first_run = true;
                                        restart_count += 1;
                                        suppress_stale_ctrl_d_until_prompt = false;
                                    }
                                }
                            }
                            SupervisorRestartContinueExitStrategy::RestartFresh => {
                                suppress_stale_ctrl_d_until_prompt = false;
                                if clean_exit_before_prompt {
                                    eprintln!(
                                        "\n{} exited cleanly before ever surfacing a prompt. Restarting fresh instead of resuming...",
                                        harness.binary
                                    );
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "fresh_restart_before_prompt restart_count={}",
                                            restart_count + 1
                                        ),
                                    );
                                } else {
                                    eprintln!(
                                        "\n{} exited after a failed resume handoff ({}). Restarting fresh instead of resuming...",
                                        harness.binary,
                                        auto_trigger_outcome.as_str()
                                    );
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "resume_restart_fresh outcome={} restart_count={}",
                                            auto_trigger_outcome.as_str(),
                                            restart_count + 1
                                        ),
                                    );
                                }
                                first_run = true;
                                auto_trigger_next_launch = auto_trigger;
                                restart_count += 1;
                            }
                            SupervisorRestartContinueExitStrategy::Resume => {
                                suppress_stale_ctrl_d_until_prompt = false;
                                eprintln!(
                                    "\n{} exited cleanly. Restarting in resume mode to keep the session attached...",
                                    harness.binary
                                );
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "auto_restart_clean with_continue=true restart_count={}",
                                        restart_count + 1
                                    ),
                                );
                                restart_count += 1;
                                continue;
                            }
                        }
                    }
                }
            }
            RestartAction::RestartAfter {
                delay,
                with_continue,
            } => {
                eprintln!(
                    "\n{} exited with code {}. Restarting in {:?}...",
                    harness.binary, code, delay
                );
                log_event(
                    &mut session_log,
                    &format!(
                        "auto_restart delay={:?} with_continue={} restart_count={}",
                        delay,
                        with_continue,
                        restart_count + 1
                    ),
                );
                std::thread::sleep(delay);
                if !with_continue {
                    first_run = true;
                }
                restart_count += 1;
                suppress_stale_ctrl_d_until_prompt = false;
            }
            RestartAction::Halt => {
                shared.transition_actor_state(
                    agent_doc_sqlite::state_store::ActorState::Blocked,
                    "supervisor",
                    "supervisor_halted",
                );
                eprintln!(
                    "\nSupervisor halted after {} restarts (flapping detected).",
                    restart_count
                );
                log_event(&mut session_log, "supervisor_halted");
                break "supervisor_halted";
            }
        }
    };

    // Restore terminal to original mode before cleanup
    drop(raw_mode);
    route_owned_completion_stop.store(true, Ordering::Relaxed);
    if let Some(handle) = route_owned_completion_thread {
        let _ = handle.join();
    }
    if let Some(handle) = capability_proof_thread {
        let _ = handle.join();
    }

    // Cleanup
    if let Some(mut rw) = resize_watcher.take() {
        rw.stop();
    }
    ipc.stop();
    shared.transition_actor_state(
        agent_doc_sqlite::state_store::ActorState::Closed,
        "supervisor",
        supervisor_exit_reason,
    );
    log_event(
        &mut session_log,
        &format!(
            "supervisor_exit reason={} pane={} restart_count={}",
            supervisor_exit_reason, pane_id, restart_count
        ),
    );
    log_event(&mut session_log, "session_end");
    start_console_status(
        &mut session_log,
        route_owned,
        format!("Session ended for {}", file.display()),
    );
    if route_owned && route_owned_completion.load(Ordering::Relaxed) {
        log_event(
            &mut session_log,
            &format!("route_owned_reap_pane pane={}", pane_id),
        );
        start_console_status(
            &mut session_log,
            route_owned,
            format!(
                "[start] route-owned cycle committed for {}; reaping pane {}",
                file.display(),
                pane_id
            ),
        );
        let tmux = tmux_router::Tmux::default_server();
        let _ = agent_doc_tmux_io::kill_pane(&tmux, &pane_id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_config::Config;
    use agent_doc_frontmatter::frontmatter::Frontmatter;
    use agent_doc_hooks_io::fire_doc_hooks;
    use agent_doc_project_config_io as project_config_io;
    use agent_doc_start_io::{
        existing_session_pane_action_from_entry, format_existing_pane_conflict_error,
        rebind_project_tmux_session_if_expected_dead, relocate_if_wrong_session,
    };
    use agent_doc_supervisor::ipc_protocol::IpcMethod;
    use agent_doc_supervisor::session_owner::ExistingSessionPaneAction;
    use std::collections::HashMap;
    use tempfile::TempDir;
    use tmux_router::IsolatedTmux;

    #[test]
    fn route_owned_start_status_logs_without_printing_by_default() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("session.log");
        let mut log = Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .unwrap(),
        );

        start_console_status(&mut log, true, "[start] harness resolved: binary=codex");
        drop(log);

        let content = std::fs::read_to_string(log_path).unwrap();
        assert!(
            content.contains("start_console_status route_owned=true printed=false"),
            "route-owned status should be log-only by default: {content}"
        );
        assert!(
            content.contains("[start] harness resolved: binary=codex"),
            "status proof should remain in the session log: {content}"
        );
    }

    #[test]
    fn route_owned_tui_supervisor_stderr_redirect_targets_log() {
        let tmp = TempDir::new().unwrap();
        let codex = agent_doc_harness::HarnessConfig::codex();
        let mut generic = codex.clone();
        generic.binary = "bash".to_string();

        assert!(supervisor_stderr_redirect_needed(&codex, true));
        assert!(!supervisor_stderr_redirect_needed(&codex, false));
        assert!(!supervisor_stderr_redirect_needed(&generic, true));
        assert_eq!(
            supervisor_stderr_redirect_path(tmp.path()),
            tmp.path()
                .join(".agent-doc")
                .join("logs")
                .join("supervisor-stderr.log")
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn relocate_noop_when_already_correct_session() {
        let iso = IsolatedTmux::new("start-reloc-noop");
        let pane = iso
            .new_session("sess-a", std::path::Path::new("/tmp"))
            .unwrap();
        // pane is already in sess-a; no relocation needed
        let result = relocate_if_wrong_session(&iso, &pane, "sess-a");
        assert!(
            result,
            "should return true (noop — already in correct session)"
        );
        // Verify pane is still in sess-a
        let sess = iso.pane_session(&pane).unwrap();
        assert_eq!(sess, "sess-a");
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn relocate_succeeds_cross_session() {
        let iso = IsolatedTmux::new("start-reloc-cross");
        let _pane_a = iso
            .new_session("sess-a", std::path::Path::new("/tmp"))
            .unwrap();
        let pane_b = iso
            .new_session("sess-b", std::path::Path::new("/tmp"))
            .unwrap();
        // pane_b is in sess-b; expected is sess-a — should auto-relocate
        let result = relocate_if_wrong_session(&iso, &pane_b, "sess-a");
        assert!(result, "should return true after successful relocation");
        let sess = iso.pane_session(&pane_b).unwrap();
        assert_eq!(sess, "sess-a", "pane should be in sess-a after relocation");
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn relocate_fails_gracefully_when_no_anchor() {
        let iso = IsolatedTmux::new("start-reloc-noanchor");
        let pane = iso
            .new_session("sess-a", std::path::Path::new("/tmp"))
            .unwrap();
        // Expected session "sess-nonexistent" has no active pane — relocation should fail gracefully
        let result = relocate_if_wrong_session(&iso, &pane, "sess-nonexistent");
        assert!(
            !result,
            "should return false when no anchor pane exists in expected session"
        );
        // pane should still be in original session
        let sess = iso.pane_session(&pane).unwrap();
        assert_eq!(
            sess, "sess-a",
            "pane should remain in original session on failure"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn start_rebinds_dead_project_session_to_current_pane_session() {
        let dir = TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "tmux_session = \"0\"\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("start-rebind-dead-session");
        let pane = iso.new_session("14", dir.path()).unwrap();

        let relocated = relocate_if_wrong_session(&iso, &pane, "0");
        assert!(
            !relocated,
            "missing anchor in dead configured session should fall back to current pane session"
        );

        rebind_project_tmux_session_if_expected_dead(&iso, &pane, "0");

        assert_eq!(
            project_config_io::project_tmux_session().as_deref(),
            Some("14")
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn start_does_not_rebind_live_project_session_pin() {
        let dir = TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "tmux_session = \"0\"\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("start-rebind-live-session");
        let _expected_pane = iso.new_session("0", dir.path()).unwrap();
        let pane = iso.new_session("14", dir.path()).unwrap();

        rebind_project_tmux_session_if_expected_dead(&iso, &pane, "0");

        assert_eq!(
            project_config_io::project_tmux_session().as_deref(),
            Some("0")
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn existing_session_pane_action_refuses_proven_live_owner() {
        let iso = IsolatedTmux::new("start-duplicate-live-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane_a = iso.new_session("test", tmp.path()).unwrap();
        let pane_b = iso.split_window(&pane_a, tmp.path(), "-dh").unwrap();
        let entry = tmux_router::RegistryEntry {
            pane: pane_a.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            session_id: "start-duplicate-live-pane".to_string(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&pane_a).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        };

        let action =
            existing_session_pane_action_from_entry(&iso, &pane_b, Some(&entry), Some(&pane_a));
        assert_eq!(
            action,
            Some(ExistingSessionPaneAction::Refuse(pane_a.clone()))
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn existing_session_refusal_keeps_launcher_pane_in_original_session() {
        let iso = IsolatedTmux::new("start-reuse-keeps-launcher-session");
        let tmp = tempfile::TempDir::new().unwrap();
        let owner_pane = iso.new_session("sess-a", tmp.path()).unwrap();
        let launcher_pane = iso.new_session("sess-b", tmp.path()).unwrap();
        let entry = tmux_router::RegistryEntry {
            pane: owner_pane.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            session_id: "start-reuse-keeps-launcher-session".to_string(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&owner_pane).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        };

        let action = existing_session_pane_action_from_entry(
            &iso,
            &launcher_pane,
            Some(&entry),
            Some(&owner_pane),
        );
        assert_eq!(
            action,
            Some(ExistingSessionPaneAction::Refuse(owner_pane.clone()))
        );
        assert_eq!(
            iso.pane_session(&launcher_pane).unwrap(),
            "sess-b",
            "refusing an existing live owner must not relocate the launcher pane"
        );
        assert_eq!(iso.pane_session(&owner_pane).unwrap(), "sess-a");
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn existing_session_pane_action_ignores_same_pane() {
        let iso = IsolatedTmux::new("start-duplicate-same-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let entry = tmux_router::RegistryEntry {
            pane: pane.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            session_id: "start-duplicate-same-pane".to_string(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&pane).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        };

        let action = existing_session_pane_action_from_entry(&iso, &pane, Some(&entry), None);
        assert_eq!(action, None);
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn existing_session_pane_action_refuses_alive_stale_registration_without_owner() {
        let iso = IsolatedTmux::new("start-stale-alive-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane_a = iso.new_session("test", tmp.path()).unwrap();
        let pane_b = iso.split_window(&pane_a, tmp.path(), "-dh").unwrap();
        let entry = tmux_router::RegistryEntry {
            pane: pane_a.clone(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            session_id: "start-stale-alive-pane".to_string(),
            file: "tasks/software/corky.md".to_string(),
            window: iso.pane_window(&pane_a).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        };

        let action = existing_session_pane_action_from_entry(&iso, &pane_b, Some(&entry), None);
        assert_eq!(
            action,
            Some(ExistingSessionPaneAction::Refuse(pane_a.clone()))
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn existing_session_pane_action_ignores_dead_registered_pane() {
        let iso = IsolatedTmux::new("start-duplicate-dead-pane");
        let tmp = tempfile::TempDir::new().unwrap();
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let entry = tmux_router::RegistryEntry {
            pane: "%999999".to_string(),
            pid: 0,
            cwd: tmp.path().display().to_string(),
            started: String::new(),
            session_id: "start-duplicate-dead-pane".to_string(),
            file: "tasks/software/corky.md".to_string(),
            window: String::new(),
            supervisor_instance_id: String::new(),
        };

        let action = existing_session_pane_action_from_entry(&iso, &pane, Some(&entry), None);
        assert_eq!(action, None);
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn format_existing_pane_conflict_error_includes_manual_tmux_commands() {
        let iso = IsolatedTmux::new("start-conflict-error");
        let tmp = TempDir::new().unwrap();
        let owner_pane = iso.new_session("test", tmp.path()).unwrap();
        let launcher_pane = iso.split_window(&owner_pane, tmp.path(), "-dh").unwrap();
        let doc = tmp.path().join("tasks/software/corky.md");
        let rendered = format_existing_pane_conflict_error(&iso, &doc, &launcher_pane, &owner_pane);
        assert!(rendered.contains("tmux list-panes -a"));
        assert!(rendered.contains(&format!("tmux kill-pane -t {}", launcher_pane)));
        assert!(rendered.contains(&format!("tmux kill-pane -t {}", owner_pane)));
        assert!(rendered.contains(&owner_pane));
        assert!(rendered.contains(&launcher_pane));
    }
    #[test]
    fn start_invalid_frontmatter_returns_contextual_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("bad.md");
        std::fs::write(&file, "---\nprompt_presets:\n  key: [oops\n---\n").unwrap();

        let err = run(&file, false, false).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("invalid YAML frontmatter in"));
        assert!(message.contains("bad.md"));
        assert!(message.contains("Frontmatter excerpt:"));
        assert!(message.contains("> 2 |   key: [oops"));
        assert!(
            message.contains("Fix the frontmatter between the opening and closing --- markers")
        );
    }
    #[test]
    fn idle_queue_drain_payload_uses_trigger_for_codex() {
        let harness = agent_doc_harness::HarnessConfig::codex();
        let payload = idle_queue_drain_payload(
            "JB Run Agent Doc on sampleorders.md stalled.",
            harness.trigger_command("tasks/sampleorders.md"),
        );

        assert_eq!(payload, "agent-doc tasks/sampleorders.md");
        assert!(!payload.contains("Agent-doc active queue continuation"));
        assert!(!payload.contains("JB Run Agent Doc on sampleorders.md stalled."));
        assert_eq!(
            idle_queue_drain_payload_kind("JB Run Agent Doc on sampleorders.md stalled."),
            "trigger"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn managed_capability_proof_status_uses_tmux_message_not_pane_output() {
        let tmp = TempDir::new().unwrap();
        let iso = IsolatedTmux::new("start-capability-proof-status");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let event = "opencode_capability_proof status=proven network=proven";

        display_managed_capability_proof_status(&iso, &pane, "opencode", event)
            .expect("proof status should be surfaced through tmux display-message");
        std::thread::sleep(Duration::from_millis(150));

        let captured = iso.capture_pane(&pane, Some(20)).unwrap();
        assert!(
            !captured.contains(event),
            "tmux display-message must not write proof diagnostics into pane output: {captured}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn dispatch_submit_text_to_tmux_uses_pane_submit_path() {
        let tmp = TempDir::new().unwrap();
        let iso = IsolatedTmux::new("start-ipc-submit-path");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let output_path = tmp.path().join("submit.txt");
        let done_path = tmp.path().join("done.txt");

        std::thread::sleep(Duration::from_millis(150));
        iso.send_keys(
            &pane,
            &format!(
                "sh -lc 'IFS= read -r line; printf \"%s\" \"$line\" > \"{}\"; touch \"{}\"'",
                output_path.display(),
                done_path.display()
            ),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(150));

        dispatch_submit_text_to_tmux(&iso, &pane, "agent-doc tasks/software/tsift.md\n", "claude")
            .unwrap();
        for _ in 0..40 {
            if done_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(done_path.exists(), "expected submitted command to complete");
        assert_eq!(
            std::fs::read_to_string(&output_path).unwrap(),
            "agent-doc tasks/software/tsift.md"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn supervisor_ipc_tmux_pipeline_delivers_submit_arrows_and_enter() {
        let tmp = TempDir::new().unwrap();
        let iso = IsolatedTmux::new("start-ipc-live-supervisor-input-e2e");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let output_path = tmp.path().join("input.bin");
        let done_path = tmp.path().join("done.txt");
        let prompt = "agent-doc tasks/software/tsift.md";
        let expected = format!("{prompt}\n\x1b[A\x1b[B\x1b[D\x1b[C\n").into_bytes();

        std::thread::sleep(Duration::from_millis(150));
        iso.send_keys(
            &pane,
            &format!(
                "sh -lc 'stty raw -echo; dd bs=1 count={} of=\"{}\" 2>/dev/null; touch \"{}\"'",
                expected.len(),
                output_path.display(),
                done_path.display()
            ),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(150));

        let _tmux_env = ScopedEnvVar::set("TMUX", tmux_env_for_server(&iso));
        let shared = Arc::new(SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            "claude",
            None,
            Some(agent_doc_sqlite::state_store::ActorState::Ready),
            Some(pane.clone()),
        ));
        let shared_for_ipc = shared.clone();
        let mut ipc =
            agent_doc_supervisor_io::ipc::SupervisorIpc::start(tmp.path(), "test-session", {
                move |method| {
                    agent_doc_supervisor_io::ipc::handle_supervisor_ipc(
                        method,
                        shared_for_ipc.as_ref(),
                    )
                }
            })
            .unwrap();

        let response = agent_doc_supervisor_io::ipc::send_command(
            ipc.path(),
            &IpcMethod::Inject {
                bytes: prompt.to_string(),
            },
        )
        .expect("supervisor IPC inject should succeed");
        assert!(response.ok, "supervisor IPC inject should return ok");

        agent_doc_tmux_io::send_key(&iso, &pane, "Up").unwrap();
        agent_doc_tmux_io::send_key(&iso, &pane, "Down").unwrap();
        agent_doc_tmux_io::send_key(&iso, &pane, "Left").unwrap();
        agent_doc_tmux_io::send_key(&iso, &pane, "Right").unwrap();
        agent_doc_tmux_io::send_key(&iso, &pane, "Enter").unwrap();

        for _ in 0..40 {
            if done_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(
            done_path.exists(),
            "expected supervisor IPC inject to submit through the live tmux pane"
        );
        let actual = std::fs::read(&output_path).unwrap();
        let expected_cr = format!("{prompt}\r\x1b[A\x1b[B\x1b[D\x1b[C\r").into_bytes();
        assert!(
            actual == expected || actual == expected_cr,
            "raw harness should receive prompt submit, arrows, and final Enter"
        );

        ipc.stop();
    }
}
