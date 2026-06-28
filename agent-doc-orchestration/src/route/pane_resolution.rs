//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn recover_dispatch_only_authoritative_waiting_input(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    target_session: &str,
    split_before: bool,
    harness: &HarnessConfig,
    pane: &str,
    generation: u64,
) -> Result<String> {
    crate::ops_log::log_op(
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
    let initial_status = crate::startup_miss::session_log_status(file, session_id)
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
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_or_create_pane_dispatch_only(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
) -> Result<String> {
    let registered = lookup_dispatch_registration(file_path, session_id)?;
    let cycle_baseline = crate::cycle_state::load(file)?;
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
        .filter(|actor| authoritative_actor_dispatch_target_eligible(actor))
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
            actor.clone(),
        );
    }
    let live_owner = if registered.is_some() {
        crate::sync::find_normal_path_owner_pane(tmux, file, session_id)
    } else {
        None
    };
    let preferred_active_window = tmux.active_window(target_session);
    let associated_candidates = crate::sync::find_associated_panes(tmux, file, session_id);
    let associated_resolution = crate::sync::resolve_associated_panes(
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
        && let Some(reason) = authoritative_actor_dispatch_guard_reason(&actor.runtime)
    {
        if dispatch_only_can_use_degraded_authoritative_actor(
            actor,
            registered.as_deref(),
            live_owner.as_deref(),
        ) {
            let dispatch_pane = actor.record.pane_id.clone();
            let file_display = file.display().to_string();
            let supervisor_health = supervisor_health_label(actor.runtime.health);
            crate::ops_log::log_op(
                file,
                &degraded_authoritative_actor_direct_submit_log_message(
                    DegradedAuthoritativeActorDirectSubmit {
                        file_display: file_display.as_str(),
                        pane_id: dispatch_pane.as_str(),
                        harness_binary: harness.binary.as_str(),
                        generation: actor.record.generation,
                        record_state: actor.record.state.as_str(),
                        supervisor_health: supervisor_health.as_str(),
                        runtime_actor_state: runtime_actor_state_label(&actor.runtime),
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
                true,
            );
        }

        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_authoritative_fallback_skipped file={} actor_pane={} harness={} generation={} record_state={} supervisor_health={} runtime_actor_state={} registered_pane={} live_owner={} reason={}",
                file.display(),
                actor.record.pane_id,
                harness.binary,
                actor.record.generation,
                actor.record.state.as_str(),
                supervisor_health_label(actor.runtime.health),
                runtime_actor_state_label(&actor.runtime),
                registered.as_deref().unwrap_or("none"),
                live_owner.as_deref().unwrap_or("none"),
                reason
            ),
        );
    }

    if let Some(ref registered_pane) = registered
        && tmux.pane_alive(registered_pane)
    {
        if let crate::sync::AssociatedPaneResolution::Ambiguous(candidates) = &associated_resolution
        {
            let error = format_associated_pane_resolution_error(
                file,
                candidates,
                preferred_active_window.as_deref(),
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_only_associated_pane_ambiguous file={} count={}",
                    file_path,
                    candidates.len()
                ),
            );
            anyhow::bail!(error);
        }
        if let crate::sync::AssociatedPaneResolution::Selected { winner, redundant } =
            &associated_resolution
            && winner.pane_id != *registered_pane
        {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_only_associated_pane_requires_manual_claim file={} pane={} sources={}",
                    file_path,
                    winner.pane_id,
                    winner.source_summary()
                ),
            );
            anyhow::bail!(format_associated_pane_selected_error(
                file, winner, redundant
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
            false,
        );
    }

    if let crate::sync::AssociatedPaneResolution::Ambiguous(candidates) = &associated_resolution {
        let error = format_associated_pane_resolution_error(
            file,
            candidates,
            preferred_active_window.as_deref(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_associated_pane_ambiguous file={} count={}",
                file_path,
                candidates.len()
            ),
        );
        anyhow::bail!(error);
    }

    if let crate::sync::AssociatedPaneResolution::Selected { winner, redundant } =
        &associated_resolution
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_associated_pane_requires_manual_claim file={} pane={} sources={}",
                file_path,
                winner.pane_id,
                winner.source_summary()
            ),
        );
        anyhow::bail!(format_associated_pane_selected_error(
            file, winner, redundant
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
    auto_start_in_session(
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
    )
}

/// Resolve an existing pane or create a new one. Returns the pane ID.
///
/// Three resolution strategies, tried in order:
/// 1. Alive registered pane → unconditionally send command. Pane IDs are
///    globally unique per tmux server, so session matching is not required.
/// 2. Lazy claim to an active pane (when registered pane is dead)
/// 3. Auto-start a new agent session
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_or_create_pane(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
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
        true,
        true,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_or_create_pane_with_auto_fix_retry(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
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
    let cycle_baseline = crate::cycle_state::load(file)?;
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
            actor,
        );
    }
    let live_owner = if registered.is_some() {
        crate::sync::find_normal_path_owner_pane(tmux, file, session_id)
    } else {
        None
    };
    let supervisor_health = if registered.is_some() {
        query_supervisor_health(file, session_id)
    } else {
        SupervisorHealth::NoSocket
    };
    let preferred_active_window = tmux.active_window(target_session);
    let associated_candidates = crate::sync::find_associated_panes(tmux, file, session_id);
    let associated_resolution = crate::sync::resolve_associated_panes(
        associated_candidates.clone(),
        preferred_active_window.as_deref(),
    );

    if let Ok(Some(miss)) = crate::startup_miss::load(file)
        && let Some(supersession) =
            crate::startup_miss::superseded_by_newer_registered_start(file, &miss)?
    {
        let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
        eprintln!(
            "[route] startup-miss on pane {} from {} for {} is superseded by newer registered owner {} — clearing stale marker",
            miss.pane_id, miss_ts, file_path, supersession.registered_pane
        );
        crate::ops_log::log_op(
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
        let _ = crate::startup_miss::clear(file);
    }

    // Strategy 0: If a previous startup-miss was recorded for the registered pane,
    // deregister it immediately so we fall through to auto-start instead of
    // reusing a pane that never successfully started a document cycle.
    if let Some(ref registered_pane) = registered
        && let Ok(Some(miss)) = crate::startup_miss::load(file)
        && miss.pane_id == *registered_pane
        && tmux.pane_alive(registered_pane)
    {
        let log_status = crate::startup_miss::session_log_status(file, &miss.session_id)
            .ok()
            .flatten();
        let miss_ts = crate::startup_miss::format_timestamp(miss.timestamp);
        let provenance = startup_miss_route_provenance(
            tmux,
            registered_pane,
            live_owner.as_deref(),
            supervisor_health,
            log_status.as_ref(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_startup_miss_detected file={} origin={:?} miss_timestamp={} {}",
                file_path, miss.origin, miss_ts, provenance
            ),
        );
        if startup_miss_should_fail_closed(
            true,
            registered_pane,
            live_owner.as_deref(),
            supervisor_health,
            log_status.as_ref(),
        ) {
            eprintln!(
                "[route] startup-miss for {} is stranded, not crashed: {}",
                file_path, provenance
            );
            crate::ops_log::log_op(
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
        if startup_miss_requires_fresh_start(
            registered_pane,
            live_owner.as_deref(),
            supervisor_health,
        ) || startup_miss_should_restart_live_owner(
            &miss,
            registered_pane,
            live_owner.as_deref(),
            log_status.as_ref(),
        ) {
            eprintln!(
                "[route] registered pane {} has an unresolved startup-miss marker from {} for {} — deregistering and starting fresh",
                registered_pane, miss_ts, file_path
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_startup_miss_deregistered file={} pane={} miss_timestamp={}",
                    file_path, registered_pane, miss_ts
                ),
            );
            let _ = deregister_dispatch_registration(file_path, session_id)?;
            let _ = crate::startup_miss::clear(file);
            // Fall through to Strategy 3 (auto-start)
            eprintln!("[route] No active pane found, auto-starting...");
            if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
                anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
            }
            fail_if_recent_session_loss_window(file, session_id)?;
            let split_before = is_first_column(file, col_args);
            ensure_auto_start_target_session(tmux, None, target_session, harness)?;
            return auto_start_in_session(
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
            );
        }

        if startup_miss_superseded_by_later_open_start(&miss, registered_pane, log_status.as_ref())
        {
            eprintln!(
                "[route] registered pane {} proves a newer open harness run after startup-miss {} for {} — clearing stale marker",
                registered_pane, miss_ts, file_path
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_startup_miss_cleared_live_owner file={} pane={} miss_timestamp={}",
                    file_path, registered_pane, miss_ts
                ),
            );
            let _ = crate::startup_miss::clear(file);
        } else {
            eprintln!(
                "[route] registered pane {} still owns {} but startup-miss {} is not superseded by a newer open harness run — keeping marker until dispatch proves recovery",
                registered_pane, file_path, miss_ts
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_startup_miss_retained_live_owner file={} pane={} miss_timestamp={}",
                    file_path, registered_pane, miss_ts
                ),
            );
        }
    }

    // Strategy 1: Alive registered pane — reuse only when the authoritative
    // actor projection or the registered supervisor path still proves the
    // document is running there. Pane IDs (%N) are globally unique per tmux
    // server, so target_session matching stays irrelevant once ownership is
    // proven.
    //
    // rescue_from_stash self-gates on target_session match, so it is a no-op
    // when the pane is in a different session — we leave it in place.
    if let Some(ref registered_pane) = registered {
        if tmux.pane_alive(registered_pane) {
            if let crate::sync::AssociatedPaneResolution::Ambiguous(candidates) =
                &associated_resolution
            {
                let error = format_associated_pane_resolution_error(
                    file,
                    candidates,
                    preferred_active_window.as_deref(),
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_associated_pane_ambiguous file={} count={}",
                        file_path,
                        candidates.len()
                    ),
                );
                anyhow::bail!(error);
            }
            if let crate::sync::AssociatedPaneResolution::Selected { winner, redundant } =
                &associated_resolution
                && winner.pane_id != *registered_pane
            {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_associated_pane_requires_manual_claim file={} pane={} sources={}",
                        file_path,
                        winner.pane_id,
                        winner.source_summary()
                    ),
                );
                anyhow::bail!(format_associated_pane_selected_error(
                    file, winner, redundant
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
                        crate::ops_log::log_op(
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
                        crate::ops_log::log_op(
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
                            require_routed_cycle_ack(
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
                            )?;
                            return Ok(registered_pane.clone());
                        }
                        eprintln!(
                            "[route] supervisor restart failed for pane {} — deregistering and continuing recovery",
                            registered_pane
                        );
                        let provenance = pane_route_provenance(tmux, registered_pane);
                        crate::ops_log::log_op(
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
                        crate::ops_log::log_op(
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
                        crate::ops_log::log_op(
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
                        crate::ops_log::log_op(
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
                            pane,
                            col_args,
                            session_id,
                            file_path,
                            target_session,
                            harness,
                            created_panes,
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
                )?;
                require_routed_cycle_ack(
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

    // Strategy 2: Lazy claim (only when a registered pane died)
    // Skip panes running non-agent processes to avoid claiming corky/shells.
    // Also skip panes already claimed by another document (pane theft prevention).
    let claimed_panes: std::collections::HashSet<String> = load_dispatch_registry(file_path)
        .unwrap_or_default()
        .values()
        .filter(|e| tmux.pane_alive(&e.pane))
        .map(|e| e.pane.clone())
        .collect();
    if let crate::sync::AssociatedPaneResolution::Ambiguous(candidates) = &associated_resolution {
        let error = format_associated_pane_resolution_error(
            file,
            candidates,
            preferred_active_window.as_deref(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_associated_pane_ambiguous file={} count={}",
                file_path,
                candidates.len()
            ),
        );
        anyhow::bail!(error);
    }
    if let crate::sync::AssociatedPaneResolution::Selected { winner, redundant } =
        &associated_resolution
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_associated_pane_requires_manual_claim file={} pane={} sources={}",
                file_path,
                winner.pane_id,
                winner.source_summary()
            ),
        );
        anyhow::bail!(format_associated_pane_selected_error(
            file, winner, redundant
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
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
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
                );
            }
        }
        register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
        let dispatch_start = dispatch_existing_managed_reopen(
            tmux, file, session_id, &new_pane, file_path, harness,
        )?;
        let ack_pane = require_routed_cycle_ack(
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
        )?;
        return Ok(ack_pane.unwrap_or(new_pane));
    }

    // Strategy 3: Auto-start
    // Re-check associated panes after the earlier recovery branches. A stale
    // registered pane can be deregistered while a live legacy owner becomes
    // provable a little later in the turn; the normal path must still fail
    // closed instead of silently re-electing that pane via auto-start.
    let late_associated_resolution = crate::sync::resolve_associated_panes(
        crate::sync::find_associated_panes(tmux, file, session_id),
        tmux.active_window(target_session).as_deref(),
    );
    if let crate::sync::AssociatedPaneResolution::Ambiguous(candidates) =
        &late_associated_resolution
    {
        let error = format_associated_pane_resolution_error(
            file,
            candidates,
            tmux.active_window(target_session).as_deref(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_associated_pane_ambiguous_late file={} count={}",
                file_path,
                candidates.len()
            ),
        );
        anyhow::bail!(error);
    }
    if let crate::sync::AssociatedPaneResolution::Selected { winner, redundant } =
        &late_associated_resolution
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_associated_pane_requires_manual_claim_late file={} pane={} sources={}",
                file_path,
                winner.pane_id,
                winner.source_summary()
            ),
        );
        anyhow::bail!(format_associated_pane_selected_error(
            file, winner, redundant
        ));
    }

    eprintln!("[route] No active pane found, auto-starting...");
    if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
        anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
    }
    fail_if_recent_session_loss_window(file, session_id)?;
    let split_before = is_first_column(file, col_args);
    ensure_auto_start_target_session(tmux, None, target_session, harness)?;
    auto_start_in_session(
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
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn retry_route_after_busy_pane_auto_fix(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
    cycle_baseline: Option<&crate::cycle_state::CycleState>,
    prompt_bearing_marker: Option<&str>,
    allow_auto_fix_retry: bool,
    allow_busy_interrupt_retry: bool,
    auto_fix_attempted: bool,
    busy_pane: &str,
    provenance: &str,
    blocker_reason: Option<&str>,
) -> Result<String> {
    let fallback_detail = blocker_reason.map(|reason| format!("still shows {reason}"));
    if allow_auto_fix_retry {
        match attempt_busy_existing_pane_auto_fix(tmux, file, session_id, busy_pane, file_path)? {
            BusyPaneAutoFixOutcome::RetryRoute => {
                return resolve_or_create_pane_with_auto_fix_retry(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                );
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart => {
                wait_for_busy_restart_handoff(tmux, file, file_path, session_id, busy_pane);
                return resolve_or_create_pane_with_auto_fix_retry(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                );
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart => {
                crate::ops_log::log_op(
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
                    emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
                    anyhow::bail!(format_busy_existing_pane_error(
                        file,
                        busy_pane,
                        harness,
                        provenance,
                        fallback_detail.as_deref(),
                        true
                    ));
                }
                wait_for_busy_restart_handoff(tmux, file, file_path, session_id, busy_pane);
                return resolve_or_create_pane_with_auto_fix_retry(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                );
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
                return resolve_or_create_pane_with_auto_fix_retry(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    false,
                    false,
                    true,
                );
            }
            BusyPaneInterruptRecoveryOutcome::Blocked { reason } => {
                emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
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
                    );
                }
                anyhow::bail!(format_busy_existing_pane_error(
                    file,
                    busy_pane,
                    harness,
                    provenance,
                    Some(detail.as_str()),
                    auto_fix_attempted || allow_auto_fix_retry
                ));
            }
            BusyPaneInterruptRecoveryOutcome::TimedOut => {
                emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
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
                    );
                }
                anyhow::bail!(format_busy_existing_pane_error(
                    file,
                    busy_pane,
                    harness,
                    provenance,
                    Some("bounded interrupt recovery never restored a dispatch-ready prompt"),
                    auto_fix_attempted || allow_auto_fix_retry
                ));
            }
            BusyPaneInterruptRecoveryOutcome::Skipped => {}
        }
    }
    if harness.binary == "codex" && tmux.pane_alive(busy_pane) {
        emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
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
                .unwrap_or("still not showing an idle prompt"),
        );
    }
    anyhow::bail!(format_busy_existing_pane_error(
        file,
        busy_pane,
        harness,
        provenance,
        fallback_detail.as_deref(),
        auto_fix_attempted || allow_auto_fix_retry
    ));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn optimistic_busy_pane_dispatch(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    cycle_baseline: Option<&crate::cycle_state::CycleState>,
    prompt_bearing_marker: Option<&str>,
    detail: &str,
) -> Result<String> {
    crate::ops_log::log_op(
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
    let dispatch_start =
        dispatch_existing_managed_reopen(tmux, file, session_id, pane, file_path, harness)?;
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
    )?;
    Ok(ack_pane.unwrap_or_else(|| pane.to_string()))
}

pub(crate) fn wait_for_busy_restart_handoff(
    tmux: &Tmux,
    file: &Path,
    file_path: &str,
    session_id: &str,
    previous_pane: &str,
) {
    let registry_base_dir = registry_base_dir_for_dispatch(file_path);
    let timeout = if cfg!(test) {
        Duration::from_secs(20)
    } else {
        Duration::from_secs(5)
    };
    let poll = Duration::from_millis(100);
    let start = std::time::Instant::now();
    let mut handed_off_pane: Option<String> = None;
    while start.elapsed() < timeout {
        if let Ok(registry) = sessions::load_in(&registry_base_dir)
            && let Some(entry) = registry
                .values()
                .find(|entry| entry.session_id == session_id)
            && !entry.pane.is_empty()
        {
            if entry.pane != previous_pane {
                handed_off_pane = Some(entry.pane.clone());
                if crate::sync::find_normal_path_owner_pane(tmux, file, session_id).as_deref()
                    == Some(entry.pane.as_str())
                {
                    eprintln!(
                        "[route] supervisor restart handed {} from pane {} to authoritative pane {} before retry",
                        file_path, previous_pane, entry.pane
                    );
                    return;
                }
            } else {
                handed_off_pane = None;
            }
        }
        match crate::sync::resolve_associated_panes(
            crate::sync::find_associated_panes(tmux, file, session_id),
            None,
        ) {
            crate::sync::AssociatedPaneResolution::Selected { winner, .. }
                if winner.pane_id != previous_pane && !winner.is_stash() =>
            {
                if let Err(err) =
                    register_dispatch_target(tmux, session_id, &winner.pane_id, file_path)
                {
                    eprintln!(
                        "[route] warning: failed to project restart handoff pane {} into the registry for {}: {}",
                        winner.pane_id, file_path, err
                    );
                }
                eprintln!(
                    "[route] supervisor restart for {} has not refreshed the registry yet, but a unique associated pane {} is alive via {} — adopting it as the handoff target before retry",
                    file_path,
                    winner.pane_id,
                    winner.source_summary()
                );
                return;
            }
            _ => {}
        }
        std::thread::sleep(poll);
    }
    if let Some(pane) = handed_off_pane {
        eprintln!(
            "[route] supervisor restart handed {} from pane {} to authoritative pane {} before retry, but live-owner proof is still catching up",
            file_path, previous_pane, pane
        );
    }
}

/// Rescue a pane from a stash window back to the agent-doc window.
/// Only rescues if the pane is in the target session — never swaps across sessions.
///
/// Returns `true` when the pane was actually moved out of a stash window so callers
/// can re-evaluate state that depends on pane location (e.g. Starting→Ready
/// promotion after the rescue makes the pane visible). Returns `false` when the
/// rescue was a no-op (pane not in stash, or session guard tripped).
pub(crate) fn rescue_from_stash(
    tmux: &Tmux,
    pane_id: &str,
    session_id: &str,
    file_path: &str,
    target_session: &str,
    split_before: bool,
) -> bool {
    // Session guard: only rescue within the target session
    let pane_session = pane_session_name(tmux, pane_id).unwrap_or_default();
    if pane_session != target_session {
        eprintln!(
            "[route] Pane {} is in session '{}', not target '{}' — skipping stash rescue",
            pane_id, pane_session, target_session
        );
        return false;
    }

    let pane_win_name = pane_window_name(tmux, pane_id).unwrap_or_default();

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
            match sessions::join_pane_guarded(tmux, pane_id, target, target_session, join_flag) {
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

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::flow::routed_reopen::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
    use crate::supervisor::ipc::{IpcMethod, IpcResponse, SupervisorIpc};
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_waits_longer_for_live_child_cycle_ack() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-child-extended-ack");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-child-extended-ack.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let mock_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-child-extended-ack";
        sessions::register(session_id, &pane, &file_path).unwrap();
        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Restart { .. }
            | IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let doc_for_thread = doc.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1300));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current)).unwrap();
        });

        let routed = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should tolerate a delayed but real live-child cycle start");
        assert_eq!(routed, pane);
        assert_eq!(
            *injects.lock().unwrap(),
            vec![routed_trigger_submit_payload(
                &HarnessConfig::codex().trigger_command(&file_path)
            )],
            "route should dispatch the bare Codex reopen through supervisor IPC before waiting for the delayed live-child ack"
        );

        let state = crate::cycle_state::load(&doc)
            .unwrap()
            .expect("cycle state should exist after delayed ack");
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_keeps_live_child_reroute_optimistic_when_cycle_ack_is_missing() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-child-skip-ack");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-owner-reregister.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let mock_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        sessions::register("route-live-child-skip", &pane, &file_path).unwrap();
        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc =
            SupervisorIpc::start(
                dir.path(),
                "route-live-child-skip",
                move |method| match method {
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        injects_for_ipc.lock().unwrap().push(bytes.clone());
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                    IpcMethod::Restart { .. }
                    | IpcMethod::Stop { .. }
                    | IpcMethod::StopAgent { .. }
                    | IpcMethod::ReplicaRegister { .. }
                    | IpcMethod::ReplicaDeregister { .. }
                    | IpcMethod::ReplicaUpdate { .. }
                    | IpcMethod::ReplicaPull { .. }
                    | IpcMethod::ReplicaAck { .. }
                    | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
                },
            )
            .unwrap();

        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-live-child-skip",
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should stay optimistic when the correct live Codex pane accepts the reopen");
        assert_eq!(resolved, pane);
        let injects = injects.lock().unwrap().clone();
        assert!(
            !injects.is_empty()
                && injects.iter().all(|inject| {
                    inject
                        == &routed_trigger_submit_payload(
                            &HarnessConfig::codex().trigger_command(&file_path),
                        )
                }),
            "route should still dispatch the trigger through supervisor IPC before accepting the optimistic startup-miss path: {injects:?}"
        );
        let miss = crate::startup_miss::load(&doc)
            .unwrap()
            .expect("optimistic route should still record a startup miss");
        assert_eq!(miss.pane_id, pane);
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_retries_fresh_restart_after_live_codex_ack_timeout() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-codex-fresh-retry");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-codex-fresh-retry.md");
        let snapshot = "---\nagent: codex\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let stale_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &stale_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-codex-fresh-retry";
        sessions::register(session_id, &pane, &file_path).unwrap();

        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let supervisor_instance_id = "busy-reroute-supervisor".to_string();
        let supervisor_instance_id_for_ipc = supervisor_instance_id.clone();
        let ipc_tmux = iso.clone();
        let injected_pane = Arc::new(std::sync::Mutex::new(None::<String>));
        let injected_pane_for_ipc = injected_pane.clone();
        *injected_pane.lock().unwrap() = Some(pane.clone());
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": true,
                        "state": "healthy",
                        "restart_count": 0,
                        "actor_state": "ready",
                        "supervisor_pid": 12345,
                        "supervisor_instance_id": supervisor_instance_id_for_ipc
                    })),
                    IpcMethod::Restart { mode } => {
                        if mode == "fresh" {
                            restart_called_for_ipc.store(true, Ordering::Relaxed);
                        }
                        IpcResponse::ok_empty()
                    }
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        if let Some(target) = injected_pane_for_ipc.lock().unwrap().clone() {
                            let _ = ipc_tmux.send_keys(&target, bytes.trim_end_matches('\n'));
                        }
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::Stop { .. }
                    | IpcMethod::StopAgent { .. }
                    | IpcMethod::ReplicaRegister { .. }
                    | IpcMethod::ReplicaDeregister { .. }
                    | IpcMethod::ReplicaUpdate { .. }
                    | IpcMethod::ReplicaPull { .. }
                    | IpcMethod::ReplicaAck { .. }
                    | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
                }
            })
            .unwrap();

        let iso_for_thread = iso.clone();
        let ready_agent = write_mock_registered_agent_doc(dir.path());
        let doc_for_thread = doc.clone();
        let current_for_thread = current.clone();
        let pane_for_thread = pane.clone();
        let restart_called_for_thread = restart_called.clone();
        std::thread::spawn(move || {
            let wait_start = std::time::Instant::now();
            while !restart_called_for_thread.load(Ordering::Relaxed)
                && wait_start.elapsed() < Duration::from_secs(2)
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            iso_for_thread
                .raw_cmd(&[
                    "respawn-pane",
                    "-k",
                    "-t",
                    &pane_for_thread,
                    &format!(
                        "exec {} {}",
                        ready_agent.display(),
                        doc_for_thread.display()
                    ),
                ])
                .unwrap();
            std::thread::sleep(Duration::from_millis(1200));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
                .unwrap();
        });

        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should retry once after a fresh Codex supervisor restart");
        assert_eq!(resolved, pane);
        assert!(
            restart_called.load(Ordering::Relaxed),
            "route should request a fresh supervisor restart before the retry"
        );

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("GOT:agent-doc "),
            "route should resend the reopen after the fresh restart: {content}"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_restarts_fresh_before_dispatch_after_tracked_codex_clear() {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/codex-hooks/sessions")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-codex-clear-pre-dispatch-restart");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-codex-clear-pre-dispatch-restart.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();

        let stale_agent =
            write_mock_registered_agent_doc_with_prefix(dir.path(), "agent-doc-stale", "STALE");
        launch_mock_registered_agent_doc(&iso, &pane, &stale_agent, &doc);

        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-codex-clear-pre-dispatch-restart";
        sessions::register(session_id, &pane, &file_path).unwrap();

        let state_path = dir
            .path()
            .join(".agent-doc/codex-hooks/sessions/clear.json");
        std::fs::write(
            &state_path,
            serde_json::json!({
                "session_id": "codex-clear-session",
                "doc_path": file_path,
                "last_turn_id": "turn-clear",
                "last_prompt": "/clear",
                "updated_at": 42u64
            })
            .to_string(),
        )
        .unwrap();

        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let supervisor_instance_id = "busy-reroute-supervisor".to_string();
        let supervisor_instance_id_for_ipc = supervisor_instance_id.clone();
        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": true,
                        "state": "healthy",
                        "restart_count": 0,
                        "actor_state": "ready",
                        "supervisor_pid": 12345,
                        "supervisor_instance_id": supervisor_instance_id_for_ipc
                    })),
                    IpcMethod::Restart { mode } => {
                        if mode == "fresh" {
                            restart_called_for_ipc.store(true, Ordering::Relaxed);
                        }
                        IpcResponse::ok_empty()
                    }
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        injects_for_ipc.lock().unwrap().push(bytes.clone());
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::Stop { .. }
                    | IpcMethod::StopAgent { .. }
                    | IpcMethod::ReplicaRegister { .. }
                    | IpcMethod::ReplicaDeregister { .. }
                    | IpcMethod::ReplicaUpdate { .. }
                    | IpcMethod::ReplicaPull { .. }
                    | IpcMethod::ReplicaAck { .. }
                    | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
                }
            })
            .unwrap();

        let iso_for_thread = iso.clone();
        let fresh_agent =
            write_mock_registered_agent_doc_with_prefix(dir.path(), "agent-doc-fresh", "FRESH");
        let doc_for_thread = doc.clone();
        let current_for_thread = current.clone();
        let pane_for_thread = pane.clone();
        let restart_called_for_thread = restart_called.clone();
        std::thread::spawn(move || {
            let wait_start = std::time::Instant::now();
            while !restart_called_for_thread.load(Ordering::Relaxed)
                && wait_start.elapsed() < Duration::from_secs(2)
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            iso_for_thread
                .raw_cmd(&[
                    "respawn-pane",
                    "-k",
                    "-t",
                    &pane_for_thread,
                    &format!(
                        "exec {} {}",
                        fresh_agent.display(),
                        doc_for_thread.display()
                    ),
                ])
                .unwrap();
            std::thread::sleep(Duration::from_millis(1200));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
                .unwrap();
        });

        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should restart fresh before rerouting after a tracked /clear");
        assert_eq!(resolved, pane);
        assert!(
            restart_called.load(Ordering::Relaxed),
            "route should request a fresh restart before dispatch"
        );
        let trigger = HarnessConfig::codex().trigger_command(&file_path);
        let injects = injects.lock().unwrap().clone();
        assert!(
            injects == vec![routed_trigger_submit_payload(&trigger)],
            "route should inject exactly one bare reopen through supervisor IPC after the fresh restart: {injects:?}"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_refuses_busy_registered_pane_before_dispatch_when_prompt_drift_exists()
     {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-pane-busy");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-owner-supervisor-pid.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let mock_agent = write_mock_busy_registered_agent_doc(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", mock_agent.display(), doc.display()),
        );
        let content =
            wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
        assert!(
            content.contains("Working..."),
            "busy mock session should be active in pane: {content}"
        );

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        sessions::register("route-live-pane-busy", &pane, &file_path).unwrap();

        let err = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-live-pane-busy",
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("route should fail closed instead of injecting into a busy live pane");

        let after = sessions::capture_pane(&iso, &pane).unwrap_or_default();
        assert!(
            !after.contains("EARLY:agent-doc "),
            "route should not inject a trigger before the pane becomes idle: {after}"
        );
        assert!(
            err.to_string()
                .contains("bounded interrupt recovery never restored a dispatch-ready prompt"),
            "unexpected error: {err:#}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_dispatch_only_fails_closed_on_busy_registered_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-dispatch-only-busy-pane");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-dispatch-only-busy-pane.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let busy_agent = write_mock_busy_registered_agent_doc(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", busy_agent.display(), doc.display()),
        );
        let content =
            wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
        assert!(
            content.contains("Working..."),
            "busy mock session should be active in pane: {content}"
        );

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        sessions::register("route-dispatch-only-busy-pane", &pane, &file_path).unwrap();

        let err = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            "route-dispatch-only-busy-pane",
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err(
            "dispatch-only route should now fail closed instead of injecting into a busy live pane",
        );

        let after =
            wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(1));
        assert!(
            !after.contains("EARLY:agent-doc "),
            "dispatch-only route must not inject a reopen into the busy authoritative pane: {after}"
        );
        assert!(
            err.to_string()
                .contains("bounded interrupt recovery never restored a dispatch-ready prompt"),
            "unexpected error: {err:#}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_dispatch_only_refuses_while_latest_run_is_still_starting() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-dispatch-only-starting-pane");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("tasks/professional/sampleportal.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();

        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-dispatch-only-starting-pane";
        sessions::register(session_id, &pane, &file_path).unwrap();
        crate::startup_miss::append_session_log_event(
            &doc,
            session_id,
            &format!(
                "session_start file={} pane={} session={}",
                doc.display(),
                pane,
                session_id
            ),
        )
        .unwrap();
        crate::startup_miss::append_session_log_event(
            &doc,
            session_id,
            "codex_start mode=fresh restart_count=0",
        )
        .unwrap();

        let busy_agent = write_mock_active_codex_turn_registered_agent_doc(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", busy_agent.display(), doc.display()),
        );
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "esc to interrupt",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("esc to interrupt"),
            "busy mock session should be active in pane: {content}"
        );
        assert_eq!(
            HarnessConfig::codex()
                .dispatch_blocker_reason(&content)
                .as_deref(),
            Some("active codex turn"),
            "busy mock session should expose the Codex active-turn blocker: {content}"
        );

        let err = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("dispatch-only route must wait for a dispatch-ready prompt during the fresh-start boot window");
        assert!(
            err.to_string()
                .contains("never reached a dispatch-ready prompt"),
            "unexpected startup-window refusal: {err:#}"
        );
        assert!(
            err.to_string()
                .contains("tasks/professional/sampleportal.md"),
            "startup-window refusal should preserve the sample portal document path: {err:#}"
        );
        let after = sessions::capture_pane(&iso, &pane).unwrap_or_default();
        assert!(
            !after.contains("EARLY:agent-doc "),
            "dispatch-only route must not submit through the live pane before the startup prompt is visible: {after}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_dispatch_only_fails_closed_on_reverse_i_search() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-dispatch-only-reverse-i-search");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-dispatch-only-reverse-i-search.md");
        std::fs::write(
            &doc,
            "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n❯ follow-up question\n",
        )
        .unwrap();
        crate::snapshot::save(
        &doc,
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        sessions::register(
            "route-test-dispatch-only-reverse-i-search",
            &pane,
            &file_path,
        )
        .unwrap();

        let busy_agent = write_mock_busy_registered_agent_doc_recovers_on_ctrl_g(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", busy_agent.display(), doc.display()),
        );
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "reverse-i-search",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("reverse-i-search"),
            "dispatch-only blocker test requires a visible reverse-i-search shell state: {content}"
        );

        let err = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            "route-test-dispatch-only-reverse-i-search",
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("dispatch-only route must fail closed on reverse-i-search");
        assert!(
            err.to_string().contains("reverse-i-search"),
            "unexpected error: {err:#}"
        );

        let after = wait_for_pane_contains(
            &iso,
            &pane,
            "reverse-i-search",
            std::time::Duration::from_secs(1),
        );
        assert!(
            !after.contains("GOT:agent-doc "),
            "dispatch-only route must not inject a reopen after detecting reverse-i-search: {after}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_retries_busy_registered_pane_once_after_interrupt_recovery() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-pane-busy-interrupt-retry");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-pane-busy-interrupt-retry.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let busy_agent = write_mock_busy_registered_agent_doc_ignores_interrupt(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", busy_agent.display(), doc.display()),
        );
        let content =
            wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
        assert!(
            content.contains("Working..."),
            "busy mock session should be active in pane: {content}"
        );

        let ready_agent = write_mock_registered_agent_doc(dir.path());
        std::fs::write(
            dir.path().join(".agent-doc/route-busy-interrupt.txt"),
            format!("exec {} {}\n", ready_agent.display(), doc.display()),
        )
        .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-pane-busy-interrupt-retry";
        sessions::register(session_id, &pane, &file_path).unwrap();
        let ipc_tmux = iso.clone();
        let pane_for_ipc = pane.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Restart { .. }
            | IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let doc_for_thread = doc.clone();
        let current_for_thread = current.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1300));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
                .unwrap();
        });

        let reused = resolve_or_create_pane_with_auto_fix_retry(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
            false,
            true,
            true,
        )
        .expect("route should retry once after interrupting a still-busy live Codex pane");
        assert_eq!(reused, pane);

        let after = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(5),
        );
        assert!(
            after.contains("GOT:agent-doc "),
            "route should dispatch the reopen after the interrupt recovery retry: {after}"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_retries_busy_registered_pane_once_after_ctrl_g_probe() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-pane-busy-ctrl-g-retry");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-pane-busy-ctrl-g-retry.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let busy_agent = write_mock_busy_registered_agent_doc_recovers_on_ctrl_g(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", busy_agent.display(), doc.display()),
        );
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "reverse-i-search",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("reverse-i-search"),
            "busy mock session should be in reverse-i-search: {content}"
        );

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-pane-busy-ctrl-g-retry";
        sessions::register(session_id, &pane, &file_path).unwrap();
        let ipc_tmux = iso.clone();
        let pane_for_ipc = pane.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Restart { .. }
            | IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let doc_for_thread = doc.clone();
        let current_for_thread = current.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1300));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
                .unwrap();
        });

        let reused = resolve_or_create_pane_with_auto_fix_retry(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
        false,
        true,
        true,
    )
    .expect(
        "route should retry once after ctrl-g clears reverse-i-search in a busy live Codex pane",
    );
        assert_eq!(reused, pane);

        let after = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(5),
        );
        assert!(
            after.contains("GOT:agent-doc "),
            "route should dispatch the reopen after the ctrl-g interrupt recovery probe: {after}"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_retries_busy_opencode_pane_after_escape_interrupt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-opencode-busy-escape-retry");
        let session = "opencode";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-opencode-busy-escape-retry.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let busy_agent = write_mock_busy_opencode_recovers_on_escape(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", busy_agent.display(), doc.display()),
        );
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "esc interrupt",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("esc interrupt"),
            "busy OpenCode mock should be active in pane: {content}"
        );

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-opencode-busy-escape-retry";
        sessions::register(session_id, &pane, &file_path).unwrap();
        let ipc_tmux = iso.clone();
        let pane_for_ipc = pane.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Restart { .. }
            | IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let doc_for_thread = doc.clone();
        let current_for_thread = current.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1300));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
                .unwrap();
        });

        let reused = resolve_or_create_pane_with_auto_fix_retry(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::opencode(),
            &mut Vec::new(),
            false,
            true,
            true,
        )
        .expect("route should retry after Escape interrupt recovers a busy OpenCode pane");
        assert_eq!(reused, pane);

        let after = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:/agent-doc ",
            std::time::Duration::from_secs(5),
        );
        assert!(
            after.contains("GOT:/agent-doc "),
            "route should dispatch the reopen after the Escape interrupt recovery: {after}"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_keeps_interrupt_timeout_busy_reroute_optimistic_for_alive_pane() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let _tmux_guard = tmux_start_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-pane-busy-interrupt-blocked");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-pane-busy-interrupt-blocked.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let busy_agent = write_mock_busy_registered_agent_doc_ignores_interrupt(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", busy_agent.display(), doc.display()),
        );
        let content =
            wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
        assert!(
            content.contains("Working..."),
            "busy mock session should be active in pane: {content}"
        );

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-pane-busy-interrupt-blocked";
        sessions::register(session_id, &pane, &file_path).unwrap();

        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let supervisor_instance_id = "busy-reroute-supervisor".to_string();
        let supervisor_instance_id_for_ipc = supervisor_instance_id.clone();
        let ipc_tmux = iso.clone();
        let injected_pane = Arc::new(std::sync::Mutex::new(None::<String>));
        let injected_pane_for_ipc = injected_pane.clone();
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": true,
                        "state": "healthy",
                        "restart_count": 0,
                        "actor_state": "ready",
                        "supervisor_pid": 12345,
                        "supervisor_instance_id": supervisor_instance_id_for_ipc
                    })),
                    IpcMethod::Restart { mode } => {
                        if mode == "fresh" {
                            restart_called_for_ipc.store(true, Ordering::Relaxed);
                        }
                        IpcResponse::ok_empty()
                    }
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        if let Some(target) = injected_pane_for_ipc.lock().unwrap().clone() {
                            let _ = ipc_tmux.send_keys(&target, &bytes);
                        }
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::Stop { .. }
                    | IpcMethod::StopAgent { .. }
                    | IpcMethod::ReplicaRegister { .. }
                    | IpcMethod::ReplicaDeregister { .. }
                    | IpcMethod::ReplicaUpdate { .. }
                    | IpcMethod::ReplicaPull { .. }
                    | IpcMethod::ReplicaAck { .. }
                    | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
                }
            })
            .unwrap();

        let reused = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect(
        "route should still inject into the authoritative pane after the bounded interrupt ladder",
    );
        assert_eq!(reused, pane);
        assert!(restart_called.load(Ordering::Relaxed));
        let miss = crate::startup_miss::load(&doc)
            .unwrap()
            .expect("optimistic busy reroute should still record a startup miss");
        assert_eq!(miss.pane_id, pane);

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_retries_busy_registered_pane_once_after_scoped_fix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-pane-busy-auto-fix");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-pane-busy-auto-fix.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let busy_agent = write_mock_busy_registered_agent_doc(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", busy_agent.display(), doc.display()),
        );
        let content =
            wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
        assert!(
            content.contains("Working..."),
            "busy mock session should be active in pane: {content}"
        );

        let ready_agent = write_mock_registered_agent_doc(dir.path());
        let hook_command = format!("exec {} {}", ready_agent.display(), doc.display());
        std::fs::write(
            dir.path().join(".agent-doc/route-busy-auto-fix.txt"),
            format!("{hook_command}\n"),
        )
        .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-pane-busy-auto-fix";
        sessions::register(session_id, &pane, &file_path).unwrap();
        let ipc_tmux = iso.clone();
        let pane_for_ipc = pane.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Restart { .. }
            | IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let doc_for_thread = doc.clone();
        let current_for_thread = current.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1300));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
                .unwrap();
        });

        let reused = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should retry once after the scoped auto-fix recovers the busy pane");
        assert_eq!(reused, pane);

        let after = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(5),
        );
        assert!(
            after.contains("GOT:agent-doc "),
            "route should inject the reopen after the scoped fix retry: {after}"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_focuses_busy_registered_pane_without_prompt_drift() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-pane-busy-no-drift");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-owner-supervisor-pid.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, snapshot).unwrap();
        let mock_agent = write_mock_busy_registered_agent_doc(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", mock_agent.display(), doc.display()),
        );
        let content =
            wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
        assert!(
            content.contains("Working..."),
            "busy mock session should be active in pane: {content}"
        );

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        sessions::register("route-live-pane-busy-no-drift", &pane, &file_path).unwrap();

        let reused = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-live-pane-busy-no-drift",
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should focus the already-running pane when there is no new drift");
        assert_eq!(reused, pane);

        let after = sessions::capture_pane(&iso, &pane).unwrap_or_default();
        assert!(
            !after.contains("EARLY:agent-doc "),
            "route should not inject a duplicate reopen into a busy live pane: {after}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_rejects_same_committed_cycle_mutation_for_prompt_drift() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-ack-same-cycle");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-supervisor-restart.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let mock_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-same-cycle";
        sessions::register(session_id, &pane, &file_path).unwrap();
        let ipc_tmux = iso.clone();
        let pane_for_ipc = pane.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Restart { .. }
            | IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let doc_for_thread = doc.clone();
        let snapshot_for_thread = snapshot.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            crate::cycle_state::mark_committed(
                &doc_for_thread,
                "commit_already_current",
                Some(&snapshot_for_thread),
                Some(&snapshot_for_thread),
            )
            .unwrap();
        });

        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect(
            "same-cycle committed churn should not block an already-accepted optimistic reroute",
        );
        assert_eq!(resolved, pane);

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains("GOT:agent-doc "),
            "route should still dispatch the trigger to the registered pane: {content}"
        );
        let miss = crate::startup_miss::load(&doc)
            .unwrap()
            .expect("optimistic same-cycle reroute should still record a startup miss");
        assert_eq!(miss.pane_id, pane);
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_accepts_registered_pane_trigger_once_new_cycle_starts() {
        let _tmux_guard = tmux_start_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-ack-ok");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let mock_agent = write_mock_registered_agent_doc_extra_line_detector(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-ok";
        sessions::register(session_id, &pane, &file_path).unwrap();
        let ipc_tmux = iso.clone();
        let pane_for_ipc = pane.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Restart { .. }
            | IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let doc_for_thread = doc.clone();
        let snapshot_for_thread = snapshot.to_string();
        let current_for_thread = current.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            crate::cycle_state::start_preflight(
                &doc_for_thread,
                Some(&snapshot_for_thread),
                Some(&current_for_thread),
            )
            .unwrap();
            crate::cycle_state::mark_committed(
                &doc_for_thread,
                "commit_success",
                Some(&snapshot_for_thread),
                Some(&current_for_thread),
            )
            .unwrap();
        });

        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should accept the new cycle ack");
        assert_eq!(resolved, pane);

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains("GOT:agent-doc "),
            "route should dispatch the trigger before observing the ack: {content}"
        );
        assert!(
            !content.contains("EXTRA:"),
            "route should not append follow-up prompt text onto the Codex reopen payload: {content}"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_accepts_content_edit_cycle_ack_without_extra_payload_lines() {
        let _tmux_guard = tmux_start_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-ack-content-edit");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nThe service returned 401 from this endpoint\n<!-- /agent:exchange -->\n";
        let current = "<!-- agent:exchange patch=append -->\n### Re: older\nThe service returned 503 from this endpoint\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, current).unwrap();
        let mock_agent = write_mock_registered_agent_doc_extra_line_detector(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-content-edit-ok";
        sessions::register(session_id, &pane, &file_path).unwrap();
        let ipc_tmux = iso.clone();
        let pane_for_ipc = pane.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Restart { .. }
            | IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let doc_for_thread = doc.clone();
        let snapshot_for_thread = snapshot.to_string();
        let current_for_thread = current.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            crate::cycle_state::start_preflight(
                &doc_for_thread,
                Some(&snapshot_for_thread),
                Some(&current_for_thread),
            )
            .unwrap();
            crate::cycle_state::mark_committed(
                &doc_for_thread,
                "commit_success",
                Some(&snapshot_for_thread),
                Some(&current_for_thread),
            )
            .unwrap();
        });

        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should accept the new cycle ack for content edits");
        assert_eq!(resolved, pane);

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains("GOT:agent-doc "),
            "route should dispatch the bare Codex reopen before observing the content-edit ack: {content}"
        );
        assert!(
            !content.contains("EXTRA:"),
            "route must not append content-edit text onto the Codex reopen payload: {content}"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn alive_registered_pane_without_live_owner_deregisters_and_lazy_claims() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-owner-missing");
        let session = "claude";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &stale_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));

        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let mut registry = sessions::SessionRegistry::default();
        registry.insert(
            file_path.clone(),
            sessions::SessionEntry {
                pane: stale_pane.clone(),
                pid: 0,
                cwd: dir.path().to_string_lossy().to_string(),
                started: String::new(),
                session_id: "route-live-owner-missing".to_string(),
                file: file_path.clone(),
                window: iso.pane_window(&stale_pane).unwrap_or_default(),
                supervisor_instance_id: String::new(),
            },
        );
        sessions::save_in(dir.path(), &registry).unwrap();
        let mock_start = write_mock_start_agent_doc(dir.path());

        let doc_for_thread = doc.clone();
        let current_for_thread = "# Session\n❯ follow-up question\n".to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            crate::cycle_state::start_preflight(
                &doc_for_thread,
                Some("# Session\n"),
                Some(&current_for_thread),
            )
            .unwrap();
            crate::cycle_state::mark_committed(
                &doc_for_thread,
                "commit_success",
                Some("# Session\n"),
                Some(&current_for_thread),
            )
            .unwrap();
        });

        let resolved = {
            let _route_bin_guard = route_bin_env_lock();
            unsafe {
                std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
            }
            let result = resolve_or_create_pane(
                &iso,
                &doc,
                None,
                &[],
                "route-live-owner-missing",
                &file_path,
                session,
                &HarnessConfig::codex(),
                &mut Vec::new(),
            );
            unsafe {
                std::env::remove_var("AGENT_DOC_ROUTE_BIN");
            }
            result
        }
        .expect("route should continue recovery after clearing the stale registration");
        assert_ne!(resolved, stale_pane);

        let reassigned = sessions::lookup("route-live-owner-missing").unwrap();
        assert!(
            reassigned.as_deref() == Some(resolved.as_str()),
            "route should re-register to the recovered pane, got: {reassigned:?}"
        );

        let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
        assert!(
            !stale_content.contains("STALE:agent-doc "),
            "route should not dispatch into the stale registered pane: {stale_content}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_dispatch_only_reuses_registered_authoritative_actor_pane_when_supervisor_state_is_missing()
     {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-authoritative-actor-dispatch-only-fallback");
        let session = "claude";
        let cwd = test_cwd();
        let actor_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            r#"exec /bin/sh -c 'printf "❯ \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &actor_pane, "❯ ", std::time::Duration::from_secs(3));

        let doc = dir.path().join("dispatch-only-claude-fallback.md");
        let snapshot = "---\nagent: claude\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-authoritative-actor-dispatch-only-fallback";
        sessions::register(session_id, &actor_pane, &file_path).unwrap();

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

        let dispatch_pane = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::claude(),
            &mut Vec::new(),
        )
        .expect(
            "dispatch-only reroute should reuse the live authoritative pane after readiness checks",
        );
        assert_eq!(dispatch_pane, actor_pane);
        let actor_after = wait_for_pane_contains(
            &iso,
            &actor_pane,
            &HarnessConfig::claude().trigger_command(&file_path),
            std::time::Duration::from_secs(3),
        );
        assert!(
            actor_after.contains(&HarnessConfig::claude().trigger_command(&file_path)),
            "degraded authoritative actor should receive the direct-pane reopen: {actor_after}"
        );

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
            .expect("dispatch-only degraded direct submit should write an ops log entry");
        assert!(
            ops_log.contains("route_dispatch_only_authoritative_degraded_direct_pane"),
            "expected authoritative degraded direct submit logging, got: {ops_log}"
        );
        assert!(
            ops_log.contains("supervisor_health=no_socket"),
            "direct-submit logging should explain the degraded supervisor state: {ops_log}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_dispatch_only_recovers_waiting_input_actor_with_fresh_restart() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-dispatch-only-waiting-input-restart");
        let session = "codex";
        let cwd = test_cwd();
        let actor_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &actor_pane, "> ", std::time::Duration::from_secs(3));

        let doc = dir.path().join("dispatch-only-waiting-input.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-dispatch-only-waiting-input";
        sessions::register(session_id, &actor_pane, &file_path).unwrap();

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

        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State => {
                let actor_state = if restart_called_for_ipc.load(Ordering::Relaxed) {
                    "ready"
                } else {
                    "waiting_input"
                };
                IpcResponse::ok(serde_json::json!({
                    "running": true,
                    "state": "healthy",
                    "actor_state": actor_state,
                    "restart_count": 0
                }))
            }
            IpcMethod::Restart { mode } => {
                assert_eq!(mode, "fresh");
                restart_called_for_ipc.store(true, Ordering::Relaxed);
                IpcResponse::ok_empty()
            }
            IpcMethod::Inject { .. } | IpcMethod::Clear { .. } => IpcResponse::ok_empty(),
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

        let resolved = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("dispatch-only reroute should recover a waiting-input authoritative actor");
        assert_eq!(resolved, actor_pane);
        assert!(
            restart_called.load(Ordering::Relaxed),
            "dispatch-only reroute should request one fresh restart when the authoritative actor is waiting for supervisor input"
        );

        let actor_after = wait_for_pane_contains(
            &iso,
            &actor_pane,
            &HarnessConfig::codex().trigger_command(&file_path),
            std::time::Duration::from_secs(3),
        );
        assert!(
            actor_after.contains(&HarnessConfig::codex().trigger_command(&file_path)),
            "dispatch-only reroute should still submit the bare reopen after recovering the waiting-input supervisor prompt: {actor_after}"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_dispatch_only_submits_to_healthy_starting_actor_without_split_churn()
    {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-starting-actor-ready-prompt");
        let session = "codex";
        let actor_pane = iso.new_session(session, dir.path()).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "codex:0", "agent-doc"]);
        let _ = iso.raw_cmd(&[
            "resize-window",
            "-t",
            "codex:agent-doc",
            "-x",
            "120",
            "-y",
            "40",
        ]);
        let prompt_script = dir.path().join("codex-ready-loop.sh");
        std::fs::write(
            &prompt_script,
            "#!/bin/sh\nprintf '\\033[2J\\033[HREADYMARK\\ngpt-5.4 high · ~/work/btakita/agent-loop · Context 0%% used\\n› \\n'\nwhile IFS= read -r CMD; do printf '[run] Nothing changed\\n'; done\n",
        )
        .unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            &format!("exec /bin/sh {}", prompt_script.display()),
        );
        let ready_output = wait_for_pane_contains(
            &iso,
            &actor_pane,
            "READYMARK",
            std::time::Duration::from_secs(3),
        );
        assert!(
            ready_output.contains("READYMARK"),
            "fixture command should execute before split setup: {ready_output}"
        );
        assert!(
            ready_prompt_candidate(&ready_output, &HarnessConfig::codex()).is_some(),
            "fixture should show a Codex dispatch-ready prompt before split setup: {ready_output}"
        );
        let sibling_one = iso.split_window(&actor_pane, dir.path(), "-dh").unwrap();
        let sibling_two = iso.split_window(&actor_pane, dir.path(), "-dh").unwrap();
        let sibling_three = iso.split_window(&actor_pane, dir.path(), "-dh").unwrap();
        iso.select_pane(&actor_pane).unwrap();
        let window = iso.pane_window(&actor_pane).unwrap();
        let panes_before = iso.list_window_panes(&window).unwrap();
        assert_eq!(panes_before.len(), 4);

        let doc = dir.path().join("stale-starting-ready-prompt.md");
        let current = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(current), Some(current)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(current), Some(current))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-stale-starting-ready-prompt";
        sessions::register(session_id, &actor_pane, &file_path).unwrap();

        crate::project_controller::store_actor_record(
            dir.path(),
            None,
            &crate::session_actor::ActorRecord {
                document_id: crate::session_actor::canonical_document_id_in(dir.path(), &file_path),
                session_id: session_id.to_string(),
                generation: 1,
                pane_id: actor_pane.clone(),
                window_id: window.clone(),
                harness: "codex".to_string(),
                state: crate::session_actor::ActorState::Starting,
                last_transition: crate::session_actor::ActorLastTransition {
                    caller: "start".to_string(),
                    reason: "session_start".to_string(),
                    timestamp: 1,
                    prior_generation: 0,
                    new_generation: 1,
                },
            },
        )
        .unwrap();

        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "starting",
                "restart_count": 0
            })),
            IpcMethod::Inject { .. } | IpcMethod::Clear { .. } => {
                panic!("ready-prompt dispatch-only reroute must use direct pane submit")
            }
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

        let resolved = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("dispatch-only reroute should submit to the healthy starting actor");
        assert_eq!(resolved, actor_pane);

        let actor_after = wait_for_pane_contains(
            &iso,
            &actor_pane,
            "[run] Nothing",
            std::time::Duration::from_secs(5),
        );
        let actor_after_compact = actor_after.split_whitespace().collect::<String>();
        assert!(
            actor_after_compact.contains("[run]Nothingchanged"),
            "healthy starting actor should execute the dispatch-only reopen: {actor_after}"
        );
        let panes_after = iso.list_window_panes(&window).unwrap();
        assert_eq!(
            panes_after.len(),
            panes_before.len(),
            "route must not create or remove panes while dispatching to the controller actor"
        );
        for pane in [&sibling_one, &sibling_two, &sibling_three] {
            assert!(
                panes_after.contains(pane),
                "unrelated panes in the split must remain visible"
            );
        }
        let record = crate::project_controller::authoritative_actor_binding(dir.path(), &doc)
            .unwrap()
            .unwrap();
        assert_eq!(record.pane_id, actor_pane);

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn auto_start_reuses_other_file_pane_only_as_split_anchor() {
        let _tmux_guard = tmux_start_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-cross-file-split-anchor");
        let requested_session = "claude";
        let cwd = test_cwd();

        let anchor_pane = iso.auto_start(requested_session, &cwd).unwrap();
        let session = pane_session_name(&iso, &anchor_pane)
            .expect("anchor pane should report its tmux session");
        let anchor_window = iso.pane_window(&anchor_pane).unwrap();
        send_keys_with_retry(
            &iso,
            &anchor_pane,
            r#"exec /bin/sh -c 'printf "> \n"; while IFS= read -r CMD; do printf "ANCHOR:%s\n" "$CMD"; done'"#,
        );
        let _ =
            wait_for_pane_contains(&iso, &anchor_pane, "\n>", std::time::Duration::from_secs(3));

        let anchor_doc = dir.path().join("other.md");
        std::fs::write(&anchor_doc, "# Other\n").unwrap();
        let target_doc = dir.path().join("target.md");
        std::fs::write(&target_doc, "# Target\n").unwrap();

        let anchor_path = anchor_doc
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let target_path = target_doc
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();

        sessions::register_full_in(
            dir.path(),
            "route-cross-file-anchor",
            &anchor_pane,
            &anchor_path,
            1234,
            &anchor_window,
        )
        .unwrap();

        let mock_start = write_mock_start_agent_doc(dir.path());
        let target_doc_for_thread = target_doc.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            crate::cycle_state::start_preflight(
                &target_doc_for_thread,
                Some("# Target\n"),
                Some("# Target\n"),
            )
            .unwrap();
        });

        let mut created_panes = Vec::new();
        let new_pane = {
            let _route_bin_guard = route_bin_env_lock();
            unsafe {
                std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
            }
            let result = resolve_or_create_pane(
                &iso,
                &target_doc,
                None,
                &[],
                "route-cross-file-target",
                &target_path,
                &session,
                &HarnessConfig::codex(),
                &mut created_panes,
            );
            unsafe {
                std::env::remove_var("AGENT_DOC_ROUTE_BIN");
            }
            result
        }
        .expect("route should provision a fresh pane without cross-file dispatch");

        assert_eq!(created_panes, vec![new_pane.clone()]);
        assert_ne!(
            new_pane, anchor_pane,
            "auto-start must create a distinct pane rather than dispatching into the anchor"
        );
        assert_eq!(
            iso.pane_window(&new_pane).unwrap(),
            anchor_window,
            "fresh pane should split alongside the existing session pane"
        );

        let target_content = wait_for_pane_contains(
            &iso,
            &new_pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(5),
        );
        assert!(
            target_content.contains("GOT:agent-doc "),
            "fresh pane should receive the routed command: {target_content}"
        );

        let anchor_content = sessions::capture_pane(&iso, &anchor_pane).unwrap_or_default();
        assert!(
            !anchor_content.contains("ANCHOR:agent-doc "),
            "existing pane for another document must stay a split anchor only: {anchor_content}"
        );

        let lookup = sessions::load_in(dir.path())
            .unwrap()
            .values()
            .find(|entry| entry.session_id == "route-cross-file-target")
            .map(|entry| entry.pane.clone());
        assert_eq!(
            lookup.as_deref(),
            Some(new_pane.as_str()),
            "target document should bind to the new pane"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_waits_longer_for_fresh_start_cycle_ack() {
        let _tmux_guard = tmux_start_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-fresh-start-extended-ack");
        let session = "claude";
        let cwd = test_cwd();
        let _anchor_pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("fresh-start-extended-ack.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let mock_start = write_mock_start_agent_doc(dir.path());

        let doc_for_thread = doc.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1300));
            crate::cycle_state::start_preflight(
                &doc_for_thread,
                Some("# Session\n"),
                Some("# Session\n"),
            )
            .unwrap();
        });

        let mut created_panes = Vec::new();
        let new_pane = {
            let _route_bin_guard = route_bin_env_lock();
            unsafe {
                std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
            }
            let result = resolve_or_create_pane(
                &iso,
                &doc,
                None,
                &[],
                "route-fresh-start-extended-ack",
                &file_path,
                session,
                &HarnessConfig::codex(),
                &mut created_panes,
            );
            unsafe {
                std::env::remove_var("AGENT_DOC_ROUTE_BIN");
            }
            result
        }
        .expect("fresh auto-start should tolerate a delayed but real initial cycle start");

        assert_eq!(created_panes, vec![new_pane.clone()]);

        let content = wait_for_pane_contains(
            &iso,
            &new_pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains("GOT:agent-doc "),
            "route should still dispatch the trigger before observing the delayed ack: {content}"
        );

        let state = crate::cycle_state::load(&doc)
            .unwrap()
            .expect("cycle state should exist after delayed fresh-start ack");
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_rebinds_fresh_start_after_ready_wait_registry_churn() {
        let _tmux_guard = tmux_start_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-fresh-start-reregister-before-dispatch");
        let session = "claude";
        let cwd = test_cwd();
        let _anchor_pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("fresh-start-reregister-before-dispatch.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let mock_start = write_mock_delayed_start_agent_doc(dir.path(), 1);

        let registry_root = dir.path().to_path_buf();
        let clear_handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let registry_path = sessions::registry_path_in(&registry_root);
            let _lock = sessions::RegistryLock::acquire(&registry_path).unwrap();
            let mut registry = sessions::load_in(&registry_root).unwrap();
            let key = registry
                .iter()
                .find(|(_, entry)| {
                    entry.session_id == "route-fresh-start-reregister-before-dispatch"
                })
                .map(|(key, _)| key.clone());
            if let Some(key) = key {
                registry.remove(&key);
                sessions::save_in(&registry_root, &registry).unwrap();
            }
        });

        let doc_for_thread = doc.clone();
        let ack_handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            crate::cycle_state::start_preflight(
                &doc_for_thread,
                Some("# Session\n"),
                Some("# Session\n"),
            )
            .unwrap();
        });

        let mut created_panes = Vec::new();
        let new_pane = {
            let _route_bin_guard = route_bin_env_lock();
            unsafe {
                std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
            }
            let result = resolve_or_create_pane(
                &iso,
                &doc,
                None,
                &[],
                "route-fresh-start-reregister-before-dispatch",
                &file_path,
                session,
                &HarnessConfig::codex(),
                &mut created_panes,
            );
            unsafe {
                std::env::remove_var("AGENT_DOC_ROUTE_BIN");
            }
            result
        }
        .expect("fresh auto-start should rebind the pane before the first guarded dispatch");

        clear_handle.join().unwrap();
        ack_handle.join().unwrap();

        assert_eq!(created_panes, vec![new_pane.clone()]);

        let content = wait_for_pane_contains(
            &iso,
            &new_pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains("GOT:agent-doc "),
            "fresh auto-start should still dispatch after the initial binding is cleared during ready-wait: {content}"
        );

        let lookup = sessions::lookup("route-fresh-start-reregister-before-dispatch").unwrap();
        assert_eq!(
            lookup.as_deref(),
            Some(new_pane.as_str()),
            "fresh auto-start should restore the new pane as the registered owner"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_keeps_fresh_start_authoritative_despite_existing_owner_rebind() {
        let _tmux_guard = tmux_start_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-fresh-start-handoff");
        let session = "claude";
        let cwd = test_cwd();
        let existing_pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("fresh-start-handoff.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let mock_agent = write_mock_registered_agent_doc(dir.path());
        send_keys_with_retry(
            &iso,
            &existing_pane,
            &format!("exec {}", mock_agent.display()),
        );
        let owner_ready =
            wait_for_pane_contains(&iso, &existing_pane, ">", std::time::Duration::from_secs(5));
        assert!(
            owner_ready.contains(">"),
            "existing owner pane should be idle before the handoff: {owner_ready}"
        );

        let mock_start = write_mock_delayed_start_agent_doc(dir.path(), 1);
        let registry_root = dir.path().to_path_buf();
        let handoff_pane = existing_pane.clone();
        let handoff_file = file_path.clone();
        let handoff = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            sessions::register_full_with_cwd_in(
                &registry_root,
                "route-fresh-start-handoff",
                &handoff_pane,
                &handoff_file,
                12345,
                "@owner",
                registry_root.to_string_lossy().as_ref(),
            )
            .unwrap();
        });
        let doc_for_ack = doc.clone();
        let ack = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            crate::cycle_state::start_preflight(
                &doc_for_ack,
                Some("# Session\n"),
                Some("# Session\n"),
            )
            .unwrap();
        });

        let mut created_panes = Vec::new();
        let routed_pane = {
            let _route_bin_guard = route_bin_env_lock();
            unsafe {
                std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
            }
            let result = resolve_or_create_pane(
                &iso,
                &doc,
                None,
                &[],
                "route-fresh-start-handoff",
                &file_path,
                session,
                &HarnessConfig::codex(),
                &mut created_panes,
            );
            unsafe {
                std::env::remove_var("AGENT_DOC_ROUTE_BIN");
            }
            result
        }
        .expect("fresh auto-start should keep the fresh pane authoritative even if another path rebinds the session during boot");

        handoff.join().unwrap();
        ack.join().unwrap();

        let new_pane = created_panes
            .first()
            .cloned()
            .expect("fresh route should still create one pane");
        assert_eq!(routed_pane, new_pane);
        assert_eq!(
            created_panes.len(),
            1,
            "fresh auto-start should still create one pane"
        );

        let owner_after = sessions::capture_pane(&iso, &existing_pane).unwrap_or_default();
        assert!(
            !owner_after.contains("GOT:agent-doc "),
            "route must not hand dispatch back to the older pane after a fresh start: {owner_after}"
        );

        let new_pane_after = sessions::capture_pane(&iso, &new_pane).unwrap_or_default();
        assert!(
            new_pane_after.contains("GOT:agent-doc "),
            "route should keep dispatching into the fresh pane after a competing registry rebind: {new_pane_after}"
        );

        let lookup = sessions::lookup("route-fresh-start-handoff").unwrap();
        assert_eq!(
            lookup.as_deref(),
            Some(new_pane.as_str()),
            "registry should restore the fresh pane as authoritative after the competing rebind"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_ignores_handoff_back_to_active_startup_miss_pane() {
        let _tmux_guard = tmux_start_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-fresh-start-ignore-startup-miss-handoff");
        let session = "claude";
        let cwd = test_cwd();
        let existing_pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir
            .path()
            .join("fresh-start-ignore-startup-miss-handoff.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let mock_agent = write_mock_registered_agent_doc(dir.path());
        send_keys_with_retry(
            &iso,
            &existing_pane,
            &format!("exec {}", mock_agent.display()),
        );
        let owner_ready =
            wait_for_pane_contains(&iso, &existing_pane, ">", std::time::Duration::from_secs(5));
        assert!(
            owner_ready.contains(">"),
            "existing owner pane should be idle before the handoff: {owner_ready}"
        );

        crate::startup_miss::record(
            &doc,
            &existing_pane,
            "route-fresh-start-ignore-startup-miss-handoff",
            "codex",
            crate::startup_miss::StartupMissOrigin::RoutedTrigger,
            Some("cycle-baseline"),
        )
        .unwrap();

        let mock_start = write_mock_delayed_start_agent_doc(dir.path(), 1);
        let registry_root = dir.path().to_path_buf();
        let handoff_pane = existing_pane.clone();
        let handoff_file = file_path.clone();
        let handoff = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            sessions::register_full_with_cwd_in(
                &registry_root,
                "route-fresh-start-ignore-startup-miss-handoff",
                &handoff_pane,
                &handoff_file,
                12345,
                "@owner",
                registry_root.to_string_lossy().as_ref(),
            )
            .unwrap();
        });
        let doc_for_ack = doc.clone();
        let ack = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            crate::cycle_state::start_preflight(
                &doc_for_ack,
                Some("# Session\n"),
                Some("# Session\n"),
            )
            .unwrap();
        });

        let mut created_panes = Vec::new();
        let routed_pane = {
            let _route_bin_guard = route_bin_env_lock();
            unsafe {
                std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
            }
            let result = resolve_or_create_pane(
                &iso,
                &doc,
                None,
                &[],
                "route-fresh-start-ignore-startup-miss-handoff",
                &file_path,
                session,
                &HarnessConfig::codex(),
                &mut created_panes,
            );
            unsafe {
                std::env::remove_var("AGENT_DOC_ROUTE_BIN");
            }
            result
        }
        .expect("fresh auto-start should keep dispatch in the new pane when the old owner still carries startup-miss provenance");

        handoff.join().unwrap();
        ack.join().unwrap();

        assert_eq!(created_panes.len(), 1, "route should still create one pane");
        let new_pane = &created_panes[0];
        assert_eq!(routed_pane, *new_pane);

        let new_pane_after = wait_for_pane_contains(
            &iso,
            new_pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            new_pane_after.contains("GOT:agent-doc "),
            "route should keep the reopen in the fresh pane when the alternate handoff target still owns startup-miss provenance: {new_pane_after}"
        );

        let old_pane_after = sessions::capture_pane(&iso, &existing_pane).unwrap_or_default();
        assert!(
            !old_pane_after.contains("GOT:agent-doc "),
            "route must not hand dispatch back to the startup-miss pane: {old_pane_after}"
        );

        let lookup = sessions::lookup("route-fresh-start-ignore-startup-miss-handoff").unwrap();
        assert_eq!(
            lookup.as_deref(),
            Some(new_pane.as_str()),
            "registry should restore the fresh pane as authoritative when the old pane is still marked startup-miss"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_fails_closed_for_halted_supervisor_when_no_live_owner() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-supervisor-restart");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-supervisor-restart";
        let mut registry = sessions::SessionRegistry::default();
        registry.insert(
            file_path.clone(),
            sessions::SessionEntry {
                pane: pane.clone(),
                pid: 0,
                cwd: dir.path().to_string_lossy().to_string(),
                started: String::new(),
                session_id: session_id.to_string(),
                file: file_path.clone(),
                window: iso.pane_window(&pane).unwrap_or_default(),
                supervisor_instance_id: String::new(),
            },
        );
        sessions::save_in(dir.path(), &registry).unwrap();

        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": false,
                        "state": "halted",
                        "restart_count": 5
                    })),
                    IpcMethod::Restart { .. } => {
                        restart_called_for_ipc.store(true, Ordering::Relaxed);
                        IpcResponse::ok_empty()
                    }
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": null })),
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::Stop { .. }
                    | IpcMethod::StopAgent { .. }
                    | IpcMethod::ReplicaRegister { .. }
                    | IpcMethod::ReplicaDeregister { .. }
                    | IpcMethod::ReplicaUpdate { .. }
                    | IpcMethod::ReplicaPull { .. }
                    | IpcMethod::ReplicaAck { .. }
                    | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
                }
            })
            .unwrap();

        let panes_before = iso
            .list_panes_ordered(&format!("{session}:0"))
            .unwrap_or_default();
        let err = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("route should fail closed instead of reviving a halted crash loop");
        let panes_after = iso
            .list_panes_ordered(&format!("{session}:0"))
            .unwrap_or_default();

        assert_eq!(
            panes_after.len(),
            panes_before.len(),
            "route should not create a duplicate pane when the registered supervisor is halted"
        );
        assert!(
            err.to_string()
                .contains("halted supervisor after 5 restarts"),
            "unexpected error: {err:#}"
        );
        assert!(
            !restart_called.load(Ordering::Relaxed),
            "route should not restart a halted supervisor automatically"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_fails_closed_after_repeated_recent_session_losses() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-recent-session-loss");
        let session = "codex";
        let cwd = test_cwd();
        let anchor = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-recent-session-loss";
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        std::fs::write(
            dir.path()
                .join(".agent-doc/logs")
                .join(format!("{session_id}.log")),
            format!(
                "[{}] supervisor_exit code=missing_pane pane=%41 reason=registered_pane_missing\n[{}] supervisor_exit code=missing_pane pane=%42 reason=registered_pane_dead\n",
                now.saturating_sub(30),
                now.saturating_sub(5)
            ),
        )
        .unwrap();

        let panes_before = iso
            .list_panes_ordered(&format!("{session}:0"))
            .unwrap_or_default();
        let err = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("route should fail closed after repeated recent pane losses");
        let panes_after = iso
            .list_panes_ordered(&format!("{session}:0"))
            .unwrap_or_default();

        assert_eq!(
            panes_after.len(),
            panes_before.len(),
            "route should not spawn a replacement pane once the repeated-loss guard trips"
        );
        assert_eq!(panes_after.first(), Some(&anchor));
        assert!(
            err.to_string().contains("refusing to auto-start"),
            "unexpected error: {err:#}"
        );
        assert!(
            err.to_string().contains("unexpected pane-loss events"),
            "unexpected error: {err:#}"
        );
    }
}
