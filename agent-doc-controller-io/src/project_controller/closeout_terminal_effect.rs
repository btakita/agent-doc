//! Lazily-owned single-flight for a closeout capability's terminal effect
//! (`#implementlazilyowned`).
//!
//! The terminal closeout commit of one claimed cycle is one Lazily effect key.
//! `lazily::EffectCasCore` owns its identity, the single in-flight attempt, and
//! the monotone applied frontier that receipts "this capability's effect closed
//! its cycle". It replaces the bespoke `terminal_effect_in_progress`
//! `compare_exchange` flag and the separate `terminal_effect_landed` flag.
//!
//! The core lock is held only to claim and to record an outcome; the effect
//! itself runs outside it. A second run while one is in flight is refused
//! without touching the in-flight attempt (no latest-wins supersession of a
//! running commit), and a flight dropped without a receipt (an error or an
//! unwinding panic) is recorded as a Lazily rejection, so the key never stays
//! claimed.

use std::sync::Arc;

use lazily::{
    EffectCasApplied, EffectCasClaim, EffectCasCore, EffectCasUpsert, EffectIdentity, EffectIntent,
    EffectPlan,
};
use parking_lot::Mutex;

const GENERATION: u64 = 0;
const NOW: u64 = 0;

/// Key: the terminal effect of this capability. Authority: the number of
/// receipted terminal runs. Payload / receipt: the run itself.
type TerminalEffectCore = EffectCasCore<(), u64, (), (), String>;

#[derive(Debug)]
struct Inner {
    identity: String,
    core: Mutex<TerminalEffectCore>,
}

/// Shared single-flight authority for one closeout capability's terminal effect.
#[derive(Debug, Clone)]
pub(crate) struct TerminalEffectSingleFlight {
    inner: Arc<Inner>,
}

impl TerminalEffectSingleFlight {
    pub(crate) fn new(cycle_id: &str, owner_id: &str) -> Self {
        Self {
            inner: Arc::new(Inner {
                identity: format!("closeout-terminal-effect:{cycle_id}:{owner_id}"),
                core: Mutex::new(TerminalEffectCore::new(GENERATION)),
            }),
        }
    }

    /// Whether a terminal run is in flight.
    pub(crate) fn in_progress(&self) -> bool {
        self.inner
            .core
            .lock()
            .state(&())
            .is_some_and(|state| state.inflight.is_some())
    }

    /// Whether a terminal run of this capability was receipted (Lazily's
    /// applied frontier).
    pub(crate) fn landed(&self) -> bool {
        self.inner
            .core
            .lock()
            .state(&())
            .is_some_and(|state| state.applied_through.is_some())
    }

    /// Claim the terminal effect, or `None` while another run is in flight.
    pub(crate) fn begin(&self) -> anyhow::Result<Option<TerminalEffectFlight>> {
        let mut core = self.inner.core.lock();
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
                .map_err(|error| anyhow::anyhow!("invalid terminal effect identity: {error:?}"))?,
            epoch,
            key: (),
            expected: applied,
            plan: EffectPlan::single(()),
        };
        match core.upsert_desired(intent).1 {
            EffectCasUpsert::Accepted => {}
            other => {
                anyhow::bail!("terminal closeout effect epoch {epoch} was not accepted: {other:?}")
            }
        }
        match core.claim(&(), GENERATION, NOW).1 {
            EffectCasClaim::Claimed(attempt) => Ok(Some(TerminalEffectFlight {
                inner: self.inner.clone(),
                attempt: Some(attempt.identity),
            })),
            other => {
                anyhow::bail!("terminal closeout effect epoch {epoch} was not claimed: {other:?}")
            }
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

/// One claimed terminal run. [`TerminalEffectFlight::land`] receipts it;
/// dropping it unreceipted records a Lazily rejection.
pub(crate) struct TerminalEffectFlight {
    inner: Arc<Inner>,
    attempt: Option<lazily::EffectAttemptIdentity>,
}

impl TerminalEffectFlight {
    /// Receipt the run: the effect closed this capability's cycle.
    pub(crate) fn land(mut self) -> anyhow::Result<()> {
        let attempt = self.attempt.take().expect("flight receipted once");
        let authority = attempt.epoch;
        match self
            .inner
            .core
            .lock()
            .record_applied(&(), &attempt, (), authority)
            .1
        {
            EffectCasApplied::Completed { .. } => Ok(()),
            other => {
                anyhow::bail!("terminal closeout effect receipt diverged from Lazily: {other:?}")
            }
        }
    }
}

impl Drop for TerminalEffectFlight {
    fn drop(&mut self) {
        if let Some(attempt) = self.attempt.take() {
            self.inner.core.lock().record_rejected(
                &(),
                &attempt,
                "terminal closeout effect ended without a receipt".to_string(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_run_is_refused_while_one_is_in_flight() {
        let flight = TerminalEffectSingleFlight::new("cycle", "owner");
        let first = flight.begin().unwrap().expect("first run claims");
        assert!(flight.in_progress());
        assert!(flight.begin().unwrap().is_none(), "single-flight per key");
        first.land().unwrap();
        assert!(!flight.in_progress());
        assert!(flight.landed());
    }

    #[test]
    fn dropped_flight_is_rejected_and_frees_the_key_without_landing() {
        let flight = TerminalEffectSingleFlight::new("cycle", "owner");
        drop(flight.begin().unwrap().expect("claims"));
        assert!(!flight.in_progress());
        assert!(!flight.landed());
        let retry = flight
            .begin()
            .unwrap()
            .expect("a rejected run may run again");
        retry.land().unwrap();
        assert!(flight.landed());
    }

    #[test]
    fn landed_frontier_is_monotone_across_later_runs() {
        let flight = TerminalEffectSingleFlight::new("cycle", "owner");
        flight.begin().unwrap().unwrap().land().unwrap();
        drop(flight.begin().unwrap().unwrap());
        assert!(flight.landed(), "a later failed run never un-receipts");
        let clone = flight.clone();
        let held = clone.begin().unwrap().unwrap();
        assert!(flight.begin().unwrap().is_none(), "clones share one key");
        held.land().unwrap();
    }
}
