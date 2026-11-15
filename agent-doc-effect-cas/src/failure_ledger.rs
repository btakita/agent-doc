//! Keyed Lazily failure ledger for editor retries (`#implementlazilyowned`,
//! plan step 7).
//!
//! An editor plugin that retries a refused effect (a local-delta flush the
//! controller did not acknowledge, a replica registration it refused, an
//! editor-op burst whose merge base was unresolvable) used to count those
//! refusals in its own `ConcurrentHashMap<String, Int>` and derive the retry
//! pace and the give-up bound from that count. The count is the retry
//! decision, so it moves here, behind the FFI, where Lazily owns it:
//!
//! - each refused attempt of `(scope, key)` is one claim of a Lazily retained
//!   intent recorded `Retryable`, and the count the caller reads is Lazily's
//!   attempt number of that intent;
//! - [`FailureLedger::clear`] receipts the retained intent (the effect
//!   converged, or the caller gave up), so the next refusal starts a fresh
//!   intent at attempt 1;
//! - [`FailureLedger::clear_scope`] drops a whole scope, which an editor uses
//!   when the manager that owned it is disposed.
//!
//! The caller keeps only the pace curve (how long a given attempt waits) and
//! the physical timer, as `retry_delay_ticks` does for the supervisor's
//! registry publication.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use parking_lot::Mutex;

use crate::latest_intent::{IntentClaim, LatestIntentSchedule};

const IDENTITY: &str = "editor-effect-failure";
const EXPECTED: &str = "unacknowledged";

/// The retained failures of one scope (one editor-side owner and family).
#[derive(Debug, Default)]
struct ScopeLedger {
    schedule: LatestIntentSchedule<String>,
    /// Lazily's attempt number of each key's last recorded failure, so a read
    /// does not have to claim.
    attempts: BTreeMap<String, u32>,
}

/// Lazily-owned failure counts keyed by `(scope, key)`. See the module docs.
#[derive(Debug, Default)]
pub struct FailureLedger {
    scopes: BTreeMap<String, ScopeLedger>,
}

impl FailureLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one refused attempt of `key` in `scope` and return the Lazily
    /// attempt number of the retained intent (1 for the first refusal since
    /// the last [`Self::clear`]).
    pub fn record_failure(&mut self, scope: &str, key: &str, reason: &str) -> Result<u32> {
        let ledger = self.scopes.entry(scope.to_string()).or_default();
        let key = key.to_string();
        let now = Instant::now();
        if !ledger.schedule.is_pending(&key) && !ledger.schedule.in_flight(&key) {
            ledger
                .schedule
                .desire(key.clone(), IDENTITY, EXPECTED, scope)?;
        }
        let attempt = match ledger.schedule.claim(&key, now) {
            IntentClaim::Claimed(attempt) => attempt,
            other => bail!("{scope}/{key}: retained failure was not claimable: {other:?}"),
        };
        let number = attempt.attempt();
        // The physical wait is the caller's timer, so Lazily retains the intent
        // as claimable at once; the attempt number is what the caller reads.
        ledger
            .schedule
            .retry_after(&attempt, reason, Duration::ZERO, now);
        ledger.attempts.insert(key, number);
        Ok(number)
    }

    /// The Lazily attempt number of `key`'s retained failure, or 0.
    pub fn count(&self, scope: &str, key: &str) -> u32 {
        self.scopes
            .get(scope)
            .and_then(|ledger| ledger.attempts.get(key).copied())
            .unwrap_or(0)
    }

    /// Receipt `key`'s retained failure. Returns whether one was retained.
    pub fn clear(&mut self, scope: &str, key: &str) -> bool {
        let Some(ledger) = self.scopes.get_mut(scope) else {
            return false;
        };
        let key = key.to_string();
        let retained = ledger.attempts.remove(&key).is_some();
        if ledger.schedule.is_pending(&key)
            && let IntentClaim::Claimed(attempt) = ledger.schedule.claim(&key, Instant::now())
        {
            ledger.schedule.applied(&attempt, "cleared");
        }
        if ledger.attempts.is_empty() {
            self.scopes.remove(scope);
        }
        retained
    }

    /// Drop every retained failure of `scope`. Returns how many were retained.
    pub fn clear_scope(&mut self, scope: &str) -> usize {
        self.scopes
            .remove(scope)
            .map_or(0, |ledger| ledger.attempts.len())
    }
}

/// The process-wide ledger the editor FFI records into.
pub fn process_failure_ledger() -> &'static Mutex<FailureLedger> {
    static LEDGER: OnceLock<Mutex<FailureLedger>> = OnceLock::new();
    LEDGER.get_or_init(|| Mutex::new(FailureLedger::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_is_the_next_lazily_attempt_of_one_retained_intent() -> Result<()> {
        let mut ledger = FailureLedger::new();
        assert_eq!(ledger.count("crdt.local", "/a.md"), 0);
        assert_eq!(ledger.record_failure("crdt.local", "/a.md", "refused")?, 1);
        assert_eq!(ledger.record_failure("crdt.local", "/a.md", "refused")?, 2);
        assert_eq!(ledger.record_failure("crdt.local", "/a.md", "refused")?, 3);
        assert_eq!(ledger.count("crdt.local", "/a.md"), 3);
        Ok(())
    }

    #[test]
    fn a_cleared_failure_restarts_at_attempt_one() -> Result<()> {
        let mut ledger = FailureLedger::new();
        ledger.record_failure("crdt.local", "/a.md", "refused")?;
        ledger.record_failure("crdt.local", "/a.md", "refused")?;
        assert!(ledger.clear("crdt.local", "/a.md"));
        assert!(!ledger.clear("crdt.local", "/a.md"));
        assert_eq!(ledger.count("crdt.local", "/a.md"), 0);
        assert_eq!(ledger.record_failure("crdt.local", "/a.md", "refused")?, 1);
        Ok(())
    }

    #[test]
    fn keys_and_scopes_are_independent() -> Result<()> {
        let mut ledger = FailureLedger::new();
        ledger.record_failure("crdt.local@1", "/a.md", "refused")?;
        ledger.record_failure("crdt.local@1", "/a.md", "refused")?;
        assert_eq!(
            ledger.record_failure("crdt.local@1", "/b.md", "refused")?,
            1
        );
        assert_eq!(
            ledger.record_failure("crdt.local@2", "/a.md", "refused")?,
            1
        );
        assert_eq!(ledger.clear_scope("crdt.local@1"), 2);
        assert_eq!(ledger.count("crdt.local@1", "/a.md"), 0);
        assert_eq!(ledger.count("crdt.local@2", "/a.md"), 1);
        assert_eq!(ledger.clear_scope("crdt.local@1"), 0);
        Ok(())
    }

    #[test]
    fn the_process_ledger_is_shared() -> Result<()> {
        let scope = "test.process-ledger";
        let first = process_failure_ledger()
            .lock()
            .record_failure(scope, "k", "x")?;
        let second = process_failure_ledger()
            .lock()
            .record_failure(scope, "k", "x")?;
        assert_eq!((first, second), (1, 2));
        assert_eq!(process_failure_ledger().lock().clear_scope(scope), 1);
        Ok(())
    }
}
