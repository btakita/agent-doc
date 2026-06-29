use super::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};
use agent_doc_template::patchback::{self, OrchestratePatchbackDecision};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchChildResult {
    pub label: String,
    pub outcome: FlowOutcome,
    pub proof: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchProgressDecision {
    Continue,
    StopSourceChanged,
    StopChildNotCompleted,
}

impl BatchProgressDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::StopSourceChanged => "source_changed_after_child",
            Self::StopChildNotCompleted => "child_not_completed",
        }
    }
}

pub fn classify_batch_progress(
    source_changed_after_child: bool,
    child: &BatchChildResult,
) -> BatchProgressDecision {
    if source_changed_after_child {
        return BatchProgressDecision::StopSourceChanged;
    }
    if child.outcome != FlowOutcome::Completed {
        return BatchProgressDecision::StopChildNotCompleted;
    }
    BatchProgressDecision::Continue
}

pub fn batch_should_continue(source_changed_after_child: bool, child: &BatchChildResult) -> bool {
    classify_batch_progress(source_changed_after_child, child) == BatchProgressDecision::Continue
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoDagScheduleDecision {
    Ready,
    SessionReviewBlocked,
}

impl AutoDagScheduleDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::SessionReviewBlocked => "session_review_blocked",
        }
    }
}

pub fn auto_dag_schedule_event(
    decision: AutoDagScheduleDecision,
    node_count: usize,
    batch_count: usize,
) -> FlowEvent {
    let outcome = match decision {
        AutoDagScheduleDecision::Ready => FlowOutcome::Completed,
        AutoDagScheduleDecision::SessionReviewBlocked => FlowOutcome::Blocked,
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
    decision: AutoDagScheduleDecision,
    node_count: usize,
    batch_count: usize,
) {
    super::proof::log_flow_event(
        file,
        auto_dag_schedule_event(decision, node_count, batch_count),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildPatchbackNormalizationDecision {
    WrappedPlainResponse,
    KeptExplicitPatch,
    KeptRejectedPlainResponse,
    KeptUnparseable,
}

impl ChildPatchbackNormalizationDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WrappedPlainResponse => "wrapped_plain_response",
            Self::KeptExplicitPatch => "kept_explicit_patch",
            Self::KeptRejectedPlainResponse => "kept_rejected_plain_response",
            Self::KeptUnparseable => "kept_unparseable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildPatchbackNormalization {
    pub response: String,
    pub decision: ChildPatchbackNormalizationDecision,
}

pub fn normalize_child_template_response(response: String) -> ChildPatchbackNormalization {
    let Ok((patches, unmatched)) = agent_doc_template::parse_patches(&response) else {
        return ChildPatchbackNormalization {
            response,
            decision: ChildPatchbackNormalizationDecision::KeptUnparseable,
        };
    };
    if patches.is_empty() && !unmatched.trim().is_empty() {
        if patchback::classify_orchestrate_plain_response(unmatched.trim()).is_accepted() {
            return ChildPatchbackNormalization {
                response: format!(
                    "<!-- patch:exchange -->\n{}\n<!-- /patch:exchange -->\n",
                    unmatched.trim_end()
                ),
                decision: ChildPatchbackNormalizationDecision::WrappedPlainResponse,
            };
        }
        return ChildPatchbackNormalization {
            response,
            decision: ChildPatchbackNormalizationDecision::KeptRejectedPlainResponse,
        };
    }

    let decision = if matches!(
        patchback::classify_orchestrate_patchback(&patches, &unmatched),
        OrchestratePatchbackDecision::AcceptExplicitPatch
    ) {
        ChildPatchbackNormalizationDecision::KeptExplicitPatch
    } else {
        ChildPatchbackNormalizationDecision::KeptRejectedPlainResponse
    };
    ChildPatchbackNormalization { response, decision }
}

pub fn child_patchback_normalization_event(
    normalization: &ChildPatchbackNormalization,
) -> FlowEvent {
    let outcome = match normalization.decision {
        ChildPatchbackNormalizationDecision::WrappedPlainResponse
        | ChildPatchbackNormalizationDecision::KeptExplicitPatch => FlowOutcome::Completed,
        ChildPatchbackNormalizationDecision::KeptRejectedPlainResponse
        | ChildPatchbackNormalizationDecision::KeptUnparseable => FlowOutcome::FailedClosed,
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
    normalization: &ChildPatchbackNormalization,
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
            classify_batch_progress(true, &child),
            BatchProgressDecision::StopSourceChanged
        );
        assert!(!batch_should_continue(true, &child));
    }

    #[test]
    fn child_plain_response_is_wrapped_as_exchange_patch() {
        let normalized =
            normalize_child_template_response("### Re: child — gpt-5\n\nImplemented.".to_string());

        assert_eq!(
            normalized.decision,
            ChildPatchbackNormalizationDecision::WrappedPlainResponse
        );
        assert!(normalized.response.starts_with("<!-- patch:exchange -->"));
        assert!(normalized.response.contains("### Re: child"));
    }

    #[test]
    fn transcript_shaped_child_response_is_not_normalized() {
        let transcript = "❯ do #next\n### Re: malformed — gpt-5\nBody";
        let normalized = normalize_child_template_response(transcript.to_string());

        assert_eq!(
            normalized.decision,
            ChildPatchbackNormalizationDecision::KeptRejectedPlainResponse
        );
        assert_eq!(normalized.response, transcript);
    }
}
