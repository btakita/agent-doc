//! The preflight admission deadline, as seen by every bounded wait beneath it
//! (`#preflightdeadline`, GH #78 follow-up).
//!
//! The `UserPromptSubmit` hook gives preflight one wall-clock budget (90s by
//! default). Every wait inside preflight already had its own local bound — a 3s
//! settle window, a per-RPC controller timeout, a 10s handoff settle — but
//! nothing capped their SUM, so a run could spend the budget a few seconds at a
//! time and be abandoned mid-step by the hook's backstop timer.
//!
//! The hook therefore installs an admission deadline (the budget minus a
//! margin) on the preflight worker thread. Each bounded wait asks [`clamp`] (or
//! [`clamp_or_exhausted`]) for its bound instead of using its local constant
//! alone, so no single wait can outlive the deadline, and a wait that finds the
//! deadline already spent fails with the typed [`AdmissionDeadlineExhausted`]
//! instead of starting.
//!
//! The deadline is installed per run by the owner of the run (the hook's
//! per-run `PreflightProgress` handle), on the worker thread only, and removed
//! by the returned guard. It lives here, in a leaf crate, because the waits it
//! bounds sit in crates (`controller-io`, `preflight-io`) that cannot depend on
//! the preflight command crate that owns the run.
//!
//! "No deadline installed" and "deadline exhausted" are distinct answers
//! (`#idlerevisionreactive`): [`remaining`] returns `None` for the former, so a
//! CLI preflight or any non-preflight caller is never clamped by accident, and
//! `Some(Duration::ZERO)` for the latter.

use std::cell::Cell;
use std::time::{Duration, Instant};

/// One installed admission deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionDeadline {
    /// The instant after which no new bounded wait may start.
    pub at: Instant,
    /// The hook budget this deadline was derived from, for messages.
    pub budget: Duration,
}

impl AdmissionDeadline {
    /// Time left before the deadline, saturating at zero.
    pub fn remaining_at(&self, now: Instant) -> Duration {
        self.at.saturating_duration_since(now)
    }
}

thread_local! {
    static INSTALLED: Cell<Option<AdmissionDeadline>> = const { Cell::new(None) };
}

/// A bounded wait found the admission deadline already spent.
///
/// Typed so the hook can tell "the deadline ended this run" apart from an
/// ordinary preflight error, and turn it into a named, retryable refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionDeadlineExhausted {
    /// The wait that refused to start.
    pub wait: &'static str,
    /// The hook budget the deadline was derived from.
    pub budget: Duration,
}

impl std::fmt::Display for AdmissionDeadlineExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "preflight admission deadline exhausted before `{}` could start (hook budget {}s)",
            self.wait,
            self.budget.as_secs()
        )
    }
}

impl std::error::Error for AdmissionDeadlineExhausted {}

/// Restores the previously installed deadline (normally none) on drop.
#[must_use = "dropping the guard immediately uninstalls the deadline"]
pub struct DeadlineGuard {
    previous: Option<AdmissionDeadline>,
}

impl Drop for DeadlineGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        INSTALLED.with(|slot| slot.set(previous));
    }
}

/// Install `deadline` for every bounded wait on this thread until the guard drops.
pub fn install(deadline: AdmissionDeadline) -> DeadlineGuard {
    let previous = INSTALLED.with(|slot| slot.replace(Some(deadline)));
    DeadlineGuard { previous }
}

/// The deadline installed on this thread, if any.
pub fn current() -> Option<AdmissionDeadline> {
    INSTALLED.with(Cell::get)
}

/// Time left before the installed deadline; `None` when no deadline is installed.
pub fn remaining() -> Option<Duration> {
    current().map(|deadline| deadline.remaining_at(Instant::now()))
}

/// True only when a deadline is installed AND it has passed.
pub fn exhausted() -> bool {
    remaining().is_some_and(|left| left.is_zero())
}

/// `wait`, shortened to the time left before the installed deadline.
///
/// Unchanged when no deadline is installed. May return zero; callers that
/// cannot accept a zero bound (socket timeouts) use [`clamp_or_exhausted`].
pub fn clamp(wait: Duration) -> Duration {
    match remaining() {
        Some(left) => wait.min(left),
        None => wait,
    }
}

/// [`clamp`], refusing with [`AdmissionDeadlineExhausted`] when no time is left.
pub fn clamp_or_exhausted(
    wait_name: &'static str,
    wait: Duration,
) -> Result<Duration, AdmissionDeadlineExhausted> {
    let Some(deadline) = current() else {
        return Ok(wait);
    };
    let left = deadline.remaining_at(Instant::now());
    if left.is_zero() {
        return Err(AdmissionDeadlineExhausted {
            wait: wait_name,
            budget: deadline.budget,
        });
    }
    Ok(wait.min(left))
}

/// Refuse with [`AdmissionDeadlineExhausted`] when the installed deadline has passed.
pub fn ensure_remaining(wait_name: &'static str) -> Result<(), AdmissionDeadlineExhausted> {
    clamp_or_exhausted(wait_name, Duration::MAX).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deadline_in(left: Duration) -> AdmissionDeadline {
        AdmissionDeadline {
            at: Instant::now() + left,
            budget: Duration::from_secs(90),
        }
    }

    #[test]
    fn no_deadline_leaves_every_wait_unchanged() {
        assert_eq!(remaining(), None);
        assert!(!exhausted());
        assert_eq!(clamp(Duration::from_secs(120)), Duration::from_secs(120));
        assert_eq!(
            clamp_or_exhausted("rpc", Duration::from_secs(120)),
            Ok(Duration::from_secs(120))
        );
    }

    #[test]
    fn waits_are_clamped_to_the_time_remaining() {
        let _guard = install(deadline_in(Duration::from_secs(2)));
        assert!(clamp(Duration::from_secs(120)) <= Duration::from_secs(2));
        assert_eq!(
            clamp(Duration::from_millis(100)),
            Duration::from_millis(100),
            "a wait already inside the deadline keeps its own bound"
        );
        let clamped = clamp_or_exhausted("rpc", Duration::from_secs(120)).unwrap();
        assert!(clamped <= Duration::from_secs(2) && !clamped.is_zero());
    }

    #[test]
    fn a_spent_deadline_refuses_with_the_named_wait() {
        let _guard = install(AdmissionDeadline {
            at: Instant::now() - Duration::from_millis(1),
            budget: Duration::from_secs(90),
        });
        assert!(exhausted());
        assert_eq!(clamp(Duration::from_secs(3)), Duration::ZERO);
        let err = clamp_or_exhausted("controller_rpc", Duration::from_secs(5)).unwrap_err();
        assert_eq!(err.wait, "controller_rpc");
        assert!(err.to_string().contains("`controller_rpc`"), "{err}");
        assert!(ensure_remaining("settle").is_err());
    }

    #[test]
    fn the_guard_restores_the_previous_deadline_and_other_threads_never_see_it() {
        let outer = deadline_in(Duration::from_secs(60));
        let _outer = install(outer);
        {
            let _inner = install(deadline_in(Duration::from_secs(1)));
            assert!(remaining().unwrap() <= Duration::from_secs(1));
        }
        assert_eq!(current(), Some(outer));
        let other = std::thread::spawn(remaining).join().unwrap();
        assert_eq!(other, None, "the deadline is per thread, never ambient");
    }
}
