//! The supervisor idle-queue watch thread and its two dedicated helpers, extracted
//! from `start.rs`. This is the consumer of the pure [`super::decisions`] policy: it
//! polls the owned pane on a timer, and on each busy→idle transition decides whether
//! to drain a live `agent:queue` head, fire a context reset, or (`#ctlrecycle` R3)
//! hot-reload a stale supervisor. As a child module of `start` it reaches the
//! supervisor's private `SupervisorShared`, log/sleep helpers, decision re-exports,
//! and `supervisor_perform_reexec` directly through `use super::*`.

use super::*;

fn record_context_clear_prompt_for_hooks(
    shared: &SupervisorShared,
    path: &Path,
    harness: &crate::harness::HarnessConfig,
    clear_cmd: &str,
) {
    if !matches!(harness.binary.as_str(), "codex" | "opencode") {
        return;
    }
    let Some(runtime) = shared.actor_runtime.as_ref() else {
        return;
    };
    if let Err(err) =
        crate::codex_hook::record_external_prompt_for_file(path, &runtime.session_id, clear_cmd)
    {
        eprintln!(
            "[agent-doc] idle-queue watch: failed to record context clear prompt for {}: {err:#}",
            path.display()
        );
    }
}

/// Read the live `queue_active: true` ready head for the owned document, if any.
///
/// Thin wrapper over [`crate::queue_continuation::live_continuation_head`] so the
/// idle-watch and the codex-stop / preflight continuation paths all derive the
/// drainable head from one shared definition (`queue_active: true` + an active
/// `resolve_activation` + a ready prompt head).
fn idle_watch_active_queue_head(file: &Path) -> Option<String> {
    let content = std::fs::read_to_string(file).ok()?;
    crate::queue_continuation::live_continuation_head(file, &content)
}

pub(super) fn spawn_idle_queue_watch_thread(
    shared: Arc<SupervisorShared>,
    stop: Arc<AtomicBool>,
    file: String,
    harness: crate::harness::HarnessConfig,
    mut session_log: Option<std::fs::File>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("idle-queue-watch".into())
        .spawn(move || {
            let path = PathBuf::from(&file);
            let mut last_dispatched: Option<String> = None;
            let mut last_context_clear_at: Option<u64> = None;
            let mut last_context_reset_head: Option<String> = None;
            let mut clear_cooldown_logged = false;
            let mut idle_busy_ticks: u32 = 0;
            // R3 (#ctlrecycle): capture this supervisor's launch binary identity (≈ the
            // installed binary at process start). A later `cargo install` makes
            // `current_binary_identity()` differ, marking this supervisor stale.
            let recycle_launch_identity = crate::project_controller::current_binary_identity().ok();
            let recycle_auto_enabled =
                crate::project_controller::supervisor_auto_recycle_enabled(&path);
            let recycle_grace = crate::project_controller::recycle_idle_grace();
            let mut recycle_stale_since: Option<std::time::Instant> = None;
            let mut recycle_detected_logged = false;
            loop {
                if !sleep_with_stop(&stop, AUTO_TRIGGER_POLL_INTERVAL) {
                    return;
                }
                // Gate on the managed capability proof exactly like the
                // auto-trigger: never dispatch before network/SSH/write-root
                // proof clears for a managed Codex launch.
                if shared.capability_proof_gate() == CapabilityProofGate::Pending {
                    continue;
                }
                let clear_cooldown_active = clear_cooldown_blocks_auto_dispatch(
                    &path,
                    &harness,
                    "idle_queue_watch",
                    &mut session_log,
                    &mut clear_cooldown_logged,
                );

                // `#stale-busy-after-auto-inject-no-clear`: poll-based self-heal
                // for a stale busy actor wedged over an idle pane. The
                // edge-triggered pty completion transition can miss an
                // injected turn's composer redraw after it returns, leaving the
                // actor `busy` with no further output to retrigger ready.
                // Re-derive ready from direct pane evidence so the session
                // never gets "truly stuck" needing a pane kill or
                // `session clear`.
                let actor_busy = actor_state_is_busy_or_starting(&shared);
                let pane_busy_cue = if actor_busy && !clear_cooldown_active {
                    supervisor_pane_has_busy_cue(&shared, &harness)
                } else {
                    None
                };
                match pane_busy_cue {
                    Some(false) => idle_busy_ticks = idle_busy_ticks.saturating_add(1),
                    _ => idle_busy_ticks = 0,
                }
                if stale_busy_idle_reconcile_decision(
                    actor_busy,
                    pane_busy_cue == Some(true),
                    clear_cooldown_active,
                    idle_busy_ticks,
                ) {
                    shared.transition_actor_state(
                        crate::session_actor::ActorState::Ready,
                        "supervisor",
                        "idle_pane_reconcile",
                    );
                    // Reset the one-shot prompt latch so a later genuine
                    // busy→ready edge still fires normally. Preserve the
                    // dispatch dedup: if the injected command returned without
                    // consuming the same active head, re-firing it every
                    // stale-busy reconcile tick loops the owner pane.
                    shared.prompt_visible_once.store(false, Ordering::Relaxed);
                    last_dispatched = reconcile_stale_busy_idle_queue_state(
                        last_dispatched,
                        &mut idle_busy_ticks,
                    );
                    log_event(
                        &mut session_log,
                        &format!(
                            "idle_queue_watch_stale_busy_reconciled harness={} pane={} after_ticks={}",
                            harness.binary,
                            shared.inject_pane.as_deref().unwrap_or("<pty>"),
                            STALE_BUSY_RECONCILE_TICKS
                        ),
                    );
                    eprintln!(
                        "[agent-doc] idle-queue watch: reconciled stale busy actor to ready from idle pane evidence (no pane kill)"
                    );
                }

                let active_head = idle_watch_active_queue_head(&path);
                let prompt_visible = idle_queue_prompt_visible(&shared, &harness);
                let turn_active = turn_active_for_owned_pane(&path, &shared);

                // R3 (#ctlrecycle) / #suprecyclequeue: recycle this supervisor onto a
                // freshly-installed binary at a turn boundary. Recycling ends the live
                // agent child, so the automatic path is opt-in
                // (`AGENT_DOC_SUPERVISOR_AUTO_RECYCLE`); otherwise we only surface
                // staleness once so the operator can restart deliberately. A turn
                // boundary is a dispatch-ready prompt with no turn running. When a queue
                // head is still waiting to drain, the *next* queue item is the
                // deliberate restart point, so we recycle immediately (the re-exec'd
                // image re-dispatches the pending head on the fresh binary); with no
                // head waiting we debounce so a brief idle gap between unrelated turns
                // never trips it.
                let turn_boundary = prompt_visible && !turn_active;
                let head_pending = active_head.is_some();
                let supervisor_stale = crate::project_controller::process_binary_is_stale(
                    recycle_launch_identity.as_ref(),
                );
                let recycle_action = supervisor_recycle_action(
                    supervisor_stale,
                    recycle_auto_enabled,
                    turn_boundary,
                    head_pending,
                );
                // The idle-grace debounce only gates the no-head-pending path; an
                // inter-queue-item recycle bypasses it.
                let (recycle_debounced, next_recycle_since) =
                    crate::project_controller::recycle_debounce_decision(
                        matches!(recycle_action, SupervisorRecycleAction::RecycleDebounced),
                        recycle_stale_since,
                        std::time::Instant::now(),
                        recycle_grace,
                    );
                recycle_stale_since = next_recycle_since;
                if matches!(recycle_action, SupervisorRecycleAction::Detect)
                    && !recycle_detected_logged
                {
                    recycle_detected_logged = true;
                    log_event(
                        &mut session_log,
                        &format!(
                            "supervisor_binary_stale_detected pane={} hint=restart_or_set_AGENT_DOC_SUPERVISOR_AUTO_RECYCLE",
                            shared.inject_pane.as_deref().unwrap_or("<pty>"),
                        ),
                    );
                    eprintln!(
                        "[agent-doc] supervisor is running a prior agent-doc binary; restart this session (or set AGENT_DOC_SUPERVISOR_AUTO_RECYCLE=1) to pick up the new build"
                    );
                }
                let do_recycle = match recycle_action {
                    SupervisorRecycleAction::RecycleImmediate => true,
                    SupervisorRecycleAction::RecycleDebounced => recycle_debounced,
                    _ => false,
                };
                if do_recycle {
                    // `#ctlrecycle` R3 — hot-reload onto the fresh binary IN PLACE via
                    // `execve`, preserving the live harness child + tmux pane. Falls
                    // back to a clean exit (child restarts) if the in-place swap cannot
                    // start.
                    let recycle_boundary = if head_pending {
                        "next_queue_item"
                    } else {
                        "idle"
                    };
                    #[cfg(unix)]
                    {
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_binary_stale_self_recycled via=execve_preserve_child boundary={} pane={} child_pid={} master_fd={}",
                                recycle_boundary,
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                shared.child_pid.load(Ordering::Relaxed),
                                shared.master_fd.load(Ordering::Relaxed),
                            ),
                        );
                        eprintln!(
                            "[agent-doc] supervisor hot-reloading onto freshly-installed agent-doc binary ({recycle_boundary}); preserving the live agent child via execve"
                        );
                        match supervisor_perform_reexec(&shared) {
                            Ok(never) => match never {},
                            Err(err) => {
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "supervisor_reexec_failed fallback=process_exit error={err}"
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] supervisor execve hot-reload failed ({err}); falling back to clean exit so the next launch uses the new build"
                                );
                                std::process::exit(0);
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_binary_stale_self_recycled via=process_exit boundary={} pane={}",
                                recycle_boundary,
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                            ),
                        );
                        eprintln!(
                            "[agent-doc] supervisor recycling onto freshly-installed agent-doc binary ({recycle_boundary}); the next launch uses the new build"
                        );
                        std::process::exit(0);
                    }
                }

                // `#autoloop-command-preemption` Phase 2b: a non-interrupting
                // `session clear` against this busy auto-loop deferred itself
                // (paused the loop via the clear cooldown + recorded a
                // deferred-clear marker). Deliver that clear here at the idle
                // gap, then drop both markers to resume the loop. When no marker
                // exists this is a complete no-op, so the existing drain path
                // below is unchanged.
                let deferred_clear = crate::queue_continuation::read_deferred_operator_clear(&path)
                    .unwrap_or(None);
                match crate::queue_preemption::plan_deferred_clear_step(
                    deferred_clear.is_some(),
                    prompt_visible && !turn_active,
                ) {
                    crate::queue_preemption::DeferredClearStep::None => {}
                    crate::queue_preemption::DeferredClearStep::WaitForIdle => {
                        // Pending clear, pane still mid-turn: do not interrupt
                        // in-flight work; wait for the next idle tick.
                        continue;
                    }
                    crate::queue_preemption::DeferredClearStep::Deliver => {
                        let clear_cmd = deferred_clear
                            .as_ref()
                            .map(|d| d.clear_command.clone())
                            .unwrap_or_default();
                        match auto_trigger_clear_command(&shared, &stop, &clear_cmd) {
                            AutoTriggerOutcome::Cancelled => return,
                            AutoTriggerOutcome::Sent => {
                                // Resume: drop the deferred-clear record AND the
                                // pause cooldown so the next tick drains normally.
                                if let Err(err) =
                                    crate::queue_continuation::clear_deferred_operator_clear_marker(
                                        &path,
                                    )
                                {
                                    eprintln!(
                                        "[agent-doc] idle-queue watch: failed to drop deferred-clear marker: {err:#}"
                                    );
                                }
                                if let Err(err) =
                                    crate::queue_continuation::clear_cooldown_marker(&path)
                                {
                                    eprintln!(
                                        "[agent-doc] idle-queue watch: failed to clear cooldown after deferred clear: {err:#}"
                                    );
                                }
                                last_dispatched = None;
                                last_context_clear_at = Some(current_epoch_secs());
                                if let Some(head) = active_head.clone() {
                                    last_context_reset_head = Some(head);
                                }
                                record_context_clear_prompt_for_hooks(
                                    &shared,
                                    &path,
                                    &harness,
                                    &clear_cmd,
                                );
                                clear_cooldown_logged = false;
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_deferred_clear_delivered harness={} cmd=\"{}\"",
                                        harness.binary, clear_cmd
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] idle-queue watch: delivered deferred operator clear and resumed the loop"
                                );
                                // Let the clear run; resume drains on a later tick.
                                continue;
                            }
                            _ => {
                                // Delivery failed: keep the marker and retry on
                                // the next idle tick (do not resume yet).
                                log_event(
                                    &mut session_log,
                                    "idle_queue_watch_deferred_clear_failed",
                                );
                                continue;
                            }
                        }
                    }
                }

                // `#nm1x-no-preempt-clear`: the accretion-driven pre-emptive
                // `/clear` interleave is opt-in. Without an explicit
                // `agent_doc_queue_context_reset` opt-in (frontmatter or
                // `.agent-doc/config.toml`), the idle-queue watch never fires a
                // pre-emptive `/clear` before a queue head — so a manual
                // `Run Agent Doc` or auto-loop drain does not churn the session
                // or hit `/clear` rejected mid-turn. Deferred *operator* clears
                // (an explicit `session clear`) are a separate path and stay live.
                // `#s760c`: the real context-usage signal is the harness
                // transcript token %, NOT exchange size (footers vary by
                // harness, and document size is not loaded-context size). When
                // opted in and idle, compute the live transcript ctx%, emit the
                // canonical `[s760] clear-decision …` line to ops.log so the
                // decision is observable in production, and fire the tracked
                // `/clear` only when ctx% crosses the resolved threshold. The
                // compaction-after-clear safety case is preserved separately: a
                // compaction shrinks the document but not the already-loaded
                // conversation, so it still warrants a reset. Everything stays
                // behind the default-off `agent_doc_queue_context_reset` opt-in,
                // and an unknown ctx% (`pct=None`) never clears (fail safe).
                let context_reset_reason = if clear_cooldown_active
                    || !crate::session_accretion::queue_context_reset_opted_in(&path)
                {
                    None
                } else {
                    let pct = live_transcript_context_pct(&path, &harness);
                    let threshold = crate::session_accretion::clear_threshold_for_doc(&path);
                    let decision = crate::context_pct::clear_decision(true, pct, threshold);
                    crate::ops_log::log_op(&path, &decision.diagnostic);
                    if crate::input_diag::verbose_enabled() {
                        eprintln!("[agent-doc] idle-queue watch: {}", decision.diagnostic);
                    }
                    if decision.clear {
                        Some(format!(
                            "transcript context {:.1}% >= clear threshold {}% (#s760c)",
                            pct.unwrap_or_default(),
                            threshold
                        ))
                    } else {
                        match crate::session_accretion::recent_exchange_compaction_timestamp(&path) {
                            Ok(Some(compaction_ts))
                                if last_context_clear_at.unwrap_or(0) < compaction_ts =>
                            {
                                Some(
                                    "exchange was compacted after the last tracked context clear (#s760c)"
                                        .to_string(),
                                )
                            }
                            Ok(_) => None,
                            Err(err) => {
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_context_reset_policy_failed harness={} file={} error={:?}",
                                        harness.binary,
                                        path.display(),
                                        err.to_string()
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] idle-queue watch: failed to inspect queue context reset policy for {}: {err:#}",
                                    path.display()
                                );
                                None
                            }
                        }
                    }
                };
                match idle_queue_context_reset_decision(
                    prompt_visible,
                    turn_active,
                    active_head.as_deref(),
                    last_context_reset_head.as_deref(),
                    context_reset_reason.is_some(),
                ) {
                    IdleQueueContextResetDecision::Reset => {
                        let head = active_head.as_deref().unwrap_or("<unknown>");
                        let clear_cmd = harness.context_clear_command();
                        match auto_trigger_clear_command(&shared, &stop, clear_cmd) {
                            AutoTriggerOutcome::Cancelled => return,
                            AutoTriggerOutcome::Sent => {
                                last_context_clear_at = Some(current_epoch_secs());
                                last_context_reset_head = active_head.clone();
                                last_dispatched = None;
                                record_context_clear_prompt_for_hooks(
                                    &shared,
                                    &path,
                                    &harness,
                                    clear_cmd,
                                );
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_context_reset harness={} cmd=\"{}\" head={:?} reason={:?}",
                                        harness.binary,
                                        clear_cmd,
                                        head,
                                        context_reset_reason.as_deref().unwrap_or("")
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] idle-queue watch: interleaved {} before active queue head {:?}: {}",
                                    clear_cmd,
                                    head,
                                    context_reset_reason.as_deref().unwrap_or("fresh context required")
                                );
                                continue;
                            }
                            outcome => {
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_context_reset_failed harness={} cmd=\"{}\" outcome={}",
                                        harness.binary,
                                        clear_cmd,
                                        outcome.as_str()
                                    ),
                                );
                                continue;
                            }
                        }
                    }
                    IdleQueueContextResetDecision::SkipNoActiveHead
                    | IdleQueueContextResetDecision::SkipNotIdle
                    | IdleQueueContextResetDecision::SkipTurnActive
                    | IdleQueueContextResetDecision::SkipAlreadyResetHead
                    | IdleQueueContextResetDecision::SkipNoResetNeeded => {}
                }

                // Single-owner tie-break: if the Claude Code `/loop` auto-loop holds
                // a fresh drain-owner lease it owns this drain, so the supervisor
                // must defer instead of double-injecting (#kp5z / #qflood).
                let drain_owner_lease =
                    crate::drain_owner::fresh_drain_owner_lease(&file, current_epoch_secs());
                match idle_queue_drain_decision(
                    clear_cooldown_active,
                    prompt_visible,
                    turn_active,
                    drain_owner_lease.is_some(),
                    active_head.as_deref(),
                    last_dispatched.as_deref(),
                ) {
                    IdleQueueDrainDecision::Dispatch => {
                        let head = active_head.expect("dispatch implies an active head");
                        let drain_payload = idle_queue_drain_payload(&file, &harness, &head);
                        let payload_kind = idle_queue_drain_payload_kind(&harness, &head);
                        let slash_command = idle_queue_head_slash_command(&head);
                        match auto_trigger_submit_queue_command(&shared, &stop, &drain_payload) {
                            AutoTriggerOutcome::Sent => {
                                log_idle_queue_drain_submit(
                                    &path,
                                    &shared,
                                    &harness,
                                    payload_kind,
                                    &head,
                                    &drain_payload,
                                );
                                if let Some(command) = slash_command.as_deref() {
                                    let completed = complete_idle_queue_slash_command_head(
                                        &path,
                                        &head,
                                        command,
                                        &mut session_log,
                                    );
                                    if crate::queue_command::is_context_clear_command(command) {
                                        last_context_clear_at = Some(current_epoch_secs());
                                        last_context_reset_head = Some(head.clone());
                                        record_context_clear_prompt_for_hooks(
                                            &shared,
                                            &path,
                                            &harness,
                                            command,
                                        );
                                    }
                                    last_dispatched = if completed { None } else { Some(head) };
                                } else {
                                    last_dispatched = Some(head);
                                }
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_drain harness={} payload_kind={} submit_mode={}",
                                        harness.binary,
                                        payload_kind,
                                        idle_queue_submit_mode(&shared, &harness)
                                    ),
                                );
                                // Already recorded in session_log above; only
                                // surface on stderr under verbose input diag so it
                                // does not bleed in front of a full-screen harness
                                // TUI (e.g. OpenCode). (#opencode-stdout-bleed)
                                if crate::input_diag::verbose_enabled() {
                                    eprintln!(
                                        "[agent-doc] idle-queue watch: drained active queue head via {payload_kind}",
                                    );
                                }
                            }
                            AutoTriggerOutcome::Cancelled => return,
                            outcome => {
                                // Do NOT record the head: a failed inject must be
                                // retried on the next idle tick, not suppressed.
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_drain_failed harness={} outcome={}",
                                        harness.binary,
                                        outcome.as_str()
                                    ),
                                );
                            }
                        }
                    }
                    IdleQueueDrainDecision::SkipNoActiveHead => {
                        // Head drained (or never present): clear dedup so a later
                        // re-enqueue of the same prompt text fires again.
                        last_dispatched = None;
                    }
                    IdleQueueDrainDecision::SkipSelfDrivingLoopOwner => {
                        // The Claude Code `/loop` owns the drain — proof the
                        // supervisor deferred (the live-verify signal for #kp5z).
                        if let Some(lease) = &drain_owner_lease {
                            let lease_age =
                                current_epoch_secs().saturating_sub(lease.heartbeat_secs);
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_drain_decision decision=SkipSelfDrivingLoopOwner owner={} lease_age={}",
                                    lease.owner, lease_age
                                ),
                            );
                        }
                    }
                    IdleQueueDrainDecision::SkipNotIdle
                    | IdleQueueDrainDecision::SkipTurnActive
                    | IdleQueueDrainDecision::SkipClearCooldown
                    | IdleQueueDrainDecision::SkipAlreadyDispatched => {}
                }
            }
        })
        .expect("spawn idle-queue watch thread")
}

