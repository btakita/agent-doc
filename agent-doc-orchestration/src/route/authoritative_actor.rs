//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn load_authoritative_actor_binding(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    respect_tracked_clear_restart: bool,
    enforce_capability_proof: bool,
) -> Result<Option<AuthoritativeActorDispatchTarget>> {
    if respect_tracked_clear_restart
        && tracked_harness_clear_requires_fresh_restart(
            harness,
            crate::codex_hook::load_latest_prompt_for_file(file)?.as_deref(),
        )
    {
        return Ok(None);
    }

    let base_dir = registry_base_dir_for_dispatch(file_path);
    let Some(record) = crate::project_controller::authoritative_actor_binding(&base_dir, file)?
    else {
        return Ok(None);
    };
    if record.session_id != session_id {
        anyhow::bail!(
            "authoritative actor record for {} is bound to session {}, not {}",
            file.display(),
            record.session_id,
            session_id
        );
    }
    if !tmux.pane_alive(&record.pane_id) {
        return Ok(None);
    }
    let expected_harness = crate::session_actor::normalize_harness_name(&harness.binary);
    if !record.harness.trim().is_empty()
        && record.harness != "default"
        && record.harness != expected_harness
    {
        let runtime = query_supervisor_runtime(file, session_id);
        let effective_state = runtime.actor_state.unwrap_or(record.state);
        let frontmatter_harness_changed =
            document_declares_expected_harness(file, &expected_harness);
        if mismatched_authoritative_actor_can_be_replaced(
            &runtime,
            effective_state,
            frontmatter_harness_changed,
        ) {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_harness_mismatch_stale file={} pane={} stored_harness={} expected_harness={} generation={} supervisor_health={} actor_state={} frontmatter_harness_changed={}",
                    file.display(),
                    record.pane_id,
                    record.harness,
                    expected_harness,
                    record.generation,
                    supervisor_health_label(runtime.health),
                    effective_state.as_str(),
                    frontmatter_harness_changed
                ),
            );
            return Ok(None);
        }
        if frontmatter_harness_changed {
            let queue_paused = crate::queue_continuation::document_queue_controller_paused(file);
            // `#actorswitchdefer` Part B: the route defer asserts "the supervisor
            // idle-watch will restart the harness at the next idle boundary." That is
            // only true while `agent_change_restart` is enabled — the idle-watch gates
            // the restart on `agent_change_restart_decision` (`#agentreloadrestart`),
            // which returns `None` (never `Restart`) when the knob is off. With the knob
            // disabled the defer would NEVER self-heal: route must bail EXPLICITLY with
            // that fact rather than hand the operator a `restart-supervisor` hint that
            // will not switch harnesses. Reverting `agent:` or re-enabling the knob are
            // the only recovery paths in that state.
            let agent_change_restart_enabled =
                crate::project_controller::agent_change_restart_enabled(file);
            if !agent_change_restart_enabled {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_authoritative_actor_harness_mismatch_deferred file={} pane={} stored_harness={} expected_harness={} generation={} supervisor_health={} actor_state={} queue_paused={} frontmatter_harness_changed=true agent_change_restart=disabled action=bail_restart_disabled",
                        file.display(),
                        record.pane_id,
                        record.harness,
                        expected_harness,
                        record.generation,
                        supervisor_health_label(runtime.health),
                        effective_state.as_str(),
                        queue_paused,
                    ),
                );
                anyhow::bail!(
                    "authoritative actor record for {} is running harness {}, but frontmatter now resolves to {}; agent_change_restart is disabled; Run Agent Doc will not switch harnesses until it is re-enabled or agent: reverts to {}",
                    file.display(),
                    record.harness,
                    expected_harness,
                    record.harness,
                );
            }
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_harness_mismatch_deferred file={} pane={} stored_harness={} expected_harness={} generation={} supervisor_health={} actor_state={} queue_paused={} frontmatter_harness_changed=true action=defer_to_boundary_restart",
                    file.display(),
                    record.pane_id,
                    record.harness,
                    expected_harness,
                    record.generation,
                    supervisor_health_label(runtime.health),
                    effective_state.as_str(),
                    queue_paused,
                ),
            );
            let recovery_hint = defer_recovery_hint(&runtime, effective_state, queue_paused, file);
            anyhow::bail!(
                "authoritative actor record for {} is running harness {}, but frontmatter now resolves to {}; deferring to boundary agent restart instead of replacing live pane{}",
                file.display(),
                record.harness,
                expected_harness,
                recovery_hint,
            );
        }
        anyhow::bail!(
            "authoritative actor record for {} is bound to harness {}, not {}",
            file.display(),
            record.harness,
            expected_harness
        );
    }
    if enforce_capability_proof {
        // `#capproofbg`: dispatch to the authoritative actor immediately while the
        // managed-capability proof runs in the background. A still-`Pending` proof
        // no longer blocks dispatch (read status without polling for it to settle);
        // a later FAILURE is surfaced asynchronously by the supervisor (Blocked
        // actor + tmux `display-message`) and gates subsequent dispatch. Only an
        // already-failed proof disables dispatch here.
        match managed_capability_proof_status(file, session_id, harness)? {
            ManagedCapabilityProofStatus::NotRequired
            | ManagedCapabilityProofStatus::Proven
            | ManagedCapabilityProofStatus::Pending => {}
            ManagedCapabilityProofStatus::Failed => {
                anyhow::bail!(
                    "managed {} capability proof for {} on pane {} failed; prompt dispatch is disabled for this pane. Inspect diagnostics, then run `agent-doc start {}` manually to recover",
                    harness.binary,
                    file.display(),
                    record.pane_id,
                    file.display()
                );
            }
            ManagedCapabilityProofStatus::Missing => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_authoritative_actor_missing_{}_capability_proof file={} pane={} harness={} generation={}",
                        harness.binary,
                        file.display(),
                        record.pane_id,
                        harness.binary,
                        record.generation
                    ),
                );
                return Ok(None);
            }
        }
    }

    let runtime = query_supervisor_runtime(file, session_id);
    let (record, runtime) = promote_starting_authoritative_actor_if_dispatch_ready(
        tmux, file, file_path, record, runtime, harness,
    );
    Ok(Some(AuthoritativeActorDispatchTarget { record, runtime }))
}

pub(crate) fn mismatched_authoritative_actor_can_be_replaced(
    runtime: &SupervisorRuntime,
    actor_state: agent_doc_sqlite::state_store::ActorState,
    _frontmatter_harness_changed: bool,
) -> bool {
    runtime.health != SupervisorHealth::Healthy
        || actor_state == agent_doc_sqlite::state_store::ActorState::Closed
}

/// Build the operator-actionable recovery suffix for the `defer_to_boundary_restart`
/// bail (`#actorswitchdefer` Part A). Examines supervisor health, queue pause state,
/// and actor state to produce the correct recovery path instead of a dead-end bail.
fn defer_recovery_hint(
    runtime: &SupervisorRuntime,
    actor_state: agent_doc_sqlite::state_store::ActorState,
    queue_paused: bool,
    file: &Path,
) -> String {
    let recovery_cmd = format!("agent-doc session restart-supervisor {}", file.display());
    if queue_paused
        || runtime.health == SupervisorHealth::Unreachable
        || runtime.health == SupervisorHealth::NoSocket
    {
        let blocker = if queue_paused {
            "queue is paused"
        } else {
            "supervisor is unreachable"
        };
        format!(
            ". {} — the boundary restart will not fire until it is healthy and resumed. Run: {}",
            blocker, recovery_cmd
        )
    } else if actor_state == agent_doc_sqlite::state_store::ActorState::Busy
        || actor_state == agent_doc_sqlite::state_store::ActorState::Starting
    {
        format!(
            ". pane is {} (not dispatch-ready) — run: {} --force",
            actor_state.as_str(),
            recovery_cmd
        )
    } else {
        format!(
            ". The supervisor idle-watch will restart the harness at the next idle boundary. To force it now: {}",
            recovery_cmd
        )
    }
}

fn document_declares_expected_harness(file: &Path, expected_harness: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(file) else {
        return false;
    };
    let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(&content) else {
        return false;
    };
    let Some(agent) = fm.agent.as_deref() else {
        return false;
    };
    crate::session_actor::normalize_harness_name(agent) == expected_harness
}

pub(crate) fn promote_starting_authoritative_actor_if_dispatch_ready(
    tmux: &Tmux,
    file: &Path,
    file_path: &str,
    record: agent_doc_sqlite::state_store::ActorRecord,
    mut runtime: SupervisorRuntime,
    harness: &HarnessConfig,
) -> (
    agent_doc_sqlite::state_store::ActorRecord,
    SupervisorRuntime,
) {
    let effective_state = runtime.actor_state.unwrap_or(record.state);
    if runtime.health != SupervisorHealth::Healthy
        || effective_state != agent_doc_sqlite::state_store::ActorState::Starting
    {
        return (record, runtime);
    }

    let _ = tmux.select_pane(&record.pane_id);
    let pane_ready = tmux
        .capture_pane(&record.pane_id, Some(80))
        .ok()
        .map(|content| ready_prompt_candidate(&content, harness).is_some())
        .unwrap_or(false);
    if !pane_ready {
        return (record, runtime);
    }

    let base_dir = registry_base_dir_for_dispatch(file_path);
    match crate::project_controller::mark_lifecycle(
        &base_dir,
        crate::project_controller::LifecycleRequest {
            file: file.to_path_buf(),
            session_id: record.session_id.clone(),
            pane_id: record.pane_id.clone(),
            generation: record.generation,
            state: agent_doc_sqlite::state_store::ActorState::Ready,
            caller: "route".to_string(),
            reason: "dispatch_ready_prompt".to_string(),
        },
    ) {
        Ok(updated) => {
            runtime.actor_state = Some(agent_doc_sqlite::state_store::ActorState::Ready);
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_promoted_ready file={} session={} pane={} generation={} reason=dispatch_ready_prompt",
                    file.display(),
                    updated.session_id,
                    updated.pane_id,
                    updated.generation
                ),
            );
            (updated, runtime)
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to promote starting authoritative actor {} for {} after seeing a dispatch-ready prompt: {}",
                record.pane_id,
                file.display(),
                err
            );
            (record, runtime)
        }
    }
}

/// Poll the live pane of a blocked-by-starting-timeout actor for a dispatch-ready
/// prompt, up to the harness recovery budget.
///
/// A `starting_actor_timeout` is sticky: once recorded, `wait_for_authoritative_actor_ready`
/// short-circuits and never re-waits, so the only recovery path is catching a
/// dispatch-ready prompt here. A single capture dead-ends a healthy but slow-starting
/// harness — e.g. a heavy Codex model with a large cached context that takes several
/// seconds to present its idle composer after a supervisor restart. Polling lets that
/// pane recover automatically. Busy panes never satisfy
/// `current_generation_ready_prompt_proven` (the harness busy cue short-circuits it),
/// so this preserves the "promote only proven idle panes" fail-closed invariant.
pub(crate) fn poll_starting_timeout_blocked_actor_dispatch_ready(
    tmux: &Tmux,
    actor: &AuthoritativeActorDispatchTarget,
    harness: &HarnessConfig,
) -> bool {
    if !actor_blocked_by_starting_timeout(actor) {
        return false;
    }
    let budget = crate::flow::routed_reopen::authoritative_actor_ready_retry_budget(
        Some(harness.binary.as_str()),
        cfg!(test),
    );
    let deadline = Instant::now() + budget.timeout;
    loop {
        let prompt_ready = current_generation_ready_prompt_proven(tmux, actor, harness);
        if starting_timeout_blocked_actor_can_recover(actor, prompt_ready) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(budget.poll_interval);
    }
}

pub(crate) fn recover_starting_timeout_blocked_actor_if_dispatch_ready(
    tmux: &Tmux,
    file: &Path,
    file_path: &str,
    actor: &AuthoritativeActorDispatchTarget,
    harness: &HarnessConfig,
) -> Option<AuthoritativeActorDispatchTarget> {
    if !poll_starting_timeout_blocked_actor_dispatch_ready(tmux, actor, harness) {
        return None;
    }

    let base_dir = registry_base_dir_for_dispatch(file_path);
    match crate::project_controller::mark_lifecycle(
        &base_dir,
        crate::project_controller::LifecycleRequest {
            file: file.to_path_buf(),
            session_id: actor.record.session_id.clone(),
            pane_id: actor.record.pane_id.clone(),
            generation: actor.record.generation,
            state: agent_doc_sqlite::state_store::ActorState::Ready,
            caller: "route".to_string(),
            reason: "dispatch_ready_prompt".to_string(),
        },
    ) {
        Ok(updated) => {
            clear_starting_actor_timeout_record(file_path);
            let mut runtime = actor.runtime.clone();
            runtime.actor_state = Some(agent_doc_sqlite::state_store::ActorState::Ready);
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_starting_timeout_recovered_ready file={} session={} pane={} generation={}",
                    file.display(),
                    updated.session_id,
                    updated.pane_id,
                    updated.generation
                ),
            );
            Some(AuthoritativeActorDispatchTarget {
                record: updated,
                runtime,
            })
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to recover timed-out starting actor {} generation {} for {} after seeing a dispatch-ready prompt: {}",
                actor.record.pane_id,
                actor.record.generation,
                file.display(),
                err
            );
            None
        }
    }
}

pub(crate) fn current_generation_ready_prompt_proven(
    tmux: &Tmux,
    target: &AuthoritativeActorDispatchTarget,
    harness: &HarnessConfig,
) -> bool {
    if tmux
        .capture_pane(&target.record.pane_id, Some(80))
        .ok()
        .map(|content| ready_prompt_candidate(&content, harness).is_some())
        .unwrap_or(false)
    {
        return true;
    }

    // Fallback: trust a current-generation transition whose reason already proves
    // dispatch readiness. `idle_pane_reconcile` (`#monster60stimeout`) is gated in
    // `start/idle_watch.rs` on `supervisor_pane_has_busy_cue == Some(false)` — the
    // supervisor captured the pane and found no busy cue before promoting the actor
    // to `Ready`. That direct pane evidence is as strong as the footer-shape proof
    // above, so accepting it here keeps the route from waiting the full 60s timeout
    // when the edge-triggered pty redraw missed re-emitting a recognized prompt shape.
    transition_proves_current_generation_ready(target)
}

/// Pure check: does the actor's last transition already prove current-generation
/// dispatch readiness without needing a fresh capture?
fn transition_proves_current_generation_ready(target: &AuthoritativeActorDispatchTarget) -> bool {
    target.record.last_transition.new_generation == target.record.generation
        && matches!(
            target.record.last_transition.reason.as_str(),
            "prompt_ready" | "dispatch_ready_prompt" | "idle_pane_reconcile"
        )
        && target.actor_state() == agent_doc_sqlite::state_store::ActorState::Ready
}

/// Outcome of a route-side controller dispatch authorization.
///
/// `#qflood2`: a benign in-flight coalesce (an identical dispatch for this cycle is
/// already in flight) must NOT re-send the routed trigger — re-sending is the flood.
/// It is also not a failure: the requested work is already happening. Modeling this
/// as a distinct variant forces every dispatch site to handle the deduped case at
/// compile time, so no send path can accidentally fire on a coalesce and a coalesce
/// can never surface as an exit-1 to the operator.
pub(crate) enum RouteDispatchAuthorization {
    Authorized,
    CoalescedDeduped { detail: String },
}

pub(crate) fn authorize_controller_dispatch(
    file: &Path,
    session_id: &str,
    file_path: &str,
    actor: &AuthoritativeActorDispatchTarget,
    command_kind: &str,
    diagnostic_payload: &str,
) -> Result<RouteDispatchAuthorization> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    let generation = actor.record.generation;
    let dispatch_request = || crate::project_controller::DispatchRequest {
        file: file.to_path_buf(),
        session_id: session_id.to_string(),
        pane_id: actor.record.pane_id.clone(),
        generation,
        command_kind: command_kind.to_string(),
        diagnostic_payload: diagnostic_payload.to_string(),
    };
    match crate::project_controller::authorize_dispatch(&base_dir, dispatch_request()) {
        Ok(_authorization) => Ok(RouteDispatchAuthorization::Authorized),
        Err(err)
            if agent_doc_controller::dispatch::dispatch_error_is_coalesced(&err.to_string()) =>
        {
            Ok(RouteDispatchAuthorization::CoalescedDeduped {
                detail: err.to_string(),
            })
        }
        // `#jbrestale`: a `queue_paused` bail whose pause was written by the churn
        // detector because a STALE supervisor re-injected a head is recoverable —
        // restart the supervisor once, lift the stale-injected pause, and re-dispatch a
        // single time instead of failing closed and forcing manual stale-supervisor
        // recovery. A deliberate operator/spent-preset pause carries no marker and
        // falls through to the terminal arm.
        Err(err) => {
            if let Some(recovery) =
                crate::project_controller::dispatch_error_stale_queue_pause_recovery(
                    &err.to_string(),
                )
            {
                recover_dispatch_via_supervisor_restart(
                    file,
                    session_id,
                    &base_dir,
                    generation,
                    recovery,
                    &dispatch_request,
                    err,
                )
            } else {
                Err(err)
            }
        }
    }
}

/// `#jbrestale`: one-shot recovery for a dispatch blocked by a stale-supervisor
/// churn-stop. Restart the supervisor (continue-mode, preserves the live child), lift
/// the stale-injected queue pause, then re-dispatch exactly once. Bounded to a single
/// restart + re-dispatch: a still-paused / re-injected queue (or a genuinely wedged one)
/// surfaces the error to the caller, so there is no restart loop. When the restart could
/// not even be issued, fail closed with the original bail and keep the pane alive.
fn recover_dispatch_via_supervisor_restart(
    file: &Path,
    session_id: &str,
    base_dir: &Path,
    generation: u64,
    recovery: crate::project_controller::StaleQueuePauseRecovery,
    dispatch_request: &dyn Fn() -> crate::project_controller::DispatchRequest,
    original_err: anyhow::Error,
) -> Result<RouteDispatchAuthorization> {
    let stale_pid = recovery.stale_pid;
    let outcome_fields = recovery.outcome.log_fields();
    if !restart_via_supervisor(file, session_id) {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_recovery action=restart_supervisor cause=churn_stop_stale_supervisor stale_pid={stale_pid} result=reexec_failed {outcome_fields}"
            ),
        );
        return Err(original_err);
    }
    // Lift the stale-injected pause so the re-dispatch is not re-blocked by the same
    // churn-stop. Pass the observed generation (the dispatch target's) so the resume is
    // not rejected as `missing_observed_generation`. A failed resume is non-fatal — the
    // re-dispatch below simply fails closed again if the pause somehow survives.
    match crate::project_controller::control_queue(
        base_dir,
        Some(file),
        "resume",
        Some(generation),
        Some("#jbrestale: auto-resume after restarting stale supervisor"),
        None,
    ) {
        Ok(_) => {}
        Err(e) => eprintln!(
            "[route] warning: failed to lift stale-injected queue pause for {}: {e}",
            file.display()
        ),
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "route_dispatch_recovery action=restart_supervisor cause=churn_stop_stale_supervisor stale_pid={stale_pid} result=restarted {outcome_fields}"
        ),
    );
    match crate::project_controller::authorize_dispatch(base_dir, dispatch_request()) {
        Ok(_authorization) => Ok(RouteDispatchAuthorization::Authorized),
        Err(err)
            if agent_doc_controller::dispatch::dispatch_error_is_coalesced(&err.to_string()) =>
        {
            Ok(RouteDispatchAuthorization::CoalescedDeduped {
                detail: err.to_string(),
            })
        }
        Err(err) => Err(err),
    }
}

/// Shared deduped-success handler for every route dispatch site: log the dedup and
/// hand back the already-running dispatch pane without re-sending the trigger.
pub(crate) fn route_dispatch_deduped_pane(
    file: &Path,
    command_kind: &str,
    dispatch_pane: String,
    detail: &str,
) -> String {
    crate::ops_log::log_op(
        file,
        &format!(
            "route_dispatch_deduped file={} pane={} command_kind={} reason=in_flight_coalesce detail={}",
            file.display(),
            dispatch_pane,
            command_kind,
            detail.chars().take(160).collect::<String>(),
        ),
    );
    dispatch_pane
}

pub(crate) fn load_authoritative_actor_dispatch_target(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    respect_tracked_clear_restart: bool,
    enforce_capability_proof: bool,
) -> Result<Option<AuthoritativeActorDispatchTarget>> {
    Ok(load_authoritative_actor_binding(
        tmux,
        file,
        session_id,
        file_path,
        harness,
        respect_tracked_clear_restart,
        enforce_capability_proof,
    )?
    .filter(authoritative_actor_dispatch_target_eligible))
}

pub(crate) fn load_authoritative_actor_for_registered_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    pane: &str,
) -> Result<Option<AuthoritativeActorDispatchTarget>> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    let document_id = crate::session_actor::canonical_document_id_in(&base_dir, file_path);
    let record = crate::project_controller::load_actor_store(&base_dir)?
        .values()
        .find(|record| {
            record.document_id == document_id
                && record.session_id == session_id
                && record.pane_id == pane
        })
        .cloned();
    let Some(record) = record else {
        return Ok(None);
    };
    if !tmux.pane_alive(&record.pane_id) {
        return Ok(None);
    }
    Ok(Some(AuthoritativeActorDispatchTarget {
        record,
        runtime: query_supervisor_runtime(file, session_id),
    }))
}

pub(crate) fn dispatch_only_can_use_degraded_authoritative_actor(
    actor: &AuthoritativeActorDispatchTarget,
    registered: Option<&str>,
    live_owner: Option<&str>,
) -> bool {
    can_use_degraded_authoritative_actor(DegradedAuthoritativeActorFacts {
        actor_pane: actor.record.pane_id.as_str(),
        transition_caller: actor.record.last_transition.caller.as_str(),
        transition_reason: actor.record.last_transition.reason.as_str(),
        registered_pane: registered,
        live_owner_pane: live_owner,
    })
}

#[cfg(test)]
pub(crate) fn authoritative_actor_start_wait_terminal_state(
    state: agent_doc_sqlite::state_store::ActorState,
) -> bool {
    crate::flow::routed_reopen::actor_start_wait_terminal_state(actor_dispatch_state(state))
}

pub(crate) fn route_starting_actor_not_ready_log_line(
    file: &Path,
    harness: &HarnessConfig,
    timeout: Duration,
    elapsed: Duration,
    facts: &AuthoritativeActorReadyFacts,
) -> String {
    let file_display = file.display().to_string();
    starting_actor_not_ready_log_line(StartingActorLogFacts {
        file_display: file_display.as_str(),
        harness_binary: harness.binary.as_str(),
        timeout,
        elapsed,
        ready_facts: facts,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct StartingActorTimeoutRecord {
    pane_id: String,
    generation: u64,
    log_line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartingActorTimeoutLogDecision {
    NewTimeout,
    DuplicateTimeout,
}

pub(crate) fn starting_actor_timeout_paths(file_path: &str) -> Option<(PathBuf, PathBuf)> {
    let requested = PathBuf::from(file_path);
    let root = agent_doc_fs::find_project_root(&requested)?;
    let hash = crate::snapshot::doc_hash_from_str(file_path);
    let state_dir = root.join(".agent-doc/state/route-starting-timeouts");
    let lock_dir = root.join(".agent-doc/locks");
    Some((
        state_dir.join(format!("{hash}.json")),
        lock_dir.join(format!("route-starting-timeout-{hash}.lock")),
    ))
}

pub(crate) fn record_starting_actor_timeout(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
    log_line: &str,
) -> Result<StartingActorTimeoutLogDecision> {
    let Some((state_path, lock_path)) = starting_actor_timeout_paths(file_path) else {
        return Ok(StartingActorTimeoutLogDecision::NewTimeout);
    };

    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    lock.lock_exclusive()?;

    let existing = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|content| serde_json::from_str::<StartingActorTimeoutRecord>(&content).ok());
    if existing.as_ref().is_some_and(|record| {
        record.pane_id == facts.pane_id && record.generation == facts.generation
    }) {
        let _ = lock.unlock();
        return Ok(StartingActorTimeoutLogDecision::DuplicateTimeout);
    }

    let record = StartingActorTimeoutRecord {
        pane_id: facts.pane_id.clone(),
        generation: facts.generation,
        log_line: log_line.to_string(),
    };
    std::fs::write(&state_path, serde_json::to_string_pretty(&record)?)?;
    let _ = lock.unlock();
    Ok(StartingActorTimeoutLogDecision::NewTimeout)
}

pub(crate) fn load_starting_actor_timeout_record(
    file_path: &str,
) -> Option<StartingActorTimeoutRecord> {
    let (state_path, _) = starting_actor_timeout_paths(file_path)?;
    std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|content| serde_json::from_str::<StartingActorTimeoutRecord>(&content).ok())
}

pub(crate) fn starting_actor_timeout_record_matches(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
) -> bool {
    if facts.actor_state != ActorDispatchState::Starting {
        return false;
    }
    starting_actor_timeout_record_identity_matches(file_path, facts)
}

pub(crate) fn starting_actor_timeout_record_identity_matches(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
) -> bool {
    load_starting_actor_timeout_record(file_path).is_some_and(|record| {
        record.pane_id == facts.pane_id && record.generation == facts.generation
    })
}

pub(crate) fn clear_starting_actor_timeout_record(file_path: &str) {
    let Some((state_path, _)) = starting_actor_timeout_paths(file_path) else {
        return;
    };
    let _ = std::fs::remove_file(state_path);
}

pub(crate) fn mark_starting_actor_timeout_blocked(
    file: &Path,
    file_path: &str,
    session_id: &str,
    facts: &AuthoritativeActorReadyFacts,
) {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    match crate::project_controller::mark_lifecycle(
        &base_dir,
        crate::project_controller::LifecycleRequest {
            file: file.to_path_buf(),
            session_id: session_id.to_string(),
            pane_id: facts.pane_id.clone(),
            generation: facts.generation,
            state: agent_doc_sqlite::state_store::ActorState::Blocked,
            caller: "route".to_string(),
            reason: "starting_actor_timeout".to_string(),
        },
    ) {
        Ok(updated) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_starting_marked_blocked file={} session={} pane={} generation={} blocker=starting_actor_timeout",
                    file.display(),
                    updated.session_id,
                    updated.pane_id,
                    updated.generation
                ),
            );
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to mark timed-out starting actor {} generation {} for {} as blocked: {}",
                facts.pane_id,
                facts.generation,
                file.display(),
                err
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::flow::routed_reopen::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
    use crate::supervisor::ipc::{IpcMethod, IpcResponse, SupervisorIpc};
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn load_authoritative_actor_dispatch_target_accepts_normalized_claude_harness_identity() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-authoritative-actor-claude-harness");
        let session = "claude";
        let cwd = test_cwd();
        let actor_pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        std::fs::write(
            &doc,
            "---\nagent: claude\n---\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-authoritative-actor-claude";
        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "ready",
                "restart_count": 0
            })),
            IpcMethod::Inject { .. } | IpcMethod::Clear { .. } => IpcResponse::ok_empty(),
            IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let actor = load_authoritative_actor_dispatch_target(
            &iso,
            &doc,
            session_id,
            &file_path,
            &HarnessConfig::claude(),
            true,
            true,
        )
        .expect("normalized Claude harness name should not fail the authoritative actor lookup")
        .expect("healthy actor record should remain dispatchable");
        assert_eq!(actor.record.harness, "claude-code");
        assert_eq!(actor.record.pane_id, actor_pane);

        ipc.stop();
    }
    #[test]
    fn dispatch_only_can_use_degraded_authoritative_actor_returns_true_when_registered_matches() {
        let actor = test_degraded_actor("%42");
        assert!(dispatch_only_can_use_degraded_authoritative_actor(
            &actor,
            Some("%42"),
            None,
        ));
    }
    #[test]
    fn dispatch_only_can_use_degraded_authoritative_actor_returns_true_when_live_owner_matches() {
        let actor = test_degraded_actor("%42");
        assert!(dispatch_only_can_use_degraded_authoritative_actor(
            &actor,
            None,
            Some("%42"),
        ));
    }
    #[test]
    fn dispatch_only_can_use_degraded_authoritative_actor_returns_true_when_both_match() {
        let actor = test_degraded_actor("%42");
        assert!(dispatch_only_can_use_degraded_authoritative_actor(
            &actor,
            Some("%42"),
            Some("%42"),
        ));
    }
    #[test]
    fn dispatch_only_can_use_degraded_authoritative_actor_returns_false_when_no_match() {
        let actor = test_degraded_actor("%42");
        assert!(!dispatch_only_can_use_degraded_authoritative_actor(
            &actor,
            Some("%99"),
            Some("%99"),
        ));
    }
    #[test]
    fn dispatch_only_can_use_degraded_authoritative_actor_returns_false_when_none_provided() {
        let actor = test_degraded_actor("%42");
        assert!(!dispatch_only_can_use_degraded_authoritative_actor(
            &actor, None, None,
        ));
    }
    #[test]
    fn mismatched_authoritative_actor_can_be_replaced_only_when_not_live_authority() {
        let healthy_ready = SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(agent_doc_sqlite::state_store::ActorState::Ready),
        };
        assert!(
            !mismatched_authoritative_actor_can_be_replaced(
                &healthy_ready,
                agent_doc_sqlite::state_store::ActorState::Ready,
                false,
            ),
            "a healthy ready actor from another harness is still authoritative and must block"
        );
        assert!(
            !mismatched_authoritative_actor_can_be_replaced(
                &healthy_ready,
                agent_doc_sqlite::state_store::ActorState::Ready,
                true,
            ),
            "an explicit frontmatter harness switch must defer to the boundary restart guard while the old-harness actor is healthy"
        );

        let healthy_closed = SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(agent_doc_sqlite::state_store::ActorState::Closed),
        };
        assert!(
            mismatched_authoritative_actor_can_be_replaced(
                &healthy_closed,
                agent_doc_sqlite::state_store::ActorState::Closed,
                false,
            ),
            "a closed actor from another harness should not strand a fresh harness start"
        );

        let unreachable = SupervisorRuntime {
            health: SupervisorHealth::Unreachable,
            actor_state: None,
        };
        assert!(
            mismatched_authoritative_actor_can_be_replaced(
                &unreachable,
                agent_doc_sqlite::state_store::ActorState::Ready,
                false,
            ),
            "an unreachable supervisor cannot prove live cross-harness ownership"
        );
    }

    #[test]
    fn document_declares_expected_harness_normalizes_claude_alias() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "---\nagent: claude\n---\n").unwrap();
        assert!(document_declares_expected_harness(&doc, "claude-code"));
        assert!(!document_declares_expected_harness(&doc, "codex"));
    }

    fn ready_target_with_reason(reason: &str) -> AuthoritativeActorDispatchTarget {
        AuthoritativeActorDispatchTarget {
            record: agent_doc_sqlite::state_store::ActorRecord {
                document_id: "test-doc".to_string(),
                session_id: "test-session".to_string(),
                generation: 5,
                pane_id: "%7".to_string(),
                window_id: "@1".to_string(),
                harness: "codex".to_string(),
                state: agent_doc_sqlite::state_store::ActorState::Ready,
                last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                    caller: "supervisor".to_string(),
                    reason: reason.to_string(),
                    timestamp: 0,
                    prior_generation: 4,
                    new_generation: 5,
                },
            },
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(agent_doc_sqlite::state_store::ActorState::Ready),
            },
        }
    }

    #[test]
    fn transition_proves_ready_accepts_idle_pane_reconcile() {
        let target = ready_target_with_reason("idle_pane_reconcile");
        assert!(
            transition_proves_current_generation_ready(&target),
            "idle_pane_reconcile is supervisor-proven direct pane evidence and must satisfy the route ready barrier"
        );
    }

    #[test]
    fn transition_proves_ready_accepts_prompt_ready_and_dispatch_ready_prompt() {
        for reason in ["prompt_ready", "dispatch_ready_prompt"] {
            let target = ready_target_with_reason(reason);
            assert!(
                transition_proves_current_generation_ready(&target),
                "{reason} must remain a valid ready-proof reason"
            );
        }
    }

    #[test]
    fn transition_proves_ready_rejects_unmatched_reason() {
        let target = ready_target_with_reason("starting_actor_timeout");
        assert!(
            !transition_proves_current_generation_ready(&target),
            "an unmatched transition reason must not satisfy the ready barrier"
        );
    }

    #[test]
    fn transition_proves_ready_rejects_stale_generation() {
        let mut target = ready_target_with_reason("idle_pane_reconcile");
        target.record.last_transition.new_generation = 3;
        assert!(
            !transition_proves_current_generation_ready(&target),
            "a prior-generation transition must not satisfy the current-generation ready barrier"
        );
    }

    #[test]
    fn transition_proves_ready_rejects_non_ready_actor() {
        let mut target = ready_target_with_reason("idle_pane_reconcile");
        target.record.state = agent_doc_sqlite::state_store::ActorState::Busy;
        target.runtime.actor_state = Some(agent_doc_sqlite::state_store::ActorState::Busy);
        assert!(
            !transition_proves_current_generation_ready(&target),
            "a non-Ready actor must not satisfy the ready barrier even with a matching reason"
        );
    }
}
