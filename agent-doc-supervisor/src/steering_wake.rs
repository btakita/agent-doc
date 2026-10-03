//! Reactive idle steering wake (`#steeringwake`).
//!
//! # The miss this closes
//!
//! Operator steering reached a running agent only through a harness
//! `PostToolUse` hook (the agent must be executing tool calls), the turn
//! boundary report, or a manual `agent-doc steering` poll. A session that had
//! ended its turn and sat idle never learned of a queue addition: on
//! 2026-10-03 the operator queued `#subagent: …/issues/116..118` into an idle
//! `agent-doc-bugs.md` session and nothing surfaced them until the operator
//! asked by hand.
//!
//! # Shape
//!
//! - **Source**: one [`SteeringWakeEvent`] stream folded by [`advance`] into
//!   [`SteeringWakeTracking`]. The supervisor feeds it the settled, unsurfaced,
//!   unclaimed steering set it derived from the document (the same
//!   `midturn_steering` derivation every other steering consumer uses) and the
//!   delivery receipt of every wake it submits.
//! - **Computed**: [`SteeringWakeState::subject`] — the drain subject an idle
//!   pane should be woken for, or `None`. It is `None` when nothing is
//!   unsurfaced, and `None` again once a receipt for the same steering set is
//!   recorded, which is the exactly-once fence.
//! - **Effect**: the supervisor's existing idle-queue dispatch. The subject joins
//!   the drain decision as an active head, so every existing guard applies
//!   unchanged: prompt-ready detection per harness, no dispatch into an active
//!   turn, the drain-owner lease, route-submit in flight, the convergence gate,
//!   and the dispatch dedup. A real drainable queue head always wins; the wake
//!   subject only fills the gap where steering exists but no head drains.
//!
//! The receipt is generation-fenced by the steering set fingerprint, which is
//! derived from the steering base's cycle id plus every item's verbatim text,
//! so a new cycle or any re-edit is a new fingerprint and a fresh wake.

use agent_doc_state_scope::LocalProcessScope;
use lazily::{Computed, StateMachine};

/// Prefix that marks an idle-drain subject as a steering wake rather than a
/// queue head. It never starts with `/`, so the drain payload is always the
/// harness trigger command, never a literal slash command.
pub const STEERING_WAKE_SUBJECT_PREFIX: &str = "agent-doc steering wake ";

/// The unsurfaced steering one observation found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteeringWakeSet {
    /// Stable identity of the whole set (base cycle + every item verbatim).
    pub fingerprint: String,
    /// Settled items in the set.
    pub items: usize,
}

/// One input to the wake graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteeringWakeEvent {
    /// The supervisor derived the steering set from the document. `None` means
    /// nothing settled and unsurfaced (empty, still being typed, already
    /// surfaced, or every item claimed by a worker).
    Observed(Option<SteeringWakeSet>),
    /// A durable receipt loaded at startup, or written after a submitted wake.
    Delivered(String),
}

/// Everything the event stream folds to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SteeringWakeTracking {
    pub observed: Option<SteeringWakeSet>,
    /// Fingerprint of the last steering set a wake was delivered for.
    pub delivered: Option<String>,
}

impl SteeringWakeTracking {
    /// The drain subject to wake an idle pane for, if any.
    pub fn subject(&self) -> Option<String> {
        let set = self.observed.as_ref()?;
        if set.items == 0 || self.delivered.as_deref() == Some(set.fingerprint.as_str()) {
            return None;
        }
        Some(steering_wake_subject(&set.fingerprint))
    }
}

/// The fold. Total and pure.
pub fn advance(
    current: &SteeringWakeTracking,
    event: &SteeringWakeEvent,
) -> Option<SteeringWakeTracking> {
    let mut next = current.clone();
    match event {
        SteeringWakeEvent::Observed(set) => next.observed = set.clone(),
        SteeringWakeEvent::Delivered(fingerprint) => next.delivered = Some(fingerprint.clone()),
    }
    Some(next)
}

/// The drain subject for a steering set fingerprint.
pub fn steering_wake_subject(fingerprint: &str) -> String {
    let short = fingerprint.get(..12).unwrap_or(fingerprint);
    format!("{STEERING_WAKE_SUBJECT_PREFIX}{short}")
}

/// True when an idle-drain subject is a steering wake, not a queue head.
pub fn is_steering_wake_subject(subject: &str) -> bool {
    subject.starts_with(STEERING_WAKE_SUBJECT_PREFIX)
}

/// A real drainable queue head always wins: its dispatch submits the same
/// trigger and the woken cycle's preflight sees every steering edit anyway.
pub fn idle_drain_subject(active_head: Option<String>, wake: Option<String>) -> Option<String> {
    active_head.or(wake)
}

/// The supervisor's steering wake graph (process-lifetime scope).
pub struct SteeringWakeState {
    scope: LocalProcessScope,
    machine: StateMachine<SteeringWakeTracking, SteeringWakeEvent>,
    subject: Computed<Option<String>>,
}

impl Default for SteeringWakeState {
    fn default() -> Self {
        Self::new()
    }
}

impl SteeringWakeState {
    pub fn new() -> Self {
        Self::new_in(LocalProcessScope::new())
    }

    /// Build the graph inside a caller-owned process scope (`#stategraphjoin`).
    pub fn new_in(scope: LocalProcessScope) -> Self {
        let machine = StateMachine::new(scope.ctx(), SteeringWakeTracking::default(), advance);
        let state = machine.state_handle();
        let subject = scope.ctx().computed(move |ctx| ctx.get(&state).subject());
        Self {
            scope,
            machine,
            subject,
        }
    }

    pub fn send(&self, event: SteeringWakeEvent) {
        self.machine.send(self.scope.ctx(), event);
    }

    pub fn tracking(&self) -> SteeringWakeTracking {
        self.machine.state(self.scope.ctx())
    }

    /// The Computed wake subject.
    pub fn subject(&self) -> Option<String> {
        self.scope.ctx().get(&self.subject)
    }

    /// Run `sink` whenever the wake subject changes (an `Effect` gated on the
    /// Computed, so a diagnostic fires on the transition, never per tick).
    pub fn on_subject_change(&self, sink: impl Fn(Option<String>) + 'static) -> lazily::Effect {
        let subject = self.subject;
        self.scope.ctx().effect(move |ctx| sink(ctx.get(&subject)))
    }

    /// The fingerprint a submitted wake for `subject` should receipt.
    pub fn fingerprint_for(&self, subject: &str) -> Option<String> {
        let tracking = self.tracking();
        let set = tracking.observed?;
        (steering_wake_subject(&set.fingerprint) == subject).then_some(set.fingerprint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(fp: &str, items: usize) -> Option<SteeringWakeSet> {
        Some(SteeringWakeSet {
            fingerprint: fp.to_string(),
            items,
        })
    }

    #[test]
    fn unsurfaced_steering_produces_one_wake_until_receipted() {
        let state = SteeringWakeState::new();
        assert_eq!(state.subject(), None);
        state.send(SteeringWakeEvent::Observed(set("aaaaaaaaaaaaaaaa", 3)));
        let subject = state.subject().expect("idle wake subject");
        assert!(is_steering_wake_subject(&subject));
        assert_eq!(
            state.fingerprint_for(&subject).as_deref(),
            Some("aaaaaaaaaaaaaaaa")
        );
        state.send(SteeringWakeEvent::Delivered("aaaaaaaaaaaaaaaa".into()));
        assert_eq!(state.subject(), None, "a receipted set never wakes twice");
        // Re-observing the same set after the receipt stays quiet.
        state.send(SteeringWakeEvent::Observed(set("aaaaaaaaaaaaaaaa", 3)));
        assert_eq!(state.subject(), None);
        // A re-edit is a new set and a fresh wake.
        state.send(SteeringWakeEvent::Observed(set("bbbbbbbbbbbbbbbb", 4)));
        assert!(state.subject().is_some());
    }

    #[test]
    fn empty_or_cleared_steering_never_wakes() {
        let state = SteeringWakeState::new();
        state.send(SteeringWakeEvent::Observed(set("cccccccccccc", 0)));
        assert_eq!(state.subject(), None);
        state.send(SteeringWakeEvent::Observed(set("cccccccccccc", 1)));
        assert!(state.subject().is_some());
        state.send(SteeringWakeEvent::Observed(None));
        assert_eq!(state.subject(), None);
    }

    #[test]
    fn a_real_queue_head_wins_over_the_wake_subject() {
        assert_eq!(
            idle_drain_subject(Some("do [#a]".into()), Some("wake".into())).as_deref(),
            Some("do [#a]")
        );
        assert_eq!(
            idle_drain_subject(None, Some("wake".into())).as_deref(),
            Some("wake")
        );
    }

    #[test]
    fn the_wake_subject_submits_the_trigger_not_a_slash_command() {
        let subject = steering_wake_subject("0123456789abcdef");
        assert_eq!(
            agent_doc_queue::idle_drain::idle_queue_drain_payload(&subject, "agent-doc plan.md"),
            "agent-doc plan.md"
        );
        assert_eq!(
            agent_doc_queue::idle_drain::idle_queue_drain_payload_kind(&subject),
            "trigger"
        );
    }
}
