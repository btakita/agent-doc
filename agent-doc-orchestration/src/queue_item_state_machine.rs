//! Per-item queue lifecycle state machine (`#queuestatemachine1`, head `#vwj7`).
//!
//! The recurring duplicate-injection bugs — multiline phantom-pin flood
//! (`#rt83qflood`), bare-id mirror dups (`#qdedupsync`), pin-variant dups
//! (`#qdup-bare-id` / `#pushpinaccum`), operator-line re-emit (`#qauthorder`) —
//! are all the same structural gap: queue convergence is a *pile of ad-hoc dedup
//! passes* with no single authoritative model of what is in the queue and why
//! each item is there. Each new duplication shape needs a new dedup pass.
//!
//! This module is Phase 1 of the fix: a pure, exhaustively-tested model of a
//! single queue item's identity and lifecycle. Convergence becomes "drive each
//! identity to its lawful state," **not** "append then dedup."
//!
//! ## The structural guarantee
//!
//! The lifecycle is a **total order**
//! (`Authored < Mirrored < InProgress < Answered < Struck < Reaped`) and the
//! transition function is the **join** (`max`) of the current state and the
//! state an event drives toward. That makes two properties hold by construction:
//!
//! - **Monotonic** — an item never moves backward. A stale-CRDT / supervisor
//!   re-emit of an `OperatorAuthored` / `BacklogMirrored` event for an identity
//!   that is already `InProgress` / `Answered` / `Struck` is a **no-op**: it
//!   cannot resurrect the item as a fresh queue head, so a visible duplicate is
//!   *structurally impossible* rather than patched after the fact.
//! - **Idempotent** — applying the same event any number of times equals
//!   applying it once. A re-emit storm converges to one item per identity
//!   (the `#queuestatemachine3` / `#brtc` SimWorld goal).
//!
//! Phase 2 (`#queuestatemachine2` / `#cgfx`) routes the real convergence passes
//! through this SM, reducing the ad-hoc dedups to transition guards. This phase
//! defines and proves the model only; it is not yet wired into convergence.
//!
//! Mirrors the pure-transition convention of [`crate::merge_control_state_machine`]
//! and [`crate::cycle_state_machine`]: the durable queue document remains the
//! crash-recovery log; this module is the pure transition authority.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

/// The coarse visible lifecycle lattice the CRDT convergence joins over
/// (`#queuestatemachine2`/`#qheadresidue`). Lives in `agent-doc-core` because the
/// convergence ([`agent_doc_core::crdt::merge_by_component`]) — the lower crate —
/// must consult it; re-exported here so this module remains the single
/// authoritative lifecycle surface. The finer-grained [`QueueItemState`] below
/// refines it: see [`QueueItemState::lifecycle`].
pub use agent_doc_core::queue_item_lifecycle::QueueItemLifecycle;

/// Durable identity of a queue head.
///
/// Two heads share an identity iff they refer to the same work, regardless of
/// cosmetic pin / prefix variation (`:pushpin: do [#abcd]`, `do [#abcd]`, and
/// `[#abcd]` are one identity). This is the dedup key convergence must drive to
/// a single lawful state.
///
/// - [`Id`](QueueItemIdentity::Id) — an id-backed head (`[#id]` or bare `#id`),
///   keyed by the durable id via [`crate::queue_continuation::extract_head_id`]
///   so every pin/prefix variant collapses.
/// - [`FreeText`](QueueItemIdentity::FreeText) — an operator free-text line with
///   no id, keyed by its whitespace-collapsed, lowercased text (the same key the
///   free-text dedup uses) so a verbatim re-emit collapses.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "key")]
pub enum QueueItemIdentity {
    Id(String),
    FreeText(String),
}

impl QueueItemIdentity {
    /// Resolve the durable identity of a queue head prompt. Id-backed heads key
    /// on the extracted id; everything else keys on normalized free text.
    pub fn from_prompt(prompt: &str) -> Self {
        match crate::queue_continuation::extract_head_id(prompt) {
            Some(id) => QueueItemIdentity::Id(id),
            None => {
                QueueItemIdentity::FreeText(crate::queue::normalize_multiline_dedup_text(prompt))
            }
        }
    }

    /// `true` for an id-backed identity.
    pub fn is_id_backed(&self) -> bool {
        matches!(self, QueueItemIdentity::Id(_))
    }
}

/// Lifecycle state of a single queue item, in ascending lawful order.
///
/// Declared in lifecycle order so the derived [`Ord`] is the lifecycle order;
/// [`QueueItemState::rank`] exposes the same order as a stable index for the
/// transition join and the property tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemState {
    /// The operator authored the item (in `agent:backlog` or directly in the
    /// queue). Position-lock (`#queue-operator-pin-position-lock`) is a property
    /// preserved from this genesis state across every later transition.
    Authored,
    /// The backlog→queue mirror surfaced the item as a queue head.
    Mirrored,
    /// The in-session loop / supervisor took this head as the active drain item
    /// (the `🚧` in-progress marker).
    InProgress,
    /// The exchange carries a `Queue-prompt` echo or a `### Re:` block answering
    /// this identity.
    Answered,
    /// The head is struck through (`~~...~~`) — answered and visibly retired.
    Struck,
    /// The item has been removed from the rendered queue. Terminal and
    /// absorbing: a reaped identity re-emitted stays reaped.
    Reaped,
}

impl QueueItemState {
    /// Position of the state in the total lifecycle order (`Authored` = 0 …
    /// `Reaped` = 5). The transition join compares ranks.
    pub fn rank(self) -> u8 {
        match self {
            QueueItemState::Authored => 0,
            QueueItemState::Mirrored => 1,
            QueueItemState::InProgress => 2,
            QueueItemState::Answered => 3,
            QueueItemState::Struck => 4,
            QueueItemState::Reaped => 5,
        }
    }

    /// Terminal states accept no advancing transition.
    pub fn is_terminal(self) -> bool {
        matches!(self, QueueItemState::Reaped)
    }

    /// Project this fine-grained lifecycle state onto the coarse
    /// [`QueueItemLifecycle`] lattice the CRDT convergence joins over
    /// (`#queuestatemachine2`). Everything strictly below `Struck` renders as a
    /// live head; `Struck`/`Reaped` render as the retired (`Struck`) lattice state.
    /// This is the contract that keeps the two models from diverging: the
    /// convergence's per-item join over rendered text agrees with this machine's
    /// rank join over events.
    pub fn lifecycle(self) -> QueueItemLifecycle {
        match self {
            QueueItemState::Authored
            | QueueItemState::Mirrored
            | QueueItemState::InProgress
            | QueueItemState::Answered => QueueItemLifecycle::Live,
            QueueItemState::Struck | QueueItemState::Reaped => QueueItemLifecycle::Struck,
        }
    }
}

/// Events driving a queue item's lifecycle.
///
/// Each event names the lawful state it drives the item *toward* via
/// [`QueueItemEvent::target`]; the transition is the join of the current state
/// and that target, so a backward (stale re-emit) event is a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueItemEvent {
    /// The operator authored / re-authored this identity. Genesis; idempotent.
    OperatorAuthored,
    /// The backlog→queue mirror emitted (or re-emitted) this identity.
    BacklogMirrored,
    /// The drain loop picked this head as the active in-progress item.
    DrainStarted,
    /// The exchange gained a `Queue-prompt` echo / `### Re:` for this identity.
    ExchangeAnswered,
    /// The head was struck through.
    StruckThrough,
    /// The item was removed from the rendered queue.
    Reaped,
}

impl QueueItemEvent {
    /// The lifecycle state this event drives the item toward.
    pub fn target(self) -> QueueItemState {
        match self {
            QueueItemEvent::OperatorAuthored => QueueItemState::Authored,
            QueueItemEvent::BacklogMirrored => QueueItemState::Mirrored,
            QueueItemEvent::DrainStarted => QueueItemState::InProgress,
            QueueItemEvent::ExchangeAnswered => QueueItemState::Answered,
            QueueItemEvent::StruckThrough => QueueItemState::Struck,
            QueueItemEvent::Reaped => QueueItemState::Reaped,
        }
    }
}

/// Pure transition: the **join** (`max`) of the current state and the state the
/// event drives toward. Always `Some` — the lifecycle is a total order with no
/// rejected transitions — so a re-emit can only *advance* or *hold*, never
/// regress. Holding (`target <= current`) is the structural anti-duplication
/// no-op: a stale re-injection of an already-advanced identity keeps its current
/// state instead of resurrecting it as a fresh head.
///
/// Returns `Option` to plug into [`ThreadSafeStateMachine`]; the `None` arm is
/// reserved for a future guarded variant and is never produced here.
pub fn transition_queue_item(
    current: &QueueItemState,
    event: &QueueItemEvent,
) -> Option<QueueItemState> {
    let target = event.target();
    Some(if target.rank() > current.rank() {
        target
    } else {
        *current
    })
}

/// Thread-safe lifecycle machine for one queue identity. The identity itself is
/// the dedup key the owner keeps; this machine tracks the lawful state.
pub struct QueueItemMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<QueueItemState, QueueItemEvent>,
}

impl QueueItemMachine {
    /// New machine for a freshly-seen identity (defaults to `Authored`).
    pub fn new(initial: QueueItemState) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_queue_item);
        Self { ctx, machine }
    }

    /// Drive an event. Returns `true` when the event was accepted (always true
    /// for this total-order lattice; the state is unchanged for a hold).
    pub fn send(&self, event: QueueItemEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    /// Current lawful state.
    pub fn state(&self) -> QueueItemState {
        self.machine.state(&self.ctx)
    }

    /// One-shot pure transition, mirroring
    /// [`crate::merge_control_state_machine::MergeOwnershipMachine::transition`].
    pub fn transition(initial: QueueItemState, event: QueueItemEvent) -> Option<QueueItemState> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [QueueItemState; 6] = [
        QueueItemState::Authored,
        QueueItemState::Mirrored,
        QueueItemState::InProgress,
        QueueItemState::Answered,
        QueueItemState::Struck,
        QueueItemState::Reaped,
    ];

    const ALL_EVENTS: [QueueItemEvent; 6] = [
        QueueItemEvent::OperatorAuthored,
        QueueItemEvent::BacklogMirrored,
        QueueItemEvent::DrainStarted,
        QueueItemEvent::ExchangeAnswered,
        QueueItemEvent::StruckThrough,
        QueueItemEvent::Reaped,
    ];

    fn t(state: QueueItemState, event: QueueItemEvent) -> QueueItemState {
        transition_queue_item(&state, &event).expect("transition is total")
    }

    /// Rank order matches the declared lifecycle order.
    #[test]
    fn rank_is_strictly_ascending_lifecycle_order() {
        for pair in ALL_STATES.windows(2) {
            assert!(
                pair[0].rank() < pair[1].rank(),
                "{:?} should rank before {:?}",
                pair[0],
                pair[1]
            );
            // Derived Ord must agree with rank().
            assert!(pair[0] < pair[1]);
        }
    }

    /// Exhaustive (state × event): every transition is total and never regresses.
    #[test]
    fn transition_is_total_and_monotonic() {
        for &state in &ALL_STATES {
            for &event in &ALL_EVENTS {
                let next = t(state, event);
                assert!(
                    next.rank() >= state.rank(),
                    "{:?} + {:?} regressed to {:?}",
                    state,
                    event,
                    next
                );
            }
        }
    }

    /// Exhaustive (state × event): a forward event advances exactly to its
    /// target; a backward/equal event holds the current state. This is the
    /// anti-duplication invariant — a stale re-emit of an already-advanced
    /// identity can never resurrect it.
    #[test]
    fn transition_is_join_of_current_and_target() {
        for &state in &ALL_STATES {
            for &event in &ALL_EVENTS {
                let next = t(state, event);
                if event.target().rank() > state.rank() {
                    assert_eq!(next, event.target(), "{:?} + {:?}", state, event);
                } else {
                    assert_eq!(next, state, "re-emit {:?} + {:?} must hold", state, event);
                }
            }
        }
    }

    /// Exhaustive (state × event): applying any event twice equals applying it
    /// once. A re-emit storm converges to one stable state per identity.
    #[test]
    fn transition_is_idempotent() {
        for &state in &ALL_STATES {
            for &event in &ALL_EVENTS {
                let once = t(state, event);
                let twice = t(once, event);
                assert_eq!(once, twice, "{:?} + {:?} not idempotent", state, event);
            }
        }
    }

    /// Reaped is absorbing: every event from Reaped holds Reaped.
    #[test]
    fn reaped_is_absorbing() {
        for &event in &ALL_EVENTS {
            assert_eq!(t(QueueItemState::Reaped, event), QueueItemState::Reaped);
        }
        assert!(QueueItemState::Reaped.is_terminal());
    }

    /// The canonical happy-path lifecycle drives genesis → reaped in order.
    #[test]
    fn canonical_lifecycle_path() {
        let machine = QueueItemMachine::new(QueueItemState::Authored);
        assert_eq!(machine.state(), QueueItemState::Authored);

        assert!(machine.send(QueueItemEvent::BacklogMirrored));
        assert_eq!(machine.state(), QueueItemState::Mirrored);
        assert!(machine.send(QueueItemEvent::DrainStarted));
        assert_eq!(machine.state(), QueueItemState::InProgress);
        assert!(machine.send(QueueItemEvent::ExchangeAnswered));
        assert_eq!(machine.state(), QueueItemState::Answered);
        assert!(machine.send(QueueItemEvent::StruckThrough));
        assert_eq!(machine.state(), QueueItemState::Struck);
        assert!(machine.send(QueueItemEvent::Reaped));
        assert_eq!(machine.state(), QueueItemState::Reaped);
    }

    /// A stale-CRDT / supervisor re-emit storm: mirror + author events keep
    /// arriving after the item is already in progress / answered. The item must
    /// stay put — this is the duplicate that convergence used to patch.
    #[test]
    fn stale_reemit_storm_never_resurrects_an_advanced_item() {
        let machine = QueueItemMachine::new(QueueItemState::Authored);
        machine.send(QueueItemEvent::BacklogMirrored);
        machine.send(QueueItemEvent::DrainStarted);
        machine.send(QueueItemEvent::ExchangeAnswered);
        assert_eq!(machine.state(), QueueItemState::Answered);

        // Re-emit storm: authored / mirrored events replayed by stale CRDT.
        for _ in 0..10 {
            machine.send(QueueItemEvent::OperatorAuthored);
            machine.send(QueueItemEvent::BacklogMirrored);
        }
        assert_eq!(
            machine.state(),
            QueueItemState::Answered,
            "stale re-emit must not regress an answered item"
        );
    }

    /// `#queuestatemachine2` single-model contract: this fine-grained machine's
    /// rank join must agree with the coarse [`QueueItemLifecycle`] join the CRDT
    /// convergence uses. For every pair of states, projecting then joining on the
    /// lattice equals joining the states then projecting — so the convergence can
    /// never disagree with the state machine about whether a head is retired.
    #[test]
    fn lifecycle_projection_commutes_with_join() {
        for &a in &ALL_STATES {
            for &b in &ALL_STATES {
                // Join the fine states (max by rank), then project.
                let joined_state = if a.rank() >= b.rank() { a } else { b };
                let project_then_join = a.lifecycle().join(b.lifecycle());
                assert_eq!(
                    joined_state.lifecycle(),
                    project_then_join,
                    "projection must commute with join for {a:?} / {b:?}"
                );
            }
        }
    }

    /// A `Struck` (answered-retired) item projects to the retired lattice state; a
    /// stale `Authored`/`Mirrored` re-emit joins to it as a no-op — the same
    /// anti-resurrection guarantee the convergence enforces over rendered text.
    #[test]
    fn struck_lifecycle_absorbs_stale_live_reemit() {
        assert_eq!(
            QueueItemState::Struck.lifecycle(),
            QueueItemLifecycle::Struck
        );
        assert_eq!(
            QueueItemState::Struck
                .lifecycle()
                .join(QueueItemState::Authored.lifecycle()),
            QueueItemLifecycle::Struck,
            "a stale authored re-emit must not un-retire a struck head"
        );
        assert_eq!(
            QueueItemState::Reaped.lifecycle(),
            QueueItemLifecycle::Struck
        );
    }

    /// Identity collapses pin / prefix variants of the same id-backed head.
    #[test]
    fn identity_collapses_pin_and_prefix_variants() {
        let bracketed = QueueItemIdentity::from_prompt(":pushpin: do [#gww8] thing");
        let bare_prefix = QueueItemIdentity::from_prompt("do [#gww8] thing");
        let plain = QueueItemIdentity::from_prompt("[#gww8]");
        assert_eq!(bracketed, QueueItemIdentity::Id("gww8".into()));
        assert_eq!(bracketed, bare_prefix);
        assert_eq!(bracketed, plain);
        assert!(bracketed.is_id_backed());
    }

    /// Free-text identity keys on normalized text; a whitespace/case re-emit
    /// collapses, distinct text does not.
    #[test]
    fn free_text_identity_keys_on_normalized_text() {
        let a = QueueItemIdentity::from_prompt("Still getting JB File Cache Conflict dialogs.");
        let b = QueueItemIdentity::from_prompt("still getting   jb file cache conflict dialogs.");
        let other = QueueItemIdentity::from_prompt("Opencode reverse-orders numeric lists.");
        assert_eq!(a, b, "whitespace/case re-emit must collapse");
        assert_ne!(a, other);
        assert!(!a.is_id_backed());
    }
}
