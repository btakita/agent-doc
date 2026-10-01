//! Phase attribution for one preflight run (`#preflightoverrunphase`, GH #78).
//!
//! The `UserPromptSubmit` hook runs preflight on a worker thread and refuses the
//! turn when the admission budget expires. That refusal used to carry a fixed
//! sentence — "usually a wedged project controller or supervisor" — because the
//! hook had no way to see where the worker was. An observed 90s overrun on a
//! 518-byte document had a ready controller, so the guess was wrong and nothing
//! in the refusal or ops.log named the phase that actually consumed the budget.
//!
//! Preflight therefore marks each of its top-level steps with [`enter`]. Phases
//! are sequential, not nested: entering one closes the previous, so the record
//! is a partition of the run. The hook owns a [`PreflightProgress`] handle,
//! installs it on the worker thread with [`install`], and reads a [`snapshot`]
//! on the overrun path — the phase still running when the budget expired is the
//! measured cause.
//!
//! The handle is per run, not process-global, so concurrent preflights (unit
//! tests run many) never write into each other's record. With no handle
//! installed, [`enter`] is a no-op.
//!
//! [`snapshot`]: PreflightProgress::snapshot

use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

#[derive(Debug, Default)]
struct Record {
    /// The phase currently running and when it started.
    current: Option<(&'static str, Instant)>,
    /// Finished phases in first-entry order, with call counts and total time.
    completed: Vec<(&'static str, u32, Duration)>,
}

impl Record {
    fn close_current(&mut self, now: Instant) {
        let Some((label, started)) = self.current.take() else {
            return;
        };
        let elapsed = now.saturating_duration_since(started);
        match self
            .completed
            .iter_mut()
            .find(|(name, _, _)| *name == label)
        {
            Some((_, calls, total)) => {
                *calls += 1;
                *total += elapsed;
            }
            None => self.completed.push((label, 1, elapsed)),
        }
    }
}

/// A shared, per-run phase record. Cloning shares the same record.
#[derive(Debug, Clone)]
pub struct PreflightProgress {
    started: Instant,
    record: Arc<Mutex<Record>>,
}

impl Default for PreflightProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// What a run had done when it was observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressSnapshot {
    /// Wall time since the handle was created.
    pub elapsed: Duration,
    /// The phase still running, and how long it has been running.
    pub running: Option<(&'static str, Duration)>,
    /// Finished phases, costliest first.
    pub completed: Vec<(&'static str, u32, Duration)>,
}

impl PreflightProgress {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            record: Arc::new(Mutex::new(Record::default())),
        }
    }

    fn enter(&self, label: &'static str) {
        let now = Instant::now();
        let mut record = self.record.lock();
        record.close_current(now);
        record.current = Some((label, now));
    }

    fn finish(&self) {
        self.record.lock().close_current(Instant::now());
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        let now = Instant::now();
        let record = self.record.lock();
        let mut completed = record.completed.clone();
        completed.sort_by_key(|sample| std::cmp::Reverse(sample.2));
        ProgressSnapshot {
            elapsed: now.saturating_duration_since(self.started),
            running: record
                .current
                .map(|(label, at)| (label, now.saturating_duration_since(at))),
            completed,
        }
    }
}

impl ProgressSnapshot {
    /// `label:Nms/Kx` per finished phase, costliest first — the same shape as
    /// `session_check.operations`.
    pub fn breakdown(&self) -> String {
        self.completed
            .iter()
            .map(|(label, calls, total)| format!("{label}:{}ms/{calls}x", total.as_millis()))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

thread_local! {
    static INSTALLED: RefCell<Option<PreflightProgress>> = const { RefCell::new(None) };
}

/// Make `progress` the record [`enter`] writes to on this thread.
///
/// Returns a guard that closes the running phase and uninstalls on drop, so a
/// preflight that returns early through `?` still ends its last phase.
pub fn install(progress: PreflightProgress) -> InstallGuard {
    INSTALLED.with(|slot| *slot.borrow_mut() = Some(progress));
    InstallGuard { _private: () }
}

pub struct InstallGuard {
    _private: (),
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        INSTALLED.with(|slot| {
            if let Some(progress) = slot.borrow_mut().take() {
                progress.finish();
            }
        });
    }
}

/// Mark the start of preflight phase `label`, ending the previous one.
pub fn enter(label: &'static str) {
    INSTALLED.with(|slot| {
        if let Some(progress) = slot.borrow().as_ref() {
            progress.enter(label);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_a_phase_closes_the_previous_one() {
        let progress = PreflightProgress::new();
        let guard = install(progress.clone());
        enter("a");
        std::thread::sleep(Duration::from_millis(2));
        enter("b");
        let snapshot = progress.snapshot();
        assert_eq!(snapshot.running.map(|(label, _)| label), Some("b"));
        assert_eq!(snapshot.completed.len(), 1);
        assert_eq!(snapshot.completed[0].0, "a");
        assert!(snapshot.completed[0].2 >= Duration::from_millis(2));
        drop(guard);
        let snapshot = progress.snapshot();
        assert_eq!(
            snapshot.running, None,
            "dropping the guard ends the last phase"
        );
        assert_eq!(snapshot.completed.len(), 2);
    }

    #[test]
    fn re_entering_a_label_accumulates() {
        let progress = PreflightProgress::new();
        let _guard = install(progress.clone());
        enter("a");
        enter("b");
        enter("a");
        enter("c");
        let snapshot = progress.snapshot();
        let a = snapshot
            .completed
            .iter()
            .find(|(l, _, _)| *l == "a")
            .unwrap();
        assert_eq!(a.1, 2);
    }

    /// Another thread's run must not leak into this record.
    #[test]
    fn enter_without_an_installed_handle_is_a_no_op() {
        let progress = PreflightProgress::new();
        let other = std::thread::spawn(|| enter("elsewhere"));
        other.join().unwrap();
        let snapshot = progress.snapshot();
        assert_eq!(snapshot.running, None);
        assert!(snapshot.completed.is_empty());
    }
}
