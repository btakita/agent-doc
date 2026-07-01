use agent_doc_controller::dispatch::RoutedReopenGuardReason;
use agent_doc_flow::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};
use std::path::Path;

pub fn prompt_ready_barrier_failed_event(reason: RoutedReopenGuardReason) -> FlowEvent {
    FlowEvent::new(
        FlowName::RoutedReopen,
        FlowStage::PromptReadyBarrier,
        FlowOutcome::FailedClosed,
    )
    .with_reason(reason.as_str())
}

pub fn dispatch_proof_failed_event(reason: RoutedReopenGuardReason) -> FlowEvent {
    FlowEvent::new(
        FlowName::RoutedReopen,
        FlowStage::DispatchProof,
        FlowOutcome::FailedClosed,
    )
    .with_reason(reason.as_str())
}

pub fn log_prompt_ready_barrier_failed(file: &Path, reason: RoutedReopenGuardReason) {
    super::proof::log_flow_event(file, prompt_ready_barrier_failed_event(reason));
}

pub fn log_dispatch_proof_failed(file: &Path, reason: RoutedReopenGuardReason) {
    super::proof::log_flow_event(file, dispatch_proof_failed_event(reason));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_failure_events_are_owned_by_routed_reopen_flow() {
        let prompt_event =
            prompt_ready_barrier_failed_event(RoutedReopenGuardReason::StartingActorNotReady);
        assert_eq!(prompt_event.flow, FlowName::RoutedReopen);
        assert_eq!(prompt_event.stage, FlowStage::PromptReadyBarrier);
        assert_eq!(prompt_event.outcome, FlowOutcome::FailedClosed);
        assert_eq!(
            prompt_event.reason.as_deref(),
            Some("starting_actor_not_ready")
        );

        let proof_event =
            dispatch_proof_failed_event(RoutedReopenGuardReason::AcceptedOnlyDispatchStartProof);
        assert_eq!(proof_event.flow, FlowName::RoutedReopen);
        assert_eq!(proof_event.stage, FlowStage::DispatchProof);
        assert_eq!(proof_event.outcome, FlowOutcome::FailedClosed);
        assert_eq!(
            proof_event.reason.as_deref(),
            Some("accepted_only_dispatch_start_proof")
        );
    }
}
