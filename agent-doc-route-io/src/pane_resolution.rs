//! Route pane-resolution helper I/O.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::admission_projection::{
    RouteAdmissionEffects, pending_prompt_bearing_context_for_route,
    require_routed_admission_projection,
};
use crate::authoritative_actor::{
    RouteDispatchAuthorization, authorize_controller_dispatch,
    dispatch_only_can_use_degraded_authoritative_actor, load_authoritative_actor_binding,
    load_authoritative_actor_dispatch_target, load_authoritative_actor_for_registered_pane,
    route_dispatch_deduped_pane,
};
use crate::authoritative_dispatch::{
    RouteAuthoritativeActorEffects, route_via_authoritative_actor,
};
use crate::busy_pane::{
    BusyPaneInterruptRecoveryOutcome, ExistingPaneDispatchReadiness,
    attempt_busy_existing_pane_auto_fix, attempt_busy_existing_pane_interrupt_recovery,
    ensure_existing_pane_ready_for_dispatch,
};
use crate::dispatch::{RouteDispatchEffects, dispatch_existing_managed_reopen};
use crate::dispatch_only::{
    DispatchOnlyRouteEffects, DispatchOnlySendReopenOptions, dispatch_only_reopen_existing_pane,
    dispatch_only_send_reopen,
};
use crate::dispatch_recovery::{
    StartingPaneRecoveryWaitOptions, resolve_fresh_dispatch_target_after_ready_wait,
    wait_for_starting_pane_recovery_target,
};
use crate::dispatch_target::register_dispatch_target;
use crate::launch_contract::reapply_codex_launch_contract_before_reuse;
use crate::pane_provenance::pane_route_provenance;
use crate::restart_handoff::wait_for_busy_restart_handoff;
use crate::session_resolution::{ensure_auto_start_target_session, find_target_pane};
use crate::startup::RouteStartupEffects;
use crate::supervisor_runtime::{
    query_supervisor_health, restart_via_supervisor, restart_via_supervisor_with_mode,
};
use agent_doc_controller::dispatch::{
    BusyPaneAutoFixOutcome, DegradedAuthoritativeActorDirectSubmit, DispatchActorState,
    DispatchOnlyReopenDelivery, DispatchRuntimeHealth, RoutedDispatchStartProof,
    StartupMissRouteFacts, degraded_authoritative_actor_direct_submit_log_message,
    is_stash_window_name, startup_miss_requires_fresh_start, startup_miss_should_fail_closed,
    startup_miss_should_restart_live_owner, startup_miss_superseded_by_later_open_start,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_session_registry_io::dispatch_registry::{
    deregister_dispatch_registration, load_dispatch_registry, lookup_dispatch_registration,
};
use agent_doc_supervisor::route_runtime::{
    SupervisorHealth,
    authoritative_actor_dispatch_guard_reason as supervisor_authoritative_actor_dispatch_guard_reason,
    authoritative_actor_dispatch_target_eligible as supervisor_authoritative_actor_dispatch_target_eligible,
};
use agent_doc_supervisor::startup_miss::StartingPaneRecoveryTarget;
use agent_doc_tmux::is_first_column;
use tmux_router::Tmux;

/// Controller background recovery is intentionally weaker than an editor route:
/// it may submit only to the exact pane whose ownership was already proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundExistingPaneDecision {
    UseExistingPane,
    MissingExplicitPane,
    ExplicitPaneDead,
    ForeignDocumentOwner,
    AuthoritativePaneChanged,
    RegistrationChanged,
    LiveOwnerChanged,
    MissingOwnershipProof,
}

impl BackgroundExistingPaneDecision {
    fn reason(self) -> &'static str {
        match self {
            Self::UseExistingPane => "existing_pane_proven",
            Self::MissingExplicitPane => "missing_explicit_pane",
            Self::ExplicitPaneDead => "explicit_pane_dead",
            Self::ForeignDocumentOwner => "foreign_document_owner",
            Self::AuthoritativePaneChanged => "authoritative_pane_changed",
            Self::RegistrationChanged => "registration_changed",
            Self::LiveOwnerChanged => "live_owner_changed",
            Self::MissingOwnershipProof => "missing_ownership_proof",
        }
    }
}

/// Pure policy used by the route and SimWorld. A background recovery route must
/// fail closed instead of searching, rescuing, or provisioning when its target
/// changed after the watchdog observation.
pub fn background_existing_pane_decision(
    explicit_pane: Option<&str>,
    explicit_pane_alive: bool,
    explicit_pane_runs_other_document: bool,
    authoritative_pane: Option<&str>,
    registered_pane: Option<&str>,
    live_owner: Option<&str>,
) -> BackgroundExistingPaneDecision {
    let Some(explicit_pane) = explicit_pane else {
        return BackgroundExistingPaneDecision::MissingExplicitPane;
    };
    if !explicit_pane_alive {
        return BackgroundExistingPaneDecision::ExplicitPaneDead;
    }
    if explicit_pane_runs_other_document {
        return BackgroundExistingPaneDecision::ForeignDocumentOwner;
    }
    if authoritative_pane.is_some_and(|pane| pane != explicit_pane) {
        return BackgroundExistingPaneDecision::AuthoritativePaneChanged;
    }
    if registered_pane.is_some_and(|pane| pane != explicit_pane) {
        return BackgroundExistingPaneDecision::RegistrationChanged;
    }
    if live_owner.is_some_and(|pane| pane != explicit_pane) {
        return BackgroundExistingPaneDecision::LiveOwnerChanged;
    }
    if authoritative_pane.is_none() && registered_pane.is_none() {
        return BackgroundExistingPaneDecision::MissingOwnershipProof;
    }
    BackgroundExistingPaneDecision::UseExistingPane
}

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
    plain_trigger: bool,
    created_panes: &mut Vec<String>,
    authoritative_effects: RouteAuthoritativeActorEffects,
    dispatch_only_effects: DispatchOnlyRouteEffects,
    startup_effects: RouteStartupEffects,
) -> Result<String> {
    let registered = lookup_dispatch_registration(file_path, session_id)?;
    let cycle_baseline = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    let pending_prompt_context = if plain_trigger {
        None
    } else {
        pending_prompt_bearing_context_for_route(file, cycle_baseline.as_ref())?
    };
    let authoritative_actor =
        load_authoritative_actor_binding(tmux, file, session_id, file_path, harness, false, false)?;
    let registered_actor = if authoritative_actor.is_none() {
        registered.as_deref().map_or(Ok(None), |pane| {
            load_authoritative_actor_for_registered_pane(tmux, file, session_id, file_path, pane)
        })?
    } else {
        None
    };
    let resolved_actor = authoritative_actor.as_ref().or(registered_actor.as_ref());
    let live_owner = if registered.is_some() {
        agent_doc_sync_io::sync::find_normal_path_owner_pane(tmux, file, session_id)
    } else {
        None
    };
    let resolved_pane = resolved_actor
        .map(|actor| actor.record.pane_id.as_str())
        .or(registered.as_deref());
    if let Some(resolved_pane) = resolved_pane
        && tmux.pane_alive(resolved_pane)
        && agent_doc_sync_io::sync::pane_runs_other_document_owner(tmux, resolved_pane, file)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_inactive_pane_focus_guard file={} pane={} outcome=blocked reason=foreign_document_owner focus_effect=preserved pane_effect=none",
                file.display(),
                resolved_pane,
            ),
        );
        anyhow::bail!(
            "refusing to route {} through pane {} because it now runs another document; tmux focus was preserved",
            file.display(),
            resolved_pane,
        );
    }

    let background_existing_pane_only = crate::invocation::background_existing_pane_only();
    if background_existing_pane_only {
        let explicit_pane_alive = pane.is_some_and(|pane| tmux.pane_alive(pane));
        let explicit_pane_runs_other_document = pane.is_some_and(|pane| {
            explicit_pane_alive
                && agent_doc_sync_io::sync::pane_runs_other_document_owner(tmux, pane, file)
        });
        let decision = background_existing_pane_decision(
            pane,
            explicit_pane_alive,
            explicit_pane_runs_other_document,
            resolved_actor.map(|actor| actor.record.pane_id.as_str()),
            registered.as_deref(),
            live_owner.as_deref(),
        );
        if decision != BackgroundExistingPaneDecision::UseExistingPane {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_background_existing_pane_guard file={} pane={} outcome=blocked reason={} focus_effect=preserved pane_effect=none",
                    file.display(),
                    pane.unwrap_or("none"),
                    decision.reason(),
                ),
            );
            anyhow::bail!(
                "background recovery for {} was blocked ({}) so tmux focus and pane layout remain unchanged",
                file.display(),
                decision.reason(),
            );
        }
        if authoritative_actor.as_ref().is_some_and(|actor| {
            supervisor_authoritative_actor_dispatch_target_eligible(&actor.runtime)
        }) {
            let existing_pane = pane.expect("background pane policy proved an explicit pane");
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_background_existing_pane_guard file={} pane={} outcome=suppressed reason=supervisor_recovered focus_effect=preserved pane_effect=none",
                    file.display(),
                    existing_pane,
                ),
            );
            return Ok(existing_pane.to_string());
        }
    }
    if let Some(actor) = authoritative_actor
        .as_ref()
        .filter(|actor| supervisor_authoritative_actor_dispatch_target_eligible(&actor.runtime))
    {
        return route_via_authoritative_actor(
            tmux,
            file,
            session_id,
            file_path,
            target_session,
            is_first_column(file, col_args),
            harness,
            cycle_baseline.as_ref(),
            pending_prompt_context.as_ref(),
            true,
            plain_trigger,
            actor.clone(),
            authoritative_effects,
        );
    }
    let preferred_active_window = tmux.active_window(target_session);
    let associated_candidates =
        agent_doc_sync_io::sync::find_associated_panes(tmux, file, session_id);
    let associated_resolution = agent_doc_tmux::resolve_associated_panes(
        associated_candidates.clone(),
        preferred_active_window.as_deref(),
    );

    let rescue_target = |pane_id: &str| {
        if background_existing_pane_only {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_background_existing_pane_guard file={} pane={} outcome=allowed action=skip_stash_rescue focus_effect=preserved pane_effect=none",
                    file.display(),
                    pane_id,
                ),
            );
            return;
        }
        rescue_from_stash(
            tmux,
            pane_id,
            session_id,
            file_path,
            target_session,
            is_first_column(file, col_args),
        );
    };

    let degraded_authoritative_actor = resolved_actor;
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
                !background_existing_pane_only,
                !background_existing_pane_only,
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
        let dispatch_pane = if background_existing_pane_only {
            pane.expect("background pane policy proved an explicit pane")
        } else {
            live_owner.as_deref().unwrap_or(registered_pane.as_str())
        };
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
            !background_existing_pane_only,
            !background_existing_pane_only,
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
        // #routelegacypane: a legacy association whose candidate panes are all DEAD
        // (the pane exited) is stale ownership evidence, not a live ambiguity. Do not
        // force a manual claim/kill prompt — auto-reclaim by falling through to the
        // normal cold-start route path. Only a LIVE (or ambiguous) candidate is real
        // contested ownership that needs an explicit operator decision.
        let any_candidate_alive = tmux.pane_alive(&winner.pane_id)
            || redundant.iter().any(|c| tmux.pane_alive(&c.pane_id));
        if any_candidate_alive {
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
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_associated_pane_dead_auto_reclaim file={} pane={} sources={} recovery=fall_through_normal_route",
                file_path,
                winner.pane_id,
                winner.source_summary()
            ),
        );
    }

    let claimed_panes: std::collections::HashSet<String> = load_dispatch_registry(file_path)
        .unwrap_or_default()
        .values()
        .filter(|entry| tmux.pane_alive(&entry.pane))
        .map(|entry| entry.pane.clone())
        .collect();
    if !background_existing_pane_only
        && registered.is_some()
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

    if background_existing_pane_only {
        let requested_pane = pane.unwrap_or("none");
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_background_existing_pane_guard file={} pane={} outcome=blocked reason=existing_target_unavailable focus_effect=preserved pane_effect=none",
                file.display(),
                requested_pane,
            ),
        );
        anyhow::bail!(
            "background recovery for {} cannot use its existing pane {}; auto-start is forbidden and tmux focus was preserved",
            file.display(),
            requested_pane,
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
        None,
        startup_effects,
    )
}

#[derive(Clone, Copy)]
pub struct ManagedPaneResolutionEffects {
    pub route_dispatch_effects: RouteDispatchEffects,
    pub route_admission_effects: RouteAdmissionEffects,
    pub route_busy_pane_retry_effects: RouteBusyPaneRetryEffects,
    pub route_startup_effects: RouteStartupEffects,
    pub route_authoritative_actor_effects: RouteAuthoritativeActorEffects,
}

/// Resolve an existing managed pane or create a new one. Returns the pane ID.
#[allow(clippy::too_many_arguments)]
pub fn resolve_or_create_pane(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
    effects: ManagedPaneResolutionEffects,
) -> Result<String> {
    resolve_or_create_pane_with_auto_fix_retry(
        tmux,
        file,
        pane,
        col_args,
        session_id,
        file_path,
        target_session,
        harness,
        created_panes,
        effects,
        true,
        true,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_or_create_pane_with_auto_fix_retry(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
    effects: ManagedPaneResolutionEffects,
    allow_auto_fix_retry: bool,
    allow_busy_interrupt_retry: bool,
    auto_fix_attempted: bool,
) -> Result<String> {
    tracing::debug!(
        session_id = &session_id[..8.min(session_id.len())],
        file = file_path,
        target_session,
        "route::resolve_or_create_pane"
    );
    let registered = lookup_dispatch_registration(file_path, session_id)?;
    let cycle_baseline = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    let pending_prompt_context =
        pending_prompt_bearing_context_for_route(file, cycle_baseline.as_ref())?;
    if let Some(actor) = load_authoritative_actor_dispatch_target(
        tmux, file, session_id, file_path, harness, true, true,
    )? {
        return route_via_authoritative_actor(
            tmux,
            file,
            session_id,
            file_path,
            target_session,
            is_first_column(file, col_args),
            harness,
            cycle_baseline.as_ref(),
            pending_prompt_context.as_ref(),
            false,
            false,
            actor,
            effects.route_authoritative_actor_effects,
        );
    }
    let live_owner = if registered.is_some() {
        agent_doc_sync_io::sync::find_normal_path_owner_pane(tmux, file, session_id)
    } else {
        None
    };
    let supervisor_health = if registered.is_some() {
        query_supervisor_health(file, session_id)
    } else {
        SupervisorHealth::NoSocket
    };
    let preferred_active_window = tmux.active_window(target_session);
    let associated_candidates =
        agent_doc_sync_io::sync::find_associated_panes(tmux, file, session_id);
    let associated_resolution = agent_doc_tmux::resolve_associated_panes(
        associated_candidates.clone(),
        preferred_active_window.as_deref(),
    );

    if let Ok(Some(miss)) = agent_doc_supervisor_io::startup_miss::load_startup_miss(file)
        && let Some(supersession) =
            agent_doc_supervisor_io::startup_miss::superseded_by_newer_registered_start(
                agent_doc_supervisor_io::startup_miss::session_registry_lookup(),
                file,
                &miss,
            )?
    {
        let miss_ts = agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
        eprintln!(
            "[route] startup-miss on pane {} from {} for {} is superseded by newer registered owner {} — clearing stale marker",
            miss.pane_id, miss_ts, file_path, supersession.registered_pane
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_startup_miss_cleared_superseded_owner file={} stale_pane={} registered_pane={} miss_timestamp={} latest_start_timestamp={}",
                file_path,
                miss.pane_id,
                supersession.registered_pane,
                miss_ts,
                supersession.latest_start_timestamp
            ),
        );
        let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
    }

    if let Some(ref registered_pane) = registered
        && let Ok(Some(miss)) = agent_doc_supervisor_io::startup_miss::load_startup_miss(file)
        && miss.pane_id == *registered_pane
        && tmux.pane_alive(registered_pane)
    {
        let log_status =
            agent_doc_supervisor_io::startup_miss::session_log_status(file, &miss.session_id)
                .ok()
                .flatten();
        let miss_ts = agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
        let provenance = startup_miss_route_provenance(
            tmux,
            registered_pane,
            live_owner.as_deref(),
            supervisor_health,
            log_status.as_ref(),
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_startup_miss_detected file={} origin={:?} miss_timestamp={} {}",
                file_path, miss.origin, miss_ts, provenance
            ),
        );
        let startup_facts = startup_miss_route_facts(
            &miss,
            registered_pane,
            true,
            live_owner.as_deref(),
            supervisor_health,
            log_status.as_ref(),
        );
        if startup_miss_should_fail_closed(startup_facts) {
            eprintln!(
                "[route] startup-miss for {} is stranded, not crashed: {}",
                file_path, provenance
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_startup_miss_stranded file={} origin={:?} {}",
                    file_path, miss.origin, provenance
                ),
            );
            anyhow::bail!(
                "startup-miss for {} remains unresolved on alive pane {}: {}. The last session never recorded a child exit or session_end, so route will not auto-start a replacement pane over a stranded session",
                file.display(),
                registered_pane,
                provenance
            );
        }
        if startup_miss_requires_fresh_start(startup_facts)
            || startup_miss_should_restart_live_owner(startup_facts)
        {
            eprintln!(
                "[route] registered pane {} has an unresolved startup-miss marker from {} for {} — deregistering and starting fresh",
                registered_pane, miss_ts, file_path
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_startup_miss_deregistered file={} pane={} miss_timestamp={}",
                    file_path, registered_pane, miss_ts
                ),
            );
            let _ = deregister_dispatch_registration(file_path, session_id)?;
            let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
            eprintln!("[route] No active pane found, auto-starting...");
            if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
                anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
            }
            fail_if_recent_session_loss_window(file, session_id)?;
            let split_before = is_first_column(file, col_args);
            ensure_auto_start_target_session(tmux, None, target_session, harness)?;
            return crate::startup::auto_start_in_session(
                tmux,
                file,
                session_id,
                file_path,
                target_session,
                false,
                split_before,
                harness,
                Some(registered_pane.as_str()),
                Some(created_panes),
                false,
                None,
                effects.route_startup_effects,
            );
        }

        if startup_miss_superseded_by_later_open_start(startup_facts) {
            eprintln!(
                "[route] registered pane {} proves a newer open harness run after startup-miss {} for {} — clearing stale marker",
                registered_pane, miss_ts, file_path
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_startup_miss_cleared_live_owner file={} pane={} miss_timestamp={}",
                    file_path, registered_pane, miss_ts
                ),
            );
            let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
        } else {
            eprintln!(
                "[route] registered pane {} still owns {} but startup-miss {} is not superseded by a newer open harness run — keeping marker until dispatch proves recovery",
                registered_pane, file_path, miss_ts
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_startup_miss_retained_live_owner file={} pane={} miss_timestamp={}",
                    file_path, registered_pane, miss_ts
                ),
            );
        }
    }

    if let Some(ref registered_pane) = registered {
        if tmux.pane_alive(registered_pane) {
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
                        "route_associated_pane_ambiguous file={} count={}",
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
                        "route_associated_pane_requires_manual_claim file={} pane={} sources={}",
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
            let mut stale_registration_cleared = false;
            match live_owner.as_deref() {
                Some(_) => {}
                None => match supervisor_health {
                    SupervisorHealth::Healthy => {
                        eprintln!(
                            "[route] registered pane {} has a healthy supervisor for {} despite missing actor/registered-owner proof — reusing registered pane",
                            registered_pane, file_path
                        );
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_registered_pane_reused_via_supervisor file={} pane={} health=healthy",
                                file_path, registered_pane
                            ),
                        );
                    }
                    SupervisorHealth::Restartable => {
                        eprintln!(
                            "[route] registered pane {} has a restartable supervisor for {} — restarting in place",
                            registered_pane, file_path
                        );
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_registered_pane_restart_via_supervisor file={} pane={}",
                                file_path, registered_pane
                            ),
                        );
                        if restart_via_supervisor(file, session_id) {
                            if let Err(e) = tmux.select_pane(registered_pane) {
                                eprintln!(
                                    "[route] warning: failed to focus restarted pane {}: {}",
                                    registered_pane, e
                                );
                            }
                            require_routed_admission_projection(
                                tmux,
                                file,
                                registered_pane,
                                session_id,
                                file_path,
                                harness,
                                cycle_baseline.as_ref(),
                                pending_prompt_context
                                    .as_ref()
                                    .map(|context| context.marker.as_str()),
                                false,
                                RoutedDispatchStartProof::CommandAcceptedOnly,
                                // `#jbroutasync`: cap admission-projection waits
                                // at the command deadline carried by the route
                                // invocation.
                                crate::invocation::wait_for_ready_override(),
                                effects.route_admission_effects,
                            )?;
                            return Ok(registered_pane.clone());
                        }
                        eprintln!(
                            "[route] supervisor restart failed for pane {} — deregistering and continuing recovery",
                            registered_pane
                        );
                        let provenance = pane_route_provenance(tmux, registered_pane);
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_registered_pane_restart_failed file={} {}",
                                file_path, provenance
                            ),
                        );
                        let _ = deregister_dispatch_registration(file_path, session_id)?;
                        stale_registration_cleared = true;
                    }
                    SupervisorHealth::Halted { restart_count } => {
                        let provenance = pane_route_provenance(tmux, registered_pane);
                        eprintln!(
                            "[route] registered pane {} for {} has a halted supervisor after {} restarts — refusing automatic restart",
                            registered_pane, file_path, restart_count
                        );
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_registered_pane_halted file={} pane={} restart_count={} {}",
                                file_path, registered_pane, restart_count, provenance
                            ),
                        );
                        anyhow::bail!(
                            "registered pane {} for {} has a halted supervisor after {} restarts; route will not auto-restart or replace it automatically. Inspect the pane, then run `agent-doc start {}` manually to recover",
                            registered_pane,
                            file.display(),
                            restart_count,
                            file.display()
                        );
                    }
                    SupervisorHealth::Unreachable | SupervisorHealth::NoSocket => {
                        let provenance = pane_route_provenance(tmux, registered_pane);
                        eprintln!(
                            "[route] registered pane {} is alive but no actor/registered owner for {} was proven and supervisor is unavailable — deregistering stale entry and continuing recovery",
                            registered_pane, file_path
                        );
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_registered_pane_deregistered_no_live_owner file={} {}",
                                file_path, provenance
                            ),
                        );
                        let _ = deregister_dispatch_registration(file_path, session_id)?;
                        stale_registration_cleared = true;
                    }
                },
            }
            if !stale_registration_cleared {
                rescue_from_stash(
                    tmux,
                    registered_pane,
                    session_id,
                    file_path,
                    target_session,
                    is_first_column(file, col_args),
                );
                let registered_pane = reapply_codex_launch_contract_before_reuse(
                    tmux,
                    file,
                    registered_pane,
                    session_id,
                    file_path,
                    harness,
                    true,
                    true,
                )?;
                register_dispatch_target(tmux, session_id, &registered_pane, file_path)?;
                let supervisor_recovered_without_path_owner =
                    live_owner.is_none() && matches!(supervisor_health, SupervisorHealth::Healthy);
                match ensure_existing_pane_ready_for_dispatch(
                    tmux,
                    file,
                    &registered_pane,
                    harness,
                    pending_prompt_context
                        .as_ref()
                        .map(|context| context.marker.as_str()),
                )? {
                    ExistingPaneDispatchReadiness::Ready => {}
                    ExistingPaneDispatchReadiness::BusyAlreadyRunning
                        if supervisor_recovered_without_path_owner =>
                    {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_registered_pane_dispatch_via_healthy_supervisor file={} pane={} reason=missing_path_owner_prompt_probe_not_authoritative",
                                file_path, registered_pane
                            ),
                        );
                    }
                    ExistingPaneDispatchReadiness::BusyAlreadyRunning => {
                        return Ok(registered_pane);
                    }
                    ExistingPaneDispatchReadiness::BusyNeedsAutoFix {
                        provenance,
                        blocker_reason,
                    } => {
                        return retry_route_after_busy_pane_auto_fix(
                            tmux,
                            file,
                            session_id,
                            file_path,
                            harness,
                            cycle_baseline.as_ref(),
                            pending_prompt_context
                                .as_ref()
                                .map(|context| context.marker.as_str()),
                            allow_auto_fix_retry,
                            allow_busy_interrupt_retry,
                            auto_fix_attempted,
                            &registered_pane,
                            &provenance,
                            blocker_reason.as_deref(),
                            |next_allow_auto_fix_retry,
                             next_allow_busy_interrupt_retry,
                             next_auto_fix_attempted| {
                                resolve_or_create_pane_with_auto_fix_retry(
                                    tmux,
                                    file,
                                    pane,
                                    col_args,
                                    session_id,
                                    file_path,
                                    target_session,
                                    harness,
                                    created_panes,
                                    effects,
                                    next_allow_auto_fix_retry,
                                    next_allow_busy_interrupt_retry,
                                    next_auto_fix_attempted,
                                )
                            },
                            effects.route_busy_pane_retry_effects,
                        );
                    }
                }
                register_dispatch_target(tmux, session_id, &registered_pane, file_path)?;
                eprintln!("[route] Pane {} is alive, sending command", registered_pane);
                let dispatch_start = dispatch_existing_managed_reopen(
                    tmux,
                    file,
                    session_id,
                    &registered_pane,
                    file_path,
                    harness,
                    effects.route_dispatch_effects,
                )?;
                require_routed_admission_projection(
                    tmux,
                    file,
                    &registered_pane,
                    session_id,
                    file_path,
                    harness,
                    cycle_baseline.as_ref(),
                    pending_prompt_context
                        .as_ref()
                        .map(|context| context.marker.as_str()),
                    true,
                    dispatch_start,
                    // `#jbroutasync`: cap admission-projection waits at the
                    // command deadline carried by the route invocation.
                    crate::invocation::wait_for_ready_override(),
                    effects.route_admission_effects,
                )?;
                return Ok(registered_pane);
            }
        }
        eprintln!("[route] Pane {} is dead", registered_pane);
    } else {
        eprintln!(
            "[route] No pane registered for session {}",
            &session_id[..std::cmp::min(8, session_id.len())]
        );
    }

    let claimed_panes: std::collections::HashSet<String> = load_dispatch_registry(file_path)
        .unwrap_or_default()
        .values()
        .filter(|e| tmux.pane_alive(&e.pane))
        .map(|e| e.pane.clone())
        .collect();
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
                "route_associated_pane_ambiguous file={} count={}",
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
                "route_associated_pane_requires_manual_claim file={} pane={} sources={}",
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
    if registered.is_some()
        && let Some(new_pane) = find_target_pane(tmux, pane, target_session, &claimed_panes)
        && is_agent_process(tmux, &new_pane, harness)
    {
        eprintln!("[route] Lazy-claiming to pane {} (dead pane)", new_pane);
        register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
        match ensure_existing_pane_ready_for_dispatch(
            tmux,
            file,
            &new_pane,
            harness,
            pending_prompt_context
                .as_ref()
                .map(|context| context.marker.as_str()),
        )? {
            ExistingPaneDispatchReadiness::Ready => {}
            ExistingPaneDispatchReadiness::BusyAlreadyRunning => return Ok(new_pane),
            ExistingPaneDispatchReadiness::BusyNeedsAutoFix {
                provenance,
                blocker_reason,
            } => {
                return retry_route_after_busy_pane_auto_fix(
                    tmux,
                    file,
                    session_id,
                    file_path,
                    harness,
                    cycle_baseline.as_ref(),
                    pending_prompt_context
                        .as_ref()
                        .map(|context| context.marker.as_str()),
                    allow_auto_fix_retry,
                    allow_busy_interrupt_retry,
                    auto_fix_attempted,
                    &new_pane,
                    &provenance,
                    blocker_reason.as_deref(),
                    |next_allow_auto_fix_retry,
                     next_allow_busy_interrupt_retry,
                     next_auto_fix_attempted| {
                        resolve_or_create_pane_with_auto_fix_retry(
                            tmux,
                            file,
                            pane,
                            col_args,
                            session_id,
                            file_path,
                            target_session,
                            harness,
                            created_panes,
                            effects,
                            next_allow_auto_fix_retry,
                            next_allow_busy_interrupt_retry,
                            next_auto_fix_attempted,
                        )
                    },
                    effects.route_busy_pane_retry_effects,
                );
            }
        }
        register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
        let dispatch_start = dispatch_existing_managed_reopen(
            tmux,
            file,
            session_id,
            &new_pane,
            file_path,
            harness,
            effects.route_dispatch_effects,
        )?;
        let admission_pane = require_routed_admission_projection(
            tmux,
            file,
            &new_pane,
            session_id,
            file_path,
            harness,
            cycle_baseline.as_ref(),
            pending_prompt_context
                .as_ref()
                .map(|context| context.marker.as_str()),
            false,
            dispatch_start,
            // `#jbroutasync`: cap admission-projection waits at the command
            // deadline carried by the route invocation.
            crate::invocation::wait_for_ready_override(),
            effects.route_admission_effects,
        )?;
        return Ok(admission_pane.unwrap_or(new_pane));
    }

    let late_associated_resolution = agent_doc_tmux::resolve_associated_panes(
        agent_doc_sync_io::sync::find_associated_panes(tmux, file, session_id),
        tmux.active_window(target_session).as_deref(),
    );
    if let agent_doc_tmux::AssociatedPaneResolution::Ambiguous(candidates) =
        &late_associated_resolution
    {
        let error = agent_doc_tmux::format_associated_pane_resolution_error(
            file.display(),
            candidates,
            tmux.active_window(target_session).as_deref(),
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_associated_pane_ambiguous_late file={} count={}",
                file_path,
                candidates.len()
            ),
        );
        anyhow::bail!(error);
    }
    if let agent_doc_tmux::AssociatedPaneResolution::Selected { winner, redundant } =
        &late_associated_resolution
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_associated_pane_requires_manual_claim_late file={} pane={} sources={}",
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
        false,
        None,
        effects.route_startup_effects,
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
        StartingPaneRecoveryWaitOptions {
            initial_status: initial_status.as_ref(),
            max_wait: None,
        },
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
            intent: agent_doc_controller::dispatch::AuthoritativeActorDispatchIntent::PromptAware,
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
    route_admission_effects: RouteAdmissionEffects,
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
    let admission_pane = require_routed_admission_projection(
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
        // `#jbroutasync`: cap admission-projection waits at the command deadline
        // carried by the route invocation.
        crate::invocation::wait_for_ready_override(),
        route_admission_effects,
    )?;
    Ok(admission_pane.unwrap_or_else(|| pane.to_string()))
}

#[derive(Debug, Clone, Copy)]
pub struct RouteBusyPaneRetryEffects {
    pub route_dispatch_effects: RouteDispatchEffects,
    pub route_admission_effects: RouteAdmissionEffects,
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
                        effects.route_admission_effects,
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
                        effects.route_admission_effects,
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
            effects.route_admission_effects,
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
    actor_state: agent_doc_controller::actor::ActorState,
) -> DispatchActorState {
    match actor_state {
        agent_doc_controller::actor::ActorState::Ready => DispatchActorState::Ready,
        agent_doc_controller::actor::ActorState::Busy => DispatchActorState::Busy,
        _ => DispatchActorState::Other,
    }
}
