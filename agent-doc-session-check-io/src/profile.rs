//! Per-operation cost attribution for one `session-check` run
//! (`#sessioncheckprofile`).
//!
//! `inspect_core` already had four phase timers, but all four sit in its first
//! quarter. On a real project `session-check` took ~2.2s while
//! `session_check.initial_integrity_detection` reported ~507ms, leaving the
//! majority unattributed — which is how a plausible-but-wrong optimization gets
//! shipped. (One was: routing the state projection through the live controller
//! looked obvious, since the controller answers in 3ms where an in-process
//! replay of 8014 `state_events` takes 1860ms. It changed `session-check`
//! timing not at all, because the replay was not on this path.)
//!
//! Per-*phase* timing cannot close that gap here. The uninstrumented tail is a
//! branch tree that calls the same handful of expensive detectors from roughly
//! thirty sites; only one branch runs per invocation, so phase boundaries
//! attribute cost to whichever branch happened to be taken rather than to the
//! work itself. What needs attributing is cost **per operation**, summed across
//! every site that reached it.
//!
//! So this records `(calls, total duration)` per labelled operation for the
//! current run and prints the breakdown when the run was slow. Instrumenting a
//! detector at its definition covers all of its call sites at once.

use std::cell::RefCell;
use std::path::Path;
use std::time::{Duration, Instant};

/// Print the breakdown only when a run is slow enough to be worth reading.
const REPORT_THRESHOLD: Duration = Duration::from_millis(250);

thread_local! {
    /// When the current run started, so a report can be emitted from a path that
    /// never returns to the caller (see [`report_now`]).
    static STARTED: RefCell<Option<Instant>> = const { RefCell::new(None) };

    /// Insertion-ordered `(label, calls, total)` for the current run.
    ///
    /// A `Vec` rather than a map: the operation count is tiny and fixed, and
    /// preserving first-call order makes the report read in execution order.
    static SAMPLES: RefCell<Vec<(&'static str, u32, Duration)>> = const { RefCell::new(Vec::new()) };
}

/// Run `op`, recording its wall time under `label`.
///
/// Repeated calls accumulate, so a detector invoked from several branches
/// reports one honest total instead of N unrelated timings.
pub fn timed<T>(label: &'static str, op: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = op();
    let elapsed = started.elapsed();
    SAMPLES.with(|samples| {
        let mut samples = samples.borrow_mut();
        match samples.iter_mut().find(|(name, _, _)| *name == label) {
            Some((_, calls, total)) => {
                *calls += 1;
                *total += elapsed;
            }
            None => samples.push((label, 1, elapsed)),
        }
    });
    out
}

/// Drop any samples from a previous run on this thread and start the clock.
pub fn reset() {
    SAMPLES.with(|samples| samples.borrow_mut().clear());
    STARTED.with(|started| *started.borrow_mut() = Some(Instant::now()));
}

/// Emit the report for a code path that is about to `std::process::exit`.
///
/// `run_with_options_inner` exits the process directly from several branches, so
/// reporting only after it returns silently produced nothing for exactly the
/// interrupted runs worth profiling — and `process::exit` skips destructors, so
/// a scope guard cannot cover it either. Holding the start time here lets any
/// such branch report without threading a timer through it.
pub fn report_now(file: &Path) {
    let elapsed = STARTED.with(|started| started.borrow().map(|at| at.elapsed()));
    if let Some(elapsed) = elapsed {
        report(file, elapsed);
    }
}

/// Emit the per-operation breakdown for a run that took `total`.
///
/// Costliest first — the point of the report is to name the thing to fix.
///
/// **Labels nest**, so the attributed sum can exceed the wall time: a self-heal
/// phase contains the document resolutions it performs, and both are counted.
/// The sum is therefore reported as `attributed_ms` with an explicit
/// `nested=true` marker rather than as a residual, because subtracting it from
/// the total would invent an "unattributed" figure that is simply wrong. Read
/// each row as "time spent inside this operation", not as a share of a
/// partition.
pub fn report(file: &Path, total: Duration) {
    if total < REPORT_THRESHOLD {
        reset();
        return;
    }
    let mut samples = SAMPLES.with(|samples| samples.borrow().clone());
    reset();
    if samples.is_empty() {
        return;
    }
    samples.sort_by_key(|sample| std::cmp::Reverse(sample.2));
    let attributed: Duration = samples.iter().map(|(_, _, total)| *total).sum();
    let breakdown = samples
        .iter()
        .map(|(label, calls, total)| format!("{label}:{}ms/{calls}x", total.as_millis()))
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!(
        "[perf] session_check.operations file={} total_ms={} attributed_ms={} nested=true {}",
        file.display(),
        total.as_millis(),
        attributed.as_millis(),
        breakdown,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_calls_accumulate_into_one_total() {
        reset();
        for _ in 0..3 {
            timed("detector", || std::thread::sleep(Duration::from_millis(1)));
        }
        SAMPLES.with(|samples| {
            let samples = samples.borrow();
            assert_eq!(samples.len(), 1, "one label must yield one row");
            let (label, calls, total) = samples[0];
            assert_eq!(label, "detector");
            assert_eq!(calls, 3, "every call site must be counted");
            assert!(total >= Duration::from_millis(3));
        });
        reset();
    }

    #[test]
    fn distinct_labels_stay_separate_and_reset_clears() {
        reset();
        timed("a", || ());
        timed("b", || ());
        SAMPLES.with(|samples| assert_eq!(samples.borrow().len(), 2));
        reset();
        SAMPLES.with(|samples| {
            assert!(
                samples.borrow().is_empty(),
                "a run must not inherit the previous run's samples"
            )
        });
    }

    /// The value is returned untouched: instrumenting a detector must not change
    /// what it reports.
    #[test]
    fn timed_is_transparent_to_the_wrapped_value() {
        reset();
        let out: Result<Option<String>, ()> = timed("x", || Ok(Some("marker".to_string())));
        assert_eq!(out, Ok(Some("marker".to_string())));
        reset();
    }

    /// A fast run prints nothing and still clears, so the next slow run reports
    /// only its own work.
    #[test]
    fn a_fast_run_reports_nothing_but_still_resets() {
        reset();
        timed("quick", || ());
        report(Path::new("/tmp/doc.md"), Duration::from_millis(1));
        SAMPLES.with(|samples| assert!(samples.borrow().is_empty()));
    }
}
