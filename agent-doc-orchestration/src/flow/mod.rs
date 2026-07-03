//! Typed flow contracts for agent-doc hot paths.
//!
//! The first implementation phase is intentionally mirror-mode: existing command
//! modules still own behavior, while the flow layer provides pure decisions and
//! typed events that those modules can emit and test without tmux.

#![allow(dead_code)]

use anyhow::Result;
use std::path::Path;

#[cfg(test)]
pub mod closeout;

pub struct OrchestrationCloseoutEffects;

pub fn closeout_effects() -> OrchestrationCloseoutEffects {
    OrchestrationCloseoutEffects
}

impl agent_doc_flow_io::closeout::CloseoutEffects for OrchestrationCloseoutEffects {
    fn commit(&self, file: &Path) -> Result<bool> {
        crate::git::commit(file)
    }

    fn run_pending_maintenance(
        &self,
        file: &Path,
        force_disk: bool,
    ) -> Result<agent_doc_preflight_io::PendingMaintenanceReport> {
        if force_disk {
            agent_doc_preflight_io::run_pending_maintenance_force_disk(
                file,
                &crate::preflight::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
            )
        } else {
            agent_doc_preflight_io::run_pending_maintenance(
                file,
                &crate::preflight::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
            )
        }
    }

    fn enforce_clean_closeout(&self, file: &Path) -> Result<()> {
        crate::session_check::enforce_clean_closeout(file)
    }

    fn cancel_preflight_cycle(&self, file: &Path) -> Result<()> {
        crate::repair::cancel_preflight_cycle(file).map(|_| ())
    }

    fn ipc_direct_disk_degraded_for_file(&self, project_root: &Path, file: &Path) -> Result<bool> {
        crate::write::ipc_direct_disk_degraded_for_file(project_root, file)
    }

    fn detect_jb_cache_conflict_cancel_recoverable(&self, file: &Path) -> Result<bool> {
        agent_doc_session_check_io::detect_jb_cache_conflict_cancel_recoverable(file)
    }

    fn detect_bypassed_response_write(&self, file: &Path) -> Result<Option<String>> {
        agent_doc_session_check_io::detect_bypassed_response_write(file)
    }

    fn mark_committed_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &crate::PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }
}

#[cfg(test)]
mod tests {
    use agent_doc_flow::types::{FlowName, FlowOutcome, FlowStage};

    #[test]
    fn typed_flow_events_cover_route_write_closeout_session_check_and_child_patchback() {
        let route_event = agent_doc_controller::dispatch::dispatch_proof_failed_event(
            agent_doc_controller::dispatch::RoutedReopenGuardReason::AcceptedOnlyDispatchStartProof,
        );
        assert_eq!(route_event.flow, FlowName::RoutedReopen);
        assert_eq!(route_event.stage, FlowStage::DispatchProof);
        assert_eq!(route_event.outcome, FlowOutcome::FailedClosed);

        let typing_decision =
            agent_doc_document_realtime::write_policy::decide_visible_write_after_typing(
                agent_doc_document_realtime::write_policy::VisibleWriteTypingFacts {
                    idle_reached: false,
                    timeout_ms: 5_000,
                },
            );
        let write_event = agent_doc_document_realtime::write_policy::visible_write_guard_event(
            typing_decision,
            "socket_ipc",
        );
        assert_eq!(write_event.flow, FlowName::DocumentMutation);
        assert_eq!(write_event.stage, FlowStage::PreWriteGuard);
        assert_eq!(write_event.outcome, FlowOutcome::Blocked);

        let closeout_event = agent_doc_turn::closeout_guard::closeout_guard_event(
            FlowStage::SessionCheck,
            FlowOutcome::FailedClosed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::SessionCheckInterrupted,
        );
        assert_eq!(closeout_event.flow, FlowName::Closeout);
        assert_eq!(closeout_event.stage, FlowStage::SessionCheck);
        assert_eq!(closeout_event.outcome, FlowOutcome::FailedClosed);

        let child_patchback = agent_doc_template::patchback::normalize_child_template_response(
            "### Re: child - gpt-5\n\nImplemented.".to_string(),
        );
        assert_eq!(
            child_patchback.decision,
            agent_doc_template::patchback::ChildPatchbackNormalizationDecision::WrappedPlainResponse
        );
        let child_event =
            agent_doc_template::patchback::child_patchback_normalization_event(&child_patchback);
        assert_eq!(child_event.flow, FlowName::OrchestrationBatch);
        assert_eq!(child_event.stage, FlowStage::ChildCloseout);
        assert_eq!(child_event.outcome, FlowOutcome::Completed);

        let child = agent_doc_work_graph::BatchChildResult {
            label: "child".to_string(),
            outcome: child_event.outcome,
            proof: child_event.reason.clone(),
        };
        assert_eq!(
            agent_doc_work_graph::classify_batch_progress(
                false,
                child.outcome == FlowOutcome::Completed
            ),
            agent_doc_work_graph::BatchProgressDecision::Continue
        );

        let auto_dag_event = agent_doc_work_graph::auto_dag_schedule_event(
            agent_doc_work_graph::AutoDagScheduleDecision::SessionReviewBlocked,
            2,
            1,
        );
        assert_eq!(auto_dag_event.flow, FlowName::OrchestrationBatch);
        assert_eq!(auto_dag_event.stage, FlowStage::QueueFreeze);
        assert_eq!(auto_dag_event.outcome, FlowOutcome::Blocked);
    }
}
