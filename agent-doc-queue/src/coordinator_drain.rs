//! Document-wide coordinator drain policy (`#coordinatordrain`).
//!
//! A coordinator is allowed to yield only when every live queue item is either
//! owned by a running worker, explicitly operator-gated, or terminal.  The
//! policy deliberately consumes a snapshot of *every* `agent:run` component;
//! selecting one active queue is an execution concern and must never hide work
//! at a terminal boundary.
//! Background execution keeps the main agent responsive; it does not transfer
//! operator conversation, steering, integration, or document reconciliation
//! ownership away from the coordinator.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};

/// One queue obligation, with its component provenance when available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorQueueItem {
    pub item: String,
    pub queue_id: Option<String>,
    pub owner: Option<String>,
    pub worker_id: Option<String>,
}

impl CoordinatorQueueItem {
    pub fn pending(item: impl Into<String>, queue_id: Option<impl Into<String>>) -> Self {
        Self {
            item: item.into(),
            queue_id: queue_id.map(Into::into),
            owner: None,
            worker_id: None,
        }
    }
}

/// Authoritative document observation supplied by the I/O adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoordinatorDrainSnapshot {
    /// Eligible, unclaimed rows. The coordinator must claim, spawn, then start
    /// every row in this set before it may yield.
    pub pending_dispatch: Vec<CoordinatorQueueItem>,
    /// Claims that have not yet been bound to a concrete harness worker.
    pub claimed_unstarted: Vec<CoordinatorQueueItem>,
    /// Claims with positive worker-binding evidence.
    pub running: Vec<CoordinatorQueueItem>,
    /// Worker reports awaiting integration and queue closeout.
    pub reported: Vec<CoordinatorQueueItem>,
    /// Lost attempts requiring recovery or a new fenced dispatch generation.
    pub lost: Vec<CoordinatorQueueItem>,
    /// Live rows that explicitly require operator action.
    pub operator_gated_count: usize,
}

/// Exhaustive coordinator boundary state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorDrainPhase {
    /// Claim/spawn/start, recovery, or report integration is still required in
    /// the coordinator turn. A Stop boundary must fail closed.
    ActionRequired,
    /// All remaining agent-owned rows have concrete running workers.
    WaitingForWorkers,
    /// Only explicit operator gates remain.
    OperatorBlocked,
    /// No live queue work remains.
    Terminal,
}

/// State-machine output retained as one value so every boundary observes the
/// same full work set instead of independently choosing a singular head.
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

impl CoordinatorDrainDecision {
    /// A coordinator may yield, but may report document-wide completion only
    /// for [`CoordinatorDrainPhase::Terminal`].
    pub fn may_yield(&self) -> bool {
        self.phase != CoordinatorDrainPhase::ActionRequired
    }

    pub fn is_terminal(&self) -> bool {
        self.phase == CoordinatorDrainPhase::Terminal
    }

    pub fn required_action_count(&self) -> usize {
        self.snapshot.pending_dispatch.len()
            + self.snapshot.claimed_unstarted.len()
            + self.snapshot.reported.len()
            + self.snapshot.lost.len()
    }
}

/// Pure exhaustive transition for a new document observation.
pub fn observe_coordinator_drain(
    _current: &CoordinatorDrainDecision,
    snapshot: &CoordinatorDrainSnapshot,
) -> Option<CoordinatorDrainDecision> {
    let action_required = !snapshot.pending_dispatch.is_empty()
        || !snapshot.claimed_unstarted.is_empty()
        || !snapshot.reported.is_empty()
        || !snapshot.lost.is_empty();
    let phase = if action_required {
        CoordinatorDrainPhase::ActionRequired
    } else if !snapshot.running.is_empty() {
        CoordinatorDrainPhase::WaitingForWorkers
    } else if snapshot.operator_gated_count > 0 {
        CoordinatorDrainPhase::OperatorBlocked
    } else {
        CoordinatorDrainPhase::Terminal
    };
    Some(CoordinatorDrainDecision {
        phase,
        snapshot: snapshot.clone(),
    })
}

/// Lazily-backed state machine joined to a document scope. The document text,
/// claim ledger, and worker receipts are sources; this is their computed policy
/// boundary. One-shot adapters create a bounded document scope per authoritative
/// read, while the controller can retain the same machine across invalidations.
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

    fn item(text: &str, queue: &str) -> CoordinatorQueueItem {
        CoordinatorQueueItem::pending(text, Some(queue))
    }

    #[test]
    fn running_head_does_not_hide_other_pending_heads() {
        let scope = agent_doc_state_scope::DocumentScope::new();
        let machine = CoordinatorDrainMachine::new_in(&scope);
        let decision = machine.observe(CoordinatorDrainSnapshot {
            pending_dispatch: vec![item("do [#b]", "other"), item("do [#c]", "inactive")],
            running: vec![CoordinatorQueueItem {
                item: "do [#a]".into(),
                queue_id: Some("active".into()),
                owner: Some("subagent:a".into()),
                worker_id: Some("worker-a".into()),
            }],
            ..Default::default()
        });

        assert_eq!(decision.phase, CoordinatorDrainPhase::ActionRequired);
        assert_eq!(decision.required_action_count(), 2);
        assert!(!decision.may_yield());
        assert_eq!(
            decision
                .snapshot
                .pending_dispatch
                .iter()
                .filter_map(|item| item.queue_id.as_deref())
                .collect::<Vec<_>>(),
            vec!["other", "inactive"]
        );
    }

    #[test]
    fn claimed_lost_and_reported_attempts_all_fail_closed() {
        for snapshot in [
            CoordinatorDrainSnapshot {
                claimed_unstarted: vec![item("do [#claim]", "run")],
                ..Default::default()
            },
            CoordinatorDrainSnapshot {
                lost: vec![item("do [#lost]", "run")],
                ..Default::default()
            },
            CoordinatorDrainSnapshot {
                reported: vec![item("do [#report]", "run")],
                ..Default::default()
            },
        ] {
            let scope = agent_doc_state_scope::DocumentScope::new();
            let decision = CoordinatorDrainMachine::new_in(&scope).observe(snapshot);
            assert_eq!(decision.phase, CoordinatorDrainPhase::ActionRequired);
            assert!(!decision.may_yield());
        }
    }

    #[test]
    fn only_running_or_operator_gated_work_may_yield_without_claiming_completion() {
        let scope = agent_doc_state_scope::DocumentScope::new();
        let running = CoordinatorDrainMachine::new_in(&scope).observe(CoordinatorDrainSnapshot {
            running: vec![item("do [#a]", "run")],
            ..Default::default()
        });
        assert_eq!(running.phase, CoordinatorDrainPhase::WaitingForWorkers);
        assert!(running.may_yield());
        assert!(!running.is_terminal());

        let scope = agent_doc_state_scope::DocumentScope::new();
        let blocked = CoordinatorDrainMachine::new_in(&scope).observe(CoordinatorDrainSnapshot {
            operator_gated_count: 1,
            ..Default::default()
        });
        assert_eq!(blocked.phase, CoordinatorDrainPhase::OperatorBlocked);
        assert!(blocked.may_yield());
        assert!(!blocked.is_terminal());
    }
}
