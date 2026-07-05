use anyhow::Result;
use std::path::Path;
use std::time::Duration;

use agent_doc_controller::dispatch::dispatch_only_starting_pane_ready_timeout_for_binary;
use agent_doc_harness::HarnessConfig;

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
use crate::pane_resolution::RouteBusyPaneRetryEffects;
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
