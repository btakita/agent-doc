//! Lazily-owned tenure of one claimed closeout owner (`#implementlazilyowned`,
//! family 2b).
//!
//! A [`CloseoutOwnerCapability`](super::CloseoutOwnerCapability) is minted with
//! one claimed tenure: a `lazily::EffectCasCore` epoch whose in-flight attempt
//! is this process's ownership of the cycle. An answered refresh that names
//! another owner (or a superseded cycle) records the attempt as Lazily
//! `Superseded`; the capability's "lost" check reads that terminal record. It
//! replaces the bespoke shared `lease_lost` `AtomicBool`. The guard's release
//! receipts the tenure as applied, after which a late loss report matches no
//! attempt and changes nothing.

use std::sync::Arc;

use lazily::{
    EffectAttemptIdentity, EffectCasApplied, EffectCasClaim, EffectCasCore, EffectCasFailure,
    EffectCasUpsert, EffectIdentity, EffectIntent, EffectPlan, EffectTerminal,
};
use parking_lot::Mutex;

const GENERATION: u64 = 0;
const NOW: u64 = 0;

/// Key: this capability's tenure. Authority: the owner identity
/// (`cycle:owner`) the tenure expects; a loss records the observed holder.
type TenureCore = EffectCasCore<(), String, (), (), String>;

#[derive(Debug)]
struct Tenure {
    core: TenureCore,
    attempt: Option<EffectAttemptIdentity>,
}

/// Shared tenure of one closeout owner capability. Clones (the heartbeat's
/// borrow) share the same Lazily key.
#[derive(Debug, Clone)]
pub(crate) struct CloseoutOwnerTenure {
    inner: Arc<Mutex<Tenure>>,
}

impl CloseoutOwnerTenure {
    /// Claim the tenure of `owner_id` for `cycle_id`.
    pub(crate) fn claim(cycle_id: &str, owner_id: &str) -> anyhow::Result<Self> {
        let owner = format!("{cycle_id}:{owner_id}");
        let mut core = TenureCore::new(GENERATION);
        let intent = EffectIntent {
            identity: EffectIdentity::new(format!("closeout-owner-tenure:{owner}#1"))
                .map_err(|error| anyhow::anyhow!("invalid closeout tenure identity: {error:?}"))?,
            epoch: 1,
            key: (),
            expected: owner,
            plan: EffectPlan::single(()),
        };
        match core.upsert_desired(intent).1 {
            EffectCasUpsert::Accepted => {}
            other => anyhow::bail!("closeout owner tenure was not accepted: {other:?}"),
        }
        let attempt = match core.claim(&(), GENERATION, NOW).1 {
            EffectCasClaim::Claimed(attempt) => attempt.identity,
            other => anyhow::bail!("closeout owner tenure was not claimed: {other:?}"),
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(Tenure {
                core,
                attempt: Some(attempt),
            })),
        })
    }

    /// Whether an answered refresh superseded this tenure.
    pub(crate) fn superseded(&self) -> bool {
        matches!(
            self.inner
                .lock()
                .core
                .state(&())
                .and_then(|state| state.terminal),
            Some(EffectTerminal::Superseded { .. })
        )
    }

    /// Record that the controller answered with another owner (`observed`).
    /// Only the claimed attempt can be superseded; after a loss or a release
    /// the report changes nothing.
    pub(crate) fn record_superseded(&self, observed: &str) {
        let mut tenure = self.inner.lock();
        let Some(attempt) = tenure.attempt.take() else {
            return;
        };
        match tenure
            .core
            .record_superseded(&(), &attempt, observed.to_string())
            .1
        {
            EffectCasFailure::Superseded => {}
            // Not the in-flight attempt any more: keep the identity so the
            // record stays consistent with the core.
            _ => tenure.attempt = Some(attempt),
        }
    }

    /// Receipt the tenure: the owner released its cycle normally.
    pub(crate) fn release(&self) {
        let mut tenure = self.inner.lock();
        let Some(attempt) = tenure.attempt.take() else {
            return;
        };
        let authority = attempt.effect.as_str().to_string();
        if !matches!(
            tenure.core.record_applied(&(), &attempt, (), authority).1,
            EffectCasApplied::Completed { .. }
        ) {
            tenure.attempt = Some(attempt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answered_loss_supersedes_the_tenure_on_every_clone() {
        let tenure = CloseoutOwnerTenure::claim("cycle", "owner").unwrap();
        let heartbeat = tenure.clone();
        assert!(!tenure.superseded());
        heartbeat.record_superseded("cycle:other-owner");
        assert!(tenure.superseded(), "clones share one Lazily key");
        // A repeated report is a no-op on the terminal record.
        heartbeat.record_superseded("cycle:third-owner");
        assert!(tenure.superseded());
    }

    #[test]
    fn a_released_tenure_ignores_a_late_loss_report() {
        let tenure = CloseoutOwnerTenure::claim("cycle", "owner").unwrap();
        tenure.release();
        tenure.record_superseded("cycle:other-owner");
        assert!(!tenure.superseded());
    }
}
