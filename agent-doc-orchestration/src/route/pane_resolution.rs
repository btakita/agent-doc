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
            let _authorization = authorize_controller_dispatch(
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
            )?;
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
