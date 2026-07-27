//! Closeout-ownership gating as a derived fact (`#closeoutterminalreactive`).
//!
//! "Does the incumbent closeout owner still block a new claim?" was answered by
//! a clock: `owner.is_active_at(now)`. A lease is a *guess about liveness*, and
//! the guess was standing in for facts the system already had.
//!
//! The wedge that proved it, 2026-07-26 on `tasks/agent-doc/agent-doc-bugs2.md`:
//! a `session-check` recovery probe claimed the owner lease during one turn, the
//! turn advanced, and the claim kept blocking every later probe until its lease
//! ran out — while the refusal it generated told the operator to re-run the very
//! command the claim was blocking. Nothing was actually in flight. The turn had
//! moved on, and that was knowable the instant it happened.
//!
//! So ownership is derived, and the derivation asks in order of certainty:
//!
//! 1. **Superseded** — the owner's `cycle_id` is not the open turn's. Moot, no
//!    clock involved. This is the case that made the timeout load-bearing.
//! 2. **Owner process gone** — typed liveness evidence, supplied by the caller.
//! 3. **Lease expired** — the stopgap, and the only branch that reads a clock.
//!
//! # The timeout is feedback, not the mechanism
//!
//! The lease stays, because a same-turn owner with a live process genuinely can
//! wedge and something must eventually free it. But it is now **distinguishable**
//! ([`CloseoutOwnerRelease::is_stopgap`]) and it should never fire: if a claim is
//! released by expiry in production, a supersession path is missing. That is why
//! the one [`Effect`] here reports the stopgap firing rather than performing the
//! release — the release is a [`Computed`], and an `Effect` whose whole body
//! assigns a value should have been a `Computed` (`#idlerevisionreactive`).
//!
//! The clock is an *observation* like any other ([`Self::observe_now`]), not an
//! ambient read inside the derivation, so the whole gate is a pure function of
//! its inputs and testable without waiting for real time to pass.

use lazily::{Computed, Effect, Source, ThreadSafeContext};

use crate::{CloseoutOwnerProjection, CloseoutOwnerRelease, DocumentScope};

/// Whether a new claim may proceed, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutGate {
    /// No incumbent owner, or no open turn to own.
    Unowned,
    /// The incumbent still owns the open turn and its process is alive.
    Blocked { owner: Box<CloseoutOwnerProjection> },
    /// The incumbent no longer blocks, for this reason.
    Released {
        owner: Box<CloseoutOwnerProjection>,
        reason: CloseoutOwnerRelease,
    },
}

impl CloseoutGate {
    pub fn blocks_claim(&self) -> bool {
        matches!(self, Self::Blocked { .. })
    }

    pub fn release_reason(&self) -> Option<CloseoutOwnerRelease> {
        match self {
            Self::Released { reason, .. } => Some(*reason),
            _ => None,
        }
    }

    /// True when the clock, not a derived fact, is what freed the claim — the
    /// signal that a supersession path is missing.
    pub fn released_by_stopgap(&self) -> bool {
        self.release_reason().is_some_and(|r| r.is_stopgap())
    }

    pub fn owner(&self) -> Option<&CloseoutOwnerProjection> {
        match self {
            Self::Unowned => None,
            Self::Blocked { owner } | Self::Released { owner, .. } => Some(owner),
        }
    }
}

/// The whole decision as a pure total function of its observations.
///
/// Kept separate from the cells so it is unit-testable with fixed inputs, and so
/// the reactive wiring has nothing in it but wiring.
pub fn closeout_gate(
    open_cycle_id: Option<&str>,
    owner: Option<&CloseoutOwnerProjection>,
    now_secs: u64,
    owner_alive: Option<bool>,
    allow_dead_owner_takeover: bool,
) -> CloseoutGate {
    let Some(owner) = owner else {
        return CloseoutGate::Unowned;
    };
    let Some(open_cycle_id) = open_cycle_id else {
        // No open turn at all: whatever the owner was doing belongs to a turn
        // that is over. Same conclusion as supersession, same absence of a clock.
        return CloseoutGate::Released {
            owner: Box::new(owner.clone()),
            reason: CloseoutOwnerRelease::SupersededByNewTurn,
        };
    };
    match owner.release_reason(
        open_cycle_id,
        now_secs,
        owner_alive,
        allow_dead_owner_takeover,
    ) {
        Some(reason) => CloseoutGate::Released {
            owner: Box::new(owner.clone()),
            reason,
        },
        None => CloseoutGate::Blocked {
            owner: Box::new(owner.clone()),
        },
    }
}

/// Document-scoped cells holding closeout-ownership gating.
///
/// The observations are [`Source`]s and the gate is a [`Computed`] over them, so
/// every consumer reads the *same* derived value and cannot drift from another
/// consumer's private derivation — the property `#retainedsettlereactive`
/// established for retained writes, applied to ownership.
pub struct CloseoutGateState {
    ctx: ThreadSafeContext,
    open_cycle_id: Source<Option<String>>,
    owner: Source<Option<CloseoutOwnerProjection>>,
    now_secs: Source<u64>,
    owner_alive: Source<Option<bool>>,
    allow_dead_owner_takeover: Source<bool>,
    gate: Computed<CloseoutGate>,
}

impl Default for CloseoutGateState {
    fn default() -> Self {
        Self::new()
    }
}

impl CloseoutGateState {
    /// Join the document's graph. Ownership's lifetime is the open document: a
    /// claim outlives any one turn (that is the whole problem), and dropping the
    /// document drops the claim with it.
    pub fn new_in(scope: &DocumentScope) -> Self {
        Self::build(scope.ctx().clone())
    }

    /// Standalone instance for unit tests, kept beside `new_in` per
    /// `#stategraphjoin`.
    pub fn new() -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        Self::build(ThreadSafeContext::new())
    }

    fn build(ctx: ThreadSafeContext) -> Self {
        let open_cycle_id = ctx.source(None::<String>);
        let owner = ctx.source(None::<CloseoutOwnerProjection>);
        let now_secs = ctx.source(0u64);
        let owner_alive = ctx.source(None::<bool>);
        let allow_dead_owner_takeover = ctx.source(false);
        let gate = ctx.computed(move |c| {
            closeout_gate(
                c.get(&open_cycle_id).as_deref(),
                c.get(&owner).as_ref(),
                c.get(&now_secs),
                c.get(&owner_alive),
                c.get(&allow_dead_owner_takeover),
            )
        });
        Self {
            ctx,
            open_cycle_id,
            owner,
            now_secs,
            owner_alive,
            allow_dead_owner_takeover,
            gate,
        }
    }

    pub fn ctx(&self) -> &ThreadSafeContext {
        &self.ctx
    }

    /// The open turn. Writing a new turn here is what cancels the previous
    /// turn's claim — supersession is a data dependency, not a cleanup call
    /// someone has to remember to make.
    pub fn observe_open_cycle(&self, cycle_id: Option<String>) {
        self.ctx.set(&self.open_cycle_id, cycle_id);
    }

    pub fn observe_owner(&self, owner: Option<CloseoutOwnerProjection>) {
        self.ctx.set(&self.owner, owner);
    }

    /// The clock as an observation rather than an ambient read, so the stopgap
    /// branch is testable without waiting for real time.
    pub fn observe_now(&self, now_secs: u64) {
        self.ctx.set(&self.now_secs, now_secs);
    }

    pub fn observe_owner_alive(&self, alive: Option<bool>) {
        self.ctx.set(&self.owner_alive, alive);
    }

    pub fn observe_allow_dead_owner_takeover(&self, allow: bool) {
        self.ctx.set(&self.allow_dead_owner_takeover, allow);
    }

    /// The shared derived fact.
    pub fn gate(&self) -> CloseoutGate {
        self.ctx.get(&self.gate)
    }

    /// The gate cell itself, for callers gating an `Effect` on it.
    pub fn gate_cell(&self) -> &Computed<CloseoutGate> {
        &self.gate
    }

    /// Report when the **stopgap** frees a claim, and only then.
    ///
    /// The feedback half: a lease expiry means a derived fact should have
    /// released this claim earlier and no path did. The effect body is the whole
    /// side effect (emitting the diagnostic) — it does not perform the release,
    /// because the release is already the `Computed` above.
    pub fn on_stopgap_release(
        &self,
        sink: impl Fn(&CloseoutOwnerProjection) + Send + Sync + 'static,
    ) -> Effect {
        let gate = self.gate;
        self.ctx.effect(move |c| {
            let gate = c.get(&gate);
            if gate.released_by_stopgap()
                && let Some(owner) = gate.owner()
            {
                sink(owner);
            }
        })
    }

    /// Stop driving [`Self::on_stopgap_release`].
    pub fn stop(&self, effect: &Effect) {
        self.ctx.dispose_effect(effect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CLOSEOUT_OWNER_LEASE_SECS, CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY};
    use std::sync::{Arc, Mutex};

    fn owner(cycle_id: &str, claimed: u64) -> CloseoutOwnerProjection {
        CloseoutOwnerProjection {
            cycle_id: cycle_id.to_string(),
            owner_id: "owner-old".to_string(),
            owner_pid: 4242,
            role: CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY.to_string(),
            claimed_secs: claimed,
            expires_secs: claimed + CLOSEOUT_OWNER_LEASE_SECS,
        }
    }

    fn state_with_owner(cycle: &str) -> CloseoutGateState {
        let state = CloseoutGateState::new();
        state.observe_owner(Some(owner(cycle, 1_000)));
        state.observe_open_cycle(Some(cycle.to_string()));
        state.observe_now(1_010);
        state.observe_owner_alive(Some(true));
        state.observe_allow_dead_owner_takeover(true);
        state
    }

    #[test]
    fn advancing_the_turn_cancels_the_previous_turns_claim() {
        let state = state_with_owner("cycle-old");
        assert!(state.gate().blocks_claim(), "a live same-turn owner blocks");

        // The only thing that changes is the open turn. No clock advance.
        state.observe_open_cycle(Some("cycle-new".to_string()));

        assert_eq!(
            state.gate().release_reason(),
            Some(CloseoutOwnerRelease::SupersededByNewTurn)
        );
        assert!(!state.gate().blocks_claim());
        assert!(
            !state.gate().released_by_stopgap(),
            "supersession must not be attributed to the timeout"
        );
    }

    #[test]
    fn a_live_same_turn_owner_is_not_stolen_from() {
        let state = state_with_owner("cycle-open");
        assert!(state.gate().blocks_claim());

        // Time passes, but not past the lease.
        state.observe_now(1_000 + CLOSEOUT_OWNER_LEASE_SECS - 1);
        assert!(state.gate().blocks_claim());
    }

    #[test]
    fn the_timeout_is_the_last_resort_and_announces_itself() {
        let state = state_with_owner("cycle-open");
        let fired: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let effect = {
            let fired = Arc::clone(&fired);
            state.on_stopgap_release(move |owner| {
                fired.lock().unwrap().push(owner.owner_id.clone());
            })
        };
        assert!(
            fired.lock().unwrap().is_empty(),
            "nothing to report while blocked"
        );

        state.observe_now(1_000 + CLOSEOUT_OWNER_LEASE_SECS);

        assert!(state.gate().released_by_stopgap());
        assert_eq!(
            fired.lock().unwrap().as_slice(),
            ["owner-old"],
            "an expiry-driven release must be reported as the stopgap firing"
        );
        state.stop(&effect);
    }

    #[test]
    fn supersession_does_not_trip_the_stopgap_report() {
        let state = state_with_owner("cycle-old");
        let fired: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let effect = {
            let fired = Arc::clone(&fired);
            state.on_stopgap_release(move |owner| {
                fired.lock().unwrap().push(owner.owner_id.clone());
            })
        };

        state.observe_open_cycle(Some("cycle-new".to_string()));
        // Well past the lease too: supersession already answered, so the clock
        // branch is never reached and the healthy path stays silent.
        state.observe_now(1_000 + CLOSEOUT_OWNER_LEASE_SECS * 2);

        assert_eq!(
            state.gate().release_reason(),
            Some(CloseoutOwnerRelease::SupersededByNewTurn)
        );
        assert!(
            fired.lock().unwrap().is_empty(),
            "the stopgap must stay silent when a derived fact released the claim"
        );
        state.stop(&effect);
    }

    #[test]
    fn a_dead_owner_is_released_by_liveness_not_by_its_lease() {
        let state = state_with_owner("cycle-open");
        state.observe_owner_alive(Some(false));

        assert_eq!(
            state.gate().release_reason(),
            Some(CloseoutOwnerRelease::OwnerProcessGone)
        );
        assert!(!state.gate().released_by_stopgap());
    }

    #[test]
    fn no_open_turn_means_no_claim_survives_it() {
        let state = state_with_owner("cycle-open");
        state.observe_open_cycle(None);

        assert_eq!(
            state.gate().release_reason(),
            Some(CloseoutOwnerRelease::SupersededByNewTurn)
        );
    }

    #[test]
    fn an_unowned_gate_blocks_nothing() {
        let state = CloseoutGateState::new();
        state.observe_open_cycle(Some("cycle-open".to_string()));
        assert_eq!(state.gate(), CloseoutGate::Unowned);
        assert!(!state.gate().blocks_claim());
    }
}
