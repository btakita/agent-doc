//! Lazily-owned single flight for one in-process effect key
//! (`#implementlazilyowned`, plan step 6).
//!
//! A long-lived owner that starts an external effect on one thread and finishes
//! it on another (the controller's self-recycle handoff) keeps that effect's
//! single flight in a [`SingleFlight`] instead of a `compare_exchange` flag.
//! `lazily::EffectCasCore` owns the effect identity, the single in-flight
//! attempt, and the monotone applied frontier ("this effect landed").
//!
//! The core lock is held only to claim and to record an outcome; the effect
//! itself runs outside it. A second [`SingleFlight::begin`] while one flight is
//! in flight is refused without touching it. A [`Flight`] that is released, or
//! dropped without a receipt (an error or an unwinding panic), is recorded as a
//! Lazily rejection, so the key never stays claimed.

use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};

use anyhow::{Result, bail};
use lazily::{
    EffectCasApplied, EffectCasClaim, EffectCasCore, EffectCasUpsert, EffectIdentity, EffectIntent,
    EffectPlan,
};

const GENERATION: u64 = 0;
const NOW: u64 = 0;

/// Key: the effect. Authority: the number of receipted runs. Payload /
/// receipt: the run itself. Error: the rendered release reason.
type FlightCore = EffectCasCore<(), u64, (), (), String>;

#[derive(Debug)]
struct Inner {
    identity: String,
    core: Mutex<FlightCore>,
}

impl Inner {
    fn core(&self) -> MutexGuard<'_, FlightCore> {
        self.core.lock()
    }
}

/// Shared Lazily single flight for one effect key. Clones share the key.
#[derive(Debug, Clone)]
pub struct SingleFlight {
    inner: Arc<Inner>,
}

impl SingleFlight {
    pub fn new(identity: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                identity: identity.into(),
                core: Mutex::new(FlightCore::new(GENERATION)),
            }),
        }
    }

    /// Whether a run holds the key's single flight.
    pub fn in_flight(&self) -> bool {
        self.inner
            .core()
            .state(&())
            .is_some_and(|state| state.inflight.is_some())
    }

    /// Whether a run of this key was receipted (Lazily's applied frontier).
    pub fn landed(&self) -> bool {
        self.inner
            .core()
            .state(&())
            .is_some_and(|state| state.applied_through.is_some())
    }

    /// Whether the key is in flight or already landed.
    pub fn claimed_or_landed(&self) -> bool {
        self.inner
            .core()
            .state(&())
            .is_some_and(|state| state.inflight.is_some() || state.applied_through.is_some())
    }

    /// Claim the key, or `None` while another run is in flight.
    pub fn begin(&self) -> Result<Option<Flight>> {
        let mut core = self.inner.core();
        let (inflight, applied, last_epoch) = match core.state(&()) {
            Some(state) => (
                state.inflight.is_some(),
                state.applied_through.unwrap_or(0),
                state.terminal.as_ref().map(terminal_epoch).unwrap_or(0),
            ),
            None => (false, 0, 0),
        };
        if inflight {
            return Ok(None);
        }
        let epoch = last_epoch.max(applied) + 1;
        let intent = EffectIntent {
            identity: EffectIdentity::new(format!("{}#{epoch}", self.inner.identity))
                .map_err(|error| anyhow::anyhow!("invalid single-flight identity: {error:?}"))?,
            epoch,
            key: (),
            expected: applied,
            plan: EffectPlan::single(()),
        };
        match core.upsert_desired(intent).1 {
            EffectCasUpsert::Accepted => {}
            other => bail!(
                "{}: single-flight epoch {epoch} was not accepted: {other:?}",
                self.inner.identity
            ),
        }
        match core.claim(&(), GENERATION, NOW).1 {
            EffectCasClaim::Claimed(attempt) => Ok(Some(Flight {
                inner: self.inner.clone(),
                attempt: Some(attempt.identity),
            })),
            other => bail!(
                "{}: single-flight epoch {epoch} was not claimed: {other:?}",
                self.inner.identity
            ),
        }
    }
}

fn terminal_epoch(terminal: &lazily::EffectTerminal<(), u64, String>) -> u64 {
    match terminal {
        lazily::EffectTerminal::Applied { epoch, .. }
        | lazily::EffectTerminal::Superseded { epoch, .. }
        | lazily::EffectTerminal::Rejected { epoch, .. } => *epoch,
    }
}

/// One claimed run. [`Flight::land`] receipts it; [`Flight::release`] or
/// dropping it unreceipted records a Lazily rejection and frees the key.
#[derive(Debug)]
pub struct Flight {
    inner: Arc<Inner>,
    attempt: Option<lazily::EffectAttemptIdentity>,
}

impl Flight {
    /// Receipt the run: the effect landed and the key's frontier advances.
    pub fn land(mut self) -> Result<()> {
        let attempt = self.attempt.take().expect("flight receipted once");
        let authority = attempt.epoch;
        match self
            .inner
            .core()
            .record_applied(&(), &attempt, (), authority)
            .1
        {
            EffectCasApplied::Completed { .. } => Ok(()),
            other => bail!(
                "{}: single-flight receipt diverged from Lazily: {other:?}",
                self.inner.identity
            ),
        }
    }

    /// End the run without a receipt: a Lazily rejection that frees the key.
    pub fn release(mut self, reason: impl Into<String>) {
        if let Some(attempt) = self.attempt.take() {
            self.inner
                .core()
                .record_rejected(&(), &attempt, reason.into());
        }
    }
}

impl Drop for Flight {
    fn drop(&mut self) {
        if let Some(attempt) = self.attempt.take() {
            self.inner.core().record_rejected(
                &(),
                &attempt,
                "single-flight run ended without a receipt".to_string(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_begin_is_refused_while_one_run_is_in_flight() {
        let flight = SingleFlight::new("test:single");
        let first = flight.begin().unwrap().expect("first run claims");
        assert!(flight.in_flight());
        assert!(flight.claimed_or_landed());
        assert!(flight.begin().unwrap().is_none(), "single flight per key");
        first.land().unwrap();
        assert!(!flight.in_flight());
        assert!(flight.landed());
        assert!(flight.claimed_or_landed());
    }

    #[test]
    fn a_released_or_dropped_run_is_rejected_and_frees_the_key() {
        let flight = SingleFlight::new("test:release");
        flight.begin().unwrap().expect("claims").release("declined");
        assert!(!flight.claimed_or_landed());
        drop(
            flight
                .begin()
                .unwrap()
                .expect("a released key claims again"),
        );
        assert!(!flight.claimed_or_landed());
        let clone = flight.clone();
        let held = clone.begin().unwrap().expect("a dropped key claims again");
        assert!(flight.begin().unwrap().is_none(), "clones share one key");
        held.land().unwrap();
        assert!(flight.landed());
    }

    #[test]
    fn a_flight_moves_to_another_thread_and_lands_there() {
        let flight = SingleFlight::new("test:thread");
        let run = flight.begin().unwrap().expect("claims");
        std::thread::spawn(move || run.land().unwrap())
            .join()
            .unwrap();
        assert!(flight.landed());
        assert!(!flight.in_flight());
    }
}
