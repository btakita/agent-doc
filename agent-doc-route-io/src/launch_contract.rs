//! Route launch-contract recovery before reusing an existing pane.

use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};

use agent_doc_controller::dispatch::fresh_route_admission_timeout;
use agent_doc_harness::HarnessConfig;
use tmux_router::Tmux;

use crate::authoritative_actor::{
    ManagedCapabilityProofStatus, load_authoritative_actor_for_registered_pane,
    managed_capability_proof_status, tracked_harness_clear_requires_fresh_restart,
};
use crate::dispatch_target::register_dispatch_target;
use crate::restart_handoff::wait_for_busy_restart_handoff;
use crate::startup_ready::wait_for_agent_ready;
use crate::supervisor_runtime::{
    SupervisorRestartRequestOutcome, request_restart_via_supervisor_with_mode,
    restart_via_supervisor_with_mode,
};

const TRACKED_CLEAR_IDLE_READY_PROOF: Duration = Duration::from_millis(450);
const CONTROLLER_REPLACEMENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackedClearRestartFallback {
    ForceControllerReplacement,
    Refuse,
}

#[derive(Debug, Clone, Copy)]
struct FreshRestartAutoTriggerFacts<'a> {
    pane: &'a str,
    generation: u64,
    state: agent_doc_controller::actor::ActorState,
    transition_reason: &'a str,
    live_busy_proof: bool,
}

fn authoritative_busy_owner_accepted(
    dispatch_pane: &str,
    facts: Option<FreshRestartAutoTriggerFacts<'_>>,
) -> bool {
    facts.is_some_and(|facts| {
        facts.pane == dispatch_pane
            && (facts.state == agent_doc_controller::actor::ActorState::Busy
                || facts.live_busy_proof)
    })
}

fn captured_pane_has_live_busy_proof(
    content: &str,
    harness: &HarnessConfig,
    cursor_y: Option<usize>,
) -> bool {
    if content.trim().is_empty() {
        return false;
    }
    harness.busy_proof_line(content).is_some()
        || !agent_doc_supervisor::detection::pane_dispatch_ready_at_cursor(
            content, harness, cursor_y,
        )
}

fn fresh_restart_auto_trigger_accepted(
    previous_generation: Option<u64>,
    dispatch_pane: &str,
    facts: Option<FreshRestartAutoTriggerFacts<'_>>,
) -> bool {
    facts.is_some_and(|facts| {
        facts.pane == dispatch_pane
            && facts.state == agent_doc_controller::actor::ActorState::Busy
            && facts.transition_reason == "auto_trigger_inject"
            && previous_generation.is_none_or(|generation| facts.generation > generation)
    })
}

fn fresh_restart_auto_trigger_accepted_for_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    pane: &str,
    previous_generation: Option<u64>,
) -> Result<bool> {
    let actor =
        load_authoritative_actor_for_registered_pane(tmux, file, session_id, file_path, pane)?;
    Ok(fresh_restart_auto_trigger_accepted(
        previous_generation,
        pane,
        actor.as_ref().map(|actor| FreshRestartAutoTriggerFacts {
            pane: &actor.record.pane_id,
            generation: actor.record.generation,
            state: actor.actor_state(),
            transition_reason: &actor.record.last_transition.reason,
            live_busy_proof: false,
        }),
    ))
}

fn authoritative_busy_owner_accepted_for_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    pane: &str,
    harness: &HarnessConfig,
) -> Result<bool> {
    let actor =
        load_authoritative_actor_for_registered_pane(tmux, file, session_id, file_path, pane)?;
    let live_busy_proof = agent_doc_tmux_io::capture_pane(tmux, pane)
        .ok()
        .is_some_and(|content| {
            captured_pane_has_live_busy_proof(
                &content,
                harness,
                agent_doc_tmux_io::pane_cursor_y(tmux, pane),
            )
        });
    Ok(authoritative_busy_owner_accepted(
        pane,
        actor.as_ref().map(|actor| FreshRestartAutoTriggerFacts {
            pane: &actor.record.pane_id,
            generation: actor.record.generation,
            state: actor.actor_state(),
            transition_reason: &actor.record.last_transition.reason,
            live_busy_proof,
        }),
    ))
}

fn tracked_clear_restart_fallback(
    rejection: &str,
    cycle_phase: Option<agent_doc_turn::CyclePhase>,
    pane_dispatch_ready: bool,
) -> TrackedClearRestartFallback {
    let rejected_for_open_cycle =
        rejection.contains("#haivendupsession") || rejection.contains("document cycle is open");
    if rejected_for_open_cycle
        && pane_dispatch_ready
        && matches!(
            cycle_phase,
            Some(
                agent_doc_turn::CyclePhase::ResponseCaptured
                    | agent_doc_turn::CyclePhase::WriteApplied
            )
        )
    {
        TrackedClearRestartFallback::ForceControllerReplacement
    } else {
        TrackedClearRestartFallback::Refuse
    }
}

fn wait_for_controller_replacement_generation(
    file: &Path,
    file_path: &str,
    session_id: &str,
    prior_generation: u64,
    timeout: Duration,
) -> Result<String> {
    let base_dir =
        agent_doc_session_registry_io::dispatch_registry::registry_base_dir_for_dispatch(file_path);
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(record) =
            agent_doc_controller_io::project_controller::authoritative_actor_binding(
                &base_dir, file,
            )?
            && record.session_id == session_id
            && record.generation > prior_generation
        {
            return Ok(record.pane_id);
        }
        std::thread::sleep(CONTROLLER_REPLACEMENT_POLL_INTERVAL);
    }
    anyhow::bail!(
        "forced fresh controller replacement for {} did not publish a generation newer than {} within {}s",
        file.display(),
        prior_generation,
        timeout.as_secs()
    )
}

fn force_controller_replacement_after_retained_clear(
    file: &Path,
    file_path: &str,
    session_id: &str,
) -> Result<String> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)
        .ok_or_else(|| anyhow::anyhow!("no project root contains {}", file.display()))?;
    let receipt = agent_doc_controller_io::project_controller::request_supervisor_replacement(
        &project_root,
        agent_doc_controller_io::project_controller::SupervisorReplacementRequest {
            file: file.to_path_buf(),
            mode: "fresh".to_string(),
            force: true,
        },
    )?;
    if !receipt.background_started {
        anyhow::bail!(
            "controller accepted forced fresh replacement for {} but did not start its replacement worker",
            file.display()
        );
    }
    wait_for_controller_replacement_generation(
        file,
        file_path,
        session_id,
        receipt.generation,
        fresh_route_admission_timeout(cfg!(test)),
    )
}

fn reapply_harness_launch_contract_after_clear(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    respect_tracked_clear_restart: bool,
) -> Result<String> {
    let latest_prompt = agent_doc_codex_hook_io::load_latest_prompt_for_file(file)?;
    if !respect_tracked_clear_restart
        || !tracked_harness_clear_requires_fresh_restart(harness, latest_prompt.as_deref())
    {
        return Ok(pane.to_string());
    }
    let latest_prompt_label = latest_prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .unwrap_or("<unknown>");
    let previous_generation =
        load_authoritative_actor_for_registered_pane(tmux, file, session_id, file_path, pane)?
            .map(|actor| actor.record.generation);

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_harness_clear_restart_fresh file={} pane={} harness={} latest_prompt={:?}",
            file.display(),
            pane,
            harness.binary,
            latest_prompt_label
        ),
    );
    eprintln!(
        "[route] latest tracked {} prompt for {} was `{}` — restarting the live session fresh before reroute so sandbox, writable roots, and network policy are reapplied",
        harness.binary,
        file.display(),
        latest_prompt_label
    );

    let dispatch_pane = match request_restart_via_supervisor_with_mode(file, session_id, "fresh") {
        SupervisorRestartRequestOutcome::Accepted => {
            wait_for_busy_restart_handoff(tmux, file, file_path, session_id, pane);
            agent_doc_sync_io::sync::find_normal_path_owner_pane(tmux, file, session_id)
                .unwrap_or_else(|| pane.to_string())
        }
        SupervisorRestartRequestOutcome::Rejected(reason) => {
            let cycle_phase = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
                .map(|state| state.phase);
            let pane_dispatch_ready =
                wait_for_agent_ready(tmux, pane, TRACKED_CLEAR_IDLE_READY_PROOF, harness);
            match tracked_clear_restart_fallback(&reason, cycle_phase, pane_dispatch_ready) {
                TrackedClearRestartFallback::ForceControllerReplacement => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "route_harness_clear_retained_closeout_force_replacement file={} pane={} harness={} phase={} rejection={:?}",
                            file.display(),
                            pane,
                            harness.binary,
                            cycle_phase
                                .map(|phase| format!("{phase:?}"))
                                .unwrap_or_else(|| "none".to_string()),
                            reason,
                        ),
                    );
                    eprintln!(
                        "[route] in-place fresh restart was blocked by the retained closeout on {}; replacing the idle owner through the controller before reroute",
                        file.display()
                    );
                    force_controller_replacement_after_retained_clear(file, file_path, session_id)?
                }
                TrackedClearRestartFallback::Refuse => {
                    anyhow::bail!(
                        "latest tracked {} prompt for {} was `{}`, but the live supervisor rejected a fresh restart: {}. Route refused a forced replacement because the pane was not proven idle at a durable retained-closeout boundary",
                        harness.binary,
                        file.display(),
                        latest_prompt_label,
                        reason
                    );
                }
            }
        }
        SupervisorRestartRequestOutcome::Unavailable(reason) => {
            anyhow::bail!(
                "latest tracked {} prompt for {} was `{}`, but route could not reach the live supervisor for a fresh restart: {}",
                harness.binary,
                file.display(),
                latest_prompt_label,
                reason
            );
        }
    };
    let pane_ready = wait_for_agent_ready(
        tmux,
        &dispatch_pane,
        fresh_route_admission_timeout(cfg!(test)),
        harness,
    );
    let auto_trigger_accepted = if pane_ready {
        false
    } else {
        fresh_restart_auto_trigger_accepted_for_pane(
            tmux,
            file,
            session_id,
            file_path,
            &dispatch_pane,
            previous_generation,
        )?
    };
    if !pane_ready && !auto_trigger_accepted {
        anyhow::bail!(
            "latest tracked {} prompt for {} was `{}`, and the fresh recovery session in pane {} never became ready. Run `agent-doc start {}` manually to recover",
            harness.binary,
            file.display(),
            latest_prompt_label,
            dispatch_pane,
            file.display()
        );
    }
    if auto_trigger_accepted {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_fresh_restart_auto_trigger_accepted file={} pane={} harness={} recovery=coalesce_active_dispatch",
                file.display(),
                dispatch_pane,
                harness.binary,
            ),
        );
        eprintln!(
            "[route] fresh {} session in pane {} already accepted the agent-doc auto-trigger; continuing as an owned active dispatch",
            harness.binary, dispatch_pane,
        );
    }
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    Ok(dispatch_pane)
}

fn reapply_capability_contract_before_reuse(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    enforce_capability_proof: bool,
) -> Result<String> {
    if !enforce_capability_proof {
        return Ok(pane.to_string());
    }
    let proof_status = managed_capability_proof_status(file, session_id, harness)?;
    let reason = match proof_status {
        ManagedCapabilityProofStatus::NotRequired
        | ManagedCapabilityProofStatus::Proven
        | ManagedCapabilityProofStatus::Pending => {
            return Ok(pane.to_string());
        }
        ManagedCapabilityProofStatus::Failed => {
            anyhow::bail!(
                "managed {} capability proof for {} on pane {} failed; prompt dispatch is disabled for this pane. Inspect diagnostics, then run `agent-doc start {}` manually to recover",
                harness.binary,
                file.display(),
                pane,
                file.display()
            );
        }
        ManagedCapabilityProofStatus::Missing => {
            if authoritative_busy_owner_accepted_for_pane(
                tmux, file, session_id, file_path, pane, harness,
            )? {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_capability_proof_missing_active_owner_coalesced file={} pane={} harness={} recovery=coalesce_active_dispatch",
                        file.display(),
                        pane,
                        harness.binary,
                    ),
                );
                eprintln!(
                    "[route] managed {} session in pane {} is already the authoritative busy owner for {}; coalescing the active dispatch before capability-proof reuse",
                    harness.binary,
                    pane,
                    file.display(),
                );
                return Ok(pane.to_string());
            }
            format!(
                "managed {} session has no current capability proof for requested network, SSH, or writable-root access",
                harness.binary
            )
        }
    };

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_{}_capability_restart_fresh file={} pane={} harness={} reason={}",
            harness.binary,
            file.display(),
            pane,
            harness.binary,
            reason.replace(' ', "_")
        ),
    );
    eprintln!(
        "[route] {} for {} on pane {} — restarting the live {} session fresh once before reuse",
        reason,
        file.display(),
        pane,
        harness.binary
    );

    if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
        anyhow::bail!(
            "{} for {} on pane {}, and route could not restart the live session fresh. Run `agent-doc start {}` manually to recover",
            reason,
            file.display(),
            pane,
            file.display()
        );
    }

    wait_for_busy_restart_handoff(tmux, file, file_path, session_id, pane);
    let dispatch_pane =
        agent_doc_sync_io::sync::find_normal_path_owner_pane(tmux, file, session_id)
            .unwrap_or_else(|| pane.to_string());
    if !wait_for_agent_ready(
        tmux,
        &dispatch_pane,
        fresh_route_admission_timeout(cfg!(test)),
        harness,
    ) {
        anyhow::bail!(
            "{} for {}, and the fresh recovery session in pane {} never became ready. Run `agent-doc start {}` manually to recover",
            reason,
            file.display(),
            dispatch_pane,
            file.display()
        );
    }
    match managed_capability_proof_status(file, session_id, harness)? {
        ManagedCapabilityProofStatus::NotRequired
        | ManagedCapabilityProofStatus::Proven
        | ManagedCapabilityProofStatus::Pending => {}
        ManagedCapabilityProofStatus::Failed => anyhow::bail!(
            "{} for {}, and the fresh recovery session in pane {} failed capability proof. Run `agent-doc start {}` manually to recover",
            reason,
            file.display(),
            dispatch_pane,
            file.display()
        ),
        ManagedCapabilityProofStatus::Missing => anyhow::bail!(
            "{} for {}, and the fresh recovery session in pane {} never recorded a capability proof. Run `agent-doc start {}` manually to recover",
            reason,
            file.display(),
            dispatch_pane,
            file.display()
        ),
    }
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    Ok(dispatch_pane)
}

#[allow(clippy::too_many_arguments)]
pub fn reapply_codex_launch_contract_before_reuse(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    respect_tracked_clear_restart: bool,
    enforce_capability_proof: bool,
) -> Result<String> {
    let dispatch_pane = reapply_harness_launch_contract_after_clear(
        tmux,
        file,
        pane,
        session_id,
        file_path,
        harness,
        respect_tracked_clear_restart,
    )?;
    reapply_capability_contract_before_reuse(
        tmux,
        file,
        &dispatch_pane,
        session_id,
        file_path,
        harness,
        enforce_capability_proof,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_restart_accepts_new_generation_auto_trigger_as_owned_dispatch() {
        let accepted = FreshRestartAutoTriggerFacts {
            pane: "%4",
            generation: 73,
            state: agent_doc_controller::actor::ActorState::Busy,
            transition_reason: "auto_trigger_inject",
            live_busy_proof: false,
        };
        assert!(fresh_restart_auto_trigger_accepted(
            Some(72),
            "%4",
            Some(accepted),
        ));
        assert!(!fresh_restart_auto_trigger_accepted(
            Some(73),
            "%4",
            Some(accepted),
        ));
        assert!(!fresh_restart_auto_trigger_accepted(
            Some(72),
            "%9",
            Some(accepted),
        ));
        assert!(!fresh_restart_auto_trigger_accepted(
            Some(72),
            "%4",
            Some(FreshRestartAutoTriggerFacts {
                transition_reason: "ipc_inject",
                ..accepted
            }),
        ));
    }

    #[test]
    fn missing_capability_proof_accepts_authoritative_busy_owner_without_restart() {
        let busy = FreshRestartAutoTriggerFacts {
            pane: "%4",
            generation: 73,
            state: agent_doc_controller::actor::ActorState::Busy,
            transition_reason: "ipc_inject",
            live_busy_proof: false,
        };
        assert!(authoritative_busy_owner_accepted("%4", Some(busy)));
        assert!(!authoritative_busy_owner_accepted("%9", Some(busy)));
        assert!(!authoritative_busy_owner_accepted(
            "%4",
            Some(FreshRestartAutoTriggerFacts {
                state: agent_doc_controller::actor::ActorState::Ready,
                ..busy
            }),
        ));
        assert!(authoritative_busy_owner_accepted(
            "%4",
            Some(FreshRestartAutoTriggerFacts {
                state: agent_doc_controller::actor::ActorState::Ready,
                live_busy_proof: true,
                ..busy
            }),
        ));
    }

    #[test]
    fn live_busy_proof_accepts_codex_approval_modal_but_not_idle_or_empty_panes() {
        let harness = HarnessConfig::codex();
        let approval_modal = "\
Would you like to run the following command?\n\n\
› 1. Yes, proceed (y)\n\
  2. No, and tell Codex what to do differently (esc)\n\n\
  Press enter to confirm or esc to cancel\n";
        assert!(captured_pane_has_live_busy_proof(
            approval_modal,
            &harness,
            None,
        ));

        let idle = "\
› Ask Codex to do anything\n\
gpt-5.6-sol xhigh · ~/work/btakita/agent-loop · Context 10% used\n";
        assert!(!captured_pane_has_live_busy_proof(idle, &harness, None));
        assert!(!captured_pane_has_live_busy_proof("", &harness, None));
    }

    #[test]
    fn tracked_clear_open_cycle_fallback_only_replaces_idle_retained_closeouts() {
        let rejection = "supervisor restart deferred: a document cycle is open (#haivendupsession)";
        for phase in [
            agent_doc_turn::CyclePhase::ResponseCaptured,
            agent_doc_turn::CyclePhase::WriteApplied,
        ] {
            assert_eq!(
                tracked_clear_restart_fallback(rejection, Some(phase), true),
                TrackedClearRestartFallback::ForceControllerReplacement
            );
            assert_eq!(
                tracked_clear_restart_fallback(rejection, Some(phase), false),
                TrackedClearRestartFallback::Refuse
            );
        }

        for phase in [
            None,
            Some(agent_doc_turn::CyclePhase::PreflightStarted),
            Some(agent_doc_turn::CyclePhase::Committed),
            Some(agent_doc_turn::CyclePhase::Abandoned),
        ] {
            assert_eq!(
                tracked_clear_restart_fallback(rejection, phase, true),
                TrackedClearRestartFallback::Refuse
            );
        }
        assert_eq!(
            tracked_clear_restart_fallback(
                "supervisor socket unavailable",
                Some(agent_doc_turn::CyclePhase::ResponseCaptured),
                true,
            ),
            TrackedClearRestartFallback::Refuse
        );
    }
}
