//! Route launch-contract recovery before reusing an existing pane.

use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};

use agent_doc_controller::dispatch::fresh_route_admission_timeout;
use agent_doc_harness::HarnessConfig;
use tmux_router::Tmux;

use crate::authoritative_actor::{
    ManagedCapabilityProofStatus, managed_capability_proof_status,
    tracked_harness_clear_requires_fresh_restart,
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
    if !wait_for_agent_ready(
        tmux,
        &dispatch_pane,
        fresh_route_admission_timeout(cfg!(test)),
        harness,
    ) {
        anyhow::bail!(
            "latest tracked {} prompt for {} was `{}`, and the fresh recovery session in pane {} never became ready. Run `agent-doc start {}` manually to recover",
            harness.binary,
            file.display(),
            latest_prompt_label,
            dispatch_pane,
            file.display()
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
