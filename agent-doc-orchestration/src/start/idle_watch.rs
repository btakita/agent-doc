//! The supervisor idle-queue watch thread and its two dedicated helpers, extracted
//! from `start.rs`. This is the consumer of the pure [`super::decisions`] policy: it
//! polls the owned pane on a timer, and on each busy→idle transition decides whether
//! to drain a live `agent:queue` head, fire a context reset, or (`#ctlrecycle` R3)
//! hot-reload a stale supervisor. As a child module of `start` it reaches the
//! supervisor's private `SupervisorShared`, log/sleep helpers, decision re-exports,
//! and `supervisor_perform_reexec` directly through `use super::*`.

use super::*;
use crate::start::decisions::{
    IdleQueueContextClearInFlightDecision, IdleQueueContextClearInFlightFacts,
    MAX_REEXEC_ESCALATIONS, reexec_escalation_within_bound,
};

const CLEAN_SESSION_CONTEXT_RESET_REASON: &str = "active queue head is a [clean-session] item - clearing to give it a fresh agent context (#cleandrainsup)";

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
/// Thin wrapper over [`crate::queue_continuation::live_drainable_continuation_head`]
/// so the supervisor idle-watch dispatch agrees with `session-check`'s continuation
/// decision: it returns a head only when there is **agent-drainable** work at the
/// queue head, skipping inert noise lines (operator bug-report observations) and
/// deferred `[clean-session]`/`[operator-verify]` heads. Otherwise the watch would
/// re-inject a no-op `/agent-doc` drain trigger every idle boundary for a queue
/// that has no continuation required (#qchurn / #goqueuestall / #goqstall2).
fn idle_watch_active_queue_head(file: &Path) -> Option<String> {
    let content = std::fs::read_to_string(file).ok()?;
    crate::queue_continuation::live_drainable_continuation_head(file, &content)
}

/// `#qstallguard` Layer C: should the supervisor idle-watch SKIP dispatch on a
/// queue under an accepted `admin queue pause`?
///
/// An accepted pause is the `#rt83`/`#qflood` flood guard — it suppresses the
/// *unattended re-injection flood*, NOT all draining (the sanctioned way to stop
/// the loop entirely is `queue: stop` / a `--- stop` fence, never pause). Before
/// this guard, pause unconditionally skipped, so a paused queue's ONLY drainer was
/// the attended in-session `/loop`; when that loop stalled/abandoned the drain,
/// the queue was stranded with no failsafe.
///
/// New rule: on a paused queue, skip ONLY when an in-session `/loop` owns the drain
/// (a fresh drain-owner lease — defer to it) or there is no drainable head. With no
/// loop owner AND a drainable head, the supervisor performs a SINGLE-OWNER failsafe
/// drain (the caller falls through to the normal, fully-guarded
/// [`idle_queue_drain_decision`], whose `turn_active` / route-in-flight / cooldown
/// guards bound it to one dispatch per turn — never the 2/sec flood the pause
/// guards against). Pure so it is unit-testable. `paused == false` is never a skip.
pub fn paused_idle_watch_should_skip(
    paused: bool,
    has_drainable_head: bool,
    loop_owner_lease_fresh: bool,
) -> bool {
    if !paused {
        return false;
    }
    loop_owner_lease_fresh || !has_drainable_head
}

fn idle_queue_context_reset_ops_log_message(
    file: &Path,
    harness: &crate::harness::HarnessConfig,
    clear_cmd: &str,
    target: &str,
    active_head: &str,
    reason: &str,
) -> String {
    format!(
        "idle_queue_watch_context_reset file={} harness={} cmd={:?} target={} head_bytes={} head_sha256={} reason={:?}",
        file.display(),
        harness.binary,
        clear_cmd,
        target,
        active_head.len(),
        crate::ops_log::content_hash(active_head),
        reason,
    )
}

fn log_idle_queue_context_reset_submit(
    file: &Path,
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
    clear_cmd: &str,
    active_head: &str,
    reason: &str,
) {
    let target = shared.inject_pane.as_deref().unwrap_or("child_pty");
    crate::ops_log::log_op(
        file,
        &idle_queue_context_reset_ops_log_message(
            file,
            harness,
            clear_cmd,
            target,
            active_head,
            reason,
        ),
    );
}

fn record_context_clear_in_flight_marker(
    file: &Path,
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
    clear_cmd: &str,
    active_head: Option<&str>,
) {
    let target = shared
        .inject_pane
        .as_deref()
        .or_else(|| {
            shared
                .actor_runtime
                .as_ref()
                .map(|runtime| runtime.pane_id.as_str())
        })
        .unwrap_or("child_pty");
    if let Err(err) = crate::context_clear_in_flight::record_context_clear_in_flight(
        file,
        target,
        &harness.binary,
        clear_cmd,
        active_head,
    ) {
        eprintln!(
            "[agent-doc] idle-queue watch: failed to record context-clear marker for {}: {err:#}",
            file.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "idle_queue_watch_context_clear_marker_failed file={} harness={} error={:?}",
                file.display(),
                harness.binary,
                err.to_string()
            ),
        );
    }
}

fn log_between_turn_enqueue_delivery(file: &Path, clear_cmd: &str, drain_payload: &str) {
    let plan = crate::start::decisions::between_turn_enqueue_plan(
        [clear_cmd, drain_payload],
        clear_cmd,
        drain_payload,
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "between_turn_enqueue deduped={} kept={} result=delivered",
            plan.deduped,
            plan.kept_labels()
        ),
    );
}

fn idle_queue_pending_payload_needs_enter_resubmit(
    harness_binary: &str,
    payload_already_pending: Option<bool>,
    already_resubmitted: bool,
) -> bool {
    crate::sessions::tmux_submit_profile_for_harness(harness_binary).pending_draft_enter_resubmit()
        && drain_dispatch_dedup_skip(payload_already_pending)
        && !already_resubmitted
}

fn context_reset_dedupe_head<'a>(
    active_head: Option<&'a str>,
    last_context_reset_head: Option<&'a str>,
    context_reset_in_flight: bool,
) -> Option<&'a str> {
    if context_reset_in_flight {
        active_head
    } else {
        last_context_reset_head
    }
}

fn idle_queue_resubmit_pending_payload(
    file: &Path,
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
    payload_kind: &str,
    active_head: &str,
    payload: &str,
) -> AutoTriggerOutcome {
    let Some(pane) = shared
        .inject_pane
        .clone()
        .or_else(|| shared.actor_runtime.as_ref().map(|r| r.pane_id.clone()))
    else {
        return AutoTriggerOutcome::SendFailed;
    };
    let submit_key = crate::sessions::tmux_submit_key_for_harness(&harness.binary);
    crate::input_diag::log_text_submit(
        Some(file),
        "supervisor.idle_queue_resubmit",
        &format!("pane:{pane}"),
        "",
        Some(&harness.binary),
        "idle_queue_pending_payload_submit_key",
        submit_key,
    );
    let tmux = crate::sessions::Tmux::default_server();
    match crate::sessions::send_submitted_text_for_harness(&tmux, &pane, "", &harness.binary) {
        Ok(()) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "idle_queue_watch_resubmit file={} harness={} action=submit_key key={} result=sent target={} payload_kind={} head_bytes={} head_sha256={} payload_bytes={}",
                    file.display(),
                    harness.binary,
                    submit_key,
                    pane,
                    payload_kind,
                    active_head.len(),
                    crate::ops_log::content_hash(active_head),
                    payload.len()
                ),
            );
            AutoTriggerOutcome::Sent
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "idle_queue_watch_resubmit file={} harness={} action=submit_key key={} result=send_failed target={} payload_kind={} head_bytes={} head_sha256={} error={:?}",
                    file.display(),
                    harness.binary,
                    submit_key,
                    pane,
                    payload_kind,
                    active_head.len(),
                    crate::ops_log::content_hash(active_head),
                    err.to_string()
                ),
            );
            eprintln!(
                "[agent-doc] idle-queue watch: {} pending payload {} re-submit failed for pane {}: {err:#}",
                harness.binary, submit_key, pane
            );
            AutoTriggerOutcome::SendFailed
        }
    }
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
            let mut last_context_reset_head: Option<String> = None;
            let mut last_context_clear_at: Option<u64> = None;
            let mut context_reset_in_flight = false;
            let mut last_pending_enter_resubmitted: Option<String> = None;
            let mut clear_cooldown_logged = false;
            let mut route_submit_in_flight_logged = false;
            let mut context_reset_policy_error_logged = false;
            let mut idle_busy_ticks: u32 = 0;
            // `#clearcontresume`: consecutive idle-prompt polls observed while a
            // manual clear cooldown is active and a go-mode head is waiting.
            // Used to debounce the cooldown auto-expiry so a resumed drain never
            // injects into an in-flight `/clear`.
            let mut clear_cooldown_idle_ticks: u32 = 0;
            // `#qflood2`: after this watch sends its OWN `/clear` (an opt-in
            // context reset or a `/clear` queue head), block the next drain
            // trigger until the cleared pane settles to a fresh idle prompt for
            // `CLEAR_COOLDOWN_RESUME_IDLE_TICKS` consecutive polls. Without this
            // the trigger was injected into the still-in-flight `/clear` and the
            // harness saw one concatenated line (`/clear /agent-doc <FILE>`).
            // Tracked in memory because the supervisor's own clears never write
            // the manual cooldown marker.
            let mut awaiting_clear_settle = false;
            let mut clear_settle_idle_ticks: u32 = 0;
            // R3 (#ctlrecycle): capture this supervisor's launch binary identity (≈ the
            // installed binary at process start). A later `cargo install` makes
            // `current_binary_identity()` differ, marking this supervisor stale.
            let recycle_launch_identity = crate::project_controller::current_binary_identity().ok();
            let recycle_auto_enabled =
                crate::project_controller::supervisor_auto_recycle_enabled(&path);
            let recycle_grace = crate::project_controller::recycle_idle_grace();
            let mut recycle_stale_since: Option<std::time::Instant> = None;
            let mut recycle_detected_logged = false;
            // `#suprecyclestall` — set once a self-`execve` recycle has failed so we
            // do not re-attempt a hopeless hot-reload (which orphans the child / hangs
            // the pane) on every later idle boundary. The supervisor keeps running on
            // its current binary until the operator restarts it.
            let mut reexec_recycle_disabled = false;
            // `#supautoinstall`: dogfood auto-install rung that PRECEDES the recycle rung.
            // When this supervisor hosts an agent-doc session editing agent-doc's OWN
            // source and a finalize committed an edit, build+install at the idle boundary
            // so the installed binary catches up — then the recycle block below hot-reloads
            // onto it. Resolved ONCE: a disabled path does zero extra work (no source
            // walk, no crate-root probe), and only an agent-doc dogfood document may
            // resolve a crate root.
            // `#agentreloadrestart`: capture the harness this supervisor launched
            // with + the agent-change-restart knob. A later frontmatter `agent:`
            // change (e.g. claude->opencode) makes the freshly-resolved harness
            // differ; at a quiet dispatch-ready boundary the watch logs the
            // detected change + the gate decision (`harness_change_detected` /
            // `agent_restart_boundary_gate`) so the operator can prove detection
            // live. Executing the restart (Phase 1b) is the focused-cycle wiring.
            let agent_change_restart_enabled =
                crate::project_controller::agent_change_restart_enabled(&path);
            let launch_harness_binary = harness.binary.clone();
            let mut agent_change_logged_for: Option<String> = None;
            let auto_install_enabled =
                crate::project_controller::supervisor_auto_install_enabled(&path);
            let install_crate_root = if auto_install_enabled {
                crate::project_controller::dogfood_agent_doc_crate_root(&path)
            } else {
                None
            };
            let mut install_stale_since: Option<std::time::Instant> = None;
            let mut install_detected_logged = false;
            // `#supautoinstall` — one-shot latch: a failed build must not be re-attempted
            // every idle boundary (it would block the watch on a hopeless multi-minute
            // build each tick). After one failure the supervisor logs once and leaves the
            // refresh to the operator, exactly like `reexec_recycle_disabled`.
            let mut auto_install_disabled = false;
            // `#supselfheal` Phase 3 (`#supselfheal-reexecescalate`): set true once an
            // in-place `execve` recycle has failed, so the recycle policy escalates to
            // a bounded kill+relaunch (`EscalateKillRelaunch`) instead of re-logging
            // `continue_current_binary` on every later idle boundary. The escalation
            // counter caps the kill+relaunches (`MAX_REEXEC_ESCALATIONS`) so a relaunch
            // that never clears the staleness cannot spin into an unbounded kill loop.
            let mut reexec_failed = false;
            let mut reexec_escalation_attempts: u32 = 0;
            let mut reexec_escalation_exhausted_logged = false;
            // `#wd40` / `#staleloop-recycle-restart`: one-shot log latch for the
            // stale-binary recycle-yield request. A continuously self-draining
            // `/loop` holds the harness `turn_active` back-to-back so this
            // supervisor never reaches its own recycle boundary; when it is stale
            // AND that loop owns the drain we write a short-TTL recycle-yield
            // request that the in-session loop reads at its next inter-item
            // boundary and yields, letting the `execve` recycle fire on its own.
            // The request file is refreshed every tick while the condition holds;
            // the log line fires once so the watch loop stays quiet.
            let mut recycle_yield_requested_logged = false;
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
                if active_head.is_none() {
                    context_reset_in_flight = false;
                    last_context_reset_head = None;
                }
                let prompt_visible = idle_queue_prompt_visible(&shared, &harness);
                let turn_active = turn_active_for_owned_pane(&path, &shared);

                // `#agentreloadrestart` Phase 1a: detect a frontmatter `agent:`
                // change and log the boundary-gate decision. Re-resolve the
                // harness from CURRENT frontmatter and compare to the one this
                // supervisor launched with; on a change (deduped per new harness),
                // emit `harness_change_detected` + `agent_restart_boundary_gate`.
                // The restart execution itself (spawn the new harness fresh) is the
                // focused-cycle Phase 1b wiring; until then a detected change is
                // surfaced so `agent:` edits are observable + operator-verifiable.
                if agent_change_restart_enabled
                    && let Ok(content) = std::fs::read_to_string(&path)
                    && let Ok((fm, _)) = crate::frontmatter::parse(&content)
                {
                    let global = crate::config::load().unwrap_or_default();
                    let resolved = crate::harness::HarnessConfig::from_context(&fm, &global);
                    let harness_changed = resolved.binary != launch_harness_binary;
                    let already_logged =
                        agent_change_logged_for.as_deref() == Some(resolved.binary.as_str());
                    if harness_changed && !already_logged {
                        let decision = crate::start::decisions::agent_change_restart_decision(
                            harness_changed,
                            agent_change_restart_enabled,
                            prompt_visible,
                            turn_active,
                        );
                        crate::ops_log::log_op(
                            &path,
                            &format!(
                                "harness_change_detected file={} old={} new={} gate={:?}",
                                path.display(),
                                launch_harness_binary,
                                resolved.binary,
                                decision,
                            ),
                        );
                        log_event(
                            &mut session_log,
                            &format!(
                                "agent_restart_boundary_gate old={} new={} decision={:?} prompt_visible={} turn_active={} note=phase1b_execution_pending",
                                launch_harness_binary,
                                resolved.binary,
                                decision,
                                prompt_visible,
                                turn_active,
                            ),
                        );
                        // Dedupe: only re-log when the resolved harness changes
                        // again (avoid per-tick spam for a standing change).
                        agent_change_logged_for = Some(resolved.binary.clone());
                    }
                }
                let route_submit_in_flight =
                    match crate::route_in_flight::route_submit_in_flight(&path) {
                        Ok(active) => active,
                        Err(err) => {
                            if !route_submit_in_flight_logged {
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_skipped harness={} reason=route_submit_marker_error file={} error={:?}",
                                        harness.binary,
                                        path.display(),
                                        err.to_string()
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] idle-queue watch: failed to inspect route-submit marker for {}: {err:#}",
                                    path.display()
                                );
                                route_submit_in_flight_logged = true;
                            }
                            true
                        }
                    };
                if route_submit_in_flight {
                    if !route_submit_in_flight_logged {
                        log_event(
                            &mut session_log,
                            &format!(
                                "idle_queue_watch_skipped harness={} reason=route_submit_in_flight file={}",
                                harness.binary,
                                path.display()
                            ),
                        );
                        crate::ops_log::log_op(
                            &path,
                            &format!(
                                "idle_queue_watch_skipped file={} harness={} reason=route_submit_in_flight",
                                path.display(),
                                harness.binary
                            ),
                        );
                        route_submit_in_flight_logged = true;
                    }
            } else {
                route_submit_in_flight_logged = false;
            }
            let context_clear_marker =
                match crate::context_clear_in_flight::context_clear_in_flight(&path) {
                    Ok(marker) => marker,
                    Err(err) => {
                        log_event(
                            &mut session_log,
                            &format!(
                                "idle_queue_watch_skipped harness={} reason=context_clear_marker_error file={} error={:?}",
                                harness.binary,
                                path.display(),
                                err.to_string()
                            ),
                        );
                        eprintln!(
                            "[agent-doc] idle-queue watch: failed to inspect context-clear marker for {}: {err:#}",
                            path.display()
                        );
                        None
                    }
            };
            let context_clear_pending = context_clear_marker.as_ref().and_then(|marker| {
                supervisor_pane_payload_already_pending(&shared, &marker.command, &harness)
            });
            if let Some(marker) = context_clear_marker.as_ref() {
                context_reset_in_flight = true;
                awaiting_clear_settle = true;
                last_context_clear_at = Some(marker.written_at);
            }
            // `#qflood2`: advance the post-`/clear` settle debounce. Require
            // consecutive fresh-idle polls (reset on any busy/non-idle tick)
            // so the next drain trigger is never injected into an in-flight
            // `/clear`. Once the cleared pane has settled, drop the gate so
            // the normal drain dispatches the head.
                if awaiting_clear_settle
                && prompt_visible
                && !turn_active
                && !route_submit_in_flight
                && !drain_dispatch_dedup_skip(context_clear_pending)
            {
                clear_settle_idle_ticks = clear_settle_idle_ticks.saturating_add(1);
            } else {
                clear_settle_idle_ticks = 0;
            }
            let clear_settled_now =
                awaiting_clear_settle && clear_settle_idle_ticks >= CLEAR_COOLDOWN_RESUME_IDLE_TICKS;
            if let Some(marker) = context_clear_marker.as_ref() {
                let marker_key = marker
                    .head_sha256
                    .clone()
                    .unwrap_or_else(|| marker.written_at.to_string());
                let resubmit_key = format!("context_clear_marker:{marker_key}");
                match crate::start::decisions::idle_queue_context_clear_in_flight_decision(
                    IdleQueueContextClearInFlightFacts {
                        marker_active: true,
                        prompt_visible,
                        turn_active,
                        route_submit_in_flight,
                        clear_already_pending: context_clear_pending,
                        already_resubmitted: last_pending_enter_resubmitted.as_deref()
                            == Some(resubmit_key.as_str()),
                        settled_idle_ticks: clear_settle_idle_ticks,
                        settle_threshold: CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
                    },
                ) {
                    IdleQueueContextClearInFlightDecision::ResubmitPendingClear => {
                        let head_label = active_head
                            .as_deref()
                            .or(marker.head_sha256.as_deref())
                            .unwrap_or("<unknown>");
                        match idle_queue_resubmit_pending_payload(
                            &path,
                            &shared,
                            &harness,
                            "context_clear",
                            head_label,
                            &marker.command,
                        ) {
                            AutoTriggerOutcome::Sent => {
                                last_pending_enter_resubmitted = Some(resubmit_key);
                                context_reset_in_flight = true;
                                awaiting_clear_settle = true;
                                clear_settle_idle_ticks = 0;
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_context_clear_marker_resubmit harness={} reason=clear_already_pending target={} cmd=\"{}\"",
                                        harness.binary, marker.target, marker.command
                                    ),
                                );
                                continue;
                            }
                            AutoTriggerOutcome::Cancelled => return,
                            outcome => {
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "idle_queue_watch_context_clear_marker_resubmit_failed harness={} outcome={}",
                                        harness.binary,
                                        outcome.as_str()
                                    ),
                                );
                                continue;
                            }
                        }
                    }
                    IdleQueueContextClearInFlightDecision::WaitForIdle
                    | IdleQueueContextClearInFlightDecision::WaitForPendingClear
                    | IdleQueueContextClearInFlightDecision::AwaitSettle => continue,
                    IdleQueueContextClearInFlightDecision::Settled => {
                        if let Err(err) =
                            crate::context_clear_in_flight::clear_context_clear_in_flight(&path)
                        {
                            eprintln!(
                                "[agent-doc] idle-queue watch: failed to clear context-clear marker for {}: {err:#}",
                                path.display()
                            );
                            crate::ops_log::log_op(
                                &path,
                                &format!(
                                    "idle_queue_watch_context_clear_marker_clear_failed file={} error={:?}",
                                    path.display(),
                                    err.to_string()
                                ),
                            );
                        } else {
                            crate::ops_log::log_op(
                                &path,
                                &format!(
                                    "idle_queue_watch_context_clear_marker_cleared file={} harness={} after_ticks={}",
                                    path.display(),
                                    harness.binary,
                                    CLEAR_COOLDOWN_RESUME_IDLE_TICKS
                                ),
                            );
                        }
                    }
                    IdleQueueContextClearInFlightDecision::Ignore => {}
                }
            }
            if clear_settled_now {
                awaiting_clear_settle = false;
                clear_settle_idle_ticks = 0;
                log_event(
                    &mut session_log,
                    &format!(
                            "idle_queue_watch_clear_settled harness={} after_ticks={}",
                            harness.binary, CLEAR_COOLDOWN_RESUME_IDLE_TICKS
                        ),
                    );
                }

                // `#clearcontresume`: a lingering manual clear cooldown must not
                // suppress an active go-mode queue drain forever. The cooldown's
                // only job is to avoid dispatching into an in-flight `/clear`;
                // once the cleared pane has settled to a fresh idle prompt for
                // `CLEAR_COOLDOWN_RESUME_IDLE_TICKS` consecutive polls and a head
                // is waiting (and no operator-deferred clear is pending — that
                // path owns its own resume), auto-expire the cooldown so the
                // recycle + clear is a continuation step, not a stall.
                if clear_cooldown_active
                    && active_head.is_some()
                    && prompt_visible
                    && !turn_active
                    && !route_submit_in_flight
                {
                    clear_cooldown_idle_ticks = clear_cooldown_idle_ticks.saturating_add(1);
                } else {
                    clear_cooldown_idle_ticks = 0;
                }
                let deferred_operator_clear_pending =
                    crate::queue_continuation::read_deferred_operator_clear(&path)
                        .unwrap_or(None)
                        .is_some();
                if clear_cooldown_resume_ready(
                    clear_cooldown_active,
                    active_head.is_some(),
                    prompt_visible,
                    turn_active,
                    deferred_operator_clear_pending,
                    clear_cooldown_idle_ticks,
                    CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
                ) {
                    match crate::queue_continuation::clear_cooldown_marker(&path) {
                        Ok(()) => {
                            clear_cooldown_idle_ticks = 0;
                            clear_cooldown_logged = false;
                            last_dispatched = None;
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_watch_clear_cooldown_resumed harness={} head={:?} after_ticks={}",
                                    harness.binary,
                                    active_head.as_deref().unwrap_or("<unknown>"),
                                    CLEAR_COOLDOWN_RESUME_IDLE_TICKS
                                ),
                            );
                            crate::ops_log::log_op(
                                &path,
                                &format!(
                                    "idle_queue_watch_clear_cooldown_resumed file={} harness={} head={:?}",
                                    path.display(),
                                    harness.binary,
                                    active_head.as_deref().unwrap_or("<unknown>")
                                ),
                            );
                            eprintln!(
                                "[agent-doc] idle-queue watch: clear cooldown settled — resuming active go-mode queue drain for {}",
                                path.display()
                            );
                            // Re-evaluate next tick with the cooldown cleared so
                            // the normal drain decision dispatches the head.
                            continue;
                        }
                        Err(err) => {
                            eprintln!(
                                "[agent-doc] idle-queue watch: failed to drop clear cooldown marker for {}: {err:#}",
                                path.display()
                            );
                            crate::ops_log::log_op(
                                &path,
                                &format!(
                                    "idle_queue_watch_clear_cooldown_resume_failed file={} error={:?}",
                                    path.display(),
                                    err.to_string()
                                ),
                            );
                        }
                    }
                }

                // R3 (#ctlrecycle) / #suprecyclequeue / #supselfheal: recycle this
                // supervisor onto a freshly-installed binary at a turn boundary. On unix
                // the recycle is a blue/green `execve`-preserve-child hot-reload that
                // keeps the live agent child + pane, so it now defaults ON
                // (`resolve_supervisor_auto_recycle`): a stale supervisor self-retires at
                // the next turn/queue boundary instead of re-filing File Cache Conflict /
                // IPC-drift dialogs forever. Operators opt out with a falsey
                // `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` / frontmatter / project knob, which
                // drops back to surfacing staleness once (`Detect`) for a deliberate
                // restart. A turn boundary is a dispatch-ready prompt with no turn
                // running. When a queue
                // head is still waiting to drain, the *next* queue item is the
                // deliberate restart point, so we recycle immediately (the re-exec'd
                // image re-dispatches the pending head on the fresh binary); with no
                // head waiting we debounce so a brief idle gap between unrelated turns
                // never trips it.
                let turn_boundary = prompt_visible && !turn_active;
                let head_pending = active_head.is_some();
                // `#supkill-a` — graceful, idle-gated self-kill. An external driver
                // (the PCP / `admin kill-supervisor`) records a per-document request;
                // the supervisor honors it at a turn boundary by tearing down its
                // harness child and exiting cleanly (no relaunch — this is a kill, not
                // a recycle). A wedged supervisor that never reaches this point is
                // force-killed externally instead (`#supkill-b`). Checked before the
                // recycle decision so a kill request wins over a hot-reload.
                if crate::supervisor_selfkill::supervisor_self_kill_action(
                    crate::supervisor_selfkill::self_kill_requested(&path),
                    turn_boundary,
                ) {
                    let child_pid = shared.child_pid.load(Ordering::Relaxed);
                    log_event(
                        &mut session_log,
                        &format!(
                            "supervisor_self_killed file={} boundary=turn child_pid={} caller=self_watchdog",
                            path.display(),
                            child_pid,
                        ),
                    );
                    eprintln!(
                        "[agent-doc] supervisor self-kill requested; tearing down the harness child and exiting"
                    );
                    crate::supervisor_selfkill::clear_self_kill_request(&path);
                    #[cfg(unix)]
                    if child_pid > 0 {
                        unsafe {
                            libc::kill(child_pid as libc::pid_t, libc::SIGTERM);
                        }
                    }
                    std::process::exit(0);
                }
                // `#supautoinstall` — dogfood auto-install rung. Default ON, but only for
                // an agent-doc dogfood session document (`install_crate_root` resolved). When a
                // finalize has committed a source edit (source mtime > installed binary
                // mtime), build+install at this turn boundary so the binary catches up; the
                // recycle block immediately below then sees the now-newer binary
                // (`process_binary_is_stale`) and hot-reloads onto it. The build runs HERE,
                // in the idle supervisor — never in the finalize client — which is what
                // root-fixes the mid-session-install drift (`#no-mid-session-install`).
                if !auto_install_disabled
                    && let Some(crate_root) = install_crate_root.as_ref()
                {
                    {
                        let source_newer = match (
                            crate::project_controller::newest_crate_source_mtime_secs(crate_root),
                            crate::project_controller::current_binary_identity().ok(),
                        ) {
                            (Some(src), Some(bin)) => {
                                crate::project_controller::source_newer_than_installed_binary(
                                    src,
                                    bin.modified_secs,
                                )
                            }
                            // Fail-open: any unreadable source/binary → not newer.
                            _ => false,
                        };
                        let install_action = supervisor_install_action(
                            source_newer,
                            auto_install_enabled,
                            turn_boundary,
                        );
                        // A build is heavy → always debounce (no head-pending immediate
                        // path): a momentary idle gap mid-edit must never trip it.
                        let (do_install, next_install_since) =
                            crate::project_controller::recycle_debounce_decision(
                                matches!(install_action, SupervisorInstallAction::Install),
                                install_stale_since,
                                std::time::Instant::now(),
                                recycle_grace,
                            );
                        install_stale_since = next_install_since;
                        if matches!(install_action, SupervisorInstallAction::Detect)
                            && !install_detected_logged
                        {
                            install_detected_logged = true;
                            log_event(
                                &mut session_log,
                                &format!(
                                    "supervisor_source_newer_detected pane={} auto_install=opted_out hint=set_AGENT_DOC_SUPERVISOR_AUTO_INSTALL_or_run_dogfood_refresh crate_root={}",
                                    shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                    crate_root.display(),
                                ),
                            );
                            eprintln!(
                                "[agent-doc] agent-doc source is newer than the installed binary and supervisor auto-install is opted OUT; set AGENT_DOC_SUPERVISOR_AUTO_INSTALL=1 or run the dogfood refresh to rebuild+install"
                            );
                        }
                        if do_install {
                            install_stale_since = None;
                            log_event(
                                &mut session_log,
                                &format!(
                                    "supervisor_auto_install_started pane={} boundary=turn crate_root={}",
                                    shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                    crate_root.display(),
                                ),
                            );
                            crate::ops_log::log_op(
                                &path,
                                &format!(
                                    "supervisor_auto_install_started file={} crate_root={} caller=idle_watch",
                                    path.display(),
                                    crate_root.display(),
                                ),
                            );
                            eprintln!(
                                "[agent-doc] supervisor auto-install: rebuilding + installing agent-doc from {} so the next queue item runs on the freshly-committed source",
                                crate_root.display()
                            );
                            match crate::project_controller::run_supervisor_auto_install(crate_root) {
                                Ok(()) => {
                                    log_event(
                                        &mut session_log,
                                        "supervisor_auto_install_succeeded next=recycle_onto_fresh_binary",
                                    );
                                    crate::ops_log::log_op(
                                        &path,
                                        &format!(
                                            "supervisor_auto_install_succeeded file={} next=recycle_onto_fresh_binary",
                                            path.display(),
                                        ),
                                    );
                                    eprintln!(
                                        "[agent-doc] supervisor auto-install succeeded; recycling onto the freshly-installed binary"
                                    );
                                }
                                Err(err) => {
                                    auto_install_disabled = true;
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "supervisor_auto_install_failed fallback=operator_refresh auto_install_disabled=true error={err}"
                                        ),
                                    );
                                    crate::ops_log::log_op(
                                        &path,
                                        &format!(
                                            "supervisor_auto_install_failed file={} fallback=operator_refresh error={:?}",
                                            path.display(),
                                            err.to_string(),
                                        ),
                                    );
                                    eprintln!(
                                        "[agent-doc] supervisor auto-install failed ({err}); leaving the rebuild to the operator (run the dogfood refresh) — not re-attempting this session"
                                    );
                                }
                            }
                        }
                    }
                }
                let supervisor_stale = crate::project_controller::process_binary_is_stale(
                    recycle_launch_identity.as_ref(),
                );
                // `#supkill-bg` — publish the live staleness probe so the IPC `Restart`
                // handler can decide drain-reexec vs immediate relaunch without
                // recomputing it.
                shared
                    .binary_stale
                    .store(supervisor_stale, Ordering::Relaxed);
                // `#supkill-bg` blue/green drain-and-supersede: an explicit
                // `restart-supervisor` routed to the in-place reexec path
                // (`restart_reexec`, stamped stale by the IPC handler) drains its
                // in-flight turn, then hot-reloads onto the fresh binary IN PLACE at
                // the turn boundary — the default healthy restart that fixes the
                // `generation closed` / stale-supervisor `#fcc0` case without dropping
                // the live turn. Checked before the opt-in auto-recycle decision so a
                // deliberate operator restart always supersedes (no env opt-in needed).
                let restart_action = supervisor_restart_action(
                    shared.restart_requested.load(Ordering::Relaxed),
                    shared.restart_reexec.load(Ordering::Relaxed),
                    turn_boundary,
                );
                if !reexec_recycle_disabled
                    && matches!(restart_action, SupervisorRestartAction::ReexecInPlace)
                {
                    #[cfg(unix)]
                    {
                        let candidate_notes = supervisor_reexec_candidates()
                            .iter()
                            .map(|(path, note)| format!("{note}={}", path.display()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_restart_drain_reexec via=execve_preserve_child boundary=turn pane={} child_pid={} master_fd={} current_exe={:?} candidates=[{candidate_notes}]",
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                shared.child_pid.load(Ordering::Relaxed),
                                shared.master_fd.load(Ordering::Relaxed),
                                std::env::current_exe().ok(),
                            ),
                        );
                        crate::ops_log::log_op(
                            &path,
                            &format!(
                                "supervisor_restart_drain_reexec file={} pane={} action=drain_and_supersede caller=operator",
                                path.display(),
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                            ),
                        );
                        eprintln!(
                            "[agent-doc] supervisor restart: draining complete, hot-reloading onto freshly-installed agent-doc binary; preserving the live agent child via execve"
                        );
                        match supervisor_perform_reexec(&shared) {
                            Ok(never) => match never {},
                            Err(err) => {
                                // A failed execve must NOT strand the restart. Clear the
                                // reexec intent so the in-process host loop's restart-kill
                                // condition fires and relaunches the child on the current
                                // binary (the restart still happens, pane survives), and
                                // disable further reexec attempts this lifetime.
                                shared.restart_reexec.store(false, Ordering::Relaxed);
                                reexec_recycle_disabled = true;
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "supervisor_restart_reexec_failed fallback=relaunch_current_binary error={err}"
                                    ),
                                );
                                crate::ops_log::log_op(
                                    &path,
                                    &format!(
                                        "supervisor_restart_reexec_failed file={} pane={} fallback=relaunch_current_binary error={:?}",
                                        path.display(),
                                        shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                        err.to_string(),
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] supervisor restart execve hot-reload failed ({err}); relaunching on the current binary"
                                );
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        // No execve on non-unix: fall back to the normal relaunch.
                        shared.restart_reexec.store(false, Ordering::Relaxed);
                    }
                }
                // `#supselfheal` Phase 1: the policy owner now accepts an
                // `explicit_admin` override (an `agent-doc admin recycle` request
                // recycles a stale supervisor even when auto-recycle is opted OUT).
                // The `admin recycle` → route-owned-supervisor IPC adapter that flips
                // this to `true` is the queued follow-up (`#supselfheal-adminwire`);
                // until it lands the idle path keeps its current behavior.
                let explicit_admin_recycle = false;
                // `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`): read the
                // persisted editor-IPC wedge fact for the owned document. The
                // write/converge closeout path latches `degraded` after repeated
                // `send_failed`/`no_ack` against a nominally-active JB listener and
                // logs `write_wedged_supervisor_recycle_requested`; the supervisor
                // reads that latch here and combines it with its own staleness probe,
                // so a wedge against a stale binary recycles immediately instead of
                // waiting for an opt-in or an idle boundary that may never come.
                let write_wedged = path
                    .canonicalize()
                    .ok()
                    .map(|canonical| {
                        let project_root =
                            crate::write::resolve_ipc_project_root_pub(&canonical);
                        crate::write::editor_ipc_write_wedged(&project_root, &canonical)
                    })
                    .unwrap_or(false);
                let recycle_action = supervisor_recycle_action(
                    supervisor_stale,
                    recycle_auto_enabled,
                    turn_boundary,
                    head_pending,
                    explicit_admin_recycle,
                    write_wedged,
                    reexec_failed,
                );
                // `#wd40` / `#staleloop-recycle-restart`: automate the manual
                // `make install` + `admin recycle` + end-turn the operator had to
                // run to force a stale supervisor onto a fresh binary during a
                // continuously self-draining session. The recycle above only fires
                // at a `turn_boundary` (`prompt_visible && !turn_active`), but a
                // self-driving `/loop` holds a fresh drain-owner lease AND keeps
                // the harness `turn_active` back-to-back, so the boundary is never
                // reached. When this supervisor is stale, a loop owns the drain,
                // and a recycle WOULD fire at a boundary, write a short-TTL
                // recycle-yield request: the in-session loop reads it at its next
                // inter-item boundary (via `queue_continuation::detect` /
                // `session-check` / preflight), yields one boundary instead of
                // re-triggering, and the resulting idle turn lets the `execve`
                // recycle fire on its own. After the recycle the fresh supervisor
                // (no longer stale) clears the request and the drain resumes.
                {
                    let drain_owner_active = crate::drain_owner::fresh_drain_owner_lease(
                        &file,
                        current_epoch_secs(),
                    )
                    .is_some();
                    // Would a real recycle (or the Phase-3 escalation) fire if the
                    // boundary were reachable? A bare `Detect` (auto-recycle opted
                    // out, no admin/wedge) does NOT hot-reload, so yielding the loop
                    // for it would only stall the drain.
                    let would_recycle_at_boundary = !matches!(
                        supervisor_recycle_action(
                            supervisor_stale,
                            recycle_auto_enabled,
                            true,
                            head_pending,
                            explicit_admin_recycle,
                            write_wedged,
                            reexec_failed,
                        ),
                        SupervisorRecycleAction::None | SupervisorRecycleAction::Detect
                    );
                    // Never yield-loop a supervisor that can no longer converge: once
                    // the bounded kill+relaunch escalation is exhausted the recycle
                    // will never fire, so asking the loop to keep yielding would stall
                    // the drain forever. Fall through to the one-time operator-restart
                    // hint instead (`#supselfheal` Phase 3 exhaustion).
                    if !reexec_escalation_exhausted_logged
                        && stale_drain_recycle_yield_requested(
                            would_recycle_at_boundary,
                            drain_owner_active,
                            turn_boundary,
                        )
                    {
                        // Refresh the request every tick so it stays live until the
                        // loop yields; log once.
                        if let Err(err) = crate::recycle_yield::request_recycle_yield(
                            &file,
                            crate::recycle_yield::RECYCLE_YIELD_STALE_BINARY,
                        ) {
                            eprintln!(
                                "[agent-doc] idle-queue watch: failed to write recycle-yield request for {}: {err:#}",
                                path.display()
                            );
                        } else if !recycle_yield_requested_logged {
                            recycle_yield_requested_logged = true;
                            log_event(
                                &mut session_log,
                                &format!(
                                    "supervisor_recycle_yield_requested pane={} reason=stale_binary_drain turn_active={} drain_owner=loop note=loop_yields_to_let_execve_recycle_fire",
                                    shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                    turn_active,
                                ),
                            );
                            crate::ops_log::log_op(
                                &path,
                                &format!(
                                    "supervisor_recycle_yield_requested file={} pane={} reason=stale_binary_drain action=signal_loop_yield",
                                    path.display(),
                                    shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                ),
                            );
                            eprintln!(
                                "[agent-doc] supervisor binary is stale while a self-draining loop owns the drain; requesting the in-session loop to yield one boundary so the recycle can hot-reload onto the fresh binary"
                            );
                        }
                    } else if !supervisor_stale {
                        // Post-recycle (or no longer stale): drop any leftover
                        // request so the loop resumes draining on the fresh binary.
                        // Reset the log latch so a later staleness can re-request.
                        crate::recycle_yield::clear_recycle_yield(&file);
                        recycle_yield_requested_logged = false;
                    }
                }
                // `#supselfheal` Phase 3 (`#supselfheal-reexecescalate`): a stale
                // supervisor whose in-place `execve` already failed cannot converge by
                // re-execing again. Escalate to a bounded kill+relaunch of the harness
                // child — reusing the `#supkill-bg` drain-and-relaunch path (request a
                // non-reexec restart; it drains the in-flight turn first, so no live
                // turn is dropped) — instead of looping `continue_current_binary`
                // forever (the indefinite wedge this plan fixes). Bounded so a relaunch
                // that never clears the staleness cannot spin into an unbounded kill
                // loop; past the bound, fall back to continuing on the current binary
                // with a one-time operator-restart hint.
                if matches!(recycle_action, SupervisorRecycleAction::EscalateKillRelaunch) {
                    if reexec_escalation_within_bound(
                        reexec_escalation_attempts,
                        MAX_REEXEC_ESCALATIONS,
                    ) {
                        reexec_escalation_attempts += 1;
                        shared.restart_reexec.store(false, Ordering::Relaxed);
                        shared.restart_requested.store(true, Ordering::Relaxed);
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_reexec_escalate_kill_relaunch pane={} attempt={}/{} reason=reexec_failed",
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                reexec_escalation_attempts,
                                MAX_REEXEC_ESCALATIONS,
                            ),
                        );
                        crate::ops_log::log_op(
                            &path,
                            &format!(
                                "supervisor_reexec_escalate_kill_relaunch file={} pane={} attempt={}/{} action=request_kill_relaunch reason=reexec_failed",
                                path.display(),
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                reexec_escalation_attempts,
                                MAX_REEXEC_ESCALATIONS,
                            ),
                        );
                        eprintln!(
                            "[agent-doc] supervisor in-place hot-reload failed; escalating to a kill+relaunch of the harness child (attempt {}/{}) to clear the wedge",
                            reexec_escalation_attempts, MAX_REEXEC_ESCALATIONS,
                        );
                    } else if !reexec_escalation_exhausted_logged {
                        reexec_escalation_exhausted_logged = true;
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_reexec_escalation_exhausted pane={} attempts={} fallback=continue_current_binary",
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                reexec_escalation_attempts,
                            ),
                        );
                        crate::ops_log::log_op(
                            &path,
                            &format!(
                                "supervisor_reexec_escalation_exhausted file={} pane={} attempts={} fallback=continue_current_binary",
                                path.display(),
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                reexec_escalation_attempts,
                            ),
                        );
                        eprintln!(
                            "[agent-doc] supervisor kill+relaunch escalation exhausted after {} attempts; continuing on the current binary — restart this session to pick up the new build",
                            reexec_escalation_attempts,
                        );
                        if let Some(pane) = shared.inject_pane.as_deref() {
                            let _ = std::process::Command::new("tmux")
                                .args([
                                    "display-message",
                                    "-t",
                                    pane,
                                    "agent-doc: binary hot-reload + relaunch failed; restart this session to pick up the new build",
                                ])
                                .status();
                        }
                    }
                    // The escalation owns this tick's recycle decision; the requested
                    // restart is consumed at the next tick's restart-action boundary.
                    continue;
                }
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
                            "supervisor_binary_stale_detected pane={} auto_recycle=opted_out hint=restart_or_unset_AGENT_DOC_SUPERVISOR_AUTO_RECYCLE",
                            shared.inject_pane.as_deref().unwrap_or("<pty>"),
                        ),
                    );
                    eprintln!(
                        "[agent-doc] supervisor is running a prior agent-doc binary and turn-boundary self-recycle is opted OUT; restart this session (or clear the falsey AGENT_DOC_SUPERVISOR_AUTO_RECYCLE / frontmatter / project knob) to pick up the new build"
                    );
                }
                let do_recycle = !reexec_recycle_disabled
                    && match recycle_action {
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
                        // Pre-attempt provenance so a recurring recycle failure is
                        // diagnosable even if the exec wedges: record the launch
                        // identity, current_exe, and the ordered candidate ladder
                        // the exec will walk.
                        let candidate_notes = supervisor_reexec_candidates()
                            .iter()
                            .map(|(path, note)| format!("{note}={}", path.display()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        log_event(
                            &mut session_log,
                            &format!(
                                "supervisor_binary_stale_self_recycled via=execve_preserve_child boundary={} pane={} child_pid={} master_fd={} current_exe={:?} candidates=[{candidate_notes}]",
                                recycle_boundary,
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                shared.child_pid.load(Ordering::Relaxed),
                                shared.master_fd.load(Ordering::Relaxed),
                                std::env::current_exe().ok(),
                            ),
                        );
                        crate::ops_log::log_op(
                            &path,
                            &format!(
                                "supervisor_binary_stale_self_recycled file={} pane={} boundary={} via=execve_preserve_child child_pid={} master_fd={} candidates=[{candidate_notes}]",
                                path.display(),
                                shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                recycle_boundary,
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
                                // `#suprecyclestall` — a failed execve must NOT kill
                                // the session. Previously we `process::exit(0)` here,
                                // which orphaned the live harness child and hung the
                                // tmux pane (no relaunch exists). Instead log the full
                                // per-candidate diagnostics, disable further recycle
                                // attempts for this supervisor (so we don't re-spam a
                                // hopeless execve every idle boundary), surface a
                                // one-time operator hint, and keep running on the
                                // current binary. The operator restarts deliberately
                                // to pick up the new build.
                                log_event(
                                    &mut session_log,
                                    &format!(
                                        "supervisor_reexec_failed fallback=continue_current_binary recycle_disabled=true error={err}"
                                    ),
                                );
                                crate::ops_log::log_op(
                                    &path,
                                    &format!(
                                        "supervisor_reexec_failed file={} pane={} boundary={} fallback=continue_current_binary error={:?}",
                                        path.display(),
                                        shared.inject_pane.as_deref().unwrap_or("<pty>"),
                                        recycle_boundary,
                                        err.to_string(),
                                    ),
                                );
                                eprintln!(
                                    "[agent-doc] supervisor execve hot-reload failed ({err}); escalating to a bounded kill+relaunch on the next idle boundary"
                                );
                                if let Some(pane) = shared.inject_pane.as_deref() {
                                    let _ = std::process::Command::new("tmux")
                                        .args([
                                            "display-message",
                                            "-t",
                                            pane,
                                            "agent-doc: binary hot-reload failed; escalating to kill+relaunch",
                                        ])
                                        .status();
                                }
                                reexec_recycle_disabled = true;
                                // `#supselfheal` Phase 3: the in-place execve cannot
                                // start (deleted inode / syscall error). Mark the
                                // failure so the recycle policy returns
                                // `EscalateKillRelaunch` next tick instead of sitting
                                // forever on `continue_current_binary`.
                                reexec_failed = true;
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
                                awaiting_clear_settle = true;
                                context_reset_in_flight = true;
                                clear_settle_idle_ticks = 0;
                                if let Some(head) = active_head.clone() {
                                    last_context_reset_head = Some(head);
                                }
                                last_context_clear_at = Some(current_epoch_secs());
                                record_context_clear_prompt_for_hooks(
                                    &shared,
                                    &path,
                                    &harness,
                                    &clear_cmd,
                                );
                                record_context_clear_in_flight_marker(
                                    &path,
                                    &shared,
                                    &harness,
                                    &clear_cmd,
                                    active_head.as_deref(),
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

                // `#cleandrainsup`: a `[clean-session]` head asks for a fresh agent
                // context regardless of the global opt-in. Ordinary Codex heads may
                // also clear here when the project opted into queue context reset and
                // the Codex transcript/accretion decision requires fresh context.
                // The reset decision still runs through prompt/turn/route gates
                // below, so a live queue edit or in-flight route cannot churn clears.
                let active_head_is_clean_session = active_head
                    .as_deref()
                    .map(|head| crate::queue_continuation::head_requires_clean_session(&path, head))
                    .unwrap_or(false);
                let context_reset_reason = if crate::start::decisions::clean_session_head_forces_context_reset(
                    active_head_is_clean_session,
                    clear_cooldown_active,
                ) {
                    context_reset_policy_error_logged = false;
                    Some(CLEAN_SESSION_CONTEXT_RESET_REASON.to_string())
                } else if harness.binary == "codex" {
                    match crate::codex_hook::codex_queue_context_reset_reason(
                        &path,
                        last_context_clear_at,
                    ) {
                        Ok(reason) => {
                            context_reset_policy_error_logged = false;
                            reason
                        }
                        Err(err) => {
                            if !context_reset_policy_error_logged {
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
                                    "[agent-doc] idle-queue watch: failed to resolve Codex queue context reset for {}: {err:#}",
                                    path.display()
                                );
                                context_reset_policy_error_logged = true;
                            }
                            None
                        }
                    }
                } else {
                    context_reset_policy_error_logged = false;
                    None
                };
                let reset_already_sent_for_active_slot = context_reset_dedupe_head(
                    active_head.as_deref(),
                    last_context_reset_head.as_deref(),
                    context_reset_in_flight,
                );
                match idle_queue_context_reset_decision(
                    prompt_visible,
                    turn_active,
                    route_submit_in_flight,
                    active_head.as_deref(),
                    reset_already_sent_for_active_slot,
                    context_reset_reason.is_some(),
                ) {
                    IdleQueueContextResetDecision::Reset => {
                        let head = active_head.as_deref().unwrap_or("<unknown>");
                        let clear_cmd = harness.context_clear_command();
                        // `#qflood2`: never stack a second `/clear` when one is
                        // already pending in the composer. For Enter-key
                        // profiles, a proven pending draft can be recovered by
                        // pressing Enter once instead of declaring the queue
                        // handled.
                        let clear_already_pending =
                            supervisor_pane_payload_already_pending(&shared, clear_cmd, &harness);
                        let resubmit_key = format!("context_reset:{head}");
                        if idle_queue_pending_payload_needs_enter_resubmit(
                            &harness.binary,
                            clear_already_pending,
                            last_pending_enter_resubmitted.as_deref()
                                == Some(resubmit_key.as_str()),
                        ) {
                            match idle_queue_resubmit_pending_payload(
                                &path,
                                &shared,
                                &harness,
                                "context_clear",
                                head,
                                clear_cmd,
                            ) {
                                AutoTriggerOutcome::Sent => {
                                    last_pending_enter_resubmitted = Some(resubmit_key);
                                    last_context_reset_head = active_head.clone();
                                    last_context_clear_at = Some(current_epoch_secs());
                                    context_reset_in_flight = true;
                                    awaiting_clear_settle = true;
                                    clear_settle_idle_ticks = 0;
                                    record_context_clear_in_flight_marker(
                                        &path,
                                        &shared,
                                        &harness,
                                        clear_cmd,
                                        Some(head),
                                    );
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_context_reset_resubmit harness={} reason=clear_already_pending head={:?}",
                                            harness.binary, head
                                        ),
                                    );
                                    continue;
                                }
                                AutoTriggerOutcome::Cancelled => return,
                                outcome => {
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_context_reset_resubmit_failed harness={} outcome={}",
                                            harness.binary,
                                            outcome.as_str()
                                        ),
                                    );
                                    continue;
                                }
                            }
                        }
                        if drain_dispatch_dedup_skip(clear_already_pending) {
                            last_context_reset_head = active_head.clone();
                            context_reset_in_flight = true;
                            awaiting_clear_settle = true;
                            clear_settle_idle_ticks = 0;
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_watch_context_reset_skipped harness={} reason=clear_already_pending head={:?}",
                                    harness.binary, head
                                ),
                            );
                            crate::ops_log::log_op(
                                &path,
                                &format!(
                                    "queue_dispatch_skipped file={} harness={} reason=clear_already_pending",
                                    path.display(),
                                    harness.binary
                                ),
                            );
                            continue;
                        }
                        match auto_trigger_clear_command(&shared, &stop, clear_cmd) {
                            AutoTriggerOutcome::Cancelled => return,
                            AutoTriggerOutcome::Sent => {
                                last_context_reset_head = active_head.clone();
                                last_context_clear_at = Some(current_epoch_secs());
                                context_reset_in_flight = true;
                                last_dispatched = None;
                                awaiting_clear_settle = true;
                                clear_settle_idle_ticks = 0;
                                // `#qcontdrain` retired the `#freshgrant` fresh-context
                                // grant: the in-session `/loop` now drains
                                // `[clean-session]` heads in place, so the supervisor no
                                // longer needs to write a grant to un-defer the
                                // freshly-cleared agent. The force-`/clear` for a
                                // clean-session head (`#cleandrainsup`) still happens
                                // above (`active_head_is_clean_session`); it just no
                                // longer records a grant sidecar.
                                record_context_clear_prompt_for_hooks(
                                    &shared,
                                    &path,
                                    &harness,
                                    clear_cmd,
                                );
                                record_context_clear_in_flight_marker(
                                    &path,
                                    &shared,
                                    &harness,
                                    clear_cmd,
                                    Some(head),
                                );
                                log_idle_queue_context_reset_submit(
                                    &path,
                                    &shared,
                                    &harness,
                                    clear_cmd,
                                    head,
                                    context_reset_reason.as_deref().unwrap_or(""),
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
                    | IdleQueueContextResetDecision::SkipRouteSubmitInFlight
                    | IdleQueueContextResetDecision::SkipAlreadyResetHead
                    | IdleQueueContextResetDecision::SkipNoResetNeeded => {}
                }

                // `#qpausego`: an accepted `admin queue pause` records a durable
                // controller pause the dispatch RPC already honors, but the
                // idle-watch injects triggers straight into the pane and so
                // historically ignored it — a `go`-mode auto-queue kept
                // re-dispatching after an accepted pause (the unattended flood
                // this fixes). Defer the *supervisor's* auto-injection while the
                // pause is active. This suppresses only the unattended injector:
                // the attended in-session `/loop` continues to drain via
                // `queue_continuation_required` (a pause does not stall it).
                // `resume`/`drain` are not `paused` and do not block here.
                // Single-owner tie-break (computed ONCE, before the pause gate so the
                // gate and the drain decision agree): a fresh drain-owner lease means
                // either the Claude Code `/loop` auto-loop owns this drain (#kp5z /
                // #qflood) OR the supervisor's OWN failsafe claim from a recent tick is
                // still live (`#qstallguard` Layer C rate-limit). Either way the
                // supervisor must defer this tick rather than re-dispatching.
                let drain_owner_lease =
                    crate::drain_owner::fresh_drain_owner_lease(&file, current_epoch_secs());

                if active_head.is_some()
                    && crate::queue_continuation::document_queue_controller_paused(&path)
                {
                    // `#qstallguard` Layer C: pause throttles to single-owner; it does
                    // not disable the failsafe. Skip when a fresh lease exists (an
                    // in-session `/loop` owner OR our own recent failsafe claim — the
                    // rate-limit) or nothing is drainable. With NO fresh lease and a
                    // drainable head, claim the lease as `supervisor-failsafe` so this
                    // fires at most once per drain-owner TTL (single-owner cadence,
                    // NOT the per-tick #rt83 flood), then fall through to dispatch ONCE
                    // using the pre-claim lease value below.
                    if paused_idle_watch_should_skip(
                        true,
                        active_head.is_some(),
                        drain_owner_lease.is_some(),
                    ) {
                        log_event(
                            &mut session_log,
                            &format!(
                                "idle_queue_watch_drain_skipped harness={} reason=queue_control_paused file={}",
                                harness.binary,
                                path.display()
                            ),
                        );
                        crate::ops_log::log_op(
                            &path,
                            &format!(
                                "queue_dispatch_skipped file={} harness={} reason=queue_control_paused",
                                path.display(),
                                harness.binary
                            ),
                        );
                        continue;
                    }
                    // No fresh lease + drainable head: single-owner failsafe. Claim the
                    // lease NOW so the NEXT tick sees it fresh and defers (rate-limit).
                    if let Err(err) =
                        crate::drain_owner::refresh_drain_owner_lease(&file, "supervisor-failsafe")
                    {
                        eprintln!(
                            "[agent-doc] warning: failed to claim failsafe drain lease for {}: {err}",
                            path.display()
                        );
                    }
                    log_event(
                        &mut session_log,
                        &format!(
                            "idle_queue_watch_paused_failsafe_drain harness={} reason=queue_paused_no_loop_owner file={}",
                            harness.binary,
                            path.display()
                        ),
                    );
                    crate::ops_log::log_op(
                        &path,
                        &format!(
                            "queue_paused_failsafe_single_owner_drain file={} harness={} reason=no_in_session_loop_owner cadence=drain_owner_ttl (#qstallguard Layer C)",
                            path.display(),
                            harness.binary
                        ),
                    );
                    // fall through to the guarded drain decision below, which uses the
                    // PRE-CLAIM `drain_owner_lease` (None this tick) so it dispatches once.
                }

                // #sqedit-race Phase 2: a direct `queue prune-noise` / `queue
                // consume` is mid read-modify-write. Defer this tick so the
                // idle-watch never reads (and re-dispatches against) a torn
                // intermediate queue head. The lease is short-TTL, so this is a
                // brief yield: the edit settles and the next tick drains normally.
                if let Some(holder_pid) =
                    crate::queue_edit_owner::foreign_queue_edit_in_flight(&file)
                {
                    log_event(
                        &mut session_log,
                        &format!(
                            "idle_queue_watch_drain_skipped reason=queue_edit_in_flight holder_pid={holder_pid} (#sqedit-race)"
                        ),
                    );
                    continue;
                }
                match idle_queue_drain_decision(
                    clear_cooldown_active,
                    prompt_visible,
                    turn_active,
                    drain_owner_lease.is_some(),
                    route_submit_in_flight,
                    active_head.as_deref(),
                    last_dispatched.as_deref(),
                ) {
                    IdleQueueDrainDecision::Dispatch => {
                        // `#qstallguard` Layer B/C interaction: the supervisor idle-watch
                        // is itself continuing the drain (whether the queue is paused —
                        // the Layer C single-owner failsafe — or normal go-mode). That is
                        // NOT an in-session stall, so clear any continuation-pending marker
                        // the prior in-session closeout dropped. Otherwise the next drained
                        // agent's preflight would see the marker with no in-session drain
                        // lease and false-fire `queue_stall_detected` for a drain the
                        // supervisor is actively progressing.
                        crate::drain_stall::clear_continuation_pending(&file);
                        // `#qflood2`: hold the trigger until a just-sent `/clear`
                        // has settled, so it is never injected into the in-flight
                        // clear (the concatenated `/clear /agent-doc <FILE>`).
                        if drain_blocked_awaiting_clear_settle(
                            awaiting_clear_settle,
                            prompt_visible,
                            turn_active,
                            clear_settle_idle_ticks,
                            CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
                        ) {
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_watch_drain_skipped harness={} reason=awaiting_clear_settle settle_ticks={}",
                                    harness.binary, clear_settle_idle_ticks
                                ),
                            );
                            continue;
                        }
                        let head = active_head.expect("dispatch implies an active head");
                        let drain_payload = idle_queue_drain_payload(&file, &harness, &head);
                        let payload_kind = idle_queue_drain_payload_kind(&harness, &head);
                        let slash_command = idle_queue_head_slash_command(&head);
                        // `#qflood2`: never stack a trigger already pending in the
                        // composer. For Enter-key profiles, a proven-pending
                        // draft may simply be waiting for the bare submit key, so
                        // submit it once.
                        let payload_already_pending =
                            supervisor_pane_payload_already_pending(
                                &shared,
                                &drain_payload,
                                &harness,
                            );
                        let resubmit_key = format!("drain:{head}");
                        if idle_queue_pending_payload_needs_enter_resubmit(
                            &harness.binary,
                            payload_already_pending,
                            last_pending_enter_resubmitted.as_deref()
                                == Some(resubmit_key.as_str()),
                        ) {
                            match idle_queue_resubmit_pending_payload(
                                &path,
                                &shared,
                                &harness,
                                payload_kind,
                                &head,
                                &drain_payload,
                            ) {
                        AutoTriggerOutcome::Sent => {
                            last_pending_enter_resubmitted = Some(resubmit_key);
                            last_dispatched = Some(head.clone());
                            if slash_command
                                .as_deref()
                                .is_some_and(crate::queue_command::is_context_clear_command)
                            {
                                record_context_clear_in_flight_marker(
                                    &path,
                                    &shared,
                                    &harness,
                                    &drain_payload,
                                    Some(&head),
                                );
                            }
                            log_event(
                                &mut session_log,
                                &format!(
                                            "idle_queue_watch_drain_resubmit harness={} reason=trigger_already_pending payload_kind={}",
                                            harness.binary, payload_kind
                                        ),
                                    );
                                    continue;
                                }
                                AutoTriggerOutcome::Cancelled => return,
                                outcome => {
                                    log_event(
                                        &mut session_log,
                                        &format!(
                                            "idle_queue_watch_drain_resubmit_failed harness={} outcome={}",
                                            harness.binary,
                                            outcome.as_str()
                                        ),
                                    );
                                    continue;
                                }
                            }
                        }
                        if drain_dispatch_dedup_skip(payload_already_pending) {
                            last_dispatched = Some(head.clone());
                            log_event(
                                &mut session_log,
                                &format!(
                                    "idle_queue_watch_drain_skipped harness={} reason=trigger_already_pending payload_kind={}",
                                    harness.binary, payload_kind
                                ),
                            );
                            crate::ops_log::log_op(
                                &path,
                                &format!(
                                    "queue_dispatch_skipped file={} harness={} reason=trigger_already_pending payload_kind={}",
                                    path.display(),
                                    harness.binary,
                                    payload_kind
                                ),
                            );
                            continue;
                        }
                        match auto_trigger_submit_queue_command(&shared, &stop, &drain_payload) {
                            AutoTriggerOutcome::Sent => {
                                context_reset_in_flight = false;
                                if last_context_reset_head.as_deref() == Some(head.as_str())
                                    && !crate::queue_command::is_context_clear_command(
                                        &drain_payload,
                                    )
                                {
                                    log_between_turn_enqueue_delivery(
                                        &path,
                                        harness.context_clear_command(),
                                        &drain_payload,
                                    );
                                }
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
                                        last_context_reset_head = Some(head.clone());
                                        last_context_clear_at = Some(current_epoch_secs());
                                        context_reset_in_flight = true;
                                        // `#qflood2`: a dispatched `/clear` head
                                        // must settle before the next head's
                                        // trigger fires, same as the context-reset
                                        // path, so they cannot concatenate.
                                        awaiting_clear_settle = true;
                                        clear_settle_idle_ticks = 0;
                                        record_context_clear_prompt_for_hooks(
                                            &shared,
                                            &path,
                                            &harness,
                                            command,
                                        );
                                        record_context_clear_in_flight_marker(
                                            &path,
                                            &shared,
                                            &harness,
                                            command,
                                            Some(&head),
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
                        last_pending_enter_resubmitted = None;
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
                    | IdleQueueDrainDecision::SkipRouteSubmitInFlight
                    | IdleQueueDrainDecision::SkipClearCooldown
                    | IdleQueueDrainDecision::SkipAlreadyDispatched => {}
                }
            }
        })
        .expect("spawn idle-queue watch thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_failsafe_drains_only_when_no_loop_owner_holds_a_drainable_head() {
        // `#qstallguard` Layer C: pause defers to an in-session loop owner...
        assert!(
            paused_idle_watch_should_skip(true, true, true),
            "loop owner present → defer (skip)"
        );
        // ...skips when there is nothing drainable...
        assert!(
            paused_idle_watch_should_skip(true, false, false),
            "no drainable head → skip"
        );
        // ...but performs a single-owner failsafe drain when the loop abandoned the
        // drain and a drainable head remains (this is the strand the pause caused).
        assert!(
            !paused_idle_watch_should_skip(true, true, false),
            "paused + drainable + no loop owner → drain (do NOT skip)"
        );
        // An unpaused queue is never gated by this rule.
        assert!(!paused_idle_watch_should_skip(false, true, false));
        assert!(!paused_idle_watch_should_skip(false, false, true));
    }

    #[test]
    fn pending_payload_enter_resubmit_is_scoped_and_one_shot() {
        assert!(idle_queue_pending_payload_needs_enter_resubmit(
            "codex",
            Some(true),
            false
        ));
        assert!(idle_queue_pending_payload_needs_enter_resubmit(
            "claude",
            Some(true),
            false
        ));
        assert!(idle_queue_pending_payload_needs_enter_resubmit(
            "opencode",
            Some(true),
            false
        ));
        assert!(!idle_queue_pending_payload_needs_enter_resubmit(
            "codex",
            Some(false),
            false
        ));
        assert!(!idle_queue_pending_payload_needs_enter_resubmit(
            "codex", None, false
        ));
        assert!(!idle_queue_pending_payload_needs_enter_resubmit(
            "codex",
            Some(true),
            true
        ));
    }

    #[test]
    fn context_reset_in_flight_dedupes_active_head_edits() {
        let edited_head = Some("operator is still typing the active queue head");
        let previous_head = Some("operator is still typing");

        assert_eq!(
            context_reset_dedupe_head(edited_head, previous_head, true),
            edited_head
        );
        assert_eq!(
            crate::start::decisions::idle_queue_context_reset_decision(
                true,
                false,
                false,
                edited_head,
                context_reset_dedupe_head(edited_head, previous_head, true),
                true,
            ),
            IdleQueueContextResetDecision::SkipAlreadyResetHead
        );
        assert_eq!(
            crate::start::decisions::idle_queue_context_reset_decision(
                true,
                false,
                false,
                edited_head,
                context_reset_dedupe_head(edited_head, previous_head, false),
                true,
            ),
            IdleQueueContextResetDecision::Reset
        );
    }

    #[test]
    fn codex_opted_in_context_reset_dispatches_for_ordinary_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_queue_context_reset = true\n",
        )
        .unwrap();
        std::fs::write(&doc, "doc\n").unwrap();
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();

        let head = "ordinary queue head";
        let reason = crate::codex_hook::codex_queue_context_reset_reason(&doc, None)
            .unwrap()
            .expect("opted-in compaction should require fresh Codex context");
        assert!(
            reason.contains("exchange was compacted"),
            "unexpected reset reason: {reason}"
        );
        assert_eq!(
            crate::start::decisions::idle_queue_context_reset_decision(
                true,
                false,
                false,
                Some(head),
                None,
                true,
            ),
            IdleQueueContextResetDecision::Reset
        );

        let harness = crate::harness::HarnessConfig::codex();
        let shared = SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            "codex",
            None,
            None,
            Some("%25".to_string()),
        );
        log_idle_queue_context_reset_submit(&doc, &shared, &harness, "/clear", head, &reason);

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("idle_queue_watch_context_reset")
                && ops_log.contains("head_sha256=")
                && ops_log.contains("exchange was compacted"),
            "ordinary opted-in reset should leave context-reset proof:\n{ops_log}"
        );
    }

    #[test]
    fn clean_session_reset_ops_log_precedes_drain_submit() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(&doc, "doc\n").unwrap();

        let head = "do [#cleandrainsup-agent]";
        let harness = crate::harness::HarnessConfig::codex();
        let shared = SupervisorShared::with_actor_runtime(
            "test",
            "test-instance".to_string(),
            "codex",
            None,
            None,
            Some("%25".to_string()),
        );

        assert!(crate::start::decisions::clean_session_head_forces_context_reset(true, false,));
        assert_eq!(
            crate::start::decisions::idle_queue_context_reset_decision(
                true,
                false,
                false,
                Some(head),
                None,
                true,
            ),
            IdleQueueContextResetDecision::Reset
        );

        log_idle_queue_context_reset_submit(
            &doc,
            &shared,
            &harness,
            "/clear",
            head,
            CLEAN_SESSION_CONTEXT_RESET_REASON,
        );

        assert!(
            crate::start::decisions::drain_blocked_awaiting_clear_settle(
                true,
                true,
                false,
                CLEAR_COOLDOWN_RESUME_IDLE_TICKS - 1,
                CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
            )
        );
        assert!(
            !crate::start::decisions::drain_blocked_awaiting_clear_settle(
                true,
                true,
                false,
                CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
                CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
            )
        );
        assert_eq!(
            crate::start::decisions::idle_queue_drain_decision(
                false,
                true,
                false,
                false,
                false,
                Some(head),
                None,
            ),
            IdleQueueDrainDecision::Dispatch
        );
        log_idle_queue_drain_submit(
            &doc,
            &shared,
            &harness,
            "trigger",
            head,
            &format!("agent-doc {}", doc.display()),
        );
        log_between_turn_enqueue_delivery(&doc, "/clear", &format!("agent-doc {}", doc.display()));

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        let reset_pos = ops_log
            .find("idle_queue_watch_context_reset")
            .expect("reset marker logged");
        let drain_pos = ops_log
            .find("idle_queue_watch_drain")
            .expect("drain marker logged");
        assert!(
            reset_pos < drain_pos,
            "clean-session reset must be observable before drain: {ops_log}"
        );
        assert!(ops_log.contains("reason=\"active queue head is a [clean-session] item"));
        assert!(ops_log.contains("#cleandrainsup"));
        assert!(ops_log.contains("head_sha256="));
        assert!(ops_log.contains("target=%25"));
        assert!(
            ops_log
                .contains("between_turn_enqueue deduped=0 kept=/clear,/agent-doc result=delivered")
        );
    }
}
