//! Pure workflow decision kernel for cross-cutting agent-doc policy.
//!
//! This module is mirror-mode: callers still gather evidence and perform I/O in
//! the existing route, queue, closeout, and editor-write paths. The policy here
//! is deliberately side-effect free so those paths can converge on a single
//! evidence -> decision -> mutation -> proof boundary.

pub mod autofix;
pub mod capture;
pub mod doctor;
pub mod doctor_json;
pub mod invariants;
pub mod orchestrate_tasks;
pub mod owner_pane_self_invocation;
pub mod pending_capture;
pub mod preflight_policy;
pub mod session_check;
pub mod session_cycle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorkflowEvidenceKind {
    StaleSupervisor,
    QueueDrainability,
    CapturedResponse,
    LiveBuffer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorkflowProof {
    ActorGenerationObserved,
    CaptureRecord,
    ClearCooldownAbsent,
    ClearCooldownPresent,
    DispatchDedupAbsent,
    DispatchDedupPresent,
    DrainOwnerLeaseAbsent,
    EditorIdle,
    EditorTypingActive,
    IdleDebounce,
    LiveBufferHash,
    LiveBufferProvenance,
    PromptIdle,
    QueueDrainabilityComputed,
    QueueHeadIdentity,
    ResponseBodyHash,
    SupersessionProof,
    SupervisorStalenessProbe,
    TurnBoundary,
    TurnInactive,
    VisibleResponseMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowMutation {
    None,
    AppendResponse,
    ApplyEditorWrite,
    ArmSupervisorRecycleDebounce,
    CompleteCapturedResponse,
    InjectQueueDrainTrigger,
    QueuePromptBehindCloseout,
    RecordStaleSupervisorDiagnostic,
    RequestEditorSave,
    RetainQueueHead,
    RetireCapture,
    ReexecSupervisor,
    ReplayCapturedResponse,
    RetryOnCurrentGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorWorkflowDecision {
    Noop,
    SurfaceStale,
    RecycleNow,
    RecycleAfterDebounce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueDrainWorkflowDecision {
    Noop,
    DeferClearCooldown,
    DeferPaneBusy,
    DeferTurnActive,
    DeferSelfDrivingLoopOwner,
    DeferAlreadyDispatched,
    DispatchHead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturedResponseWorkflowDecision {
    Noop,
    AppendResponse,
    CompleteCapturedResponse,
    ReplayCapturedResponse,
    RetireStaleCapture,
    QueuePromptBehindCloseout,
    RetryOnCurrentGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveBufferWorkflowDecision {
    ApplyEditorWrite,
    RequestEditorSave,
    DeferActiveTyping,
    BlockUnattributedDrift,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowDecision {
    Supervisor(SupervisorWorkflowDecision),
    QueueDrain(QueueDrainWorkflowDecision),
    CapturedResponse(CapturedResponseWorkflowDecision),
    LiveBuffer(LiveBufferWorkflowDecision),
}

impl WorkflowDecision {
    pub const fn evidence_kind(self) -> WorkflowEvidenceKind {
        match self {
            Self::Supervisor(_) => WorkflowEvidenceKind::StaleSupervisor,
            Self::QueueDrain(_) => WorkflowEvidenceKind::QueueDrainability,
            Self::CapturedResponse(_) => WorkflowEvidenceKind::CapturedResponse,
            Self::LiveBuffer(_) => WorkflowEvidenceKind::LiveBuffer,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowTransition {
    pub evidence_kind: WorkflowEvidenceKind,
    pub decision: WorkflowDecision,
    pub allowed_mutation: WorkflowMutation,
    pub required_proof: Vec<WorkflowProof>,
}

impl WorkflowTransition {
    fn new(
        decision: WorkflowDecision,
        allowed_mutation: WorkflowMutation,
        required_proof: &[WorkflowProof],
    ) -> Self {
        Self {
            evidence_kind: decision.evidence_kind(),
            decision,
            allowed_mutation,
            required_proof: required_proof.to_vec(),
        }
    }

    pub fn permits(&self, mutation: WorkflowMutation) -> bool {
        self.allowed_mutation == mutation
    }

    pub fn requires(&self, proof: WorkflowProof) -> bool {
        self.required_proof.contains(&proof)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StaleSupervisorEvidence {
    pub stale: bool,
    pub auto_recycle: bool,
    pub turn_boundary: bool,
    pub queue_head_pending: bool,
}

pub fn decide_stale_supervisor(evidence: StaleSupervisorEvidence) -> WorkflowTransition {
    use SupervisorWorkflowDecision::*;
    use WorkflowDecision::Supervisor;
    use WorkflowMutation::*;
    use WorkflowProof::*;

    if !evidence.stale || !evidence.turn_boundary {
        return WorkflowTransition::new(Supervisor(Noop), None, &[]);
    }
    if !evidence.auto_recycle {
        return WorkflowTransition::new(
            Supervisor(SurfaceStale),
            RecordStaleSupervisorDiagnostic,
            &[SupervisorStalenessProbe, TurnBoundary],
        );
    }
    if evidence.queue_head_pending {
        WorkflowTransition::new(
            Supervisor(RecycleNow),
            ReexecSupervisor,
            &[SupervisorStalenessProbe, TurnBoundary, QueueHeadIdentity],
        )
    } else {
        WorkflowTransition::new(
            Supervisor(RecycleAfterDebounce),
            ArmSupervisorRecycleDebounce,
            &[SupervisorStalenessProbe, TurnBoundary, IdleDebounce],
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QueueDrainabilityEvidence {
    pub queue_active: bool,
    pub drainable_head_count: usize,
    pub active_head_present: bool,
    pub clear_cooldown_active: bool,
    pub prompt_visible: bool,
    pub turn_active: bool,
    pub self_driving_loop_active: bool,
    pub already_dispatched_head: bool,
}

pub fn decide_queue_drainability(evidence: QueueDrainabilityEvidence) -> WorkflowTransition {
    use QueueDrainWorkflowDecision::*;
    use WorkflowDecision::QueueDrain;
    use WorkflowMutation::*;
    use WorkflowProof::*;

    if !evidence.queue_active || evidence.drainable_head_count == 0 || !evidence.active_head_present
    {
        return WorkflowTransition::new(QueueDrain(Noop), None, &[QueueDrainabilityComputed]);
    }
    if evidence.clear_cooldown_active {
        return WorkflowTransition::new(
            QueueDrain(DeferClearCooldown),
            RetainQueueHead,
            &[QueueDrainabilityComputed, ClearCooldownPresent],
        );
    }
    if !evidence.prompt_visible {
        return WorkflowTransition::new(
            QueueDrain(DeferPaneBusy),
            RetainQueueHead,
            &[QueueDrainabilityComputed],
        );
    }
    if evidence.turn_active {
        return WorkflowTransition::new(
            QueueDrain(DeferTurnActive),
            RetainQueueHead,
            &[QueueDrainabilityComputed, PromptIdle],
        );
    }
    if evidence.self_driving_loop_active {
        return WorkflowTransition::new(
            QueueDrain(DeferSelfDrivingLoopOwner),
            RetainQueueHead,
            &[
                QueueDrainabilityComputed,
                PromptIdle,
                TurnInactive,
                ClearCooldownAbsent,
            ],
        );
    }
    if evidence.already_dispatched_head {
        return WorkflowTransition::new(
            QueueDrain(DeferAlreadyDispatched),
            RetainQueueHead,
            &[
                QueueDrainabilityComputed,
                QueueHeadIdentity,
                DispatchDedupPresent,
            ],
        );
    }
    WorkflowTransition::new(
        QueueDrain(DispatchHead),
        InjectQueueDrainTrigger,
        &[
            QueueDrainabilityComputed,
            QueueHeadIdentity,
            PromptIdle,
            TurnInactive,
            ClearCooldownAbsent,
            DrainOwnerLeaseAbsent,
            DispatchDedupAbsent,
        ],
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturedResponseState {
    NoCapture,
    Captured,
    WriteApplied,
    Replayed,
    Committed,
    Discarded,
}

impl CapturedResponseState {
    pub const fn unresolved(self) -> bool {
        matches!(self, Self::Captured | Self::WriteApplied | Self::Replayed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturedResponseEvidence {
    pub state: CapturedResponseState,
    pub visible_response_matches_capture: bool,
    pub stale_supersession_proof: bool,
    pub current_generation_matches: bool,
    pub prompt_context_available: bool,
}

impl Default for CapturedResponseEvidence {
    fn default() -> Self {
        Self {
            state: CapturedResponseState::NoCapture,
            visible_response_matches_capture: false,
            stale_supersession_proof: false,
            current_generation_matches: true,
            prompt_context_available: false,
        }
    }
}

pub fn decide_captured_response(evidence: CapturedResponseEvidence) -> WorkflowTransition {
    use CapturedResponseState::*;
    use CapturedResponseWorkflowDecision::*;
    use WorkflowDecision::CapturedResponse;
    use WorkflowProof::*;

    if !evidence.current_generation_matches {
        return WorkflowTransition::new(
            CapturedResponse(CapturedResponseWorkflowDecision::RetryOnCurrentGeneration),
            WorkflowMutation::RetryOnCurrentGeneration,
            &[ActorGenerationObserved],
        );
    }
    if evidence.prompt_context_available && evidence.state.unresolved() {
        return WorkflowTransition::new(
            CapturedResponse(CapturedResponseWorkflowDecision::QueuePromptBehindCloseout),
            WorkflowMutation::QueuePromptBehindCloseout,
            &[CaptureRecord],
        );
    }
    match evidence.state {
        NoCapture => WorkflowTransition::new(
            CapturedResponse(CapturedResponseWorkflowDecision::AppendResponse),
            WorkflowMutation::AppendResponse,
            &[ResponseBodyHash],
        ),
        Committed | Discarded => {
            WorkflowTransition::new(CapturedResponse(Noop), WorkflowMutation::None, &[])
        }
        Captured | WriteApplied | Replayed if evidence.visible_response_matches_capture => {
            WorkflowTransition::new(
                CapturedResponse(CapturedResponseWorkflowDecision::CompleteCapturedResponse),
                WorkflowMutation::CompleteCapturedResponse,
                &[CaptureRecord, VisibleResponseMatch],
            )
        }
        Captured | WriteApplied | Replayed if evidence.stale_supersession_proof => {
            WorkflowTransition::new(
                CapturedResponse(RetireStaleCapture),
                WorkflowMutation::RetireCapture,
                &[CaptureRecord, SupersessionProof],
            )
        }
        Captured | WriteApplied | Replayed => WorkflowTransition::new(
            CapturedResponse(CapturedResponseWorkflowDecision::ReplayCapturedResponse),
            WorkflowMutation::ReplayCapturedResponse,
            &[CaptureRecord, ResponseBodyHash],
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveBufferState {
    Absent,
    Fresh,
    Diverged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveBufferEvidence {
    pub state: LiveBufferState,
    pub active_typing: bool,
    pub drift_attributed_to_live_buffer: bool,
}

impl Default for LiveBufferEvidence {
    fn default() -> Self {
        Self {
            state: LiveBufferState::Absent,
            active_typing: false,
            drift_attributed_to_live_buffer: false,
        }
    }
}

pub fn decide_live_buffer(evidence: LiveBufferEvidence) -> WorkflowTransition {
    use LiveBufferState::*;
    use LiveBufferWorkflowDecision::*;
    use WorkflowDecision::LiveBuffer;
    use WorkflowProof::*;

    if evidence.active_typing {
        return WorkflowTransition::new(
            LiveBuffer(DeferActiveTyping),
            WorkflowMutation::None,
            &[EditorTypingActive],
        );
    }
    match evidence.state {
        Absent => WorkflowTransition::new(
            LiveBuffer(LiveBufferWorkflowDecision::ApplyEditorWrite),
            WorkflowMutation::ApplyEditorWrite,
            &[EditorIdle],
        ),
        Fresh => WorkflowTransition::new(
            LiveBuffer(LiveBufferWorkflowDecision::ApplyEditorWrite),
            WorkflowMutation::ApplyEditorWrite,
            &[EditorIdle, LiveBufferHash],
        ),
        Diverged if evidence.drift_attributed_to_live_buffer => WorkflowTransition::new(
            LiveBuffer(LiveBufferWorkflowDecision::RequestEditorSave),
            WorkflowMutation::RequestEditorSave,
            &[LiveBufferHash, LiveBufferProvenance],
        ),
        Diverged => WorkflowTransition::new(
            LiveBuffer(BlockUnattributedDrift),
            WorkflowMutation::None,
            &[LiveBufferHash],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_supervisor_policy_names_mutation_and_proof() {
        let detect = decide_stale_supervisor(StaleSupervisorEvidence {
            stale: true,
            auto_recycle: false,
            turn_boundary: true,
            queue_head_pending: true,
        });
        assert_eq!(
            detect.decision,
            WorkflowDecision::Supervisor(SupervisorWorkflowDecision::SurfaceStale)
        );
        assert!(detect.permits(WorkflowMutation::RecordStaleSupervisorDiagnostic));
        assert!(detect.requires(WorkflowProof::SupervisorStalenessProbe));
        assert!(detect.requires(WorkflowProof::TurnBoundary));

        let immediate = decide_stale_supervisor(StaleSupervisorEvidence {
            stale: true,
            auto_recycle: true,
            turn_boundary: true,
            queue_head_pending: true,
        });
        assert_eq!(
            immediate.decision,
            WorkflowDecision::Supervisor(SupervisorWorkflowDecision::RecycleNow)
        );
        assert!(immediate.permits(WorkflowMutation::ReexecSupervisor));
        assert!(immediate.requires(WorkflowProof::QueueHeadIdentity));

        let debounced = decide_stale_supervisor(StaleSupervisorEvidence {
            stale: true,
            auto_recycle: true,
            turn_boundary: true,
            queue_head_pending: false,
        });
        assert_eq!(
            debounced.decision,
            WorkflowDecision::Supervisor(SupervisorWorkflowDecision::RecycleAfterDebounce)
        );
        assert!(debounced.permits(WorkflowMutation::ArmSupervisorRecycleDebounce));
        assert!(debounced.requires(WorkflowProof::IdleDebounce));
    }

    #[test]
    fn queue_drainability_policy_dispatches_only_with_identity_idle_and_dedup_proof() {
        let dispatch = decide_queue_drainability(QueueDrainabilityEvidence {
            queue_active: true,
            drainable_head_count: 1,
            active_head_present: true,
            prompt_visible: true,
            ..QueueDrainabilityEvidence::default()
        });
        assert_eq!(
            dispatch.decision,
            WorkflowDecision::QueueDrain(QueueDrainWorkflowDecision::DispatchHead)
        );
        assert!(dispatch.permits(WorkflowMutation::InjectQueueDrainTrigger));
        assert!(dispatch.requires(WorkflowProof::QueueDrainabilityComputed));
        assert!(dispatch.requires(WorkflowProof::QueueHeadIdentity));
        assert!(dispatch.requires(WorkflowProof::PromptIdle));
        assert!(dispatch.requires(WorkflowProof::DispatchDedupAbsent));

        let loop_owner = decide_queue_drainability(QueueDrainabilityEvidence {
            queue_active: true,
            drainable_head_count: 1,
            active_head_present: true,
            prompt_visible: true,
            self_driving_loop_active: true,
            ..QueueDrainabilityEvidence::default()
        });
        assert_eq!(
            loop_owner.decision,
            WorkflowDecision::QueueDrain(QueueDrainWorkflowDecision::DeferSelfDrivingLoopOwner)
        );
        assert!(loop_owner.permits(WorkflowMutation::RetainQueueHead));

        let deduped = decide_queue_drainability(QueueDrainabilityEvidence {
            queue_active: true,
            drainable_head_count: 1,
            active_head_present: true,
            prompt_visible: true,
            already_dispatched_head: true,
            ..QueueDrainabilityEvidence::default()
        });
        assert_eq!(
            deduped.decision,
            WorkflowDecision::QueueDrain(QueueDrainWorkflowDecision::DeferAlreadyDispatched)
        );
        assert!(deduped.requires(WorkflowProof::DispatchDedupPresent));
    }

    #[test]
    fn captured_response_policy_separates_append_replay_retire_and_retry() {
        let append = decide_captured_response(CapturedResponseEvidence::default());
        assert_eq!(
            append.decision,
            WorkflowDecision::CapturedResponse(CapturedResponseWorkflowDecision::AppendResponse)
        );
        assert!(append.permits(WorkflowMutation::AppendResponse));
        assert!(append.requires(WorkflowProof::ResponseBodyHash));

        let complete = decide_captured_response(CapturedResponseEvidence {
            state: CapturedResponseState::WriteApplied,
            visible_response_matches_capture: true,
            ..CapturedResponseEvidence::default()
        });
        assert_eq!(
            complete.decision,
            WorkflowDecision::CapturedResponse(
                CapturedResponseWorkflowDecision::CompleteCapturedResponse
            )
        );
        assert!(complete.permits(WorkflowMutation::CompleteCapturedResponse));
        assert!(complete.requires(WorkflowProof::VisibleResponseMatch));

        let retire = decide_captured_response(CapturedResponseEvidence {
            state: CapturedResponseState::Captured,
            stale_supersession_proof: true,
            ..CapturedResponseEvidence::default()
        });
        assert_eq!(
            retire.decision,
            WorkflowDecision::CapturedResponse(
                CapturedResponseWorkflowDecision::RetireStaleCapture
            )
        );
        assert!(retire.permits(WorkflowMutation::RetireCapture));
        assert!(retire.requires(WorkflowProof::SupersessionProof));

        let retry = decide_captured_response(CapturedResponseEvidence {
            current_generation_matches: false,
            ..CapturedResponseEvidence::default()
        });
        assert_eq!(
            retry.decision,
            WorkflowDecision::CapturedResponse(
                CapturedResponseWorkflowDecision::RetryOnCurrentGeneration
            )
        );
        assert!(retry.permits(WorkflowMutation::RetryOnCurrentGeneration));
        assert!(retry.requires(WorkflowProof::ActorGenerationObserved));
    }

    #[test]
    fn live_buffer_policy_applies_defers_requests_save_or_blocks() {
        let fresh = decide_live_buffer(LiveBufferEvidence {
            state: LiveBufferState::Fresh,
            ..LiveBufferEvidence::default()
        });
        assert_eq!(
            fresh.decision,
            WorkflowDecision::LiveBuffer(LiveBufferWorkflowDecision::ApplyEditorWrite)
        );
        assert!(fresh.permits(WorkflowMutation::ApplyEditorWrite));
        assert!(fresh.requires(WorkflowProof::EditorIdle));
        assert!(fresh.requires(WorkflowProof::LiveBufferHash));

        let typing = decide_live_buffer(LiveBufferEvidence {
            active_typing: true,
            ..LiveBufferEvidence::default()
        });
        assert_eq!(
            typing.decision,
            WorkflowDecision::LiveBuffer(LiveBufferWorkflowDecision::DeferActiveTyping)
        );
        assert!(typing.permits(WorkflowMutation::None));
        assert!(typing.requires(WorkflowProof::EditorTypingActive));

        let save = decide_live_buffer(LiveBufferEvidence {
            state: LiveBufferState::Diverged,
            drift_attributed_to_live_buffer: true,
            ..LiveBufferEvidence::default()
        });
        assert_eq!(
            save.decision,
            WorkflowDecision::LiveBuffer(LiveBufferWorkflowDecision::RequestEditorSave)
        );
        assert!(save.permits(WorkflowMutation::RequestEditorSave));
        assert!(save.requires(WorkflowProof::LiveBufferProvenance));

        let blocked = decide_live_buffer(LiveBufferEvidence {
            state: LiveBufferState::Diverged,
            drift_attributed_to_live_buffer: false,
            ..LiveBufferEvidence::default()
        });
        assert_eq!(
            blocked.decision,
            WorkflowDecision::LiveBuffer(LiveBufferWorkflowDecision::BlockUnattributedDrift)
        );
        assert!(blocked.permits(WorkflowMutation::None));
        assert!(blocked.requires(WorkflowProof::LiveBufferHash));
    }

    #[test]
    fn workflow_kernel_covers_required_decision_families() {
        let transitions = [
            decide_stale_supervisor(StaleSupervisorEvidence {
                stale: true,
                auto_recycle: true,
                turn_boundary: true,
                queue_head_pending: true,
            }),
            decide_queue_drainability(QueueDrainabilityEvidence {
                queue_active: true,
                drainable_head_count: 1,
                active_head_present: true,
                prompt_visible: true,
                ..QueueDrainabilityEvidence::default()
            }),
            decide_captured_response(CapturedResponseEvidence {
                state: CapturedResponseState::Captured,
                stale_supersession_proof: true,
                ..CapturedResponseEvidence::default()
            }),
            decide_live_buffer(LiveBufferEvidence {
                state: LiveBufferState::Diverged,
                drift_attributed_to_live_buffer: true,
                ..LiveBufferEvidence::default()
            }),
        ];
        assert_eq!(
            transitions
                .iter()
                .map(|transition| transition.evidence_kind)
                .collect::<Vec<_>>(),
            vec![
                WorkflowEvidenceKind::StaleSupervisor,
                WorkflowEvidenceKind::QueueDrainability,
                WorkflowEvidenceKind::CapturedResponse,
                WorkflowEvidenceKind::LiveBuffer,
            ]
        );
        assert!(
            transitions
                .iter()
                .all(|transition| transition.allowed_mutation != WorkflowMutation::None)
        );
        assert!(
            transitions
                .iter()
                .all(|transition| !transition.required_proof.is_empty())
        );
    }

    #[test]
    fn architecture_primitive_transition_matrix_names_mutation_and_terminal_proofs() {
        let cases = [
            (
                "stale supervisor recycle",
                decide_stale_supervisor(StaleSupervisorEvidence {
                    stale: true,
                    auto_recycle: true,
                    turn_boundary: true,
                    queue_head_pending: true,
                }),
                WorkflowDecision::Supervisor(SupervisorWorkflowDecision::RecycleNow),
                WorkflowMutation::ReexecSupervisor,
                &[
                    WorkflowProof::SupervisorStalenessProbe,
                    WorkflowProof::TurnBoundary,
                    WorkflowProof::QueueHeadIdentity,
                ][..],
            ),
            (
                "operator-only no-drainable queue",
                decide_queue_drainability(QueueDrainabilityEvidence {
                    queue_active: true,
                    drainable_head_count: 0,
                    active_head_present: true,
                    ..QueueDrainabilityEvidence::default()
                }),
                WorkflowDecision::QueueDrain(QueueDrainWorkflowDecision::Noop),
                WorkflowMutation::None,
                &[WorkflowProof::QueueDrainabilityComputed][..],
            ),
            (
                "queue edit during unresolved closeout",
                decide_captured_response(CapturedResponseEvidence {
                    state: CapturedResponseState::Captured,
                    prompt_context_available: true,
                    ..CapturedResponseEvidence::default()
                }),
                WorkflowDecision::CapturedResponse(
                    CapturedResponseWorkflowDecision::QueuePromptBehindCloseout,
                ),
                WorkflowMutation::QueuePromptBehindCloseout,
                &[WorkflowProof::CaptureRecord][..],
            ),
            (
                "degraded/file-IPC materialized response fallback",
                decide_captured_response(CapturedResponseEvidence {
                    state: CapturedResponseState::WriteApplied,
                    visible_response_matches_capture: true,
                    ..CapturedResponseEvidence::default()
                }),
                WorkflowDecision::CapturedResponse(
                    CapturedResponseWorkflowDecision::CompleteCapturedResponse,
                ),
                WorkflowMutation::CompleteCapturedResponse,
                &[
                    WorkflowProof::CaptureRecord,
                    WorkflowProof::VisibleResponseMatch,
                ][..],
            ),
            (
                "compact/replay captured response",
                decide_captured_response(CapturedResponseEvidence {
                    state: CapturedResponseState::Replayed,
                    ..CapturedResponseEvidence::default()
                }),
                WorkflowDecision::CapturedResponse(
                    CapturedResponseWorkflowDecision::ReplayCapturedResponse,
                ),
                WorkflowMutation::ReplayCapturedResponse,
                &[
                    WorkflowProof::CaptureRecord,
                    WorkflowProof::ResponseBodyHash,
                ][..],
            ),
            (
                "attributed live-buffer drift",
                decide_live_buffer(LiveBufferEvidence {
                    state: LiveBufferState::Diverged,
                    drift_attributed_to_live_buffer: true,
                    ..LiveBufferEvidence::default()
                }),
                WorkflowDecision::LiveBuffer(LiveBufferWorkflowDecision::RequestEditorSave),
                WorkflowMutation::RequestEditorSave,
                &[
                    WorkflowProof::LiveBufferHash,
                    WorkflowProof::LiveBufferProvenance,
                ][..],
            ),
            (
                "unattributed live-buffer drift",
                decide_live_buffer(LiveBufferEvidence {
                    state: LiveBufferState::Diverged,
                    drift_attributed_to_live_buffer: false,
                    ..LiveBufferEvidence::default()
                }),
                WorkflowDecision::LiveBuffer(LiveBufferWorkflowDecision::BlockUnattributedDrift),
                WorkflowMutation::None,
                &[WorkflowProof::LiveBufferHash][..],
            ),
        ];

        for (name, transition, decision, mutation, proofs) in cases {
            assert_eq!(transition.decision, decision, "{name}");
            assert_eq!(transition.allowed_mutation, mutation, "{name}");
            for proof in proofs {
                assert!(transition.requires(*proof), "{name} missing {proof:?}");
            }
            assert_eq!(
                transition.required_proof.len(),
                proofs.len(),
                "{name} must not gain or lose hidden proof obligations"
            );
        }
    }
}
