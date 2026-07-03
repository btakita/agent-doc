//! Route dispatch recovery and fresh-target validation.

use crate::dispatch_target::register_dispatch_target;
use agent_doc_controller::dispatch::{
    FreshDispatchTargetAfterReadyWaitDecision, FreshDispatchTargetAfterReadyWaitFacts,
    decide_fresh_dispatch_target_after_ready_wait,
    dispatch_only_starting_pane_recovery_retry_budget,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor::startup_miss::{
    StartingPaneRecoveryTarget, starting_pane_recovery_target,
};
use anyhow::{Context, Result};
use std::path::Path;
use tmux_router::Tmux;

pub fn resolve_fresh_dispatch_target_after_ready_wait(
    tmux: &Tmux,
    session_id: &str,
    pane: &str,
    file_path: &str,
    _startup_miss_handoff_blocked_pane: Option<&str>,
) -> Result<String> {
    let registry_base_dir =
        agent_doc_session_registry_io::dispatch_registry::registry_base_dir_for_dispatch(file_path);
    let registry =
        agent_doc_session_registry_io::load_in(&registry_base_dir).with_context(|| {
            format!(
                "failed to load route registry before fresh-dispatch validation from {}",
                registry_base_dir.display()
            )
        })?;
    let requested = agent_doc_session_registry_io::dispatch_registry::canonical_dispatch_file(
        Path::new(file_path),
    );
    let requested_display = requested.display().to_string();
    let pane_matches_file =
        agent_doc_session_registry_io::dispatch_registry::pane_registration_matches_file(
            &registry, pane, file_path,
        );
    let handoff_target = registry
        .values()
        .find(|entry| {
            entry.session_id == session_id
                && !entry.pane.is_empty()
                && entry.pane != pane
                && agent_doc_session_registry_io::dispatch_registry::canonical_registered_file(
                    entry,
                ) == requested
        })
        .map(|entry| entry.pane.as_str());
    let registered_display = registry
        .values()
        .find(|entry| entry.pane == pane)
        .map(agent_doc_session_registry_io::dispatch_registry::canonical_registered_file)
        .map(|path| path.display().to_string());
    match decide_fresh_dispatch_target_after_ready_wait(FreshDispatchTargetAfterReadyWaitFacts {
        requested_pane: pane,
        dispatch_file_display: file_path,
        requested_file_display: &requested_display,
        pane_matches_file,
        same_session_rebound_pane: handoff_target,
        registered_file_display: registered_display.as_deref(),
    }) {
        FreshDispatchTargetAfterReadyWaitDecision::KeepRequestedPane => Ok(pane.to_string()),
        FreshDispatchTargetAfterReadyWaitDecision::UseReboundPane { pane, log_line } => {
            eprintln!("{log_line}");
            Ok(pane.to_string())
        }
        FreshDispatchTargetAfterReadyWaitDecision::RejectCrossFile { message } => {
            anyhow::bail!(message)
        }
        FreshDispatchTargetAfterReadyWaitDecision::RegisterRequestedPane => {
            // A fresh route already created `pane` deliberately. If some concurrent
            // sync/layout path rebinds the same document session back to another pane
            // during the ready wait, keep the fresh pane authoritative instead of
            // handing dispatch back to the older pane and making the new pane disposable.
            register_dispatch_target(tmux, session_id, pane, file_path)?;
            Ok(pane.to_string())
        }
    }
}

pub fn wait_for_starting_pane_recovery_target(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    current_pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    initial_status: Option<&agent_doc_supervisor::startup_miss::SessionLogStatus>,
) -> Option<StartingPaneRecoveryTarget> {
    let registry_base_dir =
        agent_doc_session_registry_io::dispatch_registry::registry_base_dir_for_dispatch(file_path);
    let budget = dispatch_only_starting_pane_recovery_retry_budget(
        Some(harness.binary.as_str()),
        cfg!(test),
    );
    let deadline = std::time::Instant::now() + budget.timeout;

    while std::time::Instant::now() < deadline {
        let current_status =
            agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)
                .ok()
                .flatten();
        let registry = agent_doc_session_registry_io::load_in(&registry_base_dir).ok();
        let registered_pane =
            agent_doc_session_registry_io::lookup_in(&registry_base_dir, session_id)
                .ok()
                .flatten();

        match starting_pane_recovery_target(
            initial_status,
            current_status.as_ref(),
            current_pane,
            registered_pane.as_deref(),
        ) {
            Some(StartingPaneRecoveryTarget::DifferentPane(pane))
                if registry.as_ref().is_some_and(|registry| {
                    agent_doc_session_registry_io::dispatch_registry::pane_registration_matches_file(
                        registry, &pane, file_path,
                    )
                }) && tmux.pane_alive(&pane) =>
            {
                return Some(StartingPaneRecoveryTarget::DifferentPane(pane));
            }
            Some(StartingPaneRecoveryTarget::SamePane) => {
                return Some(StartingPaneRecoveryTarget::SamePane);
            }
            _ => {}
        }

        std::thread::sleep(budget.poll_interval);
    }

    None
}
