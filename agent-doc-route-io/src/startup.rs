//! Route startup and provisioning I/O.

use anyhow::Result;
use std::path::Path;
use std::time::Duration;

use crate::cycle_ack::{RouteCycleAckEffects, wait_for_start_ack};
use crate::dispatch::{RouteDispatchEffects, dispatch_routed_reopen};
use crate::dispatch_only::{
    DispatchOnlyRouteEffects, DispatchOnlySendReopenOptions, dispatch_only_send_reopen,
};
use crate::dispatch_recovery::resolve_fresh_dispatch_target_after_ready_wait;
use crate::dispatch_target::register_dispatch_target;
use crate::session_resolution::{
    ensure_auto_start_target_session, evict_previous_stash_pane, find_registered_pane_in_session,
    resolve_target_session,
};
use crate::startup_harness::resolve_harness_for_file;
use crate::startup_locks::{StartupLockAcquire, StartupLockMode, acquire_startup_locks};
use crate::startup_ready::{fresh_start_pane_idle_ready, wait_for_agent_ready};
use agent_doc_controller::dispatch::{
    DispatchOnlyReopenDelivery, DuplicatePanePolicyErrorFacts, RoutedDispatchStartProof,
    duplicate_pane_policy_error_message, fresh_route_start_ack_timeout,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_tmux::is_first_column;
use tmux_router::Tmux;

#[derive(Debug, Clone, Copy)]
pub struct RouteStartupEffects {
    pub route_dispatch_effects: RouteDispatchEffects,
    pub dispatch_only_route_effects: DispatchOnlyRouteEffects,
    pub route_cycle_ack_effects: RouteCycleAckEffects,
}

/// `#jbtsiftnosub`: bounded re-verify window for the auto-start cold-start gate.
/// After `wait_for_agent_ready` reports ready, the pane should already show a
/// dispatch-ready prompt, so this is the small race window between the readiness
/// proof and the actual send; if the composer is still cold-starting past this
/// bound the auto-start dispatch fails closed instead of typing into a
/// not-yet-submit-ready composer.
const AUTO_START_DISPATCH_READY_REVERIFY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingStartupRegistrationDecision<'a> {
    Reuse(&'a str),
    IgnoreStale(&'a str),
    None,
}

fn decide_existing_startup_registration<'a>(
    existing_pane: Option<&'a str>,
    existing_alive: bool,
    live_owner: Option<&str>,
) -> ExistingStartupRegistrationDecision<'a> {
    let Some(existing_pane) = existing_pane else {
        return ExistingStartupRegistrationDecision::None;
    };
    if !existing_alive {
        return ExistingStartupRegistrationDecision::None;
    }
    if live_owner == Some(existing_pane) {
        ExistingStartupRegistrationDecision::Reuse(existing_pane)
    } else {
        ExistingStartupRegistrationDecision::IgnoreStale(existing_pane)
    }
}

/// Fresh-route agent dispatch-ready wait budget (`#waitmachine2`). Historically
/// 30s; routed through the unified wait-machinery ceiling so the operator's
/// "never hang > 10s" bound is enforced in one place: the 30s request is clamped
/// to [`agent_doc_turn::wait_machine::GLOBAL_HANG_CEILING`] (10s). The underlying
/// `wait_for_agent_ready` poll loop keeps its existing fast-fail-on-dead-pane and
/// blocker-streak semantics; only the ceiling changes.
const FRESH_ROUTE_AGENT_READY_TIMEOUT: Duration =
    agent_doc_turn::wait_machine::clamp_to_ceiling(Duration::from_secs(30));

/// Auto-start a new agent session in tmux using the default session name.
/// Public so `sync.rs` can call it for unresolved files.
///
/// `context_session` is an optional session override from the calling context
/// (e.g., the sync target session). Used when frontmatter has no `tmux_session`
/// and sync has already resolved a more specific session from editor/window
/// context.
#[allow(dead_code)]
pub fn auto_start(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    effects: RouteStartupEffects,
) -> Result<String> {
    auto_start_ext(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        false,
        false,
        effects,
    )
}

/// **Provisioning** — create a new tmux pane and start Claude asynchronously.
///
/// Called by sync during Reconciliation when a file has a session UUID but no
/// registered pane. Creates the pane immediately but doesn't wait for Claude
/// to initialize (async startup).
pub fn provision_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    col_args: &[String],
    effects: RouteStartupEffects,
) -> Result<String> {
    let split_before = is_first_column(file, col_args);
    auto_start_ext(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        true,
        split_before,
        effects,
    )
}

/// Like [`provision_pane`], but returns `Ok(None)` instead of blocking when a
/// same-document or same-session startup is already in progress.
pub fn try_provision_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    col_args: &[String],
    effects: RouteStartupEffects,
) -> Result<Option<String>> {
    let split_before = is_first_column(file, col_args);
    auto_start_ext_with_lock_mode(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        true,
        split_before,
        StartupLockMode::Try,
        effects,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn auto_start_ext(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    skip_wait: bool,
    split_before: bool,
    effects: RouteStartupEffects,
) -> Result<String> {
    match auto_start_ext_with_lock_mode(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        skip_wait,
        split_before,
        StartupLockMode::Blocking,
        effects,
    )? {
        Some(pane) => Ok(pane),
        None => anyhow::bail!("startup lock was busy in blocking startup mode"),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn auto_start_ext_with_lock_mode(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    skip_wait: bool,
    split_before: bool,
    startup_lock_mode: StartupLockMode,
    effects: RouteStartupEffects,
) -> Result<Option<String>> {
    let harness = resolve_harness_for_file(file);
    let session_name = resolve_target_session(tmux, context_session, &[], Some(file), &harness);
    ensure_auto_start_target_session(tmux, context_session, &session_name, &harness)?;
    auto_start_in_session_with_lock_mode(
        tmux,
        file,
        session_id,
        file_path,
        &session_name,
        skip_wait,
        split_before,
        &harness,
        None,
        None,
        false,
        startup_lock_mode,
        effects,
    )
}

/// Auto-start a new agent session in a specific tmux session.
///
/// Strategy:
/// 1. Find an existing registered agent-doc pane in the target session
/// 2. If found: `split-window` directly in that pane's window (avoids creating
///    a throwaway window then failing to join due to minimum pane size)
/// 3. If not found: create a new window via `auto_start` (session may not exist yet)
///
/// When `skip_wait` is true, skips `wait_for_agent_ready` and `send_command`.
/// Used by sync which only needs the pane to exist with the agent starting.
#[allow(clippy::too_many_arguments)]
pub fn auto_start_in_session(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    session_name: &str,
    skip_wait: bool,
    split_before: bool,
    harness: &HarnessConfig,
    startup_miss_handoff_blocked_pane: Option<&str>,
    created_panes: Option<&mut Vec<String>>,
    dispatch_only: bool,
    effects: RouteStartupEffects,
) -> Result<String> {
    match auto_start_in_session_with_lock_mode(
        tmux,
        file,
        session_id,
        file_path,
        session_name,
        skip_wait,
        split_before,
        harness,
        startup_miss_handoff_blocked_pane,
        created_panes,
        dispatch_only,
        StartupLockMode::Blocking,
        effects,
    )? {
        Some(pane) => Ok(pane),
        None => anyhow::bail!("startup lock was busy in blocking startup mode"),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn auto_start_in_session_with_lock_mode(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    session_name: &str,
    skip_wait: bool,
    split_before: bool,
    harness: &HarnessConfig,
    startup_miss_handoff_blocked_pane: Option<&str>,
    mut created_panes: Option<&mut Vec<String>>,
    dispatch_only: bool,
    startup_lock_mode: StartupLockMode,
    effects: RouteStartupEffects,
) -> Result<Option<String>> {
    // Serialize auto-starts for both the document and the target tmux session.
    // This prevents duplicate starts for the same file and split-target races
    // when two different documents provision concurrently into the same window.
    let startup_locks = match acquire_startup_locks(file, session_name, startup_lock_mode)? {
        StartupLockAcquire::Acquired(locks) => locks,
        StartupLockAcquire::Busy => {
            eprintln!(
                "[route] startup lock busy for {} in session '{}' — provisioning already in progress",
                file_path, session_name
            );
            return Ok(None);
        }
    };
    let existing_registration = agent_doc_session_registry_io::lookup(session_id)?;
    let existing_alive = existing_registration
        .as_deref()
        .is_some_and(|existing| tmux.pane_alive(existing));
    let live_owner = if existing_alive {
        agent_doc_sync_io::sync::find_normal_path_owner_pane(tmux, file, session_id)
    } else {
        None
    };
    match decide_existing_startup_registration(
        existing_registration.as_deref(),
        existing_alive,
        live_owner.as_deref(),
    ) {
        ExistingStartupRegistrationDecision::Reuse(existing) => {
            eprintln!(
                "[route] startup already provisioned live owner pane {} for {} while waiting on locks",
                existing, file_path
            );
            return Ok(Some(existing.to_string()));
        }
        ExistingStartupRegistrationDecision::IgnoreStale(existing) => {
            eprintln!(
                "[route] ignoring alive registry pane {} for {} during startup because it is not a proven live owner; creating a fresh pane",
                existing, file_path
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_startup_ignoring_unowned_registry_pane file={} pane={}",
                    file_path, existing
                ),
            );
        }
        ExistingStartupRegistrationDecision::None => {}
    }

    // Use the document's own submodule root as the pane cwd when applicable,
    // so `/agent-doc` invocations on submodule-hosted documents spawn panes
    // inside the correct submodule (e.g. `src/session-share`) instead of the
    // agent-loop super root where the command happened to be invoked from.
    let cwd = agent_doc_git_io::dirs::resolve_pane_cwd(file);
    let registry_base_dir = agent_doc_project_root_io::project_root_or_file_parent(file)
        .unwrap_or_else(|_| cwd.clone());

    // Resolve the agent-doc binary path (same binary that's currently running)
    let agent_doc_bin = agent_doc_supervisor_process::agent_doc_start_bin();

    // Try to split directly in an existing pane.
    // When skip_wait=true (sync path), prefer panes in the target window (agent-doc window)
    // over stash panes — splitting in the stash creates invisible panes.
    let existing_pane = if skip_wait {
        // Sync path: find a pane in the agent-doc window (not stash)
        let window_panes = tmux
            .list_panes_ordered(&format!("{}:agent-doc", session_name))
            .unwrap_or_default();
        let positional = if split_before {
            window_panes.into_iter().next() // leftmost by screen position
        } else {
            window_panes.into_iter().last() // rightmost by screen position
        };
        positional
            .or_else(|| find_registered_pane_in_session(tmux, &registry_base_dir, session_name, ""))
    } else {
        find_registered_pane_in_session(tmux, &registry_base_dir, session_name, "")
    };
    let split_flag = if split_before { "-dbh" } else { "-dh" };
    let new_pane = if let Some(ref target) = existing_pane {
        match tmux.split_window(target, &cwd, split_flag) {
            Ok(pane) => {
                eprintln!(
                    "[route] split-window {} alongside registered pane {} in session '{}' → new pane {}",
                    split_flag, target, session_name, pane
                );
                pane
            }
            Err(e) => {
                anyhow::bail!(
                    "{}",
                    duplicate_pane_policy_error_message(DuplicatePanePolicyErrorFacts {
                        session_name,
                        file_path,
                        anchor_pane: Some(target),
                        cause: &format!("split-window failed alongside pane {} ({})", target, e),
                    })
                );
            }
        }
    } else {
        let has_agent_doc_window =
            agent_doc_tmux_io::has_window_named(tmux, session_name, "agent-doc");
        if has_agent_doc_window {
            anyhow::bail!(
                "{}",
                duplicate_pane_policy_error_message(DuplicatePanePolicyErrorFacts {
                    session_name,
                    file_path,
                    anchor_pane: None,
                    cause: "the target session already has an 'agent-doc' window but no safe registered anchor pane was found",
                })
            );
        } else {
            eprintln!(
                "[route] no registered pane found in session '{}', creating new window",
                session_name
            );
            tmux.auto_start(session_name, &cwd)?
        }
    };
    tmux.enable_remain_on_exit(&new_pane)?;
    if let Some(created) = created_panes.as_mut() {
        created.push(new_pane.clone());
    }

    evict_previous_stash_pane(tmux, session_id, &new_pane, session_name, harness);

    // Register immediately so subsequent route calls find this pane
    register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
    drop(startup_locks);

    // Focus the new pane immediately so the user sees Claude starting
    if let Err(e) = tmux.select_pane(&new_pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", new_pane, e);
    }

    // Rewrite file_path to be relative to the spawned pane's cwd.
    // `cwd` may be narrowed to a submodule root (see `resolve_pane_cwd`), in which
    // case a super-root-relative `file_path` like `src/session-share/tasks/foo.md`
    // will not resolve when `agent-doc start` runs from inside the submodule.
    // Fallback: if canonicalize fails or the file is not under `cwd`, use the
    // original `file_path` (preserves behavior for non-submodule docs).
    let start_path = agent_doc_fs::rewrite_start_path(file, &cwd, file_path);

    // Start agent-doc start in the new pane
    let start_cmd = format!("{} start --route-owned {}", agent_doc_bin, start_path);
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(Some(file), agent_doc_ops_log_io::log_op),
        "route.auto_start",
        &format!("pane:{new_pane}"),
        &start_cmd,
        Some(&harness.binary),
        "route_owned_start_enter",
        "Enter",
    );
    agent_doc_tmux_io::send_submitted_text_logged(
        tmux,
        &new_pane,
        &start_cmd,
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
        "sessions.send_submitted_text",
    )?;

    eprintln!(
        "[route] Started {} for {} in pane {} (session {})",
        harness.binary,
        file_path,
        new_pane,
        &session_id[..std::cmp::min(8, session_id.len())]
    );

    let cycle_baseline =
        agent_doc_cycle_state_io::load_with_closeout_projection(file).unwrap_or(None);

    if skip_wait {
        eprintln!(
            "[route] skip_wait=true — pane created, {} starting (sync path)",
            harness.binary
        );
    } else {
        eprintln!("[route] Waiting for {} to initialize...", harness.binary);
        let ready = wait_for_agent_ready(tmux, &new_pane, FRESH_ROUTE_AGENT_READY_TIMEOUT, harness);
        // Fresh-start recovery can clear the early geometry-only binding while
        // the harness is still booting. Re-validate the registration before we
        // dispatch, but keep the deliberately created fresh pane authoritative
        // for same-document rebind churn instead of treating it as disposable.
        let dispatch_pane = resolve_fresh_dispatch_target_after_ready_wait(
            tmux,
            session_id,
            &new_pane,
            file_path,
            startup_miss_handoff_blocked_pane,
        )?;
        if dispatch_pane != new_pane {
            eprintln!(
                "[route] fresh start pane {} handed ownership for {} back to existing pane {} during startup; dispatching follow-up there",
                new_pane, file_path, dispatch_pane
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "fresh_route_dispatch_handoff file={} fresh_pane={} dispatch_pane={} harness={}",
                    file.display(),
                    new_pane,
                    dispatch_pane,
                    harness.binary
                ),
            );
            if let Err(e) = tmux.select_pane(&dispatch_pane) {
                eprintln!(
                    "[route] warning: failed to focus handoff pane {}: {}",
                    dispatch_pane, e
                );
            }
        }
        let dispatch_start = if ready {
            eprintln!("[route] {} is ready, sending command", harness.binary);
            if dispatch_only {
                dispatch_only_send_reopen(
                    tmux,
                    file,
                    session_id,
                    &dispatch_pane,
                    file_path,
                    harness,
                    DispatchOnlySendReopenOptions {
                        delivery: DispatchOnlyReopenDelivery::SupervisorIpcOnce,
                        queue_prompt_text: None,
                        effects: effects.dispatch_only_route_effects,
                    },
                )?;
                RoutedDispatchStartProof::CommandAcceptedOnly
            } else {
                // #jbtsiftnosub: close the cold-start race. `wait_for_agent_ready`
                // can prove a transient dispatch-ready prompt while the harness TUI
                // is still coming up; re-verify immediately before the send that the
                // composer is actually submit-ready, and fail closed (logging
                // `dispatch_into_starting_pane`) rather than typing the trigger into
                // a not-yet-ready composer.
                crate::startup_ready::reverify_auto_start_dispatch_ready(
                    tmux,
                    file,
                    &dispatch_pane,
                    file_path,
                    harness,
                    AUTO_START_DISPATCH_READY_REVERIFY_TIMEOUT,
                )?;
                dispatch_routed_reopen(
                    tmux,
                    file,
                    &dispatch_pane,
                    file_path,
                    harness,
                    effects.route_dispatch_effects,
                )?
            }
        } else {
            eprintln!(
                "[route] Timed out waiting for {} prompt; attempting one fallback trigger injection before failing closed",
                harness.binary
            );
            let dispatch_result = if dispatch_only {
                dispatch_only_send_reopen(
                    tmux,
                    file,
                    session_id,
                    &dispatch_pane,
                    file_path,
                    harness,
                    DispatchOnlySendReopenOptions {
                        delivery: DispatchOnlyReopenDelivery::SupervisorIpcOnce,
                        queue_prompt_text: None,
                        effects: effects.dispatch_only_route_effects,
                    },
                )
                .map(|_| RoutedDispatchStartProof::CommandAcceptedOnly)
            } else {
                dispatch_routed_reopen(
                    tmux,
                    file,
                    &dispatch_pane,
                    file_path,
                    harness,
                    effects.route_dispatch_effects,
                )
            };
            match dispatch_result {
                Ok(proof) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "fresh_route_trigger_recovered file={} pane={} harness={}",
                            file.display(),
                            dispatch_pane,
                            harness.binary
                        ),
                    );
                    eprintln!(
                        "[route] Fallback trigger injection recovered the fresh {} start for {}",
                        harness.binary, file_path
                    );
                    proof
                }
                Err(err) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "fresh_route_trigger_missing file={} pane={} harness={}",
                            file.display(),
                            new_pane,
                            harness.binary
                        ),
                    );
                    return Err(err);
                }
            }
        };

        if dispatch_only {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "fresh_route_dispatch_only file={} pane={} harness={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary
                ),
            );
            let _ = dispatch_start;
            return Ok(Some(dispatch_pane));
        }

        let ack_timeout = fresh_route_start_ack_timeout(cfg!(test));
        match wait_for_start_ack(file, cycle_baseline.as_ref(), ack_timeout) {
            Some(state) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "fresh_route_start_acknowledged file={} pane={} harness={} cycle={} phase={} timeout_secs={}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        state.cycle_id,
                        match state.phase {
                            agent_doc_turn::CyclePhase::PreflightStarted => "preflight_started",
                            agent_doc_turn::CyclePhase::ResponseCaptured => "response_captured",
                            agent_doc_turn::CyclePhase::WriteApplied => "write_applied",
                            agent_doc_turn::CyclePhase::Committed => "committed",
                            agent_doc_turn::CyclePhase::Abandoned => "abandoned",
                        },
                        ack_timeout.as_secs()
                    ),
                );
                let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
            }
            None if fresh_start_pane_idle_ready(tmux, &dispatch_pane, harness) => {
                // (#route-reaps-idle-fresh-start) The trigger was proven dispatched
                // above, and the pane has returned to a dispatch-ready prompt: the
                // first cycle was a legitimate no-op (empty/halted queue, preflight
                // `no_changes`) — there was simply nothing to acknowledge. Keep the
                // live idle session instead of reaping a healthy start (the "I
                // cannot start lazily-rs.md, killed immediately" symptom). Genuine
                // misses (pane never ready / hung) still fall through to the reap
                // branch below.
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "fresh_route_start_idle_no_op file={} pane={} harness={} timeout_secs={} note=trigger dispatched, pane dispatch-ready, no-op first cycle kept as idle session",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        ack_timeout.as_secs()
                    ),
                );
                eprintln!(
                    "[route] fresh {} start for {} produced a no-op first cycle (nothing queued); pane {} is idle and dispatch-ready — keeping the live idle session",
                    harness.binary,
                    file.display(),
                    dispatch_pane
                );
                let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
            }
            None => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "fresh_route_start_missing file={} pane={} harness={} timeout_secs={}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        ack_timeout.as_secs()
                    ),
                );
                let baseline_id = cycle_baseline.as_ref().map(|b| b.cycle_id.as_str());
                let _ = agent_doc_supervisor_io::startup_miss::record_startup_miss(
                    file,
                    &dispatch_pane,
                    session_id,
                    &harness.binary,
                    agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
                    baseline_id,
                );
                (effects.route_cycle_ack_effects.emit_startup_miss_diagnostic)(
                    tmux,
                    &dispatch_pane,
                    file,
                    &format!(
                        "fresh start: trigger {} but no document cycle started",
                        dispatch_start.dispatch_stage_label()
                    ),
                );
                anyhow::bail!(
                    "fresh {} start for {} never acknowledged with a document cycle after trigger {}",
                    harness.binary,
                    file.display(),
                    dispatch_start.startup_miss_label()
                );
            }
        }
    }

    let final_pane = if skip_wait {
        register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
        new_pane
    } else {
        resolve_fresh_dispatch_target_after_ready_wait(
            tmux,
            session_id,
            &new_pane,
            file_path,
            startup_miss_handoff_blocked_pane,
        )?
    };
    let _ = file; // suppress unused warning
    Ok(Some(final_pane))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_registration_decision_reuses_matching_live_owner() {
        assert_eq!(
            decide_existing_startup_registration(Some("%7"), true, Some("%7")),
            ExistingStartupRegistrationDecision::Reuse("%7")
        );
    }

    #[test]
    fn startup_registration_decision_ignores_alive_unowned_registry_pane() {
        assert_eq!(
            decide_existing_startup_registration(Some("%7"), true, None),
            ExistingStartupRegistrationDecision::IgnoreStale("%7")
        );
        assert_eq!(
            decide_existing_startup_registration(Some("%7"), true, Some("%9")),
            ExistingStartupRegistrationDecision::IgnoreStale("%7")
        );
    }

    #[test]
    fn startup_registration_decision_ignores_missing_or_dead_registration() {
        assert_eq!(
            decide_existing_startup_registration(None, false, None),
            ExistingStartupRegistrationDecision::None
        );
        assert_eq!(
            decide_existing_startup_registration(Some("%7"), false, Some("%7")),
            ExistingStartupRegistrationDecision::None
        );
    }
}
