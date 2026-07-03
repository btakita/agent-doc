//! Route pane-resolution helper I/O.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::authoritative_actor::{
    AuthoritativeActorDispatchTarget, RouteDispatchAuthorization, authorize_controller_dispatch,
    dispatch_only_can_use_degraded_authoritative_actor, load_authoritative_actor_binding,
    load_authoritative_actor_for_registered_pane, route_dispatch_deduped_pane,
};
use crate::busy_pane::{
    BusyPaneInterruptRecoveryOutcome, attempt_busy_existing_pane_auto_fix,
    attempt_busy_existing_pane_interrupt_recovery,
};
use crate::cycle_ack::{
    RouteCycleAckEffects, pending_prompt_bearing_context_for_route, require_routed_cycle_ack,
};
use crate::dispatch::{RouteDispatchEffects, dispatch_existing_managed_reopen};
use crate::dispatch_only::{
    DispatchOnlyRouteEffects, DispatchOnlySendReopenOptions, dispatch_only_reopen_existing_pane,
    dispatch_only_send_reopen,
};
use crate::dispatch_recovery::{
    resolve_fresh_dispatch_target_after_ready_wait, wait_for_starting_pane_recovery_target,
};
use crate::dispatch_target::register_dispatch_target;
use crate::pane_provenance::pane_route_provenance;
use crate::restart_handoff::wait_for_busy_restart_handoff;
use crate::session_resolution::{ensure_auto_start_target_session, find_target_pane};
use crate::startup::RouteStartupEffects;
use crate::supervisor_runtime::restart_via_supervisor_with_mode;
use agent_doc_controller::dispatch::{
    BusyPaneAutoFixOutcome, DegradedAuthoritativeActorDirectSubmit, DispatchActorState,
    DispatchOnlyReopenDelivery, DispatchRuntimeHealth, StartupMissRouteFacts,
    degraded_authoritative_actor_direct_submit_log_message, is_stash_window_name,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_session_registry_io::dispatch_registry::{
    load_dispatch_registry, lookup_dispatch_registration,
};
use agent_doc_supervisor::route_runtime::{
    SupervisorHealth,
    authoritative_actor_dispatch_guard_reason as supervisor_authoritative_actor_dispatch_guard_reason,
    authoritative_actor_dispatch_target_eligible as supervisor_authoritative_actor_dispatch_target_eligible,
};
use agent_doc_supervisor::startup_miss::StartingPaneRecoveryTarget;
use agent_doc_tmux::is_first_column;
use agent_doc_turn::cycle_ack::PromptBearingRouteContext;
use tmux_router::Tmux;

pub fn dispatch_runtime_health(health: SupervisorHealth) -> DispatchRuntimeHealth {
    match health {
        SupervisorHealth::Healthy => DispatchRuntimeHealth::Healthy,
        SupervisorHealth::Restartable => DispatchRuntimeHealth::Restartable,
        SupervisorHealth::Halted { restart_count } => {
            DispatchRuntimeHealth::Halted { restart_count }
        }
        SupervisorHealth::Unreachable => DispatchRuntimeHealth::Unreachable,
        SupervisorHealth::NoSocket => DispatchRuntimeHealth::NoSocket,
    }
}

pub fn startup_miss_route_facts(
    miss: &agent_doc_supervisor::startup_miss::StartupMiss,
    registered_pane: &str,
    pane_alive: bool,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
    log_status: Option<&agent_doc_supervisor::startup_miss::SessionLogStatus>,
) -> StartupMissRouteFacts {
    StartupMissRouteFacts {
        miss_timestamp: miss.timestamp,
        registered_pane_is_live_owner: live_owner == Some(registered_pane),
        pane_alive,
        supervisor_health: dispatch_runtime_health(supervisor_health),
        latest_start_matches_registered_pane: log_status
            .and_then(|status| status.latest_start_pane.as_deref())
            == Some(registered_pane),
        latest_session_open: log_status
            .is_some_and(agent_doc_supervisor::startup_miss::SessionLogStatus::latest_session_open),
        latest_session_closed: log_status.is_some_and(
            agent_doc_supervisor::startup_miss::SessionLogStatus::latest_session_closed,
        ),
        latest_start_timestamp: log_status.and_then(|status| status.latest_start_timestamp),
        latest_open_run_timestamp: log_status
            .and_then(agent_doc_supervisor::startup_miss::latest_open_run_timestamp),
    }
}

pub fn startup_miss_route_provenance(
    tmux: &Tmux,
    pane_id: &str,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
    log_status: Option<&agent_doc_supervisor::startup_miss::SessionLogStatus>,
) -> String {
    let log_detail = match log_status {
        Some(status) => format!(
            "session_log={} {} last_event={}",
            agent_doc_supervisor::startup_miss::latest_log_outcome(status),
            agent_doc_supervisor::startup_miss::latest_log_anchor(status),
            agent_doc_supervisor::startup_miss::latest_log_last_event(status)
        ),
        None => "session_log=missing".to_string(),
    };
    format!(
        "{} live_owner={} supervisor_health={:?} {}",
        pane_route_provenance(tmux, pane_id),
        live_owner.unwrap_or("none"),
        supervisor_health,
        log_detail
    )
}

pub fn fail_if_recent_session_loss_window(file: &Path, session_id: &str) -> Result<()> {
    let Some(window) =
        agent_doc_supervisor_io::startup_miss::recent_session_loss_window(file, session_id)?
    else {
        return Ok(());
    };

    let first = agent_doc_supervisor::startup_miss::format_timestamp(window.first_timestamp);
    let last = agent_doc_supervisor::startup_miss::format_timestamp(window.last_timestamp);
    let latest_reason = window.latest_reason.as_deref().unwrap_or("unknown");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_repeated_session_loss_fail_closed file={} session={} count={} first={} last={} latest_reason={}",
            file.display(),
            session_id,
            window.count,
            first,
            last,
            latest_reason
        ),
    );
    anyhow::bail!(
        "refusing to auto-start {} after {} unexpected pane-loss events since {} (latest reason={} at {}). Route will not keep spawning replacements over a repeated crash window; inspect the last dead-pane/session-loss diagnostics, then run `agent-doc start {}` manually to recover",
        file.display(),
        window.count,
        first,
        latest_reason,
        last,
        file.display()
    );
}

/// Returns true if the pane is running an agent process for the given harness.
/// Returns true on query failure so route does not skip panes it cannot inspect.
pub fn is_agent_process(tmux: &Tmux, pane_id: &str, harness: &HarnessConfig) -> bool {
    agent_doc_tmux_io::target_current_command(tmux, pane_id)
        .map(|cmd| harness.is_agent_process_name(&cmd))
        .unwrap_or(true)
}

pub fn cleanup_failed_route_panes(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    created_panes: &[String],
) {
    for p in created_panes {
        if !tmux.pane_alive(p) {
            continue;
        }
        if failed_route_pane_has_startup_miss(file, p) {
            eprintln!(
                "[route] reaping startup-miss pane {} after failed fresh route for {}",
                p,
                file.display()
            );
            let _ = agent_doc_tmux_io::kill_pane(tmux, p);
            continue;
        }
        if should_preserve_failed_route_pane(tmux, file, p, session_id) {
            eprintln!(
                "[route] preserving newly-created pane {} after failed route because it is still the live registered owner for {}",
                p,
                file.display()
            );
            continue;
        }
        eprintln!(
            "[route] cleaning up orphaned pane {} (created during failed route)",
            p
        );
        let _ = agent_doc_tmux_io::kill_pane(tmux, p);
    }
}

pub fn failed_route_pane_has_startup_miss(file: &Path, pane_id: &str) -> bool {
    agent_doc_supervisor_io::startup_miss::load_startup_miss(file)
        .ok()
        .flatten()
        .is_some_and(|miss| {
            miss.pane_id == pane_id
                && matches!(
                    miss.origin,
                    agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart
                )
        })
}

pub fn failed_route_registry_root(file: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(file)
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| canonical.parent().map(|parent| parent.to_path_buf()))
}

pub fn should_preserve_failed_route_pane(
    tmux: &Tmux,
    file: &Path,
    pane_id: &str,
    session_id: &str,
) -> bool {
    let Some(root) = failed_route_registry_root(file) else {
        return false;
    };
    agent_doc_session_registry_io::load_in(&root)
        .ok()
        .and_then(|registry| {
            registry
                .values()
                .find(|entry| entry.session_id == session_id)
                .map(|entry| entry.pane.as_str() == pane_id)
        })
        .unwrap_or(false)
        && tmux.pane_alive(pane_id)
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_or_create_pane_dispatch_only(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
    mut route_authoritative_actor: impl FnMut(
        bool,
        Option<&agent_doc_cycle_state_io::CycleState>,
        Option<&PromptBearingRouteContext>,
        AuthoritativeActorDispatchTarget,
    ) -> Result<String>,
    dispatch_only_effects: DispatchOnlyRouteEffects,
    startup_effects: RouteStartupEffects,
) -> Result<String> {
    let registered = lookup_dispatch_registration(file_path, session_id)?;
    let cycle_baseline = agent_doc_cycle_state_io::load(file)?;
    let pending_prompt_context =
        pending_prompt_bearing_context_for_route(file, cycle_baseline.as_ref())?;
    let authoritative_actor =
        load_authoritative_actor_binding(tmux, file, session_id, file_path, harness, false, false)?;
    let registered_actor = if authoritative_actor.is_none() {
        registered.as_deref().map_or(Ok(None), |pane| {
            load_authoritative_actor_for_registered_pane(tmux, file, session_id, file_path, pane)
        })?
    } else {
        None
    };
    if let Some(actor) = authoritative_actor
        .as_ref()
        .filter(|actor| supervisor_authoritative_actor_dispatch_target_eligible(&actor.runtime))
    {
        return route_authoritative_actor(
            is_first_column(file, col_args),
            cycle_baseline.as_ref(),
            pending_prompt_context.as_ref(),
            actor.clone(),
        );
    }
    let live_owner = if registered.is_some() {
        agent_doc_sync_io::sync::find_normal_path_owner_pane(tmux, file, session_id)
    } else {
        None
    };
    let preferred_active_window = tmux.active_window(target_session);
    let associated_candidates =
        agent_doc_sync_io::sync::find_associated_panes(tmux, file, session_id);
    let associated_resolution = agent_doc_tmux::resolve_associated_panes(
        associated_candidates.clone(),
        preferred_active_window.as_deref(),
    );

    let rescue_target = |pane_id: &str| {
        rescue_from_stash(
            tmux,
            pane_id,
            session_id,
            file_path,
            target_session,
            is_first_column(file, col_args),
        );
    };

    let degraded_authoritative_actor = authoritative_actor.as_ref().or(registered_actor.as_ref());
    if let Some(actor) = degraded_authoritative_actor
        && let Some(reason) =
            supervisor_authoritative_actor_dispatch_guard_reason(actor.runtime.facts())
    {
        if dispatch_only_can_use_degraded_authoritative_actor(
            actor,
            registered.as_deref(),
            live_owner.as_deref(),
        ) {
            let dispatch_pane = actor.record.pane_id.clone();
            let file_display = file.display().to_string();
            let supervisor_health = actor.runtime.health.label();
            agent_doc_ops_log_io::log_op(
                file,
                &degraded_authoritative_actor_direct_submit_log_message(
                    DegradedAuthoritativeActorDirectSubmit {
                        file_display: file_display.as_str(),
                        pane_id: dispatch_pane.as_str(),
                        harness_binary: harness.binary.as_str(),
                        generation: actor.record.generation,
                        record_state: actor.record.state.as_str(),
                        supervisor_health: supervisor_health.as_str(),
                        runtime_actor_state: actor.runtime.actor_state_label(),
                        reason: reason.as_str(),
                    },
                ),
            );
            match authorize_controller_dispatch(
                file,
                session_id,
                file_path,
                actor,
                "dispatch_only_reopen",
                &format!(
                    "submit=direct_pane actor_state={} harness={} degraded_supervisor={}",
                    actor.actor_state().as_str(),
                    harness.binary,
                    reason.replace(' ', "_")
                ),
            )? {
                RouteDispatchAuthorization::CoalescedDeduped { detail } => {
                    return Ok(route_dispatch_deduped_pane(
                        file,
                        "dispatch_only_reopen",
                        dispatch_pane.clone(),
                        &detail,
                    ));
                }
                RouteDispatchAuthorization::Authorized => {}
            }
            rescue_target(dispatch_pane.as_str());
            return dispatch_only_reopen_existing_pane(
                tmux,
                file,
                pane,
                col_args,
                session_id,
                file_path,
                target_session,
                harness,
                created_panes,
                pending_prompt_context
                    .as_ref()
                    .map(|context| context.marker.as_str()),
                pending_prompt_context
                    .as_ref()
                    .map(|context| context.prompt_text.as_str()),
                true,
                true,
                false,
                dispatch_pane.as_str(),
                DispatchOnlyReopenDelivery::DirectPaneSubmit,
                dispatch_only_effects,
                true,
            );
        }

        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_authoritative_fallback_skipped file={} actor_pane={} harness={} generation={} record_state={} supervisor_health={} runtime_actor_state={} registered_pane={} live_owner={} reason={}",
                file.display(),
                actor.record.pane_id,
                harness.binary,
                actor.record.generation,
                actor.record.state.as_str(),
                actor.runtime.health.label(),
                actor.runtime.actor_state_label(),
                registered.as_deref().unwrap_or("none"),
                live_owner.as_deref().unwrap_or("none"),
                reason
            ),
        );
    }

    if let Some(ref registered_pane) = registered
        && tmux.pane_alive(registered_pane)
    {
        if let agent_doc_tmux::AssociatedPaneResolution::Ambiguous(candidates) =
            &associated_resolution
        {
            let error = agent_doc_tmux::format_associated_pane_resolution_error(
                file.display(),
                candidates,
                preferred_active_window.as_deref(),
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_only_associated_pane_ambiguous file={} count={}",
                    file_path,
                    candidates.len()
                ),
            );
            anyhow::bail!(error);
        }
        if let agent_doc_tmux::AssociatedPaneResolution::Selected { winner, redundant } =
            &associated_resolution
            && winner.pane_id != *registered_pane
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_only_associated_pane_requires_manual_claim file={} pane={} sources={}",
                    file_path,
                    winner.pane_id,
                    winner.source_summary()
                ),
            );
            anyhow::bail!(agent_doc_tmux::format_associated_pane_selected_error(
                file.display(),
                winner,
                redundant
            ));
        }
        let dispatch_pane = live_owner.as_deref().unwrap_or(registered_pane.as_str());
        rescue_target(dispatch_pane);
        return dispatch_only_reopen_existing_pane(
            tmux,
            file,
            pane,
            col_args,
            session_id,
            file_path,
            target_session,
            harness,
            created_panes,
            pending_prompt_context
                .as_ref()
                .map(|context| context.marker.as_str()),
            pending_prompt_context
                .as_ref()
                .map(|context| context.prompt_text.as_str()),
            true,
            true,
            false,
            dispatch_pane,
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            dispatch_only_effects,
            false,
        );
    }

    if let agent_doc_tmux::AssociatedPaneResolution::Ambiguous(candidates) = &associated_resolution
    {
        let error = agent_doc_tmux::format_associated_pane_resolution_error(
            file.display(),
            candidates,
            preferred_active_window.as_deref(),
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_associated_pane_ambiguous file={} count={}",
                file_path,
                candidates.len()
            ),
        );
        anyhow::bail!(error);
    }

    if let agent_doc_tmux::AssociatedPaneResolution::Selected { winner, redundant } =
        &associated_resolution
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_associated_pane_requires_manual_claim file={} pane={} sources={}",
                file_path,
                winner.pane_id,
                winner.source_summary()
            ),
        );
        anyhow::bail!(agent_doc_tmux::format_associated_pane_selected_error(
            file.display(),
            winner,
            redundant
        ));
    }

    let claimed_panes: std::collections::HashSet<String> = load_dispatch_registry(file_path)
        .unwrap_or_default()
        .values()
        .filter(|entry| tmux.pane_alive(&entry.pane))
        .map(|entry| entry.pane.clone())
        .collect();
    if registered.is_some()
        && let Some(new_pane) = find_target_pane(tmux, pane, target_session, &claimed_panes)
        && is_agent_process(tmux, &new_pane, harness)
    {
        rescue_target(&new_pane);
        return dispatch_only_reopen_existing_pane(
            tmux,
            file,
            pane,
            col_args,
            session_id,
            file_path,
            target_session,
            harness,
            created_panes,
            pending_prompt_context
                .as_ref()
                .map(|context| context.marker.as_str()),
            pending_prompt_context
                .as_ref()
                .map(|context| context.prompt_text.as_str()),
            true,
            true,
            false,
            &new_pane,
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            dispatch_only_effects,
            false,
        );
    }

    eprintln!("[route] No active pane found, auto-starting...");
    if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
        anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
    }
    fail_if_recent_session_loss_window(file, session_id)?;
    let split_before = is_first_column(file, col_args);
    ensure_auto_start_target_session(tmux, None, target_session, harness)?;
    crate::startup::auto_start_in_session(
        tmux,
        file,
        session_id,
        file_path,
        target_session,
        false,
        split_before,
        harness,
        None,
        Some(created_panes),
        true,
        startup_effects,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn recover_dispatch_only_authoritative_waiting_input(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    target_session: &str,
    split_before: bool,
    harness: &HarnessConfig,
    pane: &str,
    generation: u64,
    effects: DispatchOnlyRouteEffects,
) -> Result<String> {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_only_waiting_input_restart file={} pane={} harness={} generation={}",
            file.display(),
            pane,
            harness.binary,
            generation
        ),
    );
    eprintln!(
        "[route] authoritative actor generation {} for {} is waiting for supervisor restart input on pane {} — restarting fresh once before the dispatch-only reroute",
        generation,
        file.display(),
        pane
    );
    let initial_status =
        agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)
            .ok()
            .flatten();

    if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
        anyhow::bail!(
            "authoritative actor generation {} for {} owns pane {} but route could not restart the waiting supervisor fresh. Run `agent-doc start {}` manually to recover",
            generation,
            file.display(),
            pane,
            file.display()
        );
    }

    let dispatch_pane = match wait_for_starting_pane_recovery_target(
        tmux,
        file,
        session_id,
        pane,
        file_path,
        harness,
        initial_status.as_ref(),
    ) {
        Some(StartingPaneRecoveryTarget::DifferentPane(recovered)) => recovered,
        Some(StartingPaneRecoveryTarget::SamePane) | None => {
            resolve_fresh_dispatch_target_after_ready_wait(tmux, session_id, pane, file_path, None)?
        }
    };

    rescue_from_stash(
        tmux,
        &dispatch_pane,
        session_id,
        file_path,
        target_session,
        split_before,
    );
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    dispatch_only_send_reopen(
        tmux,
        file,
        session_id,
        &dispatch_pane,
        file_path,
        harness,
        DispatchOnlySendReopenOptions {
            delivery: DispatchOnlyReopenDelivery::DirectPaneSubmit,
            queue_prompt_text: None,
            effects,
        },
    )
}

/// Rescue a pane from a stash window back to the agent-doc window.
/// Only rescues if the pane is in the target session -- never swaps across sessions.
///
/// Returns `true` when the pane was actually moved out of a stash window so callers
/// can re-evaluate state that depends on pane location (e.g. Starting->Ready
/// promotion after the rescue makes the pane visible). Returns `false` when the
/// rescue was a no-op (pane not in stash, or session guard tripped).
pub fn rescue_from_stash(
    tmux: &Tmux,
    pane_id: &str,
    session_id: &str,
    file_path: &str,
    target_session: &str,
    split_before: bool,
) -> bool {
    let pane_session = agent_doc_tmux_io::target_session_name(tmux, pane_id).unwrap_or_default();
    if pane_session != target_session {
        eprintln!(
            "[route] Pane {} is in session '{}', not target '{}' — skipping stash rescue",
            pane_id, pane_session, target_session
        );
        return false;
    }

    let pane_win_name = agent_doc_tmux_io::target_window_name(tmux, pane_id).unwrap_or_default();

    if is_stash_window_name(&pane_win_name) {
        tracing::debug!(pane_id, window = %pane_win_name, target_session, "route: rescuing pane from stash");
        eprintln!(
            "[route] Pane {} is in stash window '{}', rescuing to agent-doc window",
            pane_id, pane_win_name
        );
        let agent_doc_window = format!("{}:agent-doc", target_session);
        let target_panes = tmux
            .list_window_panes(&agent_doc_window)
            .unwrap_or_default();
        let target = if split_before {
            target_panes.first()
        } else {
            target_panes.last()
        };
        let mut moved = false;
        if let Some(target) = target {
            let join_flag = if split_before { "-dbh" } else { "-dh" };
            match agent_doc_tmux_io::join_pane_guarded(
                tmux,
                pane_id,
                target,
                target_session,
                join_flag,
            ) {
                Ok(()) => {
                    eprintln!("[route] Rescued pane {} via join-pane", pane_id);
                    moved = true;
                }
                Err(e) => eprintln!("[route] join-pane rescue failed for {} ({})", pane_id, e),
            }
        }
        if let Err(e) = register_dispatch_target(tmux, session_id, pane_id, file_path) {
            eprintln!("[route] warning: re-register failed: {}", e);
        }
        return moved;
    }
    false
}

#[allow(clippy::too_many_arguments)]
pub fn optimistic_busy_pane_dispatch(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    cycle_baseline: Option<&agent_doc_cycle_state_io::CycleState>,
    prompt_bearing_marker: Option<&str>,
    detail: &str,
    route_dispatch_effects: RouteDispatchEffects,
    route_cycle_ack_effects: RouteCycleAckEffects,
) -> Result<String> {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_busy_existing_pane_optimistic_dispatch file={} pane={} harness={} detail={}",
            file.display(),
            pane,
            harness.binary,
            detail
        ),
    );
    eprintln!(
        "[route] pane {} for {} is still busy ({}) but remains authoritative — sending the bare {} reopen anyway",
        pane,
        file.display(),
        detail,
        harness.binary
    );
    register_dispatch_target(tmux, session_id, pane, file_path)?;
    let dispatch_start = dispatch_existing_managed_reopen(
        tmux,
        file,
        session_id,
        pane,
        file_path,
        harness,
        route_dispatch_effects,
    )?;
    let ack_pane = require_routed_cycle_ack(
        tmux,
        file,
        pane,
        session_id,
        file_path,
        harness,
        cycle_baseline,
        prompt_bearing_marker,
        true,
        dispatch_start,
        route_cycle_ack_effects,
    )?;
    Ok(ack_pane.unwrap_or_else(|| pane.to_string()))
}

#[derive(Debug, Clone, Copy)]
pub struct RouteBusyPaneRetryEffects {
    pub route_dispatch_effects: RouteDispatchEffects,
    pub route_cycle_ack_effects: RouteCycleAckEffects,
    pub emit_busy_route_diagnostic: fn(&Tmux, &str, &Path, &HarnessConfig),
}

#[allow(clippy::too_many_arguments)]
pub fn retry_route_after_busy_pane_auto_fix(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    cycle_baseline: Option<&agent_doc_cycle_state_io::CycleState>,
    prompt_bearing_marker: Option<&str>,
    allow_auto_fix_retry: bool,
    allow_busy_interrupt_retry: bool,
    auto_fix_attempted: bool,
    busy_pane: &str,
    provenance: &str,
    blocker_reason: Option<&str>,
    mut retry_route: impl FnMut(bool, bool, bool) -> Result<String>,
    effects: RouteBusyPaneRetryEffects,
) -> Result<String> {
    let fallback_detail = blocker_reason.map(|reason| format!("still shows {reason}"));
    if allow_auto_fix_retry {
        match attempt_busy_existing_pane_auto_fix(tmux, file, session_id, busy_pane, file_path)? {
            BusyPaneAutoFixOutcome::RetryRoute => {
                return retry_route(false, allow_busy_interrupt_retry, true);
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart => {
                wait_for_busy_restart_handoff(tmux, file, file_path, session_id, busy_pane);
                return retry_route(false, allow_busy_interrupt_retry, true);
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_existing_pane_retry_route_after_fresh_restart file={} pane={} harness={}",
                        file.display(),
                        busy_pane,
                        harness.binary
                    ),
                );
                eprintln!(
                    "[route] scoped fix left pane {} authoritative for {} with a healthy supervisor — restarting the live {} session fresh once before one final reroute",
                    busy_pane,
                    file.display(),
                    harness.binary
                );
                if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
                    (effects.emit_busy_route_diagnostic)(tmux, busy_pane, file, harness);
                    anyhow::bail!(
                        agent_doc_controller::dispatch::format_busy_existing_pane_error(
                            file.display(),
                            busy_pane,
                            &harness.binary,
                            provenance,
                            fallback_detail.as_deref(),
                            true
                        )
                    );
                }
                wait_for_busy_restart_handoff(tmux, file, file_path, session_id, busy_pane);
                return retry_route(false, allow_busy_interrupt_retry, true);
            }
            BusyPaneAutoFixOutcome::FailClosed => {}
        }
    }
    if allow_busy_interrupt_retry {
        match attempt_busy_existing_pane_interrupt_recovery(
            tmux,
            file,
            busy_pane,
            harness,
            blocker_reason,
        )? {
            BusyPaneInterruptRecoveryOutcome::Recovered => {
                return retry_route(false, false, true);
            }
            BusyPaneInterruptRecoveryOutcome::Blocked { reason } => {
                (effects.emit_busy_route_diagnostic)(tmux, busy_pane, file, harness);
                let detail = format!("bounded interrupt recovery still shows {reason}");
                if harness.binary == "codex" && tmux.pane_alive(busy_pane) {
                    return optimistic_busy_pane_dispatch(
                        tmux,
                        file,
                        session_id,
                        busy_pane,
                        file_path,
                        harness,
                        cycle_baseline,
                        prompt_bearing_marker,
                        detail.as_str(),
                        effects.route_dispatch_effects,
                        effects.route_cycle_ack_effects,
                    );
                }
                anyhow::bail!(
                    agent_doc_controller::dispatch::format_busy_existing_pane_error(
                        file.display(),
                        busy_pane,
                        &harness.binary,
                        provenance,
                        Some(detail.as_str()),
                        auto_fix_attempted || allow_auto_fix_retry
                    )
                );
            }
            BusyPaneInterruptRecoveryOutcome::TimedOut => {
                (effects.emit_busy_route_diagnostic)(tmux, busy_pane, file, harness);
                if harness.binary == "codex" && tmux.pane_alive(busy_pane) {
                    return optimistic_busy_pane_dispatch(
                        tmux,
                        file,
                        session_id,
                        busy_pane,
                        file_path,
                        harness,
                        cycle_baseline,
                        prompt_bearing_marker,
                        "bounded interrupt recovery never restored a dispatch-ready prompt",
                        effects.route_dispatch_effects,
                        effects.route_cycle_ack_effects,
                    );
                }
                anyhow::bail!(
                    agent_doc_controller::dispatch::format_busy_existing_pane_error(
                        file.display(),
                        busy_pane,
                        &harness.binary,
                        provenance,
                        Some("bounded interrupt recovery never restored a dispatch-ready prompt"),
                        auto_fix_attempted || allow_auto_fix_retry
                    )
                );
            }
            BusyPaneInterruptRecoveryOutcome::Skipped => {}
        }
    }
    if harness.binary == "codex" && tmux.pane_alive(busy_pane) {
        (effects.emit_busy_route_diagnostic)(tmux, busy_pane, file, harness);
        return optimistic_busy_pane_dispatch(
            tmux,
            file,
            session_id,
            busy_pane,
            file_path,
            harness,
            cycle_baseline,
            prompt_bearing_marker,
            fallback_detail
                .as_deref()
                .unwrap_or("pane remained busy after scoped recovery"),
            effects.route_dispatch_effects,
            effects.route_cycle_ack_effects,
        );
    }
    (effects.emit_busy_route_diagnostic)(tmux, busy_pane, file, harness);
    anyhow::bail!(
        agent_doc_controller::dispatch::format_busy_existing_pane_error(
            file.display(),
            busy_pane,
            &harness.binary,
            provenance,
            fallback_detail.as_deref(),
            auto_fix_attempted || allow_auto_fix_retry
        )
    );
}

pub fn controller_dispatch_actor_state(
    actor_state: agent_doc_sqlite::state_store::ActorState,
) -> DispatchActorState {
    match actor_state {
        agent_doc_sqlite::state_store::ActorState::Ready => DispatchActorState::Ready,
        agent_doc_sqlite::state_store::ActorState::Busy => DispatchActorState::Busy,
        _ => DispatchActorState::Other,
    }
}
