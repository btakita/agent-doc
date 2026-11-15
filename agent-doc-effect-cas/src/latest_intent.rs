//! Keyed latest-intent schedule (`#implementlazilyowned`, plan step 4).
//!
//! A long-lived producer that retains an effect across attempts (a refused
//! editor save, an unacknowledged editor-op burst) keeps that intent in a
//! [`LatestIntentSchedule`] instead of an application timer, an `in_flight`
//! set, and an epoch comparison. `lazily::EffectCasCore` owns:
//!
//! - the key's single flight (`claim` answers `Busy` while an attempt runs);
//! - latest-wins coalescing (desiring a newer intent supersedes the pending
//!   one, and a retryable outcome of an older in-flight attempt is recorded
//!   `Superseded` once a newer intent is pending);
//! - retry eligibility (`retry_after` records the attempt `Retryable` with its
//!   due time; `claim` answers `Waiting` until then);
//! - exact receipt matching (every outcome names the claimed attempt identity;
//!   a stale attempt is `Unknown` and changes nothing).
//!
//! The producer keeps only the physical wait and the backend call. It never
//! decides whether an intent is still wanted: a claim that comes back for an
//! intent whose backend authority moved is recorded `superseded`.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use lazily::{
    EffectApply, EffectCasApplied, EffectCasClaim, EffectCasCore, EffectCasFailure,
    EffectCasOutcome, EffectCasUpsert, EffectIdentity, EffectIntent, EffectPlan,
};

/// The single actor generation of an in-process schedule.
const GENERATION: u64 = 0;

/// One claimed attempt of a retained intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentAttempt<K> {
    attempt: lazily::EffectAttempt<K, String, String, String>,
}

impl<K> IntentAttempt<K> {
    /// The key whose single flight this attempt holds.
    pub fn key(&self) -> &K {
        &self.attempt.key
    }

    /// The Lazily-owned expected authority the intent was desired over.
    pub fn expected(&self) -> &str {
        &self.attempt.expected
    }

    /// The intent's payload (its producer-side identity).
    pub fn payload(&self) -> &str {
        &self.attempt.payload
    }

    /// 1-based attempt number of this intent.
    pub fn attempt(&self) -> u32 {
        self.attempt.identity.attempt
    }

    /// The Lazily epoch of the intent.
    pub fn epoch(&self) -> u64 {
        self.attempt.identity.epoch
    }
}

/// What a claim found for a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentClaim<K> {
    /// The key's single flight is now held by this attempt.
    Claimed(IntentAttempt<K>),
    /// A retained intent exists but its retry is not due before `at`.
    Waiting { at: Instant },
    /// Another attempt of the key is in flight.
    Busy,
    /// Nothing is desired for the key.
    Empty,
}

/// What recording a retryable attempt decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentRetry {
    /// The same intent is retained and claimable at `at`.
    Retrying { at: Instant },
    /// A newer intent was desired while the attempt ran; this one is terminal.
    Superseded,
    /// The attempt is not the key's in-flight attempt (a stale receipt).
    Unknown,
}

/// Lazily-owned keyed latest-intent schedule. See the module docs.
#[derive(Debug, Clone)]
pub struct LatestIntentSchedule<K: Ord + Clone> {
    core: EffectCasCore<K, String, String, String, String>,
    clock: Instant,
    next_epoch: u64,
}

impl<K: Ord + Clone + std::fmt::Debug> Default for LatestIntentSchedule<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone + std::fmt::Debug> LatestIntentSchedule<K> {
    pub fn new() -> Self {
        Self {
            core: EffectCasCore::new(GENERATION),
            clock: Instant::now(),
            next_epoch: 0,
        }
    }

    fn logical(&self, at: Instant) -> u64 {
        at.saturating_duration_since(self.clock)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }

    /// The logical time at or after `at`, so a due time never precedes the
    /// requested wait.
    fn logical_ceil(&self, at: Instant) -> u64 {
        let elapsed = at.saturating_duration_since(self.clock);
        let floor = self.logical(at);
        if Duration::from_millis(floor) < elapsed {
            floor.saturating_add(1)
        } else {
            floor
        }
    }

    fn instant(&self, logical: u64) -> Instant {
        self.clock + Duration::from_millis(logical)
    }

    /// Desire `payload` over `expected` as `key`'s newest intent. Lazily
    /// supersedes any older pending intent of the key; an in-flight older
    /// attempt keeps running and its outcome is matched against this newer
    /// desire. Returns the intent's Lazily epoch.
    pub fn desire(
        &mut self,
        key: K,
        identity: &str,
        expected: impl Into<String>,
        payload: impl Into<String>,
    ) -> Result<u64> {
        self.next_epoch += 1;
        let epoch = self.next_epoch;
        let intent = EffectIntent {
            identity: EffectIdentity::new(format!("{identity}#{epoch}")).map_err(|error| {
                anyhow::anyhow!("invalid intent identity {identity}: {error:?}")
            })?,
            epoch,
            key,
            expected: expected.into(),
            plan: EffectPlan::single(payload.into()),
        };
        match self.core.upsert_desired(intent).1 {
            EffectCasUpsert::Accepted => Ok(epoch),
            other => bail!("{identity}: intent epoch {epoch} was not accepted: {other:?}"),
        }
    }

    /// Claim `key`'s retained intent if its retry is due at `now`.
    ///
    /// `now` rounds up to Lazily's millisecond clock, like a due time, so a
    /// zero-delay retry is claimable at the instant it was recorded.
    pub fn claim(&mut self, key: &K, now: Instant) -> IntentClaim<K> {
        let logical = self.logical_ceil(now);
        match self.core.claim(key, GENERATION, logical).1 {
            EffectCasClaim::Claimed(attempt) => IntentClaim::Claimed(IntentAttempt { attempt }),
            EffectCasClaim::Waiting { retry_at } => IntentClaim::Waiting {
                at: self.instant(retry_at),
            },
            EffectCasClaim::Busy => IntentClaim::Busy,
            EffectCasClaim::Empty | EffectCasClaim::StaleGeneration { .. } => IntentClaim::Empty,
        }
    }

    /// Desire `payload` and claim it at once, for a producer whose first
    /// attempt runs on the caller's thread. `None` when the key is busy or
    /// another intent of it is still retained (the new intent then stays
    /// desired for the next claim).
    pub fn desire_and_claim(
        &mut self,
        key: K,
        identity: &str,
        expected: impl Into<String>,
        payload: impl Into<String>,
        now: Instant,
    ) -> Result<Option<IntentAttempt<K>>> {
        self.desire(key.clone(), identity, expected, payload)?;
        Ok(match self.claim(&key, now) {
            IntentClaim::Claimed(attempt) => Some(attempt),
            _ => None,
        })
    }

    /// The attempt applied: its intent is the key's receipted frontier.
    /// `false` for a stale attempt or one a newer desire superseded.
    pub fn applied(&mut self, attempt: &IntentAttempt<K>, receipt: impl Into<String>) -> bool {
        let receipt = receipt.into();
        let outcome = self
            .core
            .record_outcome(
                &attempt.attempt.key,
                &attempt.attempt.identity,
                EffectApply::Applied {
                    receipt: receipt.clone(),
                    authority: receipt,
                },
                0,
            )
            .1;
        matches!(
            outcome,
            EffectCasOutcome::Applied(EffectCasApplied::Completed { .. })
        )
    }

    /// The attempt is transient with the authority unchanged: Lazily retains
    /// the intent and makes it claimable `delay` after `now`.
    pub fn retry_after(
        &mut self,
        attempt: &IntentAttempt<K>,
        error: impl Into<String>,
        delay: Duration,
        now: Instant,
    ) -> IntentRetry {
        let retry_at = self.logical_ceil(now + delay);
        match self
            .core
            .record_outcome(
                &attempt.attempt.key,
                &attempt.attempt.identity,
                EffectApply::Retryable(error.into()),
                retry_at,
            )
            .1
        {
            EffectCasOutcome::Failure(EffectCasFailure::Retrying { retry_at }) => {
                IntentRetry::Retrying {
                    at: self.instant(retry_at),
                }
            }
            EffectCasOutcome::Failure(EffectCasFailure::Superseded) => IntentRetry::Superseded,
            _ => IntentRetry::Unknown,
        }
    }

    /// The backend authority moved past the attempt's expected authority:
    /// Lazily records the intent `Superseded { observed }` and drops it.
    pub fn superseded(&mut self, attempt: &IntentAttempt<K>, observed: impl Into<String>) -> bool {
        matches!(
            self.core
                .record_outcome(
                    &attempt.attempt.key,
                    &attempt.attempt.identity,
                    EffectApply::Superseded {
                        observed: observed.into(),
                    },
                    0,
                )
                .1,
            EffectCasOutcome::Failure(EffectCasFailure::Superseded)
        )
    }

    /// The backend refused the intent permanently: a Lazily rejection.
    pub fn rejected(&mut self, attempt: &IntentAttempt<K>, error: impl Into<String>) -> bool {
        matches!(
            self.core
                .record_outcome(
                    &attempt.attempt.key,
                    &attempt.attempt.identity,
                    EffectApply::Rejected(error.into()),
                    0,
                )
                .1,
            EffectCasOutcome::Failure(EffectCasFailure::Rejected)
        )
    }

    /// Whether an attempt of `key` holds its single flight.
    pub fn in_flight(&self, key: &K) -> bool {
        self.core
            .state(key)
            .is_some_and(|state| state.inflight.is_some())
    }

    /// Whether `key` has a desired intent that is not in flight.
    pub fn is_pending(&self, key: &K) -> bool {
        self.core
            .state(key)
            .is_some_and(|state| state.pending.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claimed(
        schedule: &mut LatestIntentSchedule<&'static str>,
        now: Instant,
    ) -> IntentAttempt<&'static str> {
        match schedule.claim(&"doc", now) {
            IntentClaim::Claimed(attempt) => attempt,
            other => panic!("expected a claim, got {other:?}"),
        }
    }

    #[test]
    fn a_retryable_attempt_waits_for_its_due_time_and_reclaims_the_same_intent() -> Result<()> {
        let mut schedule = LatestIntentSchedule::new();
        let now = Instant::now();
        schedule.desire("doc", "test:retry", "epoch-7", "payload")?;
        let first = claimed(&mut schedule, now);
        assert_eq!(schedule.claim(&"doc", now), IntentClaim::Busy);
        assert!(schedule.in_flight(&"doc"));
        let delay = Duration::from_secs(2);
        let IntentRetry::Retrying { at } = schedule.retry_after(&first, "refused", delay, now)
        else {
            panic!("a current intent retries");
        };
        assert!(!schedule.in_flight(&"doc"));
        assert!(schedule.is_pending(&"doc"));
        assert!(matches!(
            schedule.claim(&"doc", now + Duration::from_millis(500)),
            IntentClaim::Waiting { at: waiting } if waiting == at
        ));
        let second = claimed(&mut schedule, at);
        assert_eq!(second.epoch(), first.epoch());
        assert_eq!(second.expected(), "epoch-7");
        assert_eq!(second.attempt(), 2);
        assert!(schedule.applied(&second, "saved"));
        assert_eq!(schedule.claim(&"doc", at), IntentClaim::Empty);
        Ok(())
    }

    #[test]
    fn a_newer_desire_supersedes_a_retained_intent_and_its_in_flight_retry() -> Result<()> {
        let mut schedule = LatestIntentSchedule::new();
        let now = Instant::now();
        schedule.desire("doc", "test:latest", "epoch-1", "one")?;
        let first = claimed(&mut schedule, now);
        schedule.desire("doc", "test:latest", "epoch-2", "two")?;
        assert_eq!(
            schedule.retry_after(&first, "refused", Duration::from_secs(1), now),
            IntentRetry::Superseded
        );
        let second = claimed(&mut schedule, now);
        assert_eq!((second.expected(), second.payload()), ("epoch-2", "two"));
        assert_eq!(second.attempt(), 1);
        Ok(())
    }

    #[test]
    fn a_zero_delay_retry_is_claimable_at_once() -> Result<()> {
        let mut schedule = LatestIntentSchedule::new();
        let now = Instant::now() + Duration::from_micros(300);
        schedule.desire("doc", "test:zero", "epoch-1", "one")?;
        let first = claimed(&mut schedule, now);
        assert!(matches!(
            schedule.retry_after(&first, "again", Duration::ZERO, now),
            IntentRetry::Retrying { .. }
        ));
        assert_eq!(claimed(&mut schedule, now).attempt(), 2);
        Ok(())
    }

    #[test]
    fn a_stale_receipt_is_unknown_and_changes_nothing() -> Result<()> {
        let mut schedule = LatestIntentSchedule::new();
        let now = Instant::now();
        schedule.desire("doc", "test:stale", "epoch-1", "one")?;
        let first = claimed(&mut schedule, now);
        assert!(schedule.applied(&first, "saved"));
        assert!(!schedule.applied(&first, "saved"));
        assert_eq!(
            schedule.retry_after(&first, "late", Duration::ZERO, now),
            IntentRetry::Unknown
        );
        assert_eq!(schedule.claim(&"doc", now), IntentClaim::Empty);
        Ok(())
    }

    #[test]
    fn a_superseded_attempt_drops_its_intent() -> Result<()> {
        let mut schedule = LatestIntentSchedule::new();
        let now = Instant::now();
        let attempt = schedule
            .desire_and_claim("doc", "test:moved", "epoch-1", "one", now)?
            .expect("an idle key claims at once");
        assert!(schedule.superseded(&attempt, "epoch-3"));
        assert_eq!(schedule.claim(&"doc", now), IntentClaim::Empty);
        let rejected = schedule
            .desire_and_claim("doc", "test:moved", "epoch-4", "two", now)?
            .expect("a terminal key claims a new desire at once");
        assert!(schedule.rejected(&rejected, "fatal"));
        assert_eq!(schedule.claim(&"doc", now), IntentClaim::Empty);
        Ok(())
    }
}
