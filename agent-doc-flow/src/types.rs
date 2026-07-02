use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FlowId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CycleId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocumentId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorGeneration(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FlowName {
    SessionCycle,
    RoutedReopen,
    Closeout,
    DocumentMutation,
    OperatorClear,
    OrchestrationBatch,
}

impl FlowName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionCycle => "session_cycle",
            Self::RoutedReopen => "routed_reopen",
            Self::Closeout => "closeout",
            Self::DocumentMutation => "document_mutation",
            Self::OperatorClear => "operator_clear",
            Self::OrchestrationBatch => "orchestration_batch",
        }
    }
}

impl fmt::Display for FlowName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FlowStage {
    Preflight,
    Plan,
    Execute,
    PreWriteGuard,
    PatchbackParse,
    DocumentMutation,
    IpcSnapshotAdoption,
    SnapshotConvergence,
    PreCommitGuard,
    Commit,
    TerminalGuard,
    SessionCheck,
    ActorBinding,
    PromptReadyBarrier,
    DispatchAuthorization,
    DispatchSubmit,
    DispatchProof,
    QueueFreeze,
    ChildCloseout,
    OperatorGuard,
}

impl FlowStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Plan => "plan",
            Self::Execute => "execute",
            Self::PreWriteGuard => "pre_write_guard",
            Self::PatchbackParse => "patchback_parse",
            Self::DocumentMutation => "document_mutation",
            Self::IpcSnapshotAdoption => "ipc_snapshot_adoption",
            Self::SnapshotConvergence => "snapshot_convergence",
            Self::PreCommitGuard => "pre_commit_guard",
            Self::Commit => "commit",
            Self::TerminalGuard => "terminal_guard",
            Self::SessionCheck => "session_check",
            Self::ActorBinding => "actor_binding",
            Self::PromptReadyBarrier => "prompt_ready_barrier",
            Self::DispatchAuthorization => "dispatch_authorization",
            Self::DispatchSubmit => "dispatch_submit",
            Self::DispatchProof => "dispatch_proof",
            Self::QueueFreeze => "queue_freeze",
            Self::ChildCloseout => "child_closeout",
            Self::OperatorGuard => "operator_guard",
        }
    }
}

impl fmt::Display for FlowStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PromptOwnership {
    User,
    Assistant,
    System,
    Structural,
}

impl PromptOwnership {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Structural => "structural",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DocumentMutationKind {
    ExchangePatch,
    BacklogOp,
    IceboxPatch,
    FullContent,
    Repair,
}

impl DocumentMutationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExchangePatch => "exchange_patch",
            Self::BacklogOp => "backlog_op",
            Self::IceboxPatch => "icebox_patch",
            Self::FullContent => "full_content",
            Self::Repair => "repair",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DispatchProof {
    AcceptedOnly,
    DispatchStarted,
    Consumed,
    TimedOut,
    Rejected,
}

impl DispatchProof {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AcceptedOnly => "accepted_only",
            Self::DispatchStarted => "dispatch_started",
            Self::Consumed => "consumed",
            Self::TimedOut => "timed_out",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FlowOutcome {
    Completed,
    Blocked,
    FailedClosed,
    ExternallyBlocked,
}

impl FlowOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::FailedClosed => "failed_closed",
            Self::ExternallyBlocked => "externally_blocked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowEvent {
    pub flow: FlowName,
    pub stage: FlowStage,
    pub outcome: FlowOutcome,
    pub reason: Option<String>,
}

impl FlowEvent {
    pub fn new(flow: FlowName, stage: FlowStage, outcome: FlowOutcome) -> Self {
        Self {
            flow,
            stage,
            outcome,
            reason: None,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCycleStep {
    pub stage: FlowStage,
    pub outcome: FlowOutcome,
    pub reason: Option<String>,
}

impl SessionCycleStep {
    pub fn completed(stage: FlowStage) -> Self {
        Self {
            stage,
            outcome: FlowOutcome::Completed,
            reason: None,
        }
    }
}

pub fn flow_event(
    flow: FlowName,
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: impl Into<Option<String>>,
) -> FlowEvent {
    let mut event = FlowEvent::new(flow, stage, outcome);
    if let Some(reason) = reason.into() {
        event = event.with_reason(reason);
    }
    event
}

pub fn session_cycle_event(
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: impl Into<Option<String>>,
) -> FlowEvent {
    flow_event(FlowName::SessionCycle, stage, outcome, reason)
}

pub fn flow_event_log_message(file: &str, event: &FlowEvent) -> String {
    let mut message = format!(
        "flow_event file={} flow={} stage={} outcome={}",
        file,
        event.flow.as_str(),
        event.stage.as_str(),
        event.outcome.as_str()
    );
    if let Some(reason) = &event.reason {
        message.push_str(" reason=");
        message.push_str(&sanitize_field_value(reason));
    }
    message
}

fn sanitize_field_value(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_event_log_message_is_field_parseable() {
        let event = FlowEvent::new(
            FlowName::RoutedReopen,
            FlowStage::PromptReadyBarrier,
            FlowOutcome::FailedClosed,
        )
        .with_reason("starting actor not ready");

        let message = flow_event_log_message("tasks/a.md", &event);

        assert!(message.contains("flow=routed_reopen"));
        assert!(message.contains("stage=prompt_ready_barrier"));
        assert!(message.contains("outcome=failed_closed"));
        assert!(message.contains("reason=starting_actor_not_ready"));
    }

    #[test]
    fn flow_event_sets_optional_reason() {
        let event = flow_event(
            FlowName::SessionCycle,
            FlowStage::Plan,
            FlowOutcome::Completed,
            Some("normal".to_string()),
        );

        assert_eq!(event.flow, FlowName::SessionCycle);
        assert_eq!(event.stage, FlowStage::Plan);
        assert_eq!(event.outcome, FlowOutcome::Completed);
        assert_eq!(event.reason.as_deref(), Some("normal"));
    }

    #[test]
    fn session_cycle_event_adapts_flow_name_stage_outcome_and_reason() {
        let event = session_cycle_event(
            FlowStage::Preflight,
            FlowOutcome::Blocked,
            Some("waiting".to_string()),
        );

        assert_eq!(event.flow, FlowName::SessionCycle);
        assert_eq!(event.stage, FlowStage::Preflight);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(event.reason.as_deref(), Some("waiting"));
        assert_eq!(
            SessionCycleStep::completed(FlowStage::Plan).outcome,
            FlowOutcome::Completed
        );
    }
}
