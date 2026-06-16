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
        if mismatched_authoritative_actor_can_be_replaced(&runtime, effective_state) {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_authoritative_actor_harness_mismatch_stale file={} pane={} stored_harness={} expected_harness={} generation={} supervisor_health={} actor_state={}",
                    file.display(),
                    record.pane_id,
                    record.harness,
                    expected_harness,
                    record.generation,
                    supervisor_health_label(runtime.health),
                    effective_state.as_str()
                ),
            );
            return Ok(None);
        }
        anyhow::bail!(
            "authoritative actor record for {} is bound to harness {}, not {}",
            file.display(),
            record.harness,
            expected_harness
        );
    }
    if enforce_capability_proof {
        match wait_for_managed_capability_proof(
            file,
            session_id,
            harness,
            fresh_route_start_ack_timeout(),
        )? {
            ManagedCapabilityProofStatus::NotRequired | ManagedCapabilityProofStatus::Proven => {}
            ManagedCapabilityProofStatus::Pending => {
                anyhow::bail!(
                    "managed {} capability proof for {} on pane {} is still pending after waiting {}s; prompt dispatch remains gated until the proof succeeds",
                    harness.binary,
                    file.display(),
                    record.pane_id,
                    fresh_route_start_ack_timeout().as_secs()
                );
            }
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
    actor_state: crate::session_actor::ActorState,
) -> bool {
    runtime.health != SupervisorHealth::Healthy
        || actor_state == crate::session_actor::ActorState::Closed
}

pub(crate) fn promote_starting_authoritative_actor_if_dispatch_ready(
    tmux: &Tmux,
    file: &Path,
    file_path: &str,
    record: crate::session_actor::ActorRecord,
    mut runtime: SupervisorRuntime,
    harness: &HarnessConfig,
) -> (crate::session_actor::ActorRecord, SupervisorRuntime) {
    let effective_state = runtime.actor_state.unwrap_or(record.state);
    if runtime.health != SupervisorHealth::Healthy
        || effective_state != crate::session_actor::ActorState::Starting
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
            state: crate::session_actor::ActorState::Ready,
            caller: "route".to_string(),
            reason: "dispatch_ready_prompt".to_string(),
        },
    ) {
        Ok(updated) => {
            runtime.actor_state = Some(crate::session_actor::ActorState::Ready);
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
            state: crate::session_actor::ActorState::Ready,
            caller: "route".to_string(),
            reason: "dispatch_ready_prompt".to_string(),
        },
    ) {
        Ok(updated) => {
            clear_starting_actor_timeout_record(file_path);
            let mut runtime = actor.runtime.clone();
            runtime.actor_state = Some(crate::session_actor::ActorState::Ready);
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

    target.record.last_transition.new_generation == target.record.generation
        && matches!(
            target.record.last_transition.reason.as_str(),
            "prompt_ready" | "dispatch_ready_prompt"
        )
        && target.actor_state() == crate::session_actor::ActorState::Ready
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
        Err(err) if crate::project_controller::dispatch_error_is_coalesced(&err.to_string()) => {
            Ok(RouteDispatchAuthorization::CoalescedDeduped {
                detail: err.to_string(),
            })
        }
        // `#jbrestale`: a `queue_paused` bail whose pause was written by the churn
        // detector because a STALE supervisor re-injected a head is recoverable —
        // restart the supervisor once, lift the stale-injected pause, and re-dispatch a
        // single time instead of failing closed and forcing a manual
        // `session restart-supervisor --force`. A deliberate operator/spent-preset pause
        // carries no marker and falls through to the terminal arm.
        Err(err) => {
            if let Some(stale_pid) =
                crate::project_controller::dispatch_error_supervisor_restart_redirect(
                    &err.to_string(),
                )
            {
                recover_dispatch_via_supervisor_restart(
                    file,
                    session_id,
                    &base_dir,
                    generation,
                    stale_pid,
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
/// not even be issued, fail closed with the original bail (it carries the manual
/// `session restart-supervisor --force` guidance) and keep the pane alive.
fn recover_dispatch_via_supervisor_restart(
    file: &Path,
    session_id: &str,
    base_dir: &Path,
    generation: u64,
    stale_pid: u32,
    dispatch_request: &dyn Fn() -> crate::project_controller::DispatchRequest,
    original_err: anyhow::Error,
) -> Result<RouteDispatchAuthorization> {
    if !restart_via_supervisor(file, session_id) {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_recovery action=restart_supervisor cause=churn_stop_stale_supervisor stale_pid={stale_pid} result=reexec_failed"
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
            "route_dispatch_recovery action=restart_supervisor cause=churn_stop_stale_supervisor stale_pid={stale_pid} result=restarted"
        ),
    );
    match crate::project_controller::authorize_dispatch(base_dir, dispatch_request()) {
        Ok(_authorization) => Ok(RouteDispatchAuthorization::Authorized),
        Err(err) if crate::project_controller::dispatch_error_is_coalesced(&err.to_string()) => {
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
    state: crate::session_actor::ActorState,
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
    let root = crate::snapshot::find_project_root(&requested)?;
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
            state: crate::session_actor::ActorState::Blocked,
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
            IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
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
            actor_state: Some(crate::session_actor::ActorState::Ready),
        };
        assert!(
            !mismatched_authoritative_actor_can_be_replaced(
                &healthy_ready,
                crate::session_actor::ActorState::Ready,
            ),
            "a healthy ready actor from another harness is still authoritative and must block"
        );

        let healthy_closed = SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(crate::session_actor::ActorState::Closed),
        };
        assert!(
            mismatched_authoritative_actor_can_be_replaced(
                &healthy_closed,
                crate::session_actor::ActorState::Closed,
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
                crate::session_actor::ActorState::Ready,
            ),
            "an unreachable supervisor cannot prove live cross-harness ownership"
        );
    }
}
