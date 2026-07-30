use anyhow::Result;
use std::path::Path;
use std::time::Duration;

use agent_doc_controller::dispatch::dispatch_only_starting_pane_ready_timeout_for_binary;
use agent_doc_harness::HarnessConfig;
use agent_doc_turn::closeout_recovery::{CloseoutRecoveryDecision, CloseoutRecoveryDecisionInput};

use crate::authoritative_dispatch::RouteAuthoritativeActorEffects;
use crate::closeout_drain::RouteCloseoutDrainEffects;
use crate::command::RouteCommandEffects;
use crate::cycle_ack::RouteCycleAckEffects;
use crate::diagnostics::{
    emit_busy_route_diagnostic, emit_busy_route_queued_diagnostic,
    emit_busy_route_queued_diagnostic_from_facts, emit_startup_miss_diagnostic,
    file_route_dispatch_bug_report_with_runtime_effects,
};
use crate::dispatch::RouteDispatchEffects;
use crate::dispatch_only::{DispatchOnlyQueuedPromptOutcome, DispatchOnlyRouteEffects};
use crate::document_prep::RouteDocumentPrepEffects;
use crate::document_write::route_write_document;
use crate::pane_resolution::{ManagedPaneResolutionEffects, RouteBusyPaneRetryEffects};
use crate::queue_dispatch::{RouteQueueEffects, enqueue_route_dispatch_prompt};
use crate::startup::RouteStartupEffects;

pub fn route_dispatch_effects() -> RouteDispatchEffects {
    RouteDispatchEffects {
        file_route_dispatch_bug_report: file_route_dispatch_bug_report_with_runtime_effects,
        emit_busy_route_queued_diagnostic: emit_busy_route_queued_diagnostic_from_facts,
    }
}

pub fn route_cycle_ack_effects() -> RouteCycleAckEffects {
    RouteCycleAckEffects {
        route_dispatch_effects: route_dispatch_effects(),
        emit_startup_miss_diagnostic,
        emit_busy_route_diagnostic,
    }
}

pub fn route_busy_pane_retry_effects() -> RouteBusyPaneRetryEffects {
    RouteBusyPaneRetryEffects {
        route_dispatch_effects: route_dispatch_effects(),
        route_cycle_ack_effects: route_cycle_ack_effects(),
        emit_busy_route_diagnostic,
    }
}

pub fn route_queue_effects() -> RouteQueueEffects {
    RouteQueueEffects {
        write_document: route_write_document,
    }
}

pub fn route_document_prep_effects() -> RouteDocumentPrepEffects {
    RouteDocumentPrepEffects {
        write_document: route_write_document,
    }
}

pub fn enqueue_route_dispatch_prompt_for_dispatch_only(
    file: &Path,
    prompt_text: &str,
    source: &str,
    priority: bool,
) -> Result<DispatchOnlyQueuedPromptOutcome> {
    let outcome =
        enqueue_route_dispatch_prompt(file, prompt_text, source, priority, route_queue_effects())?;
    Ok(DispatchOnlyQueuedPromptOutcome {
        prompt_text: outcome.prompt_text,
        appended: outcome.appended,
        already_present: outcome.already_present,
        superseded: outcome.superseded,
    })
}

pub fn dispatch_only_starting_pane_ready_timeout(harness: &HarnessConfig) -> Duration {
    crate::invocation::wait_for_ready_override().unwrap_or_else(|| {
        dispatch_only_starting_pane_ready_timeout_for_binary(Some(&harness.binary), cfg!(test))
    })
}

pub fn route_dispatch_only_effects() -> DispatchOnlyRouteEffects {
    DispatchOnlyRouteEffects {
        route_dispatch_effects: route_dispatch_effects(),
        enqueue_route_dispatch_prompt: enqueue_route_dispatch_prompt_for_dispatch_only,
        emit_busy_route_queued_diagnostic,
        emit_busy_route_diagnostic,
        dispatch_only_starting_pane_ready_timeout,
        file_route_dispatch_bug_report: file_route_dispatch_bug_report_with_runtime_effects,
    }
}

pub fn route_startup_effects() -> RouteStartupEffects {
    RouteStartupEffects {
        route_dispatch_effects: route_dispatch_effects(),
        dispatch_only_route_effects: route_dispatch_only_effects(),
        route_cycle_ack_effects: route_cycle_ack_effects(),
    }
}

pub fn route_managed_pane_resolution_effects(
    repair_closeout: fn(&Path) -> Result<String>,
) -> ManagedPaneResolutionEffects {
    ManagedPaneResolutionEffects {
        route_dispatch_effects: route_dispatch_effects(),
        route_cycle_ack_effects: route_cycle_ack_effects(),
        route_busy_pane_retry_effects: route_busy_pane_retry_effects(),
        route_startup_effects: route_startup_effects(),
        route_authoritative_actor_effects: route_authoritative_actor_effects(repair_closeout),
    }
}

pub fn route_authoritative_actor_effects(
    repair_closeout: fn(&Path) -> Result<String>,
) -> RouteAuthoritativeActorEffects {
    RouteAuthoritativeActorEffects {
        closeout_drain_effects: route_closeout_drain_effects(repair_closeout),
        queue_effects: route_queue_effects(),
        route_dispatch_effects: route_dispatch_effects(),
        route_cycle_ack_effects: route_cycle_ack_effects(),
        dispatch_only_route_effects: route_dispatch_only_effects(),
        wait_for_ready_override: crate::invocation::wait_for_ready_override,
    }
}

fn route_run_pending_maintenance(file: &Path, force_disk: bool) -> Result<()> {
    if force_disk {
        agent_doc_preflight_io::run_pending_maintenance_force_disk(
            file,
            &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
        )
        .map(|_| ())
    } else {
        agent_doc_preflight_io::run_pending_maintenance(
            file,
            &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
        )
        .map(|_| ())
    }
}

fn route_inspect_session(file: &Path) -> Result<agent_doc_session_check_io::SessionCheckStatus> {
    agent_doc_session_check_io::inspect(
        file,
        &agent_doc_closeout_runtime_io::session_check_effects(),
    )
}

fn route_await_closeout_projection(
    file: &Path,
    cycle_id: &str,
    wait: std::time::Duration,
) -> Result<agent_doc_controller_io::project_controller::CloseoutCycleWaitOutcome> {
    agent_doc_controller_io::project_controller::await_closeout_cycle_progress_for_file(
        file, cycle_id, wait,
    )
}

fn route_cancel_empty_preflight(file: &Path) -> Result<bool> {
    agent_doc_repair_io::cancel_preflight_cycle(
        &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
        file,
    )
    .map(|outcome| matches!(outcome, agent_doc_turn::repair::CancelOutcome::Abandoned))
}

fn route_decide_closeout_recovery(
    file: &Path,
    input: CloseoutRecoveryDecisionInput<'_>,
) -> CloseoutRecoveryDecision {
    agent_doc_flow_io::closeout::decide_closeout_recovery(
        file,
        input,
        &agent_doc_closeout_runtime_io::closeout_effects(),
    )
}

pub fn route_closeout_drain_effects(
    repair_closeout: fn(&Path) -> Result<String>,
) -> RouteCloseoutDrainEffects {
    RouteCloseoutDrainEffects {
        force_disk_route_writes: crate::invocation::force_disk_route_writes,
        run_pending_maintenance: route_run_pending_maintenance,
        cancel_empty_preflight: route_cancel_empty_preflight,
        repair_closeout,
        inspect_session: route_inspect_session,
        await_closeout_projection: route_await_closeout_projection,
        decide_closeout_recovery: route_decide_closeout_recovery,
    }
}

pub fn route_command_effects(repair_closeout: fn(&Path) -> Result<String>) -> RouteCommandEffects {
    RouteCommandEffects {
        document_prep_effects: route_document_prep_effects(),
        managed_pane_resolution_effects: route_managed_pane_resolution_effects(repair_closeout),
        authoritative_actor_effects: route_authoritative_actor_effects(repair_closeout),
        dispatch_only_effects: route_dispatch_only_effects(),
        startup_effects: route_startup_effects(),
    }
}
