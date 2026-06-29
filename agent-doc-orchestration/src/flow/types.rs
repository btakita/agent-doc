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
pub enum PatchbackShape {
    ValidPatch,
    PlainResponse,
    MalformedPatch,
    TranscriptDump,
    MixedOutput,
    /// Raw template form: stdin carries literal `<!-- agent:NAME -->` /
    /// `<!-- /agent:NAME -->` component blocks instead of supported
    /// `<!-- patch:* -->` patch blocks. Committing these as plain text escapes
    /// the markers into the live exchange (`#closeout-repair-churn`), so this
    /// shape must fail closed before commit.
    EscapedComponentMarkers,
}

impl PatchbackShape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ValidPatch => "valid_patch",
            Self::PlainResponse => "plain_response",
            Self::MalformedPatch => "malformed_patch",
            Self::TranscriptDump => "transcript_dump",
            Self::MixedOutput => "mixed_output",
            Self::EscapedComponentMarkers => "escaped_component_markers",
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
pub enum CloseoutState {
    PreflightStarted,
    ResponseCaptured,
    WriteApplied,
    Committed,
    Abandoned,
}

impl CloseoutState {
    pub const fn as_str(self) -> &'static str {
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
