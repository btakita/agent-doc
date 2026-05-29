//! Typed flow contracts for agent-doc hot paths.
//!
//! The first implementation phase is intentionally mirror-mode: existing command
//! modules still own behavior, while the flow layer provides pure decisions and
//! typed events that those modules can emit and test without tmux.

#![allow(dead_code)]

pub mod closeout;
pub mod document_mutation;
pub mod operator_clear;
pub mod orchestration_batch;
pub mod proof;
pub mod routed_reopen;
pub mod session_cycle;
pub mod types;

#[cfg(test)]
mod tests {
    use super::*;
    use types::{FlowName, FlowOutcome, FlowStage};

    #[test]
    fn typed_flow_events_cover_route_write_closeout_session_check_and_child_patchback() {
        let route_event = routed_reopen::dispatch_proof_failed_event(
            routed_reopen::RoutedReopenGuardReason::AcceptedOnlyDispatchStartProof,
        );
        assert_eq!(route_event.flow, FlowName::RoutedReopen);
        assert_eq!(route_event.stage, FlowStage::DispatchProof);
        assert_eq!(route_event.outcome, FlowOutcome::FailedClosed);

        let typing_decision = document_mutation::decide_visible_write_after_typing(
            document_mutation::VisibleWriteTypingFacts {
                idle_reached: false,
                timeout_ms: 5_000,
            },
        );
        let write_event =
            document_mutation::visible_write_guard_event(typing_decision, "socket_ipc");
        assert_eq!(write_event.flow, FlowName::DocumentMutation);
        assert_eq!(write_event.stage, FlowStage::PreWriteGuard);
        assert_eq!(write_event.outcome, FlowOutcome::Blocked);

        let closeout_event = closeout::closeout_guard_event(
            FlowStage::SessionCheck,
            FlowOutcome::FailedClosed,
            closeout::CloseoutGuardReason::SessionCheckInterrupted,
        );
        assert_eq!(closeout_event.flow, FlowName::Closeout);
        assert_eq!(closeout_event.stage, FlowStage::SessionCheck);
        assert_eq!(closeout_event.outcome, FlowOutcome::FailedClosed);

        let child_patchback = orchestration_batch::normalize_child_template_response(
            "### Re: child - gpt-5\n\nImplemented.".to_string(),
        );
        assert_eq!(
            child_patchback.decision,
            orchestration_batch::ChildPatchbackNormalizationDecision::WrappedPlainResponse
        );
        let child_event =
            orchestration_batch::child_patchback_normalization_event(&child_patchback);
        assert_eq!(child_event.flow, FlowName::OrchestrationBatch);
        assert_eq!(child_event.stage, FlowStage::ChildCloseout);
        assert_eq!(child_event.outcome, FlowOutcome::Completed);

        let child = orchestration_batch::BatchChildResult {
            label: "child".to_string(),
            outcome: child_event.outcome,
            proof: child_event.reason.clone(),
        };
        assert!(orchestration_batch::batch_should_continue(false, &child));

        let auto_dag_event = orchestration_batch::auto_dag_schedule_event(
            orchestration_batch::AutoDagScheduleDecision::SessionReviewBlocked,
            2,
            1,
        );
        assert_eq!(auto_dag_event.flow, FlowName::OrchestrationBatch);
        assert_eq!(auto_dag_event.stage, FlowStage::QueueFreeze);
        assert_eq!(auto_dag_event.outcome, FlowOutcome::Blocked);
    }
}
