//! Pure queue-item lifecycle lattice shared by CRDT convergence (`agent-doc-core`)
//! and the orchestration-layer queue state machine
//! (`agent_doc_orchestration::queue_item_state_machine`, `#queuestatemachine1`).
//!
//! ## Why this lives in core (`#queuestatemachine2` / `#qheadresidue`)
//!
//! The orchestration `queue_item_state_machine` models a queue head's lifecycle as
//! a total order whose transition is the **join** (`max`) over lifecycle rank, so a
//! stale re-emit "cannot resurrect the item as a fresh queue head." That guarantee
//! only held in isolation: the actual CRDT queue convergence
//! ([`crate::crdt::merge_by_component`]) bypassed it and ran a blind Yrs leaf merge
//! per item, which let a stale un-strike from the persisted `.yrs` base win and
//! resurrect a struck head every cycle (the live churn).
//!
//! `agent-doc-core` is the lower crate (orchestration depends on it, not the
//! reverse), so the pure lattice the convergence needs to consult lives here. The
//! orchestration `queue_item_state_machine` re-exports [`QueueItemLifecycle`] and
//! keeps its richer event-driven `QueueItemState` machine on top of it, so there is
//! a single authoritative lifecycle model rather than two diverging copies.
//!
//! ## The structural guarantee, applied to convergence
//!
//! A queue item's *visible* lifecycle state is recoverable from its rendered text:
//! a struck (`~~…~~`) item is answered-and-retired; an un-struck item is live. The
//! lattice orders `Live < Struck`. Reconciling a matched item across the three
//! merge sides is the **join** of their visible states: if any authoritative side
//! says `Struck`, a stale `Live` contribution is a no-op and the item stays struck.
//! Resurrection becomes structurally impossible instead of patched after the fact.

use serde::{Deserialize, Serialize};

/// Visible lifecycle state of a single rendered queue item, in ascending lawful
/// order. This is the coarse lattice the CRDT convergence joins over; the
/// orchestration `QueueItemState` is a finer-grained refinement that maps onto it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemLifecycle {
    /// The item is a live (un-struck) queue head — still actionable.
    Live,
    /// The item is struck through (`~~…~~`) — answered and visibly retired. A
    /// stale live re-emit must never regress past this.
    Struck,
}

impl QueueItemLifecycle {
    /// Position in the total lifecycle order (`Live` = 0, `Struck` = 1). The join
    /// compares ranks.
    pub fn rank(self) -> u8 {
        match self {
            QueueItemLifecycle::Live => 0,
            QueueItemLifecycle::Struck => 1,
        }
    }

    /// The join (`max`) of two lifecycle states — the lawful reconciliation of two
    /// sides' visible states. Monotonic: a `Live` re-emit can never regress a
    /// `Struck` item. Mirrors `transition_queue_item`'s rank join in the
    /// orchestration state machine.
    pub fn join(self, other: QueueItemLifecycle) -> QueueItemLifecycle {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    /// Classify a rendered queue item's first line. A markdown strike (`~~`)
    /// anywhere on the identity line marks the item answered-and-retired.
    ///
    /// Keyed off the first line only (continuation lines are body, not the head
    /// identity) so a struck head with an unstruck continuation still classifies
    /// as `Struck`.
    pub fn classify(item: &str) -> QueueItemLifecycle {
        let first = item.lines().next().unwrap_or("");
        if first.contains("~~") {
            QueueItemLifecycle::Struck
        } else {
            QueueItemLifecycle::Live
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_orders_live_before_struck() {
        assert!(QueueItemLifecycle::Live.rank() < QueueItemLifecycle::Struck.rank());
        assert!(QueueItemLifecycle::Live < QueueItemLifecycle::Struck);
    }

    #[test]
    fn join_is_monotonic_max() {
        use QueueItemLifecycle::*;
        assert_eq!(Live.join(Live), Live);
        assert_eq!(Live.join(Struck), Struck);
        assert_eq!(Struck.join(Live), Struck, "struck never regresses to live");
        assert_eq!(Struck.join(Struck), Struck);
    }

    #[test]
    fn join_is_idempotent_and_commutative() {
        use QueueItemLifecycle::*;
        for &a in &[Live, Struck] {
            assert_eq!(a.join(a), a, "idempotent");
            for &b in &[Live, Struck] {
                assert_eq!(a.join(b), b.join(a), "commutative");
            }
        }
    }

    #[test]
    fn classify_detects_strike_on_head_line() {
        assert_eq!(
            QueueItemLifecycle::classify("- do [#abcd] fix"),
            QueueItemLifecycle::Live
        );
        assert_eq!(
            QueueItemLifecycle::classify("- ~~do [#abcd] fix~~"),
            QueueItemLifecycle::Struck
        );
        // Strike on the head, plain continuation → still struck.
        assert_eq!(
            QueueItemLifecycle::classify("- ~~do [#abcd] fix~~\n  note line"),
            QueueItemLifecycle::Struck
        );
        // Continuation-only strike does not retire the head.
        assert_eq!(
            QueueItemLifecycle::classify("- do [#abcd] fix\n  ~~old note~~"),
            QueueItemLifecycle::Live
        );
    }
}
