//! Controller-owned per-document background maintenance (`#q-e9483473162d`).
//!
//! ## Spec
//! - The operator direction: hooks are quick operations and document
//!   reconciliation runs in the background. Preflight admission publishes the
//!   document's maintenance request (one latest row per document, claimed by
//!   generation, fenced by the admitted authority hash) and then only notifies
//!   the project controller, which runs the request on its own worker thread.
//!   A hook never creates a process while a controller serves the project.
//! - The worker body lives above this crate (it needs preflight, queue and
//!   commit code that depends on the controller), so the controller drives it
//!   through a [`DocumentMaintenanceRunner`]; production wires it to the
//!   `ProjectControllerRuntimeEffects` document-maintenance port.
//! - [`DocumentMaintenanceScheduler::enqueue`] reserves the document's
//!   maintenance ownership synchronously, so a session check that reads the
//!   request in the debounce window sees a live owner instead of an orphan,
//!   then returns. Each document has one long-lived worker thread: requests are
//!   coalesced (a notify while a run is pending or running folds into the next
//!   run) and debounced (a run starts after `debounce` of quiet, or `max_delay`
//!   after the first notify of a burst). The thread keeps the document's
//!   per-thread derived-state memos warm between runs and exits after
//!   `idle_exit` without work.
//! - The request row, its generation claim and its hash fence are unchanged:
//!   the runner consumes the latest request, and an operator edit after
//!   admission still supersedes it.
//! - dogfood `maintlockhold`: a run holds the document's maintenance claim (the
//!   queue apply flock and lease) for its compute and fenced writes, not for
//!   repeated whole-document reads. Each effect-fence crossing observes the
//!   authority hash through a revision-keyed memo
//!   (`agent_doc_document_realtime_io::authority_hash_memo`), so an unchanged
//!   canonical is never re-materialized under the relay hub lock; on a 963KB
//!   document those re-reads had stretched ~2 s of compute into 42-185 s
//!   claims. Each request logs `queue_maintenance_background_claim_held` with its
//!   hold and materialization counts.
//! - Operator-initiated mutations (editor commands such as `Run Agent Doc`,
//!   and `agent-doc queue`) enter `OperatorMutationWaitScope`: they wait
//!   behind a run whose lease heartbeat stays fresh (bounded by
//!   `OPERATOR_BACKGROUND_MAINTENANCE_WAIT`) and then apply on its resulting
//!   cut, instead of failing with `queue mutation deferred`.
//!
//! ## Agentic Contracts
//! - `enqueue` never runs maintenance inline and never waits for a run.
//! - A run that fails is retried a bounded number of times with backoff; the
//!   request stays pending (and recoverable by a closeout) whatever happens.
//! - A panic in a run is caught, logged, and releases the reservation.
//! - A token that was reserved and never run (spawn failure) is released.
//!
//! ## Evals
//! - `a_burst_of_notifies_coalesces_into_one_debounced_run`
//! - `a_notify_during_a_run_schedules_exactly_one_more_run`
//! - `documents_run_independently`
//! - `a_failed_run_is_retried_with_backoff_then_given_up`
//! - `a_panicking_run_releases_its_reservation`
//! - `an_unreserved_document_does_not_run`

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The work the scheduler runs for one document.
pub trait DocumentMaintenanceRunner: Send + Sync + 'static {
    /// Reserve the document's maintenance ownership. `Ok(None)` means there is
    /// nothing for this controller to run: no request is pending, or another
    /// live owner holds the reservation and will consume the latest request.
    fn reserve(&self, file: &Path) -> Result<Option<String>>;
    /// Run the pending request under the reservation `token`.
    fn run(&self, file: &Path, token: &str) -> Result<()>;
    /// Release `token` if it is still the document's reservation.
    fn release(&self, file: &Path, token: &str);
    fn log(&self, file: &Path, line: &str) {
        agent_doc_ops_log_io::log_op(file, line);
    }
    /// The prefix of this runner's scheduler log lines and worker thread name.
    fn label(&self) -> &'static str {
        "document_maintenance"
    }
}

/// Scheduling knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentMaintenanceTiming {
    /// Quiet period after the latest notify before a run starts.
    pub debounce: Duration,
    /// Longest a pending run waits behind a continuous burst of notifies.
    pub max_delay: Duration,
    /// A document worker without work exits after this long.
    pub idle_exit: Duration,
    /// First retry delay after a failed run; doubled per consecutive failure.
    pub retry_base: Duration,
    /// Consecutive failed runs before the scheduler stops retrying (the
    /// request stays pending for the next notify or closeout).
    pub max_retries: u32,
}

impl Default for DocumentMaintenanceTiming {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(150),
            max_delay: Duration::from_secs(1),
            idle_exit: Duration::from_secs(600),
            retry_base: Duration::from_secs(2),
            max_retries: 3,
        }
    }
}

/// What [`DocumentMaintenanceScheduler::enqueue`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentMaintenanceEnqueueOutcome {
    /// This notify reserved the document and scheduled a run.
    Scheduled,
    /// A run was already pending; this notify folds into it.
    CoalescedPending,
    /// A run is in progress; one more run follows it.
    CoalescedRunning,
    /// Nothing for this controller to run now (no pending request, or another
    /// live owner holds it). A run is still scheduled to re-check.
    Deferred,
}

impl DocumentMaintenanceEnqueueOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::CoalescedPending => "coalesced_pending",
            Self::CoalescedRunning => "coalesced_running",
            Self::Deferred => "deferred",
        }
    }
}

#[derive(Default)]
struct SlotState {
    token: Option<String>,
    first_pending: Option<Instant>,
    last_notify: Option<Instant>,
    not_before: Option<Instant>,
    running: bool,
    worker_alive: bool,
    failures: u32,
    notifies: u64,
}

struct Slot {
    file: PathBuf,
    state: Mutex<SlotState>,
    wake: Condvar,
}

/// The per-document maintenance scheduler of one controller process.
pub struct DocumentMaintenanceScheduler {
    runner: Arc<dyn DocumentMaintenanceRunner>,
    timing: DocumentMaintenanceTiming,
    slots: Mutex<HashMap<PathBuf, Arc<Slot>>>,
}

impl DocumentMaintenanceScheduler {
    pub fn new(
        runner: Arc<dyn DocumentMaintenanceRunner>,
        timing: DocumentMaintenanceTiming,
    ) -> Arc<Self> {
        Arc::new(Self {
            runner,
            timing,
            slots: Mutex::new(HashMap::new()),
        })
    }

    fn slot(&self, file: &Path) -> Arc<Slot> {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        Arc::clone(slots.entry(file.to_path_buf()).or_insert_with(|| {
            Arc::new(Slot {
                file: file.to_path_buf(),
                state: Mutex::new(SlotState::default()),
                wake: Condvar::new(),
            })
        }))
    }

    /// Record that `file` has a newer maintenance request and schedule its
    /// run. Reserves the document when no reservation or run is in flight.
    pub fn enqueue(self: &Arc<Self>, file: &Path) -> Result<DocumentMaintenanceEnqueueOutcome> {
        let slot = self.slot(file);
        let mut state = slot
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let now = Instant::now();
        let outcome = if state.running {
            DocumentMaintenanceEnqueueOutcome::CoalescedRunning
        } else if state.token.is_some() {
            DocumentMaintenanceEnqueueOutcome::CoalescedPending
        } else {
            match self.runner.reserve(&slot.file)? {
                Some(token) => {
                    state.token = Some(token);
                    DocumentMaintenanceEnqueueOutcome::Scheduled
                }
                None => DocumentMaintenanceEnqueueOutcome::Deferred,
            }
        };
        state.notifies += 1;
        state.last_notify = Some(now);
        state.first_pending.get_or_insert(now);
        // A fresh request is new work: it is not held behind the backoff of
        // an earlier failed run.
        state.not_before = None;
        state.failures = 0;
        if !state.worker_alive {
            let scheduler = Arc::clone(self);
            let worker_slot = Arc::clone(&slot);
            match std::thread::Builder::new()
                .name(format!(
                    "agent-doc-{}",
                    self.runner.label().replace('_', "-")
                ))
                .spawn(move || scheduler.run_worker(&worker_slot))
            {
                Ok(_) => state.worker_alive = true,
                Err(error) => {
                    if let Some(token) = state.token.take() {
                        self.runner.release(&slot.file, &token);
                    }
                    state.first_pending = None;
                    state.last_notify = None;
                    return Err(anyhow::anyhow!(
                        "failed to start the document maintenance worker: {error}"
                    ));
                }
            }
        }
        drop(state);
        slot.wake.notify_one();
        Ok(outcome)
    }

    fn due_at(&self, state: &SlotState) -> Option<Instant> {
        let first = state.first_pending?;
        let quiet = state.last_notify.unwrap_or(first) + self.timing.debounce;
        let due = quiet.min(first + self.timing.max_delay);
        Some(match state.not_before {
            Some(not_before) => due.max(not_before),
            None => due,
        })
    }

    fn run_worker(&self, slot: &Slot) {
        loop {
            let token = {
                let mut state = slot
                    .state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                loop {
                    match self.due_at(&state) {
                        None => {
                            let (next, timeout) = slot
                                .wake
                                .wait_timeout(state, self.timing.idle_exit)
                                .unwrap_or_else(|poison| poison.into_inner());
                            state = next;
                            if timeout.timed_out() && state.first_pending.is_none() {
                                state.worker_alive = false;
                                if let Some(token) = state.token.take() {
                                    self.runner.release(&slot.file, &token);
                                }
                                return;
                            }
                        }
                        Some(due) => {
                            let now = Instant::now();
                            if now >= due {
                                break;
                            }
                            let (next, _) = slot
                                .wake
                                .wait_timeout(state, due - now)
                                .unwrap_or_else(|poison| poison.into_inner());
                            state = next;
                        }
                    }
                }
                state.first_pending = None;
                state.last_notify = None;
                state.not_before = None;
                state.running = true;
                state.token.take()
            };
            let failed = self.run_once(slot, token);
            let mut state = slot
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.running = false;
            if failed {
                state.failures += 1;
                if state.failures <= self.timing.max_retries && state.first_pending.is_none() {
                    let delay = self.timing.retry_base * 2u32.saturating_pow(state.failures - 1);
                    let now = Instant::now();
                    state.first_pending = Some(now);
                    state.last_notify = Some(now);
                    state.not_before = Some(now + delay);
                    self.runner.log(
                        &slot.file,
                        &format!(
                            "{}_retry_scheduled file={} attempt={} delay_ms={} (#q-e9483473162d)",
                            self.runner.label(),
                            slot.file.display(),
                            state.failures,
                            delay.as_millis(),
                        ),
                    );
                }
            } else {
                state.failures = 0;
            }
        }
    }

    /// Run one pending request. Returns whether the run failed.
    fn run_once(&self, slot: &Slot, token: Option<String>) -> bool {
        let file = slot.file.as_path();
        let token = match token {
            Some(token) => token,
            None => match self.runner.reserve(file) {
                Ok(Some(token)) => token,
                Ok(None) => {
                    self.runner.log(
                        file,
                        &format!(
                            "{}_run_skipped file={} reason=no_reservation (#q-e9483473162d)",
                            self.runner.label(),
                            file.display(),
                        ),
                    );
                    return false;
                }
                Err(error) => {
                    self.runner.log(
                        file,
                        &format!(
                            "{}_reserve_failed file={} error={} (#q-e9483473162d)",
                            self.runner.label(),
                            file.display(),
                            format!("{error:#}").replace('\n', " | "),
                        ),
                    );
                    return true;
                }
            },
        };
        let started = Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.runner.run(file, &token)
        }));
        // The runner releases its own reservation; this covers a panic or an
        // early return that left it behind.
        self.runner.release(file, &token);
        let (failed, detail) = match outcome {
            Ok(Ok(())) => (false, "outcome=ok".to_string()),
            Ok(Err(error)) => (
                true,
                format!(
                    "outcome=error error={}",
                    format!("{error:#}").replace('\n', " | ")
                ),
            ),
            Err(payload) => (
                true,
                format!(
                    "outcome=panic panic={}",
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "non-string panic payload".to_string())
                        .replace('\n', " | ")
                ),
            ),
        };
        self.runner.log(
            file,
            &format!(
                "{}_run_finished file={} elapsed_ms={} {detail} (#q-e9483473162d)",
                self.runner.label(),
                file.display(),
                started.elapsed().as_millis(),
            ),
        );
        failed
    }

    /// Notifies this scheduler accepted for `file` (diagnostics and tests).
    pub fn notifies(&self, file: &Path) -> u64 {
        self.slot(file)
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .notifies
    }
}

/// The production runner: the binary's document-maintenance port.
struct RuntimeEffectsRunner;

impl DocumentMaintenanceRunner for RuntimeEffectsRunner {
    fn reserve(&self, file: &Path) -> Result<Option<String>> {
        crate::project_controller::runtime_effects()?.reserve_document_maintenance(file)
    }

    fn run(&self, file: &Path, token: &str) -> Result<()> {
        crate::project_controller::runtime_effects()?.run_document_maintenance(file, token)
    }

    fn release(&self, file: &Path, token: &str) {
        if let Ok(effects) = crate::project_controller::runtime_effects() {
            effects.release_document_maintenance(file, token);
        }
    }
}

static GLOBAL_SCHEDULER: OnceLock<Arc<DocumentMaintenanceScheduler>> = OnceLock::new();

/// This controller process's scheduler. A controller serves one project root,
/// but the scheduler is keyed by document path, so a process hosting several
/// in-process controllers (tests) shares it safely.
pub fn global() -> &'static Arc<DocumentMaintenanceScheduler> {
    GLOBAL_SCHEDULER.get_or_init(|| {
        DocumentMaintenanceScheduler::new(
            Arc::new(RuntimeEffectsRunner),
            DocumentMaintenanceTiming::default(),
        )
    })
}

/// `#q-c71d106debc4`: the closeout-time queue-continuation reconcile, run on the same
/// per-document worker machinery. The committing process publishes a pending
/// reconcile fact and notifies; `reserve` answers its generation and `run`
/// consumes it through the binary's continuation-reconcile port. There is no
/// lease to release: the fact is consumed by generation.
struct ContinuationReconcileRunner;

impl DocumentMaintenanceRunner for ContinuationReconcileRunner {
    fn reserve(&self, file: &Path) -> Result<Option<String>> {
        crate::project_controller::runtime_effects()?.pending_continuation_reconcile(file)
    }

    fn run(&self, file: &Path, _token: &str) -> Result<()> {
        crate::project_controller::runtime_effects()?.run_continuation_reconcile(file)
    }

    fn release(&self, _file: &Path, _token: &str) {}

    fn label(&self) -> &'static str {
        "continuation_reconcile"
    }
}

/// Continuation reconciles answer a Stop hook that may be waiting on them, so
/// they start almost at once; a burst of closeouts still folds into one run.
const CONTINUATION_RECONCILE_TIMING: DocumentMaintenanceTiming = DocumentMaintenanceTiming {
    debounce: Duration::from_millis(10),
    max_delay: Duration::from_millis(100),
    idle_exit: Duration::from_secs(600),
    retry_base: Duration::from_millis(500),
    max_retries: 2,
};

static CONTINUATION_RECONCILE_SCHEDULER: OnceLock<Arc<DocumentMaintenanceScheduler>> =
    OnceLock::new();

/// This controller process's continuation-reconcile scheduler (`#q-c71d106debc4`).
pub fn continuation_reconcile() -> &'static Arc<DocumentMaintenanceScheduler> {
    CONTINUATION_RECONCILE_SCHEDULER.get_or_init(|| {
        DocumentMaintenanceScheduler::new(
            Arc::new(ContinuationReconcileRunner),
            CONTINUATION_RECONCILE_TIMING,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    /// A runner over one real reservation slot per document: `reserve` hands
    /// out a token only while none is held, exactly like the lease row, and
    /// `run` records the run and releases.
    #[derive(Default)]
    struct LedgerRunner {
        held: Mutex<HashMap<PathBuf, String>>,
        /// `(published, consumed)` request generations per document, like
        /// the request row: completing a run consumes only the generation it
        /// started with, so a request published during the run stays pending.
        generations: Mutex<HashMap<PathBuf, (u64, u64)>>,
        runs: Mutex<Vec<PathBuf>>,
        sequence: AtomicU64,
        run_delay: Mutex<Duration>,
        fail_runs: AtomicUsize,
        panic_runs: AtomicUsize,
        reservations: AtomicUsize,
    }

    impl LedgerRunner {
        fn publish(&self, file: &Path) {
            self.generations
                .lock()
                .unwrap()
                .entry(file.to_path_buf())
                .or_default()
                .0 += 1;
        }

        fn pending(&self, file: &Path) -> bool {
            self.generations
                .lock()
                .unwrap()
                .get(file)
                .is_some_and(|(published, consumed)| published > consumed)
        }

        fn runs_for(&self, file: &Path) -> usize {
            self.runs
                .lock()
                .unwrap()
                .iter()
                .filter(|run| *run == file)
                .count()
        }
    }

    impl DocumentMaintenanceRunner for LedgerRunner {
        fn reserve(&self, file: &Path) -> Result<Option<String>> {
            if !self.pending(file) {
                return Ok(None);
            }
            let mut held = self.held.lock().unwrap();
            if held.contains_key(file) {
                return Ok(None);
            }
            let token = format!("t{}", self.sequence.fetch_add(1, Ordering::SeqCst));
            held.insert(file.to_path_buf(), token.clone());
            self.reservations.fetch_add(1, Ordering::SeqCst);
            Ok(Some(token))
        }

        fn run(&self, file: &Path, token: &str) -> Result<()> {
            assert_eq!(
                self.held.lock().unwrap().get(file).map(String::as_str),
                Some(token),
                "a run must hold its document's reservation"
            );
            let started_generation = self
                .generations
                .lock()
                .unwrap()
                .get(file)
                .map(|(published, _)| *published)
                .unwrap_or_default();
            let delay = *self.run_delay.lock().unwrap();
            std::thread::sleep(delay);
            self.runs.lock().unwrap().push(file.to_path_buf());
            if self
                .panic_runs
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
            {
                panic!("injected maintenance panic");
            }
            if self
                .fail_runs
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
            {
                anyhow::bail!("injected maintenance failure");
            }
            // A completed run consumes the generation it started with.
            if let Some(entry) = self.generations.lock().unwrap().get_mut(file) {
                entry.1 = entry.1.max(started_generation);
            }
            self.held.lock().unwrap().remove(file);
            Ok(())
        }

        fn release(&self, file: &Path, token: &str) {
            let mut held = self.held.lock().unwrap();
            if held.get(file).map(String::as_str) == Some(token) {
                held.remove(file);
            }
        }

        fn log(&self, _file: &Path, _line: &str) {}
    }

    fn timing() -> DocumentMaintenanceTiming {
        DocumentMaintenanceTiming {
            debounce: Duration::from_millis(60),
            max_delay: Duration::from_millis(400),
            idle_exit: Duration::from_secs(5),
            retry_base: Duration::from_millis(40),
            max_retries: 2,
        }
    }

    fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_burst_of_notifies_coalesces_into_one_debounced_run() {
        let runner = Arc::new(LedgerRunner::default());
        let scheduler = DocumentMaintenanceScheduler::new(runner.clone(), timing());
        let doc = PathBuf::from("/project/a.md");
        runner.publish(&doc);
        let first = scheduler.enqueue(&doc).unwrap();
        assert_eq!(first, DocumentMaintenanceEnqueueOutcome::Scheduled);
        for _ in 0..4 {
            runner.publish(&doc);
            assert_eq!(
                scheduler.enqueue(&doc).unwrap(),
                DocumentMaintenanceEnqueueOutcome::CoalescedPending
            );
        }
        // Nothing runs inside the quiet window.
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(runner.runs_for(&doc), 0, "enqueue must never run inline");
        wait_until("the debounced run", || runner.runs_for(&doc) == 1);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(runner.runs_for(&doc), 1, "the burst ran exactly once");
        assert_eq!(runner.reservations.load(Ordering::SeqCst), 1);
        assert_eq!(scheduler.notifies(&doc), 5);
    }

    #[test]
    fn a_notify_during_a_run_schedules_exactly_one_more_run() {
        let runner = Arc::new(LedgerRunner::default());
        *runner.run_delay.lock().unwrap() = Duration::from_millis(150);
        let scheduler = DocumentMaintenanceScheduler::new(runner.clone(), timing());
        let doc = PathBuf::from("/project/a.md");
        runner.publish(&doc);
        scheduler.enqueue(&doc).unwrap();
        wait_until("the first run to start", || {
            scheduler.slot(&doc).state.lock().unwrap().running
        });
        runner.publish(&doc);
        assert_eq!(
            scheduler.enqueue(&doc).unwrap(),
            DocumentMaintenanceEnqueueOutcome::CoalescedRunning
        );
        runner.publish(&doc);
        scheduler.enqueue(&doc).unwrap();
        wait_until("the follow-up run", || runner.runs_for(&doc) == 2);
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(runner.runs_for(&doc), 2);
    }

    #[test]
    fn documents_run_independently() {
        let runner = Arc::new(LedgerRunner::default());
        *runner.run_delay.lock().unwrap() = Duration::from_millis(300);
        let scheduler = DocumentMaintenanceScheduler::new(runner.clone(), timing());
        let slow = PathBuf::from("/project/slow.md");
        let fast = PathBuf::from("/project/fast.md");
        runner.publish(&slow);
        scheduler.enqueue(&slow).unwrap();
        wait_until("the slow run to start", || {
            scheduler.slot(&slow).state.lock().unwrap().running
        });
        *runner.run_delay.lock().unwrap() = Duration::ZERO;
        runner.publish(&fast);
        scheduler.enqueue(&fast).unwrap();
        wait_until("the fast document's run", || runner.runs_for(&fast) == 1);
        assert_eq!(
            runner.runs_for(&slow),
            0,
            "a slow document must not hold back another document"
        );
        wait_until("the slow run", || runner.runs_for(&slow) == 1);
    }

    #[test]
    fn a_failed_run_is_retried_with_backoff_then_given_up() {
        let runner = Arc::new(LedgerRunner::default());
        runner.fail_runs.store(10, Ordering::SeqCst);
        let scheduler = DocumentMaintenanceScheduler::new(runner.clone(), timing());
        let doc = PathBuf::from("/project/a.md");
        runner.publish(&doc);
        scheduler.enqueue(&doc).unwrap();
        // One run plus `max_retries` retries.
        wait_until("every retry", || runner.runs_for(&doc) == 3);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(runner.runs_for(&doc), 3, "retries are bounded");
        assert!(
            runner.held.lock().unwrap().is_empty(),
            "a failed run must not keep its reservation"
        );
        // A fresh notify is new work and runs again.
        runner.fail_runs.store(0, Ordering::SeqCst);
        runner.publish(&doc);
        scheduler.enqueue(&doc).unwrap();
        wait_until("the fresh run", || runner.runs_for(&doc) == 4);
    }

    #[test]
    fn a_panicking_run_releases_its_reservation() {
        let runner = Arc::new(LedgerRunner::default());
        runner.panic_runs.store(1, Ordering::SeqCst);
        let scheduler = DocumentMaintenanceScheduler::new(runner.clone(), timing());
        let doc = PathBuf::from("/project/a.md");
        runner.publish(&doc);
        scheduler.enqueue(&doc).unwrap();
        // The panic is a failure; its retry completes the request.
        wait_until("the retry after the panic", || runner.runs_for(&doc) == 2);
        wait_until("the reservation to clear", || {
            runner.held.lock().unwrap().is_empty()
        });
        assert!(!runner.pending(&doc));
    }

    #[test]
    fn an_unreserved_document_does_not_run() {
        let runner = Arc::new(LedgerRunner::default());
        let scheduler = DocumentMaintenanceScheduler::new(runner.clone(), timing());
        let doc = PathBuf::from("/project/a.md");
        // Another owner (a detached worker or an adopting closeout) holds it.
        runner.publish(&doc);
        runner
            .held
            .lock()
            .unwrap()
            .insert(doc.clone(), "other-owner".to_string());
        assert_eq!(
            scheduler.enqueue(&doc).unwrap(),
            DocumentMaintenanceEnqueueOutcome::Deferred
        );
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(runner.runs_for(&doc), 0);
        assert_eq!(
            runner.held.lock().unwrap().get(&doc).map(String::as_str),
            Some("other-owner"),
            "the scheduler never displaces a live owner"
        );
    }
}
