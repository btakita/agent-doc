//! Document-wide coordinator drain policy (`#coordinatordrain`).
//!
//! A coordinator is allowed to yield only when every live queue item is either
//! waiting on a running/blocked attempt, explicitly operator-gated, or
//! terminal. The state machine consumes the same project-wide, post-repair
//! [`QueueSyncPayload`] preflight emits: selecting one active queue is an
//! execution concern and must never hide work at a terminal boundary.
//! Background execution keeps the main agent responsive; it does not transfer
//! operator conversation, steering, integration, or document reconciliation
//! ownership away from the coordinator.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};

use crate::queue_sync_payload::{QueueSyncOperation, QueueSyncPayload};

/// Authoritative project queue observation supplied by preflight or a terminal
/// gate. It includes the current document, inactive runs, related session
/// documents, and fail-closed scan errors.
pub type CoordinatorDrainSnapshot = QueueSyncPayload;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorDrainPhase {
    /// Dispatch/start, inline execution, steering delivery, recovery, report
    /// integration, or reconciliation is still required. Stop fails closed.
    ActionRequired,
    /// All remaining non-operator rows are explicitly waiting.
    WaitingForWorkers,
    /// Only explicit operator gates remain.
    OperatorBlocked,
    /// No live queue work and no scan debt remain.
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorDrainDecision {
    pub phase: CoordinatorDrainPhase,
    pub snapshot: CoordinatorDrainSnapshot,
}

impl Default for CoordinatorDrainDecision {
    fn default() -> Self {
        Self {
            phase: CoordinatorDrainPhase::Terminal,
            snapshot: CoordinatorDrainSnapshot::default(),
        }
    }
}

fn operation_requires_action(operation: QueueSyncOperation) -> bool {
    matches!(
        operation,
        QueueSyncOperation::Dispatch
            | QueueSyncOperation::InlineExecuteThenReconcile
            | QueueSyncOperation::ForwardSteering
            | QueueSyncOperation::IntegrateClose
            | QueueSyncOperation::RecoverRequeue
            | QueueSyncOperation::Reconcile
    )
}

impl CoordinatorDrainDecision {
    /// A coordinator may yield, but may report project-wide completion only
    /// for [`CoordinatorDrainPhase::Terminal`].
    pub fn may_yield(&self) -> bool {
        self.phase != CoordinatorDrainPhase::ActionRequired
    }

    pub fn is_terminal(&self) -> bool {
        self.phase == CoordinatorDrainPhase::Terminal
    }

    pub fn action_items(&self) -> impl Iterator<Item = &crate::queue_sync_payload::QueueSyncItem> {
        self.snapshot
            .items()
            .filter(|item| operation_requires_action(item.operation))
    }

    pub fn required_action_count(&self) -> usize {
        self.snapshot.scan_errors.len()
            + self.snapshot.placement_reconcile.len()
            + self.action_items().count()
    }
}

pub fn observe_coordinator_drain(
    _current: &CoordinatorDrainDecision,
    snapshot: &CoordinatorDrainSnapshot,
) -> Option<CoordinatorDrainDecision> {
    let action_required = !snapshot.scan_errors.is_empty()
        || !snapshot.placement_reconcile.is_empty()
        || snapshot
            .items()
            .any(|item| operation_requires_action(item.operation));
    let waiting = snapshot
        .items()
        .any(|item| item.operation == QueueSyncOperation::Wait);
    let operator_blocked = snapshot
        .items()
        .any(|item| item.operation == QueueSyncOperation::OperatorGate);
    let phase = if action_required {
        CoordinatorDrainPhase::ActionRequired
    } else if waiting {
        CoordinatorDrainPhase::WaitingForWorkers
    } else if operator_blocked {
        CoordinatorDrainPhase::OperatorBlocked
    } else {
        CoordinatorDrainPhase::Terminal
    };
    Some(CoordinatorDrainDecision {
        phase,
        snapshot: snapshot.clone(),
    })
}

/// Lazily-backed state machine joined to a document scope. Document text,
/// claim ledger, worker receipts, and related-document discovery are sources;
/// this is their computed policy boundary.
pub struct CoordinatorDrainMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<CoordinatorDrainDecision, CoordinatorDrainSnapshot>,
}

impl CoordinatorDrainMachine {
    pub fn new_in(scope: &agent_doc_state_scope::DocumentScope) -> Self {
        let ctx = scope.ctx().clone();
        let machine = ThreadSafeStateMachine::new(
            &ctx,
            CoordinatorDrainDecision::default(),
            observe_coordinator_drain,
        );
        Self { ctx, machine }
    }

    pub fn observe(&self, snapshot: CoordinatorDrainSnapshot) -> CoordinatorDrainDecision {
        self.machine.send(&self.ctx, snapshot);
        self.machine.state(&self.ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_sync_payload::{
        QueueCoordinatorRole, QueueExecutionMode, QueueExecutorRole, QueueSyncDocumentScope,
        QueueSyncExecutionEvidence, QueueSyncItem, QueueSyncLifecycle, QueueSyncPlacementDebt,
        QueueSyncResponseEvidence, QueueSyncScanError,
    };

    fn item(
        text: &str,
        queue: &str,
        lifecycle: QueueSyncLifecycle,
        operation: QueueSyncOperation,
        execution_mode: QueueExecutionMode,
    ) -> QueueSyncItem {
        let (coordinator_role, executor_role) = match execution_mode {
            QueueExecutionMode::SubagentDispatch => (
                QueueCoordinatorRole::DispatchAndCoordinate,
                QueueExecutorRole::Subagent,
            ),
            QueueExecutionMode::MainInline => (
                QueueCoordinatorRole::ExecuteAndCoordinate,
                QueueExecutorRole::MainAgent,
            ),
        };
        QueueSyncItem {
            document_path: "tasks/bugs.md".into(),
            document_scope: QueueSyncDocumentScope::Current,
            queue_id: queue.into(),
            component_occurrence: 0,
            item: text.into(),
            lifecycle,
            operation,
            execution_mode,
            coordinator_role,
            executor_role,
            incomplete_since_previous_turn: true,
            owner: None,
            worker_id: None,
            generation: None,
            execution_evidence: QueueSyncExecutionEvidence::Unknown,
            completion_outbox_recovered: false,
            completion_response_receipt: false,
            response_id: None,
            response_capture_sha256: None,
            response_evidence: QueueSyncResponseEvidence::NotApplicable,
            recovered_response: None,
            missing_response_debt: false,
            retry_safe: false,
            recovered_effects: Vec::new(),
        }
    }

    #[test]
    fn running_head_does_not_hide_other_pending_heads() {
        let scope = agent_doc_state_scope::DocumentScope::new();
        let machine = CoordinatorDrainMachine::new_in(&scope);
        let mut running = item(
            "do [#a]",
            "active",
            QueueSyncLifecycle::Running,
            QueueSyncOperation::Wait,
            QueueExecutionMode::SubagentDispatch,
        );
        running.owner = Some("subagent:a".into());
        running.worker_id = Some("worker-a".into());
        let decision = machine.observe(CoordinatorDrainSnapshot {
            pending: vec![
                item(
                    "do [#b]",
                    "other",
                    QueueSyncLifecycle::Pending,
                    QueueSyncOperation::Dispatch,
                    QueueExecutionMode::SubagentDispatch,
                ),
                item(
                    "do [#c]",
                    "inactive",
                    QueueSyncLifecycle::Pending,
                    QueueSyncOperation::Dispatch,
                    QueueExecutionMode::SubagentDispatch,
                ),
            ],
            running: vec![running],
            ..Default::default()
        });

        assert_eq!(decision.phase, CoordinatorDrainPhase::ActionRequired);
        assert_eq!(decision.required_action_count(), 2);
        assert!(!decision.may_yield());
        assert_eq!(
            decision
                .snapshot
                .pending
                .iter()
                .map(|item| item.queue_id.as_str())
                .collect::<Vec<_>>(),
            vec!["other", "inactive"]
        );
    }

    #[test]
    fn every_cleanup_operation_and_scan_error_fails_closed() {
        for (lifecycle, operation) in [
            (
                QueueSyncLifecycle::ClaimedUnstarted,
                QueueSyncOperation::Dispatch,
            ),
            (
                QueueSyncLifecycle::ReportedCheckmark,
                QueueSyncOperation::IntegrateClose,
            ),
            (
                QueueSyncLifecycle::LostReported,
                QueueSyncOperation::IntegrateClose,
            ),
            (QueueSyncLifecycle::Lost, QueueSyncOperation::RecoverRequeue),
            (
                QueueSyncLifecycle::ReconcileUnknown,
                QueueSyncOperation::Reconcile,
            ),
            (
                QueueSyncLifecycle::Running,
                QueueSyncOperation::ForwardSteering,
            ),
        ] {
            let scope = agent_doc_state_scope::DocumentScope::new();
            let mut snapshot = CoordinatorDrainSnapshot::default();
            let row = item(
                "do [#debt]",
                "run",
                lifecycle,
                operation,
                QueueExecutionMode::SubagentDispatch,
            );
            match lifecycle {
                QueueSyncLifecycle::ClaimedUnstarted => snapshot.claimed_unstarted.push(row),
                QueueSyncLifecycle::ReportedCheckmark => snapshot.reported_checkmark.push(row),
                QueueSyncLifecycle::LostReported => snapshot.lost_reported.push(row),
                QueueSyncLifecycle::Lost => snapshot.lost.push(row),
                QueueSyncLifecycle::ReconcileUnknown => snapshot.reconcile_unknown.push(row),
                QueueSyncLifecycle::Running => snapshot.running.push(row),
                _ => unreachable!(),
            }
            let decision = CoordinatorDrainMachine::new_in(&scope).observe(snapshot);
            assert_eq!(decision.phase, CoordinatorDrainPhase::ActionRequired);
            assert!(!decision.may_yield());
        }

        let scope = agent_doc_state_scope::DocumentScope::new();
        let decision = CoordinatorDrainMachine::new_in(&scope).observe(CoordinatorDrainSnapshot {
            scan_errors: vec![QueueSyncScanError {
                document_path: "tasks/unreadable.md".into(),
                detail: "read failed".into(),
                operation: QueueSyncOperation::Reconcile,
            }],
            ..Default::default()
        });
        assert_eq!(decision.phase, CoordinatorDrainPhase::ActionRequired);

        let scope = agent_doc_state_scope::DocumentScope::new();
        let decision = CoordinatorDrainMachine::new_in(&scope).observe(CoordinatorDrainSnapshot {
            placement_reconcile: vec![QueueSyncPlacementDebt {
                document_path: "tasks/bugs.md".into(),
                document_scope: QueueSyncDocumentScope::Current,
                item_id: "unplaced".into(),
                item_text: "do [#unplaced]".into(),
                requested_queue_id: Some("missing".into()),
                eligible_queue_ids: vec!["active".into(), "inactive".into()],
                operation: QueueSyncOperation::Reconcile,
            }],
            ..Default::default()
        });
        assert_eq!(decision.phase, CoordinatorDrainPhase::ActionRequired);
        assert_eq!(decision.required_action_count(), 1);
    }

    #[test]
    fn inline_debt_fails_closed_without_requesting_a_worker() {
        let scope = agent_doc_state_scope::DocumentScope::new();
        let inline = item(
            "do [#inline]",
            "main",
            QueueSyncLifecycle::Pending,
            QueueSyncOperation::InlineExecuteThenReconcile,
            QueueExecutionMode::MainInline,
        );
        let decision = CoordinatorDrainMachine::new_in(&scope).observe(CoordinatorDrainSnapshot {
            pending: vec![inline.clone()],
            ..Default::default()
        });
        assert_eq!(inline.executor_role, QueueExecutorRole::MainAgent);
        assert_eq!(decision.phase, CoordinatorDrainPhase::ActionRequired);
        assert_eq!(decision.required_action_count(), 1);
    }

    #[test]
    fn only_wait_or_operator_gated_work_may_yield_without_claiming_completion() {
        let scope = agent_doc_state_scope::DocumentScope::new();
        let running = CoordinatorDrainMachine::new_in(&scope).observe(CoordinatorDrainSnapshot {
            running: vec![item(
                "do [#a]",
                "run",
                QueueSyncLifecycle::Running,
                QueueSyncOperation::Wait,
                QueueExecutionMode::SubagentDispatch,
            )],
            ..Default::default()
        });
        assert_eq!(running.phase, CoordinatorDrainPhase::WaitingForWorkers);
        assert!(running.may_yield());
        assert!(!running.is_terminal());

        let scope = agent_doc_state_scope::DocumentScope::new();
        let blocked = CoordinatorDrainMachine::new_in(&scope).observe(CoordinatorDrainSnapshot {
            operator_gated: vec![item(
                "approve [#a]",
                "run",
                QueueSyncLifecycle::OperatorGated,
                QueueSyncOperation::OperatorGate,
                QueueExecutionMode::MainInline,
            )],
            ..Default::default()
        });
        assert_eq!(blocked.phase, CoordinatorDrainPhase::OperatorBlocked);
        assert!(blocked.may_yield());
        assert!(!blocked.is_terminal());
    }
}
