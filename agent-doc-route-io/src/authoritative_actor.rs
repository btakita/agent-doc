//! Authoritative actor dispatch target and controller authorization I/O.

use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};

use agent_doc_controller::dispatch::{
    ActorDispatchState, AuthoritativeActorReadyFacts, AuthoritativePromptReadyBarrierFacts,
    DegradedAuthoritativeActorFacts, PromptReadyBarrierDecision, RetryBudget,
    RoutedReopenGuardReason, STARTING_ACTOR_TIMEOUT_REASON, StartingActorLogFacts,
    StartingTimeoutActorFacts, actor_blocked_by_starting_timeout, actor_recovery_hint,
    authoritative_actor_ready_retry_budget, can_use_degraded_authoritative_actor,
    classify_authoritative_prompt_ready_barrier, prompt_ready_barrier_failed_event,
    starting_actor_not_ready_log_line, starting_actor_ready_log_line,
    starting_actor_terminal_log_line, starting_actor_timeout_coalesced_log_line,
    starting_timeout_blocked_actor_can_recover,
};
use agent_doc_controller_io::starting_actor_timeout::{
    StartingActorTimeoutLogDecision, clear_starting_actor_timeout_record,
    record_starting_actor_timeout, starting_actor_timeout_record_identity_matches,
    starting_actor_timeout_record_matches,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_session_registry_io::dispatch_registry::registry_base_dir_for_dispatch;
use agent_doc_supervisor::route_runtime::{
    DeferToBoundaryRestartRecoveryFacts, RouteActorState, SupervisorHealth, SupervisorRuntime,
    authoritative_actor_dispatch_target_eligible as supervisor_authoritative_actor_dispatch_target_eligible,
    defer_to_boundary_restart_recovery_hint, effective_authoritative_actor_state,
};
use tmux_router::Tmux;

use crate::supervisor_runtime::{query_supervisor_runtime, restart_via_supervisor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeActorDispatchTarget {
    pub record: agent_doc_sqlite::state_store::ActorRecord,
    pub runtime: SupervisorRuntime,
}

impl AuthoritativeActorDispatchTarget {
    pub fn actor_state(&self) -> agent_doc_sqlite::state_store::ActorState {
        sqlite_actor_state_from_route(effective_authoritative_actor_state(
            route_actor_state_from_sqlite(self.record.state),
            self.runtime.actor_state,
        ))
    }
}

pub fn route_actor_state_from_sqlite(
    state: agent_doc_sqlite::state_store::ActorState,
) -> RouteActorState {
    match state {
        agent_doc_sqlite::state_store::ActorState::Starting => RouteActorState::Starting,
        agent_doc_sqlite::state_store::ActorState::Ready => RouteActorState::Ready,
        agent_doc_sqlite::state_store::ActorState::Busy => RouteActorState::Busy,
        agent_doc_sqlite::state_store::ActorState::WaitingInput => RouteActorState::WaitingInput,
        agent_doc_sqlite::state_store::ActorState::Closed => RouteActorState::Closed,
        agent_doc_sqlite::state_store::ActorState::Blocked => RouteActorState::Blocked,
    }
}

pub fn sqlite_actor_state_from_route(
    state: RouteActorState,
) -> agent_doc_sqlite::state_store::ActorState {
    match state {
        RouteActorState::Starting => agent_doc_sqlite::state_store::ActorState::Starting,
        RouteActorState::Ready => agent_doc_sqlite::state_store::ActorState::Ready,
        RouteActorState::Busy => agent_doc_sqlite::state_store::ActorState::Busy,
        RouteActorState::WaitingInput => agent_doc_sqlite::state_store::ActorState::WaitingInput,
        RouteActorState::Closed => agent_doc_sqlite::state_store::ActorState::Closed,
        RouteActorState::Blocked => agent_doc_sqlite::state_store::ActorState::Blocked,
    }
}

pub fn actor_dispatch_state(
    state: agent_doc_sqlite::state_store::ActorState,
) -> ActorDispatchState {
    match state {
        agent_doc_sqlite::state_store::ActorState::Ready => ActorDispatchState::Ready,
        agent_doc_sqlite::state_store::ActorState::Starting => ActorDispatchState::Starting,
        agent_doc_sqlite::state_store::ActorState::Busy => ActorDispatchState::Busy,
        agent_doc_sqlite::state_store::ActorState::WaitingInput => ActorDispatchState::WaitingInput,
        agent_doc_sqlite::state_store::ActorState::Blocked => ActorDispatchState::Blocked,
        agent_doc_sqlite::state_store::ActorState::Closed => ActorDispatchState::Closed,
    }
}

pub fn authoritative_actor_dispatch_recovery_hint(
    state: agent_doc_sqlite::state_store::ActorState,
    file: &Path,
) -> String {
    actor_recovery_hint(actor_dispatch_state(state), &file.display().to_string())
}

pub fn authoritative_actor_dispatch_can_queue_optimistically(
    state: agent_doc_sqlite::state_store::ActorState,
) -> bool {
    agent_doc_controller::dispatch::actor_can_queue_optimistically(actor_dispatch_state(state))
}

pub fn authoritative_actor_ready_facts_from_target(
    target: &AuthoritativeActorDispatchTarget,
    prompt_ready: bool,
) -> AuthoritativeActorReadyFacts {
    AuthoritativeActorReadyFacts {
        pane_id: target.record.pane_id.clone(),
        generation: target.record.generation,
        actor_state: actor_dispatch_state(target.actor_state()),
        supervisor_health: target.runtime.health.label(),
        runtime_state: target.runtime.actor_state_label().to_string(),
        prompt_ready,
        last_transition_reason: target.record.last_transition.reason.clone(),
        last_transition_caller: target.record.last_transition.caller.clone(),
    }
}

pub fn tracked_harness_clear_requires_fresh_restart(
    harness: &HarnessConfig,
    latest_prompt: Option<&str>,
) -> bool {
    matches!(harness.binary.as_str(), "codex" | "opencode")
        && latest_prompt.is_some_and(agent_doc_codex_hook_io::prompt_requests_clear)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedCapabilityProofStatus {
    NotRequired,
    Pending,
    Proven,
    Failed,
    Missing,
}

pub fn managed_capability_proof_status(
    file: &Path,
    session_id: &str,
    harness: &HarnessConfig,
) -> Result<ManagedCapabilityProofStatus> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    let fm = agent_doc_frontmatter_io::session::parse_for_file_with_context(
        &content,
        file,
        &rc.ssh_context(),
    )
    .map(|(fm, _)| fm)?;
    #[cfg(test)]
    let global_config = agent_doc_config::Config::default();
    #[cfg(not(test))]
    let global_config = rc.global_config();
    if !agent_doc_agent_io::agent::codex::managed_capability_contract_required_for_doc_and_harness(
        file,
        &fm,
        &global_config,
        &harness.binary,
    ) {
        return Ok(ManagedCapabilityProofStatus::NotRequired);
    }
    let prefix = format!("{}_capability_proof status=", harness.binary);
    let expected_writable_contract = if harness.binary == "codex" {
        agent_doc_agent_io::agent::codex::managed_writable_root_contract_id_for_doc(
            file,
            &fm,
            &global_config,
        )
    } else {
        None
    };
    let proven_prefix = format!("{}proven", prefix);
    let proven = if let Some(contract) = expected_writable_contract.as_deref() {
        agent_doc_supervisor_io::startup_miss::session_log_has_event_after_latest_start_containing(
            file,
            session_id,
            &proven_prefix,
            &format!("writable_root_contract={contract}"),
        )?
    } else {
        agent_doc_supervisor_io::startup_miss::session_log_has_event_after_latest_start(
            file,
            session_id,
            &proven_prefix,
        )?
    };
    if proven {
        return Ok(ManagedCapabilityProofStatus::Proven);
    }
    if agent_doc_supervisor_io::startup_miss::session_log_has_event_after_latest_start(
        file,
        session_id,
        &format!("{}failed", prefix),
    )? {
        return Ok(ManagedCapabilityProofStatus::Failed);
    }
    if agent_doc_supervisor_io::startup_miss::session_log_has_event_after_latest_start(
        file,
        session_id,
        &format!("{}pending", prefix),
    )? {
        return Ok(ManagedCapabilityProofStatus::Pending);
    }
    Ok(ManagedCapabilityProofStatus::Missing)
}

pub fn load_authoritative_actor_binding(
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
            agent_doc_codex_hook_io::load_latest_prompt_for_file(file)?.as_deref(),
        )
    {
        return Ok(None);
    }

    let base_dir = registry_base_dir_for_dispatch(file_path);
    let Some(record) =
        agent_doc_controller_io::project_controller::authoritative_actor_binding(&base_dir, file)?
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
    let expected_harness = agent_doc_harness::normalize_harness_name(&harness.binary);
    if !record.harness.trim().is_empty()
        && record.harness != "default"
        && record.harness != expected_harness
    {
        let runtime = query_supervisor_runtime(file, session_id);
        let effective_state = effective_authoritative_actor_state(
            route_actor_state_from_sqlite(record.state),
            runtime.actor_state,
        );
        let frontmatter_harness_changed =
            document_declares_expected_harness(file, &expected_harness);
        if agent_doc_supervisor::route_runtime::mismatched_authoritative_actor_can_be_replaced(
            &runtime,
            effective_state,
        ) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_authoritative_actor_harness_mismatch_stale file={} pane={} stored_harness={} expected_harness={} generation={} supervisor_health={} actor_state={} frontmatter_harness_changed={}",
                    file.display(),
                    record.pane_id,
                    record.harness,
                    expected_harness,
                    record.generation,
                    runtime.health.label(),
                    effective_state.as_str(),
                    frontmatter_harness_changed
                ),
            );
            return Ok(None);
        }
        if frontmatter_harness_changed {
            let queue_paused =
                agent_doc_queue_io::controller_pause::document_queue_controller_paused(file);
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
                agent_doc_supervisor_io::config::agent_change_restart_enabled(file);
            if !agent_change_restart_enabled {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_authoritative_actor_harness_mismatch_deferred file={} pane={} stored_harness={} expected_harness={} generation={} supervisor_health={} actor_state={} queue_paused={} frontmatter_harness_changed=true agent_change_restart=disabled action=bail_restart_disabled",
                        file.display(),
                        record.pane_id,
                        record.harness,
                        expected_harness,
                        record.generation,
                        runtime.health.label(),
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
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_authoritative_actor_harness_mismatch_deferred file={} pane={} stored_harness={} expected_harness={} generation={} supervisor_health={} actor_state={} queue_paused={} frontmatter_harness_changed=true action=defer_to_boundary_restart",
                    file.display(),
                    record.pane_id,
                    record.harness,
                    expected_harness,
                    record.generation,
                    runtime.health.label(),
                    effective_state.as_str(),
                    queue_paused,
                ),
            );
            let recovery_cmd = format!("agent-doc session restart-supervisor {}", file.display());
            let recovery_hint =
                defer_to_boundary_restart_recovery_hint(DeferToBoundaryRestartRecoveryFacts {
                    supervisor_health: runtime.health,
                    actor_state: effective_state,
                    queue_paused,
                    recovery_command: &recovery_cmd,
                });
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
                agent_doc_ops_log_io::log_op(
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
    agent_doc_harness::normalize_harness_name(agent) == expected_harness
}

pub fn promote_starting_authoritative_actor_if_dispatch_ready(
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
    let effective_state = effective_authoritative_actor_state(
        route_actor_state_from_sqlite(record.state),
        runtime.actor_state,
    );
    if runtime.health != SupervisorHealth::Healthy || effective_state != RouteActorState::Starting {
        return (record, runtime);
    }

    let _ = tmux.select_pane(&record.pane_id);
    let pane_ready = tmux
        .capture_pane(&record.pane_id, Some(80))
        .ok()
        .map(|content| agent_doc_harness::ready_prompt_candidate(&content, harness).is_some())
        .unwrap_or(false);
    if !pane_ready {
        return (record, runtime);
    }

    let base_dir = registry_base_dir_for_dispatch(file_path);
    match agent_doc_controller_io::project_controller::mark_lifecycle(
        &base_dir,
        agent_doc_controller_io::project_controller::LifecycleRequest {
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
            runtime.actor_state = Some(RouteActorState::Ready);
            agent_doc_ops_log_io::log_op(
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

/// Wait for a starting authoritative actor to become dispatch-ready.
///
/// The caller owns the optional user-facing timeout override because the route
/// command scopes that setting per invocation. The default budget remains the
/// harness-specific controller policy.
pub fn wait_for_authoritative_actor_ready(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    initial: &AuthoritativeActorDispatchTarget,
    override_timeout: Option<Duration>,
) -> Result<Option<AuthoritativeActorDispatchTarget>> {
    let budget = match override_timeout {
        Some(timeout) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_wait_for_ready_override file={} harness={} timeout_secs={}",
                    file.display(),
                    harness.binary,
                    timeout.as_secs()
                ),
            );
            RetryBudget::new(timeout, Duration::from_millis(100))
        }
        None => authoritative_actor_ready_retry_budget(Some(harness.binary.as_str()), cfg!(test)),
    };
    let deadline = Instant::now() + budget.timeout;
    let mut last_facts = authoritative_actor_ready_facts_from_target(
        initial,
        current_generation_ready_prompt_proven(tmux, initial, harness),
    );
    let start = Instant::now();
    if last_facts.actor_state != ActorDispatchState::Starting
        && starting_actor_timeout_record_identity_matches(file_path, &last_facts)
    {
        clear_starting_actor_timeout_record(file_path);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_starting_actor_timeout_cleared_nonstarting file={} pane={} generation={} actor_state={}",
                file.display(),
                last_facts.pane_id,
                last_facts.generation,
                last_facts.actor_state.as_str()
            ),
        );
    }
    if starting_actor_timeout_record_matches(file_path, &last_facts) {
        mark_starting_actor_timeout_blocked(file, file_path, session_id, &last_facts);
        let file_display = file.display().to_string();
        agent_doc_ops_log_io::log_op(
            file,
            &starting_actor_timeout_coalesced_log_line(
                file_display.as_str(),
                harness.binary.as_str(),
                start.elapsed(),
                &last_facts,
            ),
        );
        return Ok(None);
    }

    while Instant::now() < deadline {
        if let Some(refreshed) = load_authoritative_actor_binding(
            tmux, file, session_id, file_path, harness, false, false,
        )? {
            let prompt_ready = current_generation_ready_prompt_proven(tmux, &refreshed, harness);
            last_facts = authoritative_actor_ready_facts_from_target(&refreshed, prompt_ready);
            match classify_authoritative_prompt_ready_barrier(
                AuthoritativePromptReadyBarrierFacts {
                    ready_facts: &last_facts,
                    dispatch_eligible: supervisor_authoritative_actor_dispatch_target_eligible(
                        &refreshed.runtime,
                    ),
                },
            ) {
                PromptReadyBarrierDecision::Ready => {
                    let elapsed = start.elapsed();
                    let file_display = file.display().to_string();
                    clear_starting_actor_timeout_record(file_path);
                    agent_doc_ops_log_io::log_op(
                        file,
                        &starting_actor_ready_log_line(
                            file_display.as_str(),
                            harness.binary.as_str(),
                            elapsed,
                            &last_facts,
                        ),
                    );
                    if override_timeout.is_some() {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_wait_for_ready_elapsed file={} harness={} elapsed_ms={} timeout_ms={}",
                                file.display(),
                                harness.binary,
                                elapsed.as_millis(),
                                budget.timeout.as_millis()
                            ),
                        );
                    }
                    return Ok(Some(refreshed));
                }
                PromptReadyBarrierDecision::Terminal => {
                    let elapsed = start.elapsed();
                    let file_display = file.display().to_string();
                    clear_starting_actor_timeout_record(file_path);
                    agent_doc_ops_log_io::log_op(
                        file,
                        &starting_actor_terminal_log_line(
                            file_display.as_str(),
                            harness.binary.as_str(),
                            elapsed,
                            &last_facts,
                        ),
                    );
                    return Ok(Some(refreshed));
                }
                PromptReadyBarrierDecision::Continue => {}
            }
        }
        std::thread::sleep(budget.poll_interval);
    }

    let elapsed = start.elapsed();
    let log_line = route_starting_actor_not_ready_log_line(
        file,
        harness,
        budget.timeout,
        elapsed,
        &last_facts,
    );
    if last_facts.actor_state == ActorDispatchState::Starting {
        match record_starting_actor_timeout(file_path, &last_facts, &log_line) {
            Ok(StartingActorTimeoutLogDecision::NewTimeout) => {
                agent_doc_ops_log_io::log_op(file, &log_line);
                agent_doc_flow_io::log_flow_event(
                    file,
                    prompt_ready_barrier_failed_event(
                        RoutedReopenGuardReason::StartingActorNotReady,
                    ),
                    agent_doc_ops_log_io::log_op,
                );
                mark_starting_actor_timeout_blocked(file, file_path, session_id, &last_facts);
            }
            Ok(StartingActorTimeoutLogDecision::DuplicateTimeout) => {
                mark_starting_actor_timeout_blocked(file, file_path, session_id, &last_facts);
                let file_display = file.display().to_string();
                agent_doc_ops_log_io::log_op(
                    file,
                    &starting_actor_timeout_coalesced_log_line(
                        file_display.as_str(),
                        harness.binary.as_str(),
                        elapsed,
                        &last_facts,
                    ),
                );
            }
            Err(err) => {
                eprintln!(
                    "[route] warning: failed to persist starting actor timeout for {}: {}",
                    file.display(),
                    err
                );
                agent_doc_ops_log_io::log_op(file, &log_line);
                agent_doc_flow_io::log_flow_event(
                    file,
                    prompt_ready_barrier_failed_event(
                        RoutedReopenGuardReason::StartingActorNotReadyUnpersisted,
                    ),
                    agent_doc_ops_log_io::log_op,
                );
            }
        }
    } else {
        clear_starting_actor_timeout_record(file_path);
        agent_doc_ops_log_io::log_op(file, &log_line);
        // Diagnostic: capture the pane content at timeout so we can analyze why
        // ready_prompt_candidate never matched.
        if let Ok(content) = tmux.capture_pane(&initial.record.pane_id, Some(80)) {
            let candidate = agent_doc_harness::ready_prompt_candidate(&content, harness);
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_wait_for_ready_timeout_diagnostic file={} pane={} harness={} candidate={:?} bottom_idle_chrome={} has_busy_cue={} lines={}",
                    file.display(),
                    initial.record.pane_id,
                    harness.binary,
                    candidate,
                    harness.is_bottom_idle_chrome(&content, 12),
                    harness.has_busy_cue(&content),
                    content.lines().count()
                ),
            );
        }
    }
    if override_timeout.is_some() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_wait_for_ready_timeout file={} harness={} elapsed_ms={} timeout_ms={}",
                file.display(),
                harness.binary,
                elapsed.as_millis(),
                budget.timeout.as_millis()
            ),
        );
    }
    Ok(None)
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
pub fn poll_starting_timeout_blocked_actor_dispatch_ready(
    tmux: &Tmux,
    actor: &AuthoritativeActorDispatchTarget,
    harness: &HarnessConfig,
) -> bool {
    if !actor_blocked_by_starting_timeout(StartingTimeoutActorFacts {
        actor_blocked: actor.record.state == agent_doc_sqlite::state_store::ActorState::Blocked,
        last_transition_reason: &actor.record.last_transition.reason,
        prompt_ready: false,
    }) {
        return false;
    }
    let budget = authoritative_actor_ready_retry_budget(Some(harness.binary.as_str()), cfg!(test));
    let deadline = Instant::now() + budget.timeout;
    loop {
        let prompt_ready = current_generation_ready_prompt_proven(tmux, actor, harness);
        if starting_timeout_blocked_actor_can_recover(StartingTimeoutActorFacts {
            actor_blocked: actor.record.state == agent_doc_sqlite::state_store::ActorState::Blocked,
            last_transition_reason: &actor.record.last_transition.reason,
            prompt_ready,
        }) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(budget.poll_interval);
    }
}

pub fn recover_starting_timeout_blocked_actor_if_dispatch_ready(
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
    match agent_doc_controller_io::project_controller::mark_lifecycle(
        &base_dir,
        agent_doc_controller_io::project_controller::LifecycleRequest {
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
            runtime.actor_state = Some(RouteActorState::Ready);
            agent_doc_ops_log_io::log_op(
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

pub fn current_generation_ready_prompt_proven(
    tmux: &Tmux,
    target: &AuthoritativeActorDispatchTarget,
    harness: &HarnessConfig,
) -> bool {
    if tmux
        .capture_pane(&target.record.pane_id, Some(80))
        .ok()
        .map(|content| agent_doc_harness::ready_prompt_candidate(&content, harness).is_some())
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
    agent_doc_supervisor::route_runtime::transition_proves_current_generation_ready(
        agent_doc_supervisor::route_runtime::CurrentGenerationReadyTransitionFacts {
            current_generation: target.record.generation,
            transition_generation: target.record.last_transition.new_generation,
            transition_reason: target.record.last_transition.reason.as_str(),
            actor_state: route_actor_state_from_sqlite(target.actor_state()),
        },
    )
}

/// Outcome of a route-side controller dispatch authorization.
///
/// `#qflood2`: a benign in-flight coalesce (an identical dispatch for this cycle is
/// already in flight) must NOT re-send the routed trigger — re-sending is the flood.
/// It is also not a failure: the requested work is already happening. Modeling this
/// as a distinct variant forces every dispatch site to handle the deduped case at
/// compile time, so no send path can accidentally fire on a coalesce and a coalesce
/// can never surface as an exit-1 to the operator.
pub enum RouteDispatchAuthorization {
    Authorized,
    CoalescedDeduped { detail: String },
}

pub fn authorize_controller_dispatch(
    file: &Path,
    session_id: &str,
    file_path: &str,
    actor: &AuthoritativeActorDispatchTarget,
    command_kind: &str,
    diagnostic_payload: &str,
) -> Result<RouteDispatchAuthorization> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    let generation = actor.record.generation;
    let dispatch_request = || agent_doc_controller_io::project_controller::DispatchRequest {
        file: file.to_path_buf(),
        session_id: session_id.to_string(),
        pane_id: actor.record.pane_id.clone(),
        generation,
        command_kind: command_kind.to_string(),
        diagnostic_payload: diagnostic_payload.to_string(),
    };
    match agent_doc_controller_io::project_controller::authorize_dispatch(
        &base_dir,
        dispatch_request(),
    ) {
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
                agent_doc_controller::dispatch::stale_queue_pause_recovery_from_dispatch_error(
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
    recovery: agent_doc_controller::dispatch::StaleQueuePauseRecovery,
    dispatch_request: &dyn Fn() -> agent_doc_controller_io::project_controller::DispatchRequest,
    original_err: anyhow::Error,
) -> Result<RouteDispatchAuthorization> {
    let stale_pid = recovery.stale_pid;
    let outcome_fields = recovery.outcome.log_fields();
    if !restart_via_supervisor(file, session_id) {
        agent_doc_ops_log_io::log_op(
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
    match agent_doc_controller_io::project_controller::control_queue(
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
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_recovery action=restart_supervisor cause=churn_stop_stale_supervisor stale_pid={stale_pid} result=restarted {outcome_fields}"
        ),
    );
    match agent_doc_controller_io::project_controller::authorize_dispatch(
        base_dir,
        dispatch_request(),
    ) {
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
pub fn route_dispatch_deduped_pane(
    file: &Path,
    command_kind: &str,
    dispatch_pane: String,
    detail: &str,
) -> String {
    agent_doc_ops_log_io::log_op(
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

pub fn load_authoritative_actor_dispatch_target(
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
    .filter(|target| supervisor_authoritative_actor_dispatch_target_eligible(&target.runtime)))
}

pub fn load_authoritative_actor_for_registered_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    pane: &str,
) -> Result<Option<AuthoritativeActorDispatchTarget>> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(&base_dir, file_path);
    let record = agent_doc_controller_io::project_controller::load_actor_store(&base_dir)?
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

pub fn dispatch_only_can_use_degraded_authoritative_actor(
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

pub fn authoritative_actor_start_wait_terminal_state(
    state: agent_doc_sqlite::state_store::ActorState,
) -> bool {
    agent_doc_controller::dispatch::actor_start_wait_terminal_state(actor_dispatch_state(state))
}

pub fn route_starting_actor_not_ready_log_line(
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

pub fn mark_starting_actor_timeout_blocked(
    file: &Path,
    file_path: &str,
    session_id: &str,
    facts: &AuthoritativeActorReadyFacts,
) {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    match agent_doc_controller_io::project_controller::mark_lifecycle(
        &base_dir,
        agent_doc_controller_io::project_controller::LifecycleRequest {
            file: file.to_path_buf(),
            session_id: session_id.to_string(),
            pane_id: facts.pane_id.clone(),
            generation: facts.generation,
            state: agent_doc_sqlite::state_store::ActorState::Blocked,
            caller: "route".to_string(),
            reason: STARTING_ACTOR_TIMEOUT_REASON.to_string(),
        },
    ) {
        Ok(updated) => {
            agent_doc_ops_log_io::log_op(
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
    use agent_doc_supervisor::ipc_protocol::{IpcMethod, IpcResponse};
    use agent_doc_supervisor_io::ipc::SupervisorIpc;
    use tmux_router::IsolatedTmux;

    fn test_cwd() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    struct ScopedCurrentDir {
        prev_cwd: std::path::PathBuf,
    }

    impl ScopedCurrentDir {
        fn set(path: &std::path::Path) -> Self {
            let prev_cwd = std::env::current_dir().unwrap_or_else(|_| test_cwd());
            std::env::set_current_dir(path).unwrap();
            Self { prev_cwd }
        }
    }

    impl Drop for ScopedCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev_cwd);
        }
    }

    fn test_actor_record(pane_id: &str) -> agent_doc_sqlite::state_store::ActorRecord {
        agent_doc_sqlite::state_store::ActorRecord {
            document_id: "test-doc".to_string(),
            session_id: "test-session".to_string(),
            generation: 1,
            pane_id: pane_id.to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state: agent_doc_sqlite::state_store::ActorState::Ready,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "test".to_string(),
                reason: "test".to_string(),
                timestamp: 0,
                prior_generation: 0,
                new_generation: 1,
            },
        }
    }

    fn test_degraded_actor(pane_id: &str) -> AuthoritativeActorDispatchTarget {
        AuthoritativeActorDispatchTarget {
            record: test_actor_record(pane_id),
            runtime: SupervisorRuntime {
                health: SupervisorHealth::NoSocket,
                actor_state: None,
            },
        }
    }

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
        agent_doc_session_actor_io::project_binding_in(
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
            | IpcMethod::ReplicaAwareness { .. }
            | IpcMethod::CrdtCheckpoint { .. } => IpcResponse::ok_empty(),
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
    fn document_declares_expected_harness_normalizes_claude_alias() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "---\nagent: claude\n---\n").unwrap();
        assert!(document_declares_expected_harness(&doc, "claude-code"));
        assert!(!document_declares_expected_harness(&doc, "codex"));
    }
}
