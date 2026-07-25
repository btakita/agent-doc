//! Queue element descriptor and local realtime model.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};
use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "queue",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::DerivedProjection,
    write_policy: ElementWritePolicy::ProjectionOnly,
    scheduling_role: ElementSchedulingRole::ActiveQueue,
    realtime_model: ElementRealtimeModel::Queue,
    composition_role: ElementCompositionRole::Consumer,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

/// Visible lifecycle state of a single rendered queue item, in ascending lawful
/// order. This is the coarse lattice the CRDT convergence joins over; richer
/// orchestration queue state machines refine this state but must map back to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemLifecycle {
    /// The item is a live, un-struck queue head and is still actionable.
    Live,
    /// The item is struck through (`~~...~~`) and visibly retired. A stale live
    /// re-emit must never regress past this.
    Struck,
}

impl QueueItemLifecycle {
    /// Position in the total lifecycle order (`Live` = 0, `Struck` = 1).
    pub fn rank(self) -> u8 {
        match self {
            QueueItemLifecycle::Live => 0,
            QueueItemLifecycle::Struck => 1,
        }
    }

    /// The join (`max`) of two lifecycle states. Monotonic: a `Live` re-emit can
    /// never regress a `Struck` item.
    pub fn join(self, other: QueueItemLifecycle) -> QueueItemLifecycle {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    /// Classify a rendered queue item's first line. A markdown strike (`~~`)
    /// anywhere on the identity line marks the item answered-and-retired.
    pub fn classify(item: &str) -> QueueItemLifecycle {
        let first = item.lines().next().unwrap_or("");
        if first.contains("~~") {
            QueueItemLifecycle::Struck
        } else {
            QueueItemLifecycle::Live
        }
    }
}

/// Durable identity of a queue head.
///
/// Two heads share an identity iff they refer to the same work, regardless of
/// cosmetic pin / prefix variation (`:pushpin: do [#abcd]`, `do [#abcd]`, and
/// `[#abcd]` are one identity). Free-text heads key on whitespace-collapsed,
/// lowercased text so a verbatim re-emit collapses.
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
        match extract_head_id(prompt) {
            Some(id) => QueueItemIdentity::Id(id),
            // Strip lifecycle/pin markers before keying free text so a
            // marker-decorated re-emit (`🚧 <text>`, `📌 <text>`) keys to the
            // same identity as the operator's bare `<text>` line. Without this
            // the convergence pass treats them as distinct free-text heads and
            // cannot collapse a resurrected copy against the bare original
            // (`#qdedup-directive-twin` free-text variant).
            None => QueueItemIdentity::FreeText(normalize_multiline_dedup_text(
                &strip_priority_markers(prompt),
            )),
        }
    }

    /// `true` for an id-backed identity.
    pub fn is_id_backed(&self) -> bool {
        matches!(self, QueueItemIdentity::Id(_))
    }
}

/// Lifecycle state of a single queue item, in ascending lawful order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemState {
    Authored,
    Mirrored,
    InProgress,
    Answered,
    Struck,
    Reaped,
}

impl QueueItemState {
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

    pub fn is_terminal(self) -> bool {
        matches!(self, QueueItemState::Reaped)
    }

    /// Project this fine-grained lifecycle state onto the coarse rendered queue
    /// lattice the CRDT convergence joins over.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueItemEvent {
    OperatorAuthored,
    BacklogMirrored,
    DrainStarted,
    ExchangeAnswered,
    StruckThrough,
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

/// Pure transition: the join (`max`) of the current state and the state the
/// event drives toward.
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

/// Thread-safe lifecycle machine for one queue identity.
pub struct QueueItemMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<QueueItemState, QueueItemEvent>,
}

impl QueueItemMachine {
    /// Join this machine to `scope`'s graph (`#stategraphjoin`).
    ///
    /// A queue identity outlives any single turn — it is authored in one and consumed in a later one — so it belongs to the document.
    ///
    /// The scope owns the context, so dropping the scope drops this machine's cells —
    /// teardown is the scope's lifetime, not a separate deregistration step.
    pub fn new_in(scope: &agent_doc_state_scope::DocumentScope, initial: QueueItemState) -> Self {
        let ctx = scope.ctx().clone();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_queue_item);
        Self { ctx, machine }
    }

    /// Standalone machine in a private context — a pure-transition helper for unit
    /// tests only.
    ///
    /// A long-lived owner must use [`Self::new_in`] instead: nothing outside a private
    /// context can derive from its cells and invalidation never crosses it, so a
    /// `Computed` built over one is Computed in name only.
    pub fn new(initial: QueueItemState) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_queue_item);
        Self { ctx, machine }
    }

    pub fn send(&self, event: QueueItemEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> QueueItemState {
        self.machine.state(&self.ctx)
    }

    pub fn transition(initial: QueueItemState, event: QueueItemEvent) -> Option<QueueItemState> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn normalize_multiline_dedup_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn extract_head_id(prompt: &str) -> Option<String> {
    let stripped = strip_priority_markers(prompt);
    let trimmed = stripped.trim();
    let lower = trimmed.to_ascii_lowercase();
    let topic = lower.strip_prefix("do ").unwrap_or(&lower).trim();
    let raw = topic
        .strip_prefix("[#")
        .and_then(|rest| rest.split_once(']').map(|(id, _)| id))
        .or_else(|| topic.strip_prefix('#').map(id_prefix))?;
    if raw.is_empty()
        || !raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        None
    } else {
        Some(raw.to_ascii_lowercase())
    }
}

fn id_prefix(text: &str) -> &str {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')))
        .next()
        .unwrap_or("")
}

/// Strip all leading lifecycle/priority markers (`:pushpin:`/`📌`/`📍`/`🚧`/`⏭️`
/// and the `**pin**`/`**prioritized**` word forms) from a queue head so a
/// marker-decorated line keys to the same durable identity as the bare line.
/// Shared with the CRDT cell-merge free-text keyer so merge, convergence, and
/// dedup all normalize the same marker set.
pub fn strip_priority_markers(text: &str) -> String {
    let mut t = text.trim();
    loop {
        let trimmed = t.trim_start();
        let stripped = [
            "**prioritized**",
            "__prioritized__",
            "**pin**",
            "__pin__",
            ":pushpin:",
            ":pin:",
            "📌",
            "*prioritized*",
            "_prioritized_",
            "*pin*",
            "_pin_",
            ":round_pushpin:",
            "📍",
            "🚧",
            "⏭\u{fe0f}",
        ]
        .iter()
        .find_map(|marker| trimmed.strip_prefix(marker));
        match stripped {
            Some(rest) => t = rest.trim_start(),
            None => break,
        }
    }
    t.trim().to_string()
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

    #[test]
    fn rank_orders_live_before_struck() {
        assert!(QueueItemLifecycle::Live.rank() < QueueItemLifecycle::Struck.rank());
        assert!(QueueItemLifecycle::Live < QueueItemLifecycle::Struck);
    }

    #[test]
    fn free_text_identity_is_marker_invariant() {
        // A marker-decorated free-text re-emit must key to the SAME identity as
        // the operator's bare line so convergence can collapse the resurrected
        // copy (#qdedup-directive-twin free-text variant). Covers the new emoji
        // set (📌/📍/🚧/⏭️) plus shortcodes and word forms.
        let bare = QueueItemIdentity::from_prompt("fix the flaky test");
        for decorated in [
            "🚧 fix the flaky test",
            "📌 fix the flaky test",
            "📍 fix the flaky test",
            "⏭\u{fe0f} fix the flaky test",
            ":pushpin: fix the flaky test",
            "**pin** fix the flaky test",
        ] {
            assert_eq!(
                QueueItemIdentity::from_prompt(decorated),
                bare,
                "marker-decorated free text must share the bare line's identity: {decorated:?}"
            );
        }
        assert_eq!(
            bare,
            QueueItemIdentity::FreeText("fix the flaky test".to_string())
        );
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
        assert_eq!(
            QueueItemLifecycle::classify("- ~~do [#abcd] fix~~\n  note line"),
            QueueItemLifecycle::Struck
        );
        assert_eq!(
            QueueItemLifecycle::classify("- do [#abcd] fix\n  ~~old note~~"),
            QueueItemLifecycle::Live
        );
    }

    #[test]
    fn queue_item_state_rank_is_strictly_ascending_lifecycle_order() {
        for pair in ALL_STATES.windows(2) {
            assert!(pair[0].rank() < pair[1].rank());
            assert!(pair[0] < pair[1]);
        }
    }

    #[test]
    fn queue_item_transition_is_total_monotonic_and_idempotent() {
        for &state in &ALL_STATES {
            for &event in &ALL_EVENTS {
                let once = t(state, event);
                let twice = t(once, event);
                assert!(once.rank() >= state.rank());
                assert_eq!(once, twice);
                if event.target().rank() > state.rank() {
                    assert_eq!(once, event.target());
                } else {
                    assert_eq!(once, state);
                }
            }
        }
    }

    #[test]
    fn queue_item_reaped_is_absorbing() {
        for &event in &ALL_EVENTS {
            assert_eq!(t(QueueItemState::Reaped, event), QueueItemState::Reaped);
        }
        assert!(QueueItemState::Reaped.is_terminal());
    }

    #[test]
    fn queue_item_machine_drives_canonical_lifecycle() {
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

    #[test]
    fn queue_item_stale_reemit_storm_never_resurrects_an_advanced_item() {
        let machine = QueueItemMachine::new(QueueItemState::Authored);
        machine.send(QueueItemEvent::BacklogMirrored);
        machine.send(QueueItemEvent::DrainStarted);
        machine.send(QueueItemEvent::ExchangeAnswered);
        assert_eq!(machine.state(), QueueItemState::Answered);

        for _ in 0..10 {
            machine.send(QueueItemEvent::OperatorAuthored);
            machine.send(QueueItemEvent::BacklogMirrored);
        }
        assert_eq!(machine.state(), QueueItemState::Answered);
    }

    #[test]
    fn queue_item_lifecycle_projection_commutes_with_join() {
        for &a in &ALL_STATES {
            for &b in &ALL_STATES {
                let joined_state = if a.rank() >= b.rank() { a } else { b };
                assert_eq!(joined_state.lifecycle(), a.lifecycle().join(b.lifecycle()));
            }
        }
    }

    #[test]
    fn queue_item_identity_collapses_pin_and_prefix_variants() {
        let bracketed = QueueItemIdentity::from_prompt(":pushpin: do [#gww8] thing");
        let bare_prefix = QueueItemIdentity::from_prompt("do [#gww8] thing");
        let plain = QueueItemIdentity::from_prompt("[#gww8]");
        assert_eq!(bracketed, QueueItemIdentity::Id("gww8".into()));
        assert_eq!(bracketed, bare_prefix);
        assert_eq!(bracketed, plain);
        assert!(bracketed.is_id_backed());
    }

    #[test]
    fn queue_item_free_text_identity_keys_on_normalized_text() {
        let a = QueueItemIdentity::from_prompt("Still getting JB File Cache Conflict dialogs.");
        let b = QueueItemIdentity::from_prompt("still getting   jb file cache conflict dialogs.");
        let other = QueueItemIdentity::from_prompt("Opencode reverse-orders numeric lists.");
        assert_eq!(a, b);
        assert_ne!(a, other);
        assert!(!a.is_id_backed());
    }
}
