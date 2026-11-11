//! `#shellstart`: launch `agent-doc start` in a claiming pane that is a shell.
//!
//! The launch is the pane's own process, not text typed into the shell. A fresh
//! shell can swallow typed bytes, and the document trigger must not be that text.

use anyhow::Result;
use std::path::Path;
use tmux_router::Tmux;

use crate::startup::{
    FRESH_ROUTE_AGENT_READY_TIMEOUT, launch_route_owned_start, route_owned_reap_policy_for_start,
};
use crate::startup_post_launch_barrier::PostLaunchBarrier;
use crate::startup_ready::wait_for_fresh_agent_ready;
use agent_doc_controller::shell_start::{
    ClaimingShellStartDecision, decide_claiming_shell_start, sole_live_shell_skips_manual_claim,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor::route_owned::RouteOwnedStartPurpose;
use agent_doc_tmux::AssociatedPaneCandidate;

/// The pane is an alive bare shell and no `agent-doc start` serves `file`.
pub fn claiming_shell_needs_start(tmux: &Tmux, file: &Path, pane: &str) -> bool {
    let pane_alive = tmux.pane_alive(pane);
    let bare_shell = pane_alive
        && agent_doc_tmux_io::target_current_command(tmux, pane)
            .as_deref()
            .map(str::trim)
            .is_some_and(agent_doc_tmux::pane_current_command_is_bare_shell);
    let start_running =
        agent_doc_supervisor_io::process::serving_supervisor_pid_for_doc(file).is_some();
    decide_claiming_shell_start(pane_alive, bare_shell, start_running)
        == ClaimingShellStartDecision::StartAgentDoc
}

/// The selected association is the only live pane, and it is a claiming shell
/// with no start. A second live pane stays a manual claim.
pub fn sole_live_claiming_shell_pane(
    tmux: &Tmux,
    file: &Path,
    winner: &AssociatedPaneCandidate,
    redundant: &[AssociatedPaneCandidate],
) -> bool {
    let redundant_alive = redundant
        .iter()
        .any(|candidate| tmux.pane_alive(&candidate.pane_id));
    let winner_alive = tmux.pane_alive(&winner.pane_id);
    let bare_shell = winner_alive
        && agent_doc_tmux_io::target_current_command(tmux, &winner.pane_id)
            .as_deref()
            .map(str::trim)
            .is_some_and(agent_doc_tmux::pane_current_command_is_bare_shell);
    let start_running =
        agent_doc_supervisor_io::process::serving_supervisor_pid_for_doc(file).is_some();
    sole_live_shell_skips_manual_claim(winner_alive, bare_shell, redundant_alive, start_running)
}

/// Launch route-owned `agent-doc start` in `pane` when it is a claiming shell
/// with no session, then wait until the harness is dispatch-ready.
///
/// Returns true when this call launched start. Returns false when the pane is
/// not that shell or a start is already serving the document, so the caller
/// keeps its existing dispatch path and does not start a second session.
pub fn ensure_claiming_shell_session(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    dispatch_only: bool,
) -> Result<bool> {
    if !claiming_shell_needs_start(tmux, file, pane) {
        return Ok(false);
    }
    let cwd = agent_doc_git_io::dirs::resolve_pane_cwd(file);
    let registry_base_dir = agent_doc_project_root_io::project_root_or_file_parent(file)
        .unwrap_or_else(|_| cwd.clone());
    let start_path = agent_doc_fs::rewrite_start_path(file, &cwd, file_path);
    let reap_policy = route_owned_reap_policy_for_start(
        false,
        dispatch_only,
        agent_doc_controller_io::route_snapshot::editor_route_attempt_id().as_deref(),
    );
    let start_cmd =
        agent_doc_supervisor_process::start_command::route_owned_start_command_with_options(
            &agent_doc_supervisor_process::agent_doc_start_bin(),
            std::path::Path::new(&start_path),
            &agent_doc_supervisor_process::start_command::RouteOwnedStartOptions {
                reap_policy,
                start_purpose: RouteOwnedStartPurpose::Dispatch,
                resume: None,
            },
        );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_claiming_shell_start file={} pane={} policy={} harness={} action=start",
            file.display(),
            pane,
            reap_policy.as_str(),
            harness.binary,
        ),
    );
    let post_launch_barrier = PostLaunchBarrier::arm(&registry_base_dir, file)?;
    launch_route_owned_start(tmux, file, pane, &cwd, &start_cmd, &harness.binary)?;
    if let Some(barrier) = post_launch_barrier {
        barrier.await_child_generation(file)?;
    }
    let ready_timeout =
        crate::invocation::wait_for_ready_override().unwrap_or(FRESH_ROUTE_AGENT_READY_TIMEOUT);
    let ready = wait_for_fresh_agent_ready(tmux, pane, ready_timeout, harness, file)?;
    if !ready {
        anyhow::bail!(
            "claiming shell for {} in pane {pane} did not become a ready {} session after `agent-doc start`",
            file.display(),
            harness.binary,
        );
    }
    Ok(true)
}
