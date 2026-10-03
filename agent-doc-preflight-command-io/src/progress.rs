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
//! ## Admission deadline (`#preflightdeadline`)
//!
//! Every wait inside preflight has its own local bound, but nothing capped their
//! sum against the hook budget, so a run could be abandoned mid-step. A handle
//! built with [`PreflightProgress::with_admission_deadline`] carries a deadline
//! (the budget minus [`admission_deadline_margin`]) and does two things with it:
//!
//! 1. [`install`] also installs it as the thread's
//!    [`agent_doc_debounce::admission_deadline`], which clamps every bounded wait
//!    beneath preflight (controller RPC and connect timeouts, handoff settle,
//!    settle windows and slices) to the time remaining.
//! 2. [`enter`] checks it at every phase boundary and returns the typed
//!    [`PreflightAdmissionRefused`] instead of starting the next phase, so the
//!    run ends at a step boundary rather than being abandoned inside one.
//!
//! A wait that the clamp cut short ends its phase with an ordinary error; the
//! hook turns that into the same typed refusal, naming the phase that was
//! running, with [`PreflightProgress::refuse_failed_run`].
//!
//! [`snapshot`]: PreflightProgress::snapshot

use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_doc_debounce::admission_deadline::{self, AdmissionDeadline};
use parking_lot::Mutex;

/// Time the hook keeps for itself between the admission deadline and its own
/// backstop timer: enough to unwind from a phase boundary, print the refusal,
/// and exit before the hook budget abandons the worker.
pub const ADMISSION_DEADLINE_MARGIN: Duration = Duration::from_secs(5);

/// The margin applied to `budget`: [`ADMISSION_DEADLINE_MARGIN`], but never more
/// than a quarter of the budget, so a short (clamped or overridden) budget keeps
/// most of its time for preflight.
pub fn admission_deadline_margin(budget: Duration) -> Duration {
    ADMISSION_DEADLINE_MARGIN.min(budget / 4)
}

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
    deadline: Option<AdmissionDeadline>,
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
    /// A record with no admission deadline: phases are attributed, never refused.
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            deadline: None,
            record: Arc::new(Mutex::new(Record::default())),
        }
    }

    /// A record whose run must finish within `budget` (the hook's admission
    /// budget). Its deadline is `budget` minus [`admission_deadline_margin`].
    pub fn with_admission_deadline(budget: Duration) -> Self {
        let mut progress = Self::new();
        progress.deadline = Some(AdmissionDeadline {
            at: progress.started + budget.saturating_sub(admission_deadline_margin(budget)),
            budget,
        });
        progress
    }

    /// The admission deadline this run carries, if any.
    pub fn deadline(&self) -> Option<AdmissionDeadline> {
        self.deadline
    }

    /// True only when this run carries a deadline and it has passed.
    pub fn deadline_passed(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| deadline.remaining_at(Instant::now()).is_zero())
    }

    /// Returns `anyhow::Error` wrapping [`PreflightAdmissionRefused`] (recover it
    /// with `downcast_ref`), which keeps the `Result` small on the hot path.
    fn enter(&self, label: &'static str) -> anyhow::Result<()> {
        let now = Instant::now();
        let mut record = self.record.lock();
        let previous = record
            .current
            .map(|(name, at)| (name, now.saturating_duration_since(at)));
        record.close_current(now);
        if let Some(deadline) = self.deadline
            && deadline.remaining_at(now).is_zero()
        {
            // Do not start the phase: the run stops at this boundary.
            drop(record);
            return Err(anyhow::Error::new(self.refusal(
                RefusalPoint::BeforePhase(label),
                previous,
                None,
            )));
        }
        record.current = Some((label, now));
        Ok(())
    }

    fn refusal(
        &self,
        point: RefusalPoint,
        previous: Option<(&'static str, Duration)>,
        cause: Option<String>,
    ) -> PreflightAdmissionRefused {
        let snapshot = self.snapshot();
        let deadline = self.deadline.unwrap_or(AdmissionDeadline {
            at: self.started,
            budget: Duration::ZERO,
        });
        PreflightAdmissionRefused {
            point,
            previous,
            elapsed: snapshot.elapsed,
            deadline_after: deadline.at.saturating_duration_since(self.started),
            budget: deadline.budget,
            completed: snapshot.breakdown(),
            cause,
        }
    }

    /// Classify a run that failed: when the admission deadline ended it (the
    /// deadline has passed, or a bounded wait refused with
    /// [`admission_deadline::AdmissionDeadlineExhausted`]), return the typed
    /// refusal naming the phase that was running, with the original error as
    /// its cause. Any other failure, and a failure that already is a
    /// [`PreflightAdmissionRefused`], is returned unchanged.
    ///
    /// Call it on the worker thread before the [`InstallGuard`] drops, while the
    /// running phase is still recorded.
    pub fn refuse_failed_run(&self, err: anyhow::Error) -> anyhow::Error {
        if self.deadline.is_none() || err.downcast_ref::<PreflightAdmissionRefused>().is_some() {
            return err;
        }
        let wait_refused = err
            .chain()
            .any(|cause| cause.is::<admission_deadline::AdmissionDeadlineExhausted>());
        if !wait_refused && !self.deadline_passed() {
            return err;
        }
        let snapshot = self.snapshot();
        let point = match snapshot.running {
            Some((label, _)) => RefusalPoint::DuringPhase(label),
            None => RefusalPoint::DuringPhase("unattributed"),
        };
        anyhow::Error::new(self.refusal(point, snapshot.running, Some(format!("{err:#}"))))
    }

    fn finish(&self) {
        self.record.lock().close_current(Instant::now());
    }

    #[cfg(test)]
    fn enter_unchecked_for_test(&self, label: &'static str) {
        let now = Instant::now();
        let mut record = self.record.lock();
        record.close_current(now);
        record.current = Some((label, now));
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

/// Where the admission deadline stopped a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalPoint {
    /// The deadline had passed at the boundary before this phase; it never started.
    BeforePhase(&'static str),
    /// A bounded wait inside this phase was clamped to the deadline and ended it.
    DuringPhase(&'static str),
}

/// A preflight run the admission deadline stopped (`#preflightdeadline`).
///
/// Typed so the hook can surface it as the named, retryable refusal it is,
/// rather than as an ordinary preflight error or an abandoned worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightAdmissionRefused {
    pub point: RefusalPoint,
    /// The phase that ran last before the refusal, and how long it ran.
    pub previous: Option<(&'static str, Duration)>,
    /// Wall time since the run started.
    pub elapsed: Duration,
    /// When, after the run started, the deadline fell.
    pub deadline_after: Duration,
    /// The hook budget the deadline was derived from.
    pub budget: Duration,
    /// Finished phases, costliest first, as `label:Nms/Kx`.
    pub completed: String,
    /// For [`RefusalPoint::DuringPhase`], the error the clamped wait ended with.
    pub cause: Option<String>,
}

impl PreflightAdmissionRefused {
    /// The phase the refusal names.
    pub fn phase(&self) -> &'static str {
        match self.point {
            RefusalPoint::BeforePhase(label) | RefusalPoint::DuringPhase(label) => label,
        }
    }
}

impl std::fmt::Display for PreflightAdmissionRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let window = format!(
            "{}ms into the run; the admission deadline is {}s of the hook's {}s budget",
            self.elapsed.as_millis(),
            self.deadline_after.as_secs(),
            self.budget.as_secs()
        );
        match self.point {
            RefusalPoint::BeforePhase(phase) => {
                write!(
                    f,
                    "preflight admission deadline reached at the boundary before phase `{phase}` \
                     ({window}); `{phase}` was not started"
                )?;
                if let Some((last, took)) = self.previous {
                    write!(f, "; the last phase, `{last}`, took {}ms", took.as_millis())?;
                }
            }
            RefusalPoint::DuringPhase(phase) => {
                write!(
                    f,
                    "preflight admission deadline expired during phase `{phase}` ({window})"
                )?;
                if let Some((_, took)) = self.previous {
                    write!(f, " after {}ms in that phase", took.as_millis())?;
                }
                write!(f, "; its bounded wait was clamped to the deadline")?;
                if let Some(cause) = &self.cause {
                    write!(f, " and ended with: {cause}")?;
                }
            }
        }
        if !self.completed.is_empty() {
            write!(f, "; completed phases, costliest first: {}", self.completed)?;
        }
        write!(f, " (#preflightdeadline)")
    }
}

impl std::error::Error for PreflightAdmissionRefused {}

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
///
/// A handle carrying an admission deadline also installs it as this thread's
/// [`agent_doc_debounce::admission_deadline`], so every bounded wait beneath
/// preflight is clamped to it until the guard drops.
pub fn install(progress: PreflightProgress) -> InstallGuard {
    let deadline = progress.deadline.map(admission_deadline::install);
    INSTALLED.with(|slot| *slot.borrow_mut() = Some(progress));
    InstallGuard {
        _deadline: deadline,
    }
}

pub struct InstallGuard {
    /// Dropped after [`Drop::drop`] below runs, restoring the previous deadline.
    _deadline: Option<admission_deadline::DeadlineGuard>,
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
///
/// Refuses with [`PreflightAdmissionRefused`] (inside the `anyhow::Error`)
/// instead of starting `label` when
/// the installed run's admission deadline has passed, so preflight stops at a
/// step boundary. A no-op `Ok` with no handle installed.
pub fn enter(label: &'static str) -> anyhow::Result<()> {
    INSTALLED.with(|slot| match slot.borrow().as_ref() {
        Some(progress) => progress.enter(label),
        None => Ok(()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_a_phase_closes_the_previous_one() {
        let progress = PreflightProgress::new();
        let guard = install(progress.clone());
        enter("a").unwrap();
        std::thread::sleep(Duration::from_millis(2));
        enter("b").unwrap();
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
        enter("a").unwrap();
        enter("b").unwrap();
        enter("a").unwrap();
        enter("c").unwrap();
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
        let other = std::thread::spawn(|| enter("elsewhere").expect("no handle means no deadline"));
        other.join().unwrap();
        let snapshot = progress.snapshot();
        assert_eq!(snapshot.running, None);
        assert!(snapshot.completed.is_empty());
    }

    fn spent_deadline_progress() -> PreflightProgress {
        let mut progress = PreflightProgress::with_admission_deadline(Duration::from_secs(90));
        progress.deadline = Some(AdmissionDeadline {
            at: progress.started,
            budget: Duration::from_secs(90),
        });
        progress
    }

    #[test]
    fn the_deadline_is_the_budget_minus_its_margin() {
        let progress = PreflightProgress::with_admission_deadline(Duration::from_secs(90));
        let deadline = progress.deadline().unwrap();
        assert_eq!(deadline.budget, Duration::from_secs(90));
        assert_eq!(
            deadline.at.duration_since(progress.started),
            Duration::from_secs(85)
        );
        assert_eq!(
            admission_deadline_margin(Duration::from_secs(10)),
            Duration::from_millis(2500),
            "a short budget keeps most of its time"
        );
        assert!(!progress.deadline_passed());
        assert!(PreflightProgress::new().deadline().is_none());
    }

    /// `#preflightdeadline` (2): crossing the deadline at a phase boundary
    /// refuses with the typed refusal naming the phase, and that phase never
    /// starts.
    #[test]
    fn crossing_the_deadline_at_a_phase_boundary_refuses_by_name() {
        let progress = PreflightProgress::with_admission_deadline(Duration::from_millis(40));
        let _guard = install(progress.clone());
        enter("resolve_initial_document").expect("inside the deadline the phase starts");
        std::thread::sleep(Duration::from_millis(40));
        let err = enter("settle_debounce").expect_err("the deadline has passed");
        let refusal = err
            .downcast_ref::<PreflightAdmissionRefused>()
            .expect("typed refusal");
        assert_eq!(refusal.point, RefusalPoint::BeforePhase("settle_debounce"));
        assert_eq!(refusal.phase(), "settle_debounce");
        assert_eq!(
            refusal.previous.map(|(l, _)| l),
            Some("resolve_initial_document")
        );
        let message = refusal.to_string();
        assert!(
            message.contains("boundary before phase `settle_debounce`"),
            "{message}"
        );
        assert!(
            message.contains("the last phase, `resolve_initial_document`"),
            "{message}"
        );
        assert!(message.contains("resolve_initial_document:"), "{message}");
        let snapshot = progress.snapshot();
        assert_eq!(snapshot.running, None, "the refused phase never started");
    }

    /// `#preflightdeadline` (1): installing the run installs its deadline for
    /// every bounded wait on the thread, and the guard removes it.
    #[test]
    fn installing_the_run_clamps_bounded_waits_to_its_deadline() {
        assert_eq!(admission_deadline::remaining(), None);
        {
            let _guard = install(PreflightProgress::with_admission_deadline(
                Duration::from_secs(8),
            ));
            let clamped = admission_deadline::clamp(Duration::from_secs(120));
            assert!(clamped <= Duration::from_secs(6), "{clamped:?}");
        }
        assert_eq!(admission_deadline::remaining(), None);
        let _guard = install(PreflightProgress::new());
        assert_eq!(
            admission_deadline::remaining(),
            None,
            "a run without a deadline clamps nothing"
        );
    }

    #[test]
    fn a_wait_that_hit_the_deadline_becomes_a_refusal_naming_the_running_phase() {
        let progress = spent_deadline_progress();
        progress.enter_unchecked_for_test("pre_mutation_debounce");
        let err = progress.refuse_failed_run(anyhow::Error::new(
            admission_deadline::AdmissionDeadlineExhausted {
                wait: "preflight_visible_mutation_settle",
                budget: Duration::from_secs(90),
            },
        ));
        let refusal = err
            .downcast_ref::<PreflightAdmissionRefused>()
            .expect("typed refusal");
        assert_eq!(
            refusal.point,
            RefusalPoint::DuringPhase("pre_mutation_debounce")
        );
        assert!(
            refusal
                .to_string()
                .contains("preflight_visible_mutation_settle"),
            "the cause is kept: {refusal}"
        );
    }

    #[test]
    fn an_ordinary_failure_inside_the_deadline_keeps_its_own_error() {
        let progress = PreflightProgress::with_admission_deadline(Duration::from_secs(90));
        let err = progress.refuse_failed_run(anyhow::anyhow!("refused for its own reason"));
        assert!(err.downcast_ref::<PreflightAdmissionRefused>().is_none());
        assert_eq!(format!("{err:#}"), "refused for its own reason");
    }
}
