use super::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};
use agent_doc_template::patchback;
use agent_doc_work_graph::BatchProgressDecision;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchChildResult {
    pub label: String,
    pub outcome: FlowOutcome,
    pub proof: Option<String>,
}

pub fn queue_freeze_event(task_count: usize, from_exchange: bool) -> FlowEvent {
    FlowEvent::new(
        FlowName::OrchestrationBatch,
        FlowStage::QueueFreeze,
        FlowOutcome::Completed,
    )
    .with_reason(format!(
        "tasks:{task_count}:source:{}",
        if from_exchange {
            "exchange"
        } else {
            "explicit"
        }
    ))
}

pub fn source_changed_event(completed_steps: usize, total_steps: usize) -> FlowEvent {
    FlowEvent::new(
        FlowName::OrchestrationBatch,
        FlowStage::QueueFreeze,
        FlowOutcome::Blocked,
    )
    .with_reason(format!(
        "{}:{}_of_{}",
        BatchProgressDecision::StopSourceChanged.as_str(),
        completed_steps,
        total_steps
    ))
}

pub fn child_closeout_event(child: &BatchChildResult) -> FlowEvent {
    FlowEvent::new(
        FlowName::OrchestrationBatch,
        FlowStage::ChildCloseout,
        child.outcome,
    )
    .with_reason(
        child
            .proof
            .as_deref()
            .unwrap_or(child.label.as_str())
            .to_string(),
    )
}

pub fn log_queue_freeze_event(file: &Path, task_count: usize, from_exchange: bool) {
    super::proof::log_flow_event(file, queue_freeze_event(task_count, from_exchange));
}

pub fn log_source_changed_event(file: &Path, completed_steps: usize, total_steps: usize) {
    super::proof::log_flow_event(file, source_changed_event(completed_steps, total_steps));
}

pub fn log_child_closeout_event(file: &Path, child: &BatchChildResult) {
    super::proof::log_flow_event(file, child_closeout_event(child));
}

pub fn auto_dag_schedule_event(
    decision: agent_doc_work_graph::AutoDagScheduleDecision,
    node_count: usize,
    batch_count: usize,
) -> FlowEvent {
    let outcome = match decision {
        agent_doc_work_graph::AutoDagScheduleDecision::Ready => FlowOutcome::Completed,
        agent_doc_work_graph::AutoDagScheduleDecision::SessionReviewBlocked => FlowOutcome::Blocked,
    };
    FlowEvent::new(
        FlowName::OrchestrationBatch,
        FlowStage::QueueFreeze,
        outcome,
    )
    .with_reason(format!(
        "auto_dag_schedule:{}:nodes:{node_count}:batches:{batch_count}",
        decision.as_str()
    ))
}

pub fn log_auto_dag_schedule_event(
    file: &Path,
    decision: agent_doc_work_graph::AutoDagScheduleDecision,
    node_count: usize,
    batch_count: usize,
) {
    super::proof::log_flow_event(
        file,
        auto_dag_schedule_event(decision, node_count, batch_count),
    );
}

pub fn child_patchback_normalization_event(
    normalization: &patchback::ChildPatchbackNormalization,
) -> FlowEvent {
    let outcome = match normalization.decision {
        patchback::ChildPatchbackNormalizationDecision::WrappedPlainResponse
        | patchback::ChildPatchbackNormalizationDecision::KeptExplicitPatch => {
            FlowOutcome::Completed
        }
        patchback::ChildPatchbackNormalizationDecision::KeptRejectedPlainResponse
        | patchback::ChildPatchbackNormalizationDecision::KeptUnparseable => {
            FlowOutcome::FailedClosed
        }
    };
    FlowEvent::new(
        FlowName::OrchestrationBatch,
        FlowStage::ChildCloseout,
        outcome,
    )
    .with_reason(format!(
        "child_patchback:{}",
        normalization.decision.as_str()
    ))
}

pub fn log_child_patchback_normalization_event(
    file: &Path,
    normalization: &patchback::ChildPatchbackNormalization,
) {
    super::proof::log_flow_event(file, child_patchback_normalization_event(normalization));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_change_stops_batch_even_after_successful_child() {
        let child = BatchChildResult {
            label: "do #first".to_string(),
            outcome: FlowOutcome::Completed,
            proof: Some("finalize_session_check".to_string()),
        };

        assert_eq!(
            agent_doc_work_graph::classify_batch_progress(
                true,
                child.outcome == FlowOutcome::Completed
            ),
            BatchProgressDecision::StopSourceChanged
        );
    }
}
