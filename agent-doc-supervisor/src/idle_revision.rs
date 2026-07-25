//! Reactive idle-watch revision state (`#idlerevisionreactive`).
//!
//! # The wedge this exists to stop
//!
//! The supervisor's idle watch polls every 500ms, and each tick asks: *has the
//! document changed since I last looked?* That cheap question gates an expensive
//! reconcile which issues several more controller RPCs. The cheap probe was an
//! `Option<Revision>`, and `None` meant three different things:
//!
//! - we deliberately did not look (controller cooldown, queue pause);
//! - we looked and the controller could not answer (it is wedged or slow);
//! - the document has no readable metadata.
//!
//! All three collapsed to "changed", so the expensive path ran. That inverts the
//! intended behavior at exactly the wrong moment: when the controller cannot
//! answer a *cheap* question, the supervisor responded by asking it many
//! *expensive* ones. With a 500ms poll against a 60s full-reconcile interval,
//! that is up to **120x the intended controller load, produced by the wedge, and
//! feeding it**. Two supervisors doing this to one controller is what the write
//! path sees as `controller_model_backpressure` and a 5s authority-resolve
//! timeout.
//!
//! Absence of evidence is not evidence of change. [`RevisionObservation`] keeps
//! the three facts distinct, and only [`RevisionObservation::Observed`] can ever
//! report a change.
//!
//! # Why this is cells and not a few `if`s
//!
//! The old code carried the same state as loop-local `mut` bindings updated by
//! whichever branch remembered to. That is the failure mode `#stategraphjoin`
//! describes: a derived fact that only updates because some path remembered to
//! update it. Pressure is a *state* the graph derives from the observation
//! stream, not a flag someone sets.
//!
//! # Shape
//!
//! Three layers, each testable on its own:
//!
//! 1. [`RevisionTracking`] + [`advance`] — the history-dependent fold, as plain
//!    data and a pure total function. No graph.
//! 2. One [`StateMachine`] cell holding that fold.
//! 3. [`Computed`]s over the machine's state handle, and an [`Effect`] gated on
//!    one of them.
//!
//! The first draft skipped layer 1 and wrote three [`lazily::Source`]s by hand
//! from an `observe` method. Both bugs it shipped with were ordering hazards
//! between those writes — a baseline advanced in the tick it was compared
//! against, and a baseline that lagged two observations when an unanswered probe
//! sat between two real ones. Neither is expressible once the history is one
//! value advanced by one function, which is the argument for the decomposition.
//!
//! [`Effect`]: lazily::Effect
//!
//! The scope is a [`LocalProcessScope`]: the idle watch is one single-threaded
//! loop that lives as long as the supervisor process.

use agent_doc_state_scope::LocalProcessScope;
use lazily::{Computed, StateMachine};

/// Consecutive unanswered probes before the controller is called degraded.
///
/// One timeout is noise: a controller busy with a real write legitimately misses
/// a probe. A sustained streak is a wedge. At a 500ms poll this is ~2s.
pub const UNRESOLVED_STREAK_DEGRADED: u32 = 4;

/// How many suppressed observations a degraded controller gets before the next
/// probe is let through.
///
/// Counted in *observations*, not wall-clock, so the backoff is a pure function
/// of the observation stream and needs no clock input. The idle watch observes on
/// its quiescent-maintenance cadence (~5s), so this is ~30s — matching the
/// wall-clock backoff it replaces.
pub const SUPPRESSED_OBSERVATIONS_BEFORE_RETRY: u32 = 6;

/// What one idle-watch tick learned about the document's revision.
///
/// The variants are deliberately not an `Option`: "I did not look", "I looked and
/// got no answer", and "here is the revision" drive different decisions, and
/// flattening them is what produced the load amplification described above.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RevisionObservation {
    /// The revision, as an opaque fingerprint. Equal fingerprints mean the
    /// projection is still valid.
    Observed(String),
    /// We chose not to look: the controller is in a pressure cooldown, or the
    /// document's queue is controller-paused. Skipping the probe is the whole
    /// point of a cooldown, so it must not then trigger the expensive path.
    ///
    /// The default: before the first tick we have not looked at anything.
    #[default]
    Suppressed,
    /// We looked and could not find out. This is the wedge signal.
    Unresolved,
}

impl RevisionObservation {
    pub fn observed(fingerprint: impl Into<String>) -> Self {
        Self::Observed(fingerprint.into())
    }

    fn fingerprint(&self) -> Option<&str> {
        match self {
            Self::Observed(value) => Some(value.as_str()),
            Self::Suppressed | Self::Unresolved => None,
        }
    }
}

/// Whether the controller is answering the idle watch's cheap probe.
///
/// This is the failure mode as *state*. The old code had no such value: a failed
/// probe was an error return that each caller re-derived a reaction to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerProbeHealth {
    /// Answering, or deliberately not being asked.
    Responsive,
    /// A sustained run of unanswered probes. The idle watch must back off rather
    /// than escalate to the expensive path.
    Degraded { unresolved_streak: u32 },
}

impl ControllerProbeHealth {
    pub fn is_degraded(self) -> bool {
        matches!(self, Self::Degraded { .. })
    }
}

/// Everything the observation stream folds to.
///
/// Plain data with a pure transition ([`advance`]), so the history-dependent part
/// of the problem is unit-testable without a graph at all, and the cell layer has
/// nothing left to get wrong.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RevisionTracking {
    /// This tick's probe.
    pub observation: RevisionObservation,
    /// The fingerprint the current observation is compared against: the most
    /// recent `Observed` tick *before* this one.
    pub baseline: Option<String>,
    /// The most recent `Observed` fingerprint, including this tick. Distinct from
    /// `baseline` so staleness stays readable after the fold; collapsing them
    /// compares a tick against itself.
    pub last_observed: Option<String>,
    /// Consecutive unanswered probes. Suppressed ticks hold it: a cooldown is
    /// caused by the wedge, so it is not evidence the wedge cleared.
    pub unresolved_streak: u32,
    /// Consecutive suppressed ticks. This is the backoff's clock: counting the
    /// observations we skipped is what lets the retry deadline be *derived*
    /// rather than stamped from a wall-clock reading.
    pub suppressed_run: u32,
}

impl RevisionTracking {
    pub fn projection_stale(&self) -> bool {
        // Only a real observation can report change. A suppressed or unanswered
        // probe leaves the projection exactly as valid as it was.
        let Some(fingerprint) = self.observation.fingerprint() else {
            return false;
        };
        self.baseline.as_deref() != Some(fingerprint)
    }

    pub fn probe_health(&self) -> ControllerProbeHealth {
        if self.unresolved_streak >= UNRESOLVED_STREAK_DEGRADED {
            ControllerProbeHealth::Degraded {
                unresolved_streak: self.unresolved_streak,
            }
        } else {
            ControllerProbeHealth::Responsive
        }
    }

    /// Whether the controller should be probed on the next tick.
    ///
    /// The backoff as a *derived value* rather than a stamped deadline. A
    /// deadline cannot be a `Computed`: it is a function of *when* health changed,
    /// so expressing it that way needs a clock input that invalidates the graph
    /// continuously. Counting skipped observations instead keeps it a pure
    /// function of the stream, with no clock, no cell, and no effect writing a
    /// variable.
    ///
    /// Self-regulating: degraded health stops the probe, each skipped tick feeds
    /// `suppressed_run`, and once enough have accumulated the next probe is let
    /// through. Whatever that probe learns resets the count, so the loop cannot
    /// latch — see the liveness test.
    pub fn should_probe_controller(&self) -> bool {
        match self.probe_health() {
            ControllerProbeHealth::Responsive => true,
            ControllerProbeHealth::Degraded { .. } => {
                self.suppressed_run >= SUPPRESSED_OBSERVATIONS_BEFORE_RETRY
            }
        }
    }
}

/// The fold: one observation advances the tracking state.
///
/// Total (never rejects an event) and pure. Both bugs found while building this
/// were ordering hazards in a hand-written multi-cell write; expressing the
/// history as one value advanced by one function is what removes that class.
pub fn advance(
    current: &RevisionTracking,
    observation: &RevisionObservation,
) -> Option<RevisionTracking> {
    let mut next = current.clone();
    next.observation = observation.clone();
    match observation {
        RevisionObservation::Observed(fingerprint) => {
            next.baseline = current.last_observed.clone();
            next.last_observed = Some(fingerprint.clone());
            next.unresolved_streak = 0;
            next.suppressed_run = 0;
        }
        RevisionObservation::Suppressed => {
            next.suppressed_run = current.suppressed_run.saturating_add(1);
        }
        RevisionObservation::Unresolved => {
            next.unresolved_streak = current.unresolved_streak.saturating_add(1);
            // A probe that ran and failed restarts the backoff: we spent the
            // retry, so the next one must wait the full run again.
            next.suppressed_run = 0;
        }
    }
    Some(next)
}

/// The idle watch's revision graph.
///
/// One [`StateMachine`] cell holds the fold; every other value is a [`Computed`]
/// over its state handle, so nothing here can be written out of step with the
/// observation that produced it.
pub struct IdleRevisionState {
    scope: LocalProcessScope,
    machine: StateMachine<RevisionTracking, RevisionObservation>,
    projection_stale: Computed<bool>,
    probe_health: Computed<ControllerProbeHealth>,
    should_probe_controller: Computed<bool>,
}

impl Default for IdleRevisionState {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleRevisionState {
    pub fn new() -> Self {
        Self::new_in(LocalProcessScope::new())
    }

    /// Build the graph inside a caller-owned process scope (`#stategraphjoin`).
    ///
    /// `lazily::Context` is `Rc`-based and not `Clone`, so a local scope is owned
    /// rather than shared by clone: the state takes the scope whose lifetime it
    /// has.
    pub fn new_in(scope: LocalProcessScope) -> Self {
        let machine = StateMachine::new(scope.ctx(), RevisionTracking::default(), advance);
        let state = machine.state_handle();
        let projection_stale = scope.ctx().computed(move |ctx| ctx.get(&state).projection_stale());
        let probe_health = scope.ctx().computed(move |ctx| ctx.get(&state).probe_health());
        let should_probe_controller = scope
            .ctx()
            .computed(move |ctx| ctx.get(&state).should_probe_controller());

        Self {
            scope,
            machine,
            projection_stale,
            probe_health,
            should_probe_controller,
        }
    }

    /// Record this tick's probe.
    ///
    /// One event into one machine. Every derived value updates because its input
    /// changed, not because this method remembered to update it.
    pub fn observe(&self, observation: RevisionObservation) {
        self.machine.send(self.scope.ctx(), observation);
    }

    /// The whole folded state, for callers that want more than one derived view.
    pub fn tracking(&self) -> RevisionTracking {
        self.machine.state(self.scope.ctx())
    }

    /// True only when a real observation differs from the last real observation.
    pub fn projection_stale(&self) -> bool {
        self.scope.ctx().get(&self.projection_stale)
    }

    pub fn probe_health(&self) -> ControllerProbeHealth {
        self.scope.ctx().get(&self.probe_health)
    }

    /// Whether to probe the controller on the next tick. Derived, so the caller
    /// obeys a value rather than consulting a deadline someone had to stamp.
    pub fn should_probe_controller(&self) -> bool {
        self.scope.ctx().get(&self.should_probe_controller)
    }

    pub fn unresolved_streak(&self) -> u32 {
        self.tracking().unresolved_streak
    }

    /// Run `sink` whenever probe health *changes*, and only then.
    ///
    /// This replaces the "remember to log the degradation once" bookkeeping the
    /// loop used to carry as a `last_*_hash` binding. As an `Effect` gated on a
    /// derived value it fires on the transition and is idempotent otherwise, so
    /// the "someone forgot to call it" failure mode is gone rather than guarded.
    pub fn on_probe_health_change(
        &self,
        sink: impl Fn(ControllerProbeHealth) + 'static,
    ) -> lazily::Effect {
        let probe_health = self.probe_health;
        self.scope.ctx().effect(move |ctx| {
            sink(ctx.get(&probe_health));
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Fold the whole stream with no graph involved. This is the layer the two
    /// original ordering bugs lived in, so it is pinned without cells.
    fn fold(observations: &[RevisionObservation]) -> RevisionTracking {
        observations.iter().fold(
            RevisionTracking::default(),
            |state, observation| advance(&state, observation).expect("the fold is total"),
        )
    }

    #[test]
    fn the_fold_compares_against_the_previous_observation_not_the_current_one() {
        // The first bug: advancing the baseline to the incoming value compares a
        // tick against itself, so every change reads as no-change.
        assert!(
            fold(&[RevisionObservation::observed("a")]).projection_stale(),
            "a first observation differs from the empty baseline"
        );
        assert!(
            !fold(&[
                RevisionObservation::observed("a"),
                RevisionObservation::observed("a"),
            ])
            .projection_stale(),
            "an equal revision is not a change"
        );
        assert!(
            fold(&[
                RevisionObservation::observed("a"),
                RevisionObservation::observed("b"),
            ])
            .projection_stale(),
            "a different revision is a change"
        );
    }

    #[test]
    fn the_fold_baseline_skips_over_ticks_that_observed_nothing() {
        // The second bug: the baseline lagged two observations when a probe that
        // learned nothing sat between two real ones, so returning to an earlier
        // revision across the gap read as no-change.
        let state = fold(&[
            RevisionObservation::observed("a"),
            RevisionObservation::observed("b"),
            RevisionObservation::Unresolved,
            RevisionObservation::Suppressed,
            RevisionObservation::observed("a"),
        ]);
        assert_eq!(state.baseline.as_deref(), Some("b"));
        assert!(
            state.projection_stale(),
            "the baseline is the last thing actually observed, so a change across \
             a gap of uninformative ticks is still a change"
        );
    }

    #[test]
    fn the_fold_is_total_so_no_observation_can_be_dropped() {
        for observation in [
            RevisionObservation::observed("a"),
            RevisionObservation::Suppressed,
            RevisionObservation::Unresolved,
        ] {
            assert!(
                advance(&RevisionTracking::default(), &observation).is_some(),
                "a rejected event would silently keep the previous tick's answer, \
                 which is the staleness this module exists to prevent: {observation:?}"
            );
        }
    }

    /// The regression. A controller that cannot answer must not be asked more.
    #[test]
    fn an_unanswered_probe_does_not_invalidate_the_projection() {
        let state = IdleRevisionState::new();
        state.observe(RevisionObservation::observed("rev-1"));
        assert!(
            state.projection_stale(),
            "the first real observation differs from the empty baseline"
        );

        state.observe(RevisionObservation::observed("rev-1"));
        assert!(!state.projection_stale(), "an unchanged revision is not stale");

        state.observe(RevisionObservation::Unresolved);
        assert!(
            !state.projection_stale(),
            "a probe the controller could not answer is absence of evidence, not \
             evidence of change — treating it as change is what made a wedged \
             controller receive 120x the intended load"
        );

        state.observe(RevisionObservation::Suppressed);
        assert!(
            !state.projection_stale(),
            "a probe we deliberately skipped during a cooldown must not trigger the \
             expensive path the cooldown exists to avoid"
        );
    }

    /// The other half: backing off must not lose a real change. An unanswered
    /// stretch must not make the next real observation look unchanged.
    #[test]
    fn a_real_change_after_an_unanswered_stretch_is_still_seen() {
        let state = IdleRevisionState::new();
        state.observe(RevisionObservation::observed("rev-1"));
        state.observe(RevisionObservation::observed("rev-1"));
        assert!(!state.projection_stale());

        for _ in 0..10 {
            state.observe(RevisionObservation::Unresolved);
        }
        state.observe(RevisionObservation::observed("rev-2"));

        assert!(
            state.projection_stale(),
            "the last *observed* revision is the baseline, so a change across an \
             unanswered stretch is still a change"
        );
    }

    #[test]
    fn probe_health_degrades_only_on_a_sustained_streak_and_recovers_on_an_answer() {
        let state = IdleRevisionState::new();
        assert_eq!(state.probe_health(), ControllerProbeHealth::Responsive);

        for _ in 0..UNRESOLVED_STREAK_DEGRADED - 1 {
            state.observe(RevisionObservation::Unresolved);
        }
        assert_eq!(
            state.probe_health(),
            ControllerProbeHealth::Responsive,
            "one slow answer is noise, not a wedge"
        );

        state.observe(RevisionObservation::Unresolved);
        assert_eq!(
            state.probe_health(),
            ControllerProbeHealth::Degraded {
                unresolved_streak: UNRESOLVED_STREAK_DEGRADED
            }
        );

        state.observe(RevisionObservation::observed("rev-9"));
        assert_eq!(
            state.probe_health(),
            ControllerProbeHealth::Responsive,
            "a single real answer proves the controller is back"
        );
    }

    /// A cooldown is caused by the wedge, so it must not erase the evidence of it.
    #[test]
    fn suppressed_ticks_hold_the_streak_rather_than_clearing_it() {
        let state = IdleRevisionState::new();
        for _ in 0..UNRESOLVED_STREAK_DEGRADED {
            state.observe(RevisionObservation::Unresolved);
        }
        assert!(state.probe_health().is_degraded());

        state.observe(RevisionObservation::Suppressed);
        assert!(
            state.probe_health().is_degraded(),
            "backing off is the response to the wedge; it is not proof the wedge \
             cleared, and only a real answer is"
        );
    }

    /// Liveness, and the reason the backoff is derived rather than an effect that
    /// stamps a deadline.
    ///
    /// Suppressed ticks deliberately hold the unresolved streak, so health alone
    /// is self-sustaining: degraded stops the probe, the suppressed tick holds the
    /// streak, and the controller would never be asked again. What breaks the
    /// latch is `should_probe_controller` counting the skipped observations — the
    /// backoff clears *because the stream advanced*, with no clock involved.
    ///
    /// This drives the full cycle the way the caller does, obeying the derived
    /// answer, so a "simplification" that gates the probe on health directly
    /// latches here instead of in production.
    #[test]
    fn the_derived_backoff_lets_a_probe_through_and_cannot_latch() {
        let state = IdleRevisionState::new();
        for _ in 0..UNRESOLVED_STREAK_DEGRADED {
            state.observe(RevisionObservation::Unresolved);
        }
        assert!(state.probe_health().is_degraded());
        assert!(
            !state.should_probe_controller(),
            "a degraded controller is given room immediately"
        );

        // Drive it exactly as the caller does: obey the derived answer, feed the
        // resulting observation back in. If the backoff could latch, this loop
        // would never see `should_probe_controller`.
        let mut suppressed_ticks = 0;
        while !state.should_probe_controller() {
            state.observe(RevisionObservation::Suppressed);
            suppressed_ticks += 1;
            assert!(
                state.probe_health().is_degraded(),
                "a skipped tick is not evidence the controller recovered"
            );
            assert!(
                suppressed_ticks <= SUPPRESSED_OBSERVATIONS_BEFORE_RETRY,
                "the backoff must clear from the observation stream alone; \
                 latched after {suppressed_ticks} skipped ticks"
            );
        }
        assert_eq!(suppressed_ticks, SUPPRESSED_OBSERVATIONS_BEFORE_RETRY);

        // The probe that gets through fails: we spent the retry, so the caller
        // must be told to back off again rather than hammering.
        state.observe(RevisionObservation::Unresolved);
        assert!(
            !state.should_probe_controller(),
            "a failed retry restarts the backoff instead of re-probing every tick"
        );

        // The next one answers. Only that clears the health.
        for _ in 0..SUPPRESSED_OBSERVATIONS_BEFORE_RETRY {
            state.observe(RevisionObservation::Suppressed);
        }
        assert!(state.should_probe_controller());
        state.observe(RevisionObservation::observed("rev-after-cooldown"));
        assert_eq!(
            state.probe_health(),
            ControllerProbeHealth::Responsive,
            "a real answer is the only thing that clears degraded health"
        );
        assert_eq!(state.unresolved_streak(), 0);
        assert!(state.should_probe_controller());
    }

    /// The Effect fires on transitions, which is what makes the diagnostic
    /// impossible to forget and impossible to spam.
    #[test]
    fn the_health_effect_runs_on_transitions_not_on_every_tick() {
        let state = IdleRevisionState::new();
        let seen: Rc<RefCell<Vec<ControllerProbeHealth>>> = Rc::new(RefCell::new(Vec::new()));
        let _effect = {
            let seen = Rc::clone(&seen);
            state.on_probe_health_change(move |health| seen.borrow_mut().push(health))
        };

        let initial = seen.borrow().len();
        assert!(initial >= 1, "an effect materializes with its current value");

        for _ in 0..UNRESOLVED_STREAK_DEGRADED * 3 {
            state.observe(RevisionObservation::Unresolved);
        }
        state.observe(RevisionObservation::observed("rev-1"));

        let health_values = seen.borrow().clone();
        let degraded = health_values.iter().filter(|h| h.is_degraded()).count();
        assert!(
            degraded >= 1,
            "the degradation must be reported at all: {health_values:?}"
        );
        assert!(
            health_values.len() < (UNRESOLVED_STREAK_DEGRADED * 3) as usize,
            "an effect gated on derived health must not fire once per tick — that \
             is the per-tick log spam it replaces: {health_values:?}"
        );
        assert_eq!(
            health_values.last(),
            Some(&ControllerProbeHealth::Responsive),
            "recovery is a transition too"
        );
    }
}
