use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct FlowId(pub(crate) String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct CycleId(pub(crate) String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct DocumentId(pub(crate) String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ActorGeneration(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FlowName {
    SessionCycle,
    RoutedReopen,
    Closeout,
    DocumentMutation,
    OperatorClear,
    OrchestrationBatch,
}

impl FlowName {
    pub(crate) const fn as_str(self) -> &'static str {
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
pub(crate) enum FlowStage {
    Preflight,
    Plan,
    Execute,
    PreWriteGuard,
    PatchbackParse,
    DocumentMutation,
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
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Plan => "plan",
            Self::Execute => "execute",
            Self::PreWriteGuard => "pre_write_guard",
            Self::PatchbackParse => "patchback_parse",
            Self::DocumentMutation => "document_mutation",
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
pub(crate) enum PromptOwnership {
    User,
    Assistant,
    System,
    Structural,
}

impl PromptOwnership {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Structural => "structural",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum DocumentMutationKind {
    ExchangePatch,
    BacklogOp,
    IceboxPatch,
    FullContent,
    Repair,
}

impl DocumentMutationKind {
    pub(crate) const fn as_str(self) -> &'static str {
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
pub(crate) enum PatchbackShape {
    ValidPatch,
    PlainResponse,
    MalformedPatch,
    TranscriptDump,
    MixedOutput,
}

impl PatchbackShape {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ValidPatch => "valid_patch",
            Self::PlainResponse => "plain_response",
            Self::MalformedPatch => "malformed_patch",
            Self::TranscriptDump => "transcript_dump",
            Self::MixedOutput => "mixed_output",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum RouteDecision {
    ReuseReady,
    WaitForReady,
    FreshRestart,
    StartNew,
    FailClosed,
}

impl RouteDecision {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ReuseReady => "reuse_ready",
            Self::WaitForReady => "wait_for_ready",
            Self::FreshRestart => "fresh_restart",
            Self::StartNew => "start_new",
            Self::FailClosed => "fail_closed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum DispatchProof {
    AcceptedOnly,
    DispatchStarted,
    Consumed,
    TimedOut,
    Rejected,
}

impl DispatchProof {
    pub(crate) const fn as_str(self) -> &'static str {
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
pub(crate) enum CloseoutState {
    PreflightStarted,
    ResponseCaptured,
    WriteApplied,
    Committed,
    Abandoned,
}

impl CloseoutState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::PreflightStarted => "preflight_started",
            Self::ResponseCaptured => "response_captured",
            Self::WriteApplied => "write_applied",
            Self::Committed => "committed",
            Self::Abandoned => "abandoned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FlowOutcome {
    Completed,
    Blocked,
    FailedClosed,
    ExternallyBlocked,
}

impl FlowOutcome {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::FailedClosed => "failed_closed",
            Self::ExternallyBlocked => "externally_blocked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlowEvent {
    pub(crate) flow: FlowName,
    pub(crate) stage: FlowStage,
    pub(crate) outcome: FlowOutcome,
    pub(crate) reason: Option<String>,
}

impl FlowEvent {
    pub(crate) fn new(flow: FlowName, stage: FlowStage, outcome: FlowOutcome) -> Self {
        Self {
            flow,
            stage,
            outcome,
            reason: None,
        }
    }

    pub(crate) fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}
