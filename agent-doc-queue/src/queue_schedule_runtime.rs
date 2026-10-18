//! The controller's single queue-schedule loop, as a Lazily graph plus an
//! injected clock.
//!
//! One [`QueueScheduleGraph`] per controller process holds every scheduled
//! `(document, queue_id)` schedule. Its inputs are three Lazily `Source` maps
//! (the parsed [`QueueScheduleSpec`] per schedule, the durable
//! [`QueueScheduleState`] per schedule, and which schedules have a tick in
//! flight); one `Computed`
//! derives the earliest wake across all of them; one `Effect` on that
//! `Computed` bumps the [`ScheduleWaker`] whenever the earliest wake changes.
//! That Effect is the realtime path: an operator edit of `interval=`/`time=`
//! sets the spec Source, the earliest wake recomputes, and the parked loop
//! wakes and re-parks against the new deadline — no polling.
//!
//! [`run_scheduler_loop`] is the one loop. Each turn it takes every due
//! schedule (marking it in flight, so it drops out of the earliest wake until
//! its worker reports back), hands each to `dispatch` — the caller's background
//! worker, never work on this thread — then parks until the earliest wake, a
//! graph change, or [`MAX_SCHEDULER_PARK_SECS`]. The park is the only wait, so:
//!
//! - wakeups track deadlines, not schedules (one wake serves every schedule
//!   due at that instant);
//! - a due schedule already in flight cannot spin the loop, because in-flight
//!   schedules are excluded from the earliest wake;
//! - a wake that dispatched nothing parks at least [`MIN_SCHEDULER_IDLE_PARK_SECS`].
//!
//! No IO happens here: the caller supplies the clock, the zone, and the
//! dispatch sink. Tests drive it with a virtual clock.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lazily::{Computed, Effect, ThreadSafeContext, ThreadSafeSourceMap};
use parking_lot::{Condvar, Mutex};

use crate::queue_schedule::{LocalOffset, QueueScheduleSpec, QueueScheduleState, wake_deadline};

/// Longest single park. Bounds how long a stopped controller's loop lingers
/// and how late a tick can be after the host suspends (a monotonic timed wait
/// does not advance while the machine sleeps).
pub const MAX_SCHEDULER_PARK_SECS: u64 = 300;
/// Shortest park after a wake that dispatched nothing.
pub const MIN_SCHEDULER_IDLE_PARK_SECS: u64 = 1;

/// One `(document, queue_id)` schedule as the graph sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledDocument {
    pub file: PathBuf,
    pub queue_id: String,
    pub spec: QueueScheduleSpec,
}

/// A `(document, queue_id)` schedule whose tick is due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueSchedule {
    pub key: String,
    pub file: PathBuf,
    pub queue_id: String,
}

/// Wakes the parked loop. `generation` changes on every notify, so a notify
/// that lands between the loop's read of the graph and its park is never lost.
#[derive(Default)]
pub struct ScheduleWaker {
    generation: Mutex<u64>,
    condvar: Condvar,
}

impl ScheduleWaker {
    pub fn generation(&self) -> u64 {
        *self.generation.lock()
    }

    pub fn notify(&self) {
        let mut generation = self.generation.lock();
        *generation = generation.wrapping_add(1);
        self.condvar.notify_all();
    }

    /// Wait until `generation` moves past `observed`, or `timeout` elapses.
    pub fn wait(&self, observed: u64, timeout: std::time::Duration) {
        let mut generation = self.generation.lock();
        if *generation != observed {
            return;
        }
        let _ = self.condvar.wait_for(&mut generation, timeout);
    }
}

/// Time source for the loop. Production reads the system clock and parks on
/// the waker; tests advance virtual time.
pub trait ScheduleClock: Send + Sync {
    /// Unix epoch seconds.
    fn now_secs(&self) -> u64;
    /// Park until `until` (epoch seconds), a waker notify past `observed`, or
    /// `max_park` — whichever comes first.
    fn park(&self, waker: &ScheduleWaker, observed: u64, until: Option<u64>, max_park: u64);
}

/// The system clock.
pub struct SystemScheduleClock;

impl ScheduleClock for SystemScheduleClock {
    fn now_secs(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default()
    }

    fn park(&self, waker: &ScheduleWaker, observed: u64, until: Option<u64>, max_park: u64) {
        let now = self.now_secs();
        let secs = until.map_or(max_park, |until| until.saturating_sub(now).min(max_park));
        waker.wait(observed, std::time::Duration::from_secs(secs));
    }
}

/// The Lazily graph of every `(document, queue_id)` schedule in one controller.
pub struct QueueScheduleGraph {
    ctx: ThreadSafeContext,
    documents: ThreadSafeSourceMap<String, Option<ScheduledDocument>>,
    states: ThreadSafeSourceMap<String, Option<QueueScheduleState>>,
    in_flight: ThreadSafeSourceMap<String, bool>,
    earliest: Computed<Option<u64>>,
    wake_effect: Effect,
    waker: Arc<ScheduleWaker>,
    tz: Arc<dyn LocalOffset + Send + Sync>,
    /// The second each schedule was last dispatched. A schedule is dispatched
    /// at most once per second even if a worker reports back a state that is
    /// still due, so no worker bug can turn the loop into a busy loop.
    dispatched_at: Mutex<std::collections::HashMap<String, u64>>,
}

impl QueueScheduleGraph {
    /// Build the graph joined to `scope`'s graph (`#stategraphjoin`). The
    /// controller passes its process scope: the scheduler lives as long as the
    /// controller process.
    pub fn new_in(
        scope: &agent_doc_state_scope::ProcessScope,
        tz: Arc<dyn LocalOffset + Send + Sync>,
    ) -> Self {
        Self::build(scope.ctx().clone(), tz)
    }

    /// Standalone graph in a private context — a pure helper for unit tests
    /// only. A long-lived owner must use [`Self::new_in`].
    pub fn new(tz: Arc<dyn LocalOffset + Send + Sync>) -> Self {
        // #stategraphjoin-allow: standalone pure helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        Self::build(ThreadSafeContext::new(), tz)
    }

    fn build(ctx: ThreadSafeContext, tz: Arc<dyn LocalOffset + Send + Sync>) -> Self {
        let documents = ThreadSafeSourceMap::<String, Option<ScheduledDocument>>::new(&ctx);
        let states = ThreadSafeSourceMap::<String, Option<QueueScheduleState>>::new(&ctx);
        let in_flight = ThreadSafeSourceMap::<String, bool>::new(&ctx);
        let earliest = {
            let documents = documents.clone();
            let states = states.clone();
            let in_flight = in_flight.clone();
            let tz = Arc::clone(&tz);
            ctx.computed(move |ctx| {
                documents
                    .keys(ctx)
                    .iter()
                    .filter_map(|key| {
                        document_wake(&documents, &states, &in_flight, tz.as_ref(), ctx, key)
                    })
                    .min()
            })
        };
        let waker = Arc::new(ScheduleWaker::default());
        let wake_effect = {
            let waker = Arc::clone(&waker);
            ctx.effect(move |ctx| {
                let _earliest = ctx.get(&earliest);
                waker.notify();
            })
        };
        Self {
            ctx,
            documents,
            states,
            in_flight,
            earliest,
            wake_effect,
            waker,
            tz,
            dispatched_at: Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn waker(&self) -> &Arc<ScheduleWaker> {
        &self.waker
    }

    pub fn tz(&self) -> &(dyn LocalOffset + Send + Sync) {
        self.tz.as_ref()
    }

    /// Mint every per-schedule cell before the schedule becomes visible.
    ///
    /// A non-minting `observe` of an absent key is not a dependency, so a cell
    /// minted *after* the earliest-wake `Computed` read it would never
    /// invalidate that `Computed`. Every key therefore gets all three cells up
    /// front.
    fn ensure_cells(&self, ctx: &ThreadSafeContext, key: &str) {
        let key = key.to_string();
        if !self.states.contains_key(ctx, &key) {
            self.states.set(ctx, key.clone(), None);
        }
        if !self.in_flight.contains_key(ctx, &key) {
            self.in_flight.set(ctx, key, false);
        }
    }

    /// Publish a document's schedule (`None` unschedules it). Equal specs are
    /// a no-op: Lazily suppresses the downstream recompute.
    pub fn set_document(&self, key: &str, document: Option<ScheduledDocument>) {
        if document.is_none() && !self.documents.contains_key(&self.ctx, &key.to_string()) {
            return;
        }
        self.ctx.batch(|ctx| {
            self.ensure_cells(ctx, key);
            self.documents.set(ctx, key.to_string(), document);
        });
    }

    /// Publish a document's durable state as last committed.
    pub fn set_state(&self, key: &str, state: Option<QueueScheduleState>) {
        self.ctx.batch(|ctx| {
            self.ensure_cells(ctx, key);
            self.states.set(ctx, key.to_string(), state);
        });
    }

    /// The worker finished a tick: publish its state and release the in-flight
    /// mark in one batch, so the earliest wake recomputes once.
    pub fn complete(&self, key: &str, state: Option<QueueScheduleState>) {
        self.ctx.batch(|ctx| {
            self.states.set(ctx, key.to_string(), state);
            self.in_flight.set(ctx, key.to_string(), false);
        });
    }

    /// Release an in-flight mark without changing state (the worker could not
    /// even load the document).
    pub fn release(&self, key: &str) {
        self.in_flight.set(&self.ctx, key.to_string(), false);
    }

    pub fn document(&self, key: &str) -> Option<ScheduledDocument> {
        self.documents
            .observe(&self.ctx, &key.to_string())
            .flatten()
    }

    pub fn state(&self, key: &str) -> Option<QueueScheduleState> {
        self.states.observe(&self.ctx, &key.to_string()).flatten()
    }

    pub fn scheduled_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .documents
            .keys(&self.ctx)
            .into_iter()
            .filter(|key| self.document(key).is_some())
            .collect();
        keys.sort();
        keys
    }

    /// The earliest wake across every document not in flight.
    pub fn earliest_wake(&self) -> Option<u64> {
        self.ctx.get(&self.earliest)
    }

    /// Every document due at `now`, marked in flight.
    pub fn take_due(&self, now: u64) -> Vec<DueSchedule> {
        let mut dispatched_at = self.dispatched_at.lock();
        let mut due: Vec<DueSchedule> = self
            .documents
            .keys(&self.ctx)
            .into_iter()
            .filter_map(|key| {
                let wake = document_wake(
                    &self.documents,
                    &self.states,
                    &self.in_flight,
                    self.tz.as_ref(),
                    &self.ctx,
                    &key,
                )?;
                if wake > now || dispatched_at.get(&key) == Some(&now) {
                    return None;
                }
                let document = self.document(&key)?;
                Some(DueSchedule {
                    key,
                    file: document.file,
                    queue_id: document.queue_id,
                })
            })
            .collect();
        due.sort_by(|a, b| a.key.cmp(&b.key));
        for item in &due {
            dispatched_at.insert(item.key.clone(), now);
        }
        drop(dispatched_at);
        if !due.is_empty() {
            self.ctx.batch(|ctx| {
                for item in &due {
                    self.in_flight.set(ctx, item.key.clone(), true);
                }
            });
        }
        due
    }
}

impl Drop for QueueScheduleGraph {
    fn drop(&mut self) {
        self.ctx.dispose_effect(&self.wake_effect);
    }
}

fn document_wake(
    documents: &ThreadSafeSourceMap<String, Option<ScheduledDocument>>,
    states: &ThreadSafeSourceMap<String, Option<QueueScheduleState>>,
    in_flight: &ThreadSafeSourceMap<String, bool>,
    tz: &dyn LocalOffset,
    ctx: &ThreadSafeContext,
    key: &String,
) -> Option<u64> {
    let document = documents.observe(ctx, key).flatten()?;
    if in_flight.observe(ctx, key).unwrap_or(false) {
        return None;
    }
    match states.observe(ctx, key).flatten() {
        // Not admitted yet: the worker creates the durable row at once.
        None => Some(0),
        Some(state) => wake_deadline(&state, &document.spec, tz),
    }
}

/// Counters a loop run reports (also the CPU guard tests' instrument).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SchedulerLoopStats {
    /// Times the loop woke and evaluated the graph.
    pub wakeups: u64,
    /// Wakeups that dispatched at least one document.
    pub productive_wakeups: u64,
    /// Documents dispatched.
    pub dispatched: u64,
}

/// Run the one scheduler loop until `should_stop`.
///
/// `dispatch` must only enqueue (the background worker does the work and
/// reports back through [`QueueScheduleGraph::complete`]).
pub fn run_scheduler_loop(
    graph: &QueueScheduleGraph,
    clock: &dyn ScheduleClock,
    should_stop: &AtomicBool,
    mut dispatch: impl FnMut(DueSchedule),
) -> SchedulerLoopStats {
    let mut stats = SchedulerLoopStats::default();
    while !should_stop.load(Ordering::SeqCst) {
        let observed = graph.waker().generation();
        let now = clock.now_secs();
        let due = graph.take_due(now);
        stats.wakeups += 1;
        if !due.is_empty() {
            stats.productive_wakeups += 1;
        }
        for item in due.iter().cloned() {
            stats.dispatched += 1;
            dispatch(item);
        }
        if should_stop.load(Ordering::SeqCst) {
            break;
        }
        // In-flight documents are excluded from the earliest wake, so a wake at
        // or before `now` here is a document that became due (or was reported
        // back still due) after `take_due`; take it next turn without
        // sleeping. That turn cannot redispatch a document dispatched this
        // second, so it either dispatches something new or parks at the idle
        // floor: a wake that dispatched nothing never re-runs sooner.
        let floor = if due.is_empty() {
            now.saturating_add(MIN_SCHEDULER_IDLE_PARK_SECS)
        } else {
            now
        };
        let until = graph.earliest_wake().map(|wake| wake.max(floor));
        if until.is_some_and(|until| until <= now) {
            continue;
        }
        clock.park(graph.waker(), observed, until, MAX_SCHEDULER_PARK_SECS);
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_schedule::{
        FireFacts, FixedOffset, QueueScheduleSpec, fire, observe, queue_schedule_spec_for_attrs,
        reconcile,
    };
    use std::collections::HashMap;

    fn spec(pairs: &[(&str, &str)]) -> QueueScheduleSpec {
        let attrs: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        queue_schedule_spec_for_attrs(&attrs).0.unwrap()
    }

    fn graph() -> QueueScheduleGraph {
        QueueScheduleGraph::new(Arc::new(FixedOffset(0)))
    }

    fn document(name: &str, spec: QueueScheduleSpec) -> ScheduledDocument {
        ScheduledDocument {
            file: PathBuf::from(format!("/project/{name}.md")),
            queue_id: "default".to_string(),
            spec,
        }
    }

    /// Virtual time: a park jumps straight to its deadline (or the max park),
    /// and the run stops once virtual time passes `horizon`.
    struct VirtualClock {
        now: Mutex<u64>,
        horizon: u64,
        stop: Arc<AtomicBool>,
        parks: Mutex<Vec<u64>>,
    }

    impl VirtualClock {
        fn new(start: u64, horizon: u64, stop: Arc<AtomicBool>) -> Self {
            Self {
                now: Mutex::new(start),
                horizon,
                stop,
                parks: Mutex::new(Vec::new()),
            }
        }
    }

    impl ScheduleClock for VirtualClock {
        fn now_secs(&self) -> u64 {
            *self.now.lock()
        }

        fn park(&self, _waker: &ScheduleWaker, _observed: u64, until: Option<u64>, max_park: u64) {
            let mut now = self.now.lock();
            let target = until.unwrap_or(*now + max_park).min(*now + max_park);
            assert!(target > *now, "a park must advance time (no spin)");
            *now = target;
            self.parks.lock().push(target);
            if *now >= self.horizon {
                self.stop.store(true, Ordering::SeqCst);
            }
        }
    }

    /// The worker, inline: fire the tick, then drain the pass at once.
    fn work(graph: &QueueScheduleGraph, now: u64, due: &DueSchedule) {
        let document = graph.document(&due.key).unwrap();
        let state = reconcile(graph.state(&due.key), &document.spec, now);
        let facts = FireFacts {
            live_heads: vec!["check".to_string()],
            paused: false,
        };
        let (state, _) = fire(state, &document.spec, now, &facts, graph.tz());
        let (state, _) = observe(state, &document.spec, &[], now);
        let state = crate::queue_schedule::restore_applied(state);
        graph.complete(&due.key, Some(state));
    }

    #[test]
    fn cpu_guard_wakeups_track_distinct_deadlines_not_documents() {
        // 60 queues across four cadences, one virtual day. A thrashing loop
        // (per-queue timers, busy polling, catch-up bursts) would wake orders
        // of magnitude more than the distinct deadlines below.
        let graph = graph();
        let cadences = [("10m", 600u64), ("15m", 900), ("1h", 3_600), ("4h", 14_400)];
        for index in 0..60 {
            let (raw, _) = cadences[index % cadences.len()];
            graph.set_document(
                &format!("doc{index}"),
                Some(document(&format!("doc{index}"), spec(&[("interval", raw)]))),
            );
        }
        let stop = Arc::new(AtomicBool::new(false));
        let day = 86_400;
        let clock = VirtualClock::new(0, day, Arc::clone(&stop));
        let mut dispatched = Vec::new();
        let stats = run_scheduler_loop(&graph, &clock, &stop, |due| dispatched.push(due));
        // Dispatch is asynchronous in production; here the worker runs between
        // wakes, which the loop above cannot observe, so re-run with an inline
        // worker for the real count.
        assert_eq!(stats.dispatched, 60, "every queue is admitted once at t=0");

        let graph = graph_with_states(60, &cadences);
        let stop = Arc::new(AtomicBool::new(false));
        let clock = VirtualClock::new(0, day, Arc::clone(&stop));
        let clock_ref = &clock;
        let graph_ref = &graph;
        let stats = run_scheduler_loop(graph_ref, clock_ref, &stop, |due| {
            work(graph_ref, clock_ref.now_secs(), &due);
        });
        // Distinct deadlines in a day: every multiple of 10m and 15m (the 1h
        // and 4h ticks coincide with them) = 144 + 96 - 48 = 192, plus t=0.
        let distinct_deadlines = 193;
        assert!(
            stats.productive_wakeups <= distinct_deadlines,
            "productive wakeups {} exceed distinct deadlines {distinct_deadlines}",
            stats.productive_wakeups
        );
        // The only other wakes are the bounded heartbeat of the max park.
        let heartbeats = day / MAX_SCHEDULER_PARK_SECS;
        assert!(
            stats.wakeups - stats.productive_wakeups <= heartbeats + 2,
            "the loop woke {} times ({} productive) for {distinct_deadlines} deadlines",
            stats.wakeups,
            stats.productive_wakeups
        );
        let expected_ticks: u64 = (0..60)
            .map(|index| day / cadences[index % cadences.len()].1)
            .sum::<u64>()
            + 60;
        assert!(
            stats.dispatched.abs_diff(expected_ticks) <= 60,
            "dispatched {} ticks, expected about {expected_ticks}",
            stats.dispatched
        );
    }

    fn graph_with_states(count: usize, cadences: &[(&str, u64)]) -> QueueScheduleGraph {
        let graph = graph();
        for index in 0..count {
            let (raw, _) = cadences[index % cadences.len()];
            let key = format!("doc{index}");
            let spec = spec(&[("interval", raw)]);
            graph.set_state(&key, Some(reconcile(None, &spec, 0)));
            graph.set_document(&key, Some(document(&key, spec)));
        }
        graph
    }

    #[test]
    fn a_clock_jump_after_suspend_coalesces_into_one_tick_per_document() {
        // The host sleeps for ten hours between two parks. Sixty 10m ticks per
        // document were missed; the loop must fire each document ONCE on wake
        // (the latest missed deadline), then resume the normal cadence.
        struct JumpClock {
            now: Mutex<u64>,
            jumped: AtomicBool,
            stop: Arc<AtomicBool>,
            horizon: u64,
        }
        impl ScheduleClock for JumpClock {
            fn now_secs(&self) -> u64 {
                *self.now.lock()
            }
            fn park(&self, _: &ScheduleWaker, _: u64, until: Option<u64>, max_park: u64) {
                let mut now = self.now.lock();
                let target = until.unwrap_or(*now + max_park).min(*now + max_park);
                assert!(target > *now, "a park must advance time (no spin)");
                *now = if *now >= 1_200 && !self.jumped.swap(true, Ordering::SeqCst) {
                    *now + 36_000
                } else {
                    target
                };
                if *now >= self.horizon {
                    self.stop.store(true, Ordering::SeqCst);
                }
            }
        }
        let graph = graph_with_states(3, &[("10m", 600)]);
        let stop = Arc::new(AtomicBool::new(false));
        let clock = JumpClock {
            now: Mutex::new(0),
            jumped: AtomicBool::new(false),
            stop: Arc::clone(&stop),
            horizon: 36_000 + 1_200 + 600,
        };
        let fired: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());
        let graph_ref = &graph;
        let clock_ref = &clock;
        let stats = run_scheduler_loop(graph_ref, clock_ref, &stop, |due| {
            let now = clock_ref.now_secs();
            fired.lock().push((due.key.clone(), now));
            work(graph_ref, now, &due);
        });
        let fired = fired.into_inner();
        let after_jump: Vec<_> = fired.iter().filter(|(_, at)| *at >= 36_000).collect();
        // t=0, 600, 1200 before the jump; one coalesced tick on wake at 37_200;
        // the next regular tick at 37_800 = 36_000 + 3 * 600 is past the
        // horizon check only once.
        for key in ["doc0", "doc1", "doc2"] {
            let ticks: Vec<u64> = after_jump
                .iter()
                .filter(|(fired_key, _)| fired_key == key)
                .map(|(_, at)| *at)
                .collect();
            assert_eq!(
                ticks,
                vec![37_200],
                "{key}: one coalesced tick, then the cadence"
            );
            let state = graph.state(key).unwrap();
            assert!(state.coalesced_ticks >= 1, "{key}: {state:?}");
            assert_eq!(state.last_fired_due_secs, Some(37_200));
        }
        assert!(
            stats.wakeups <= 10,
            "a jump must not trigger a catch-up burst: {stats:?}"
        );
    }

    #[test]
    fn a_simulated_week_idle_wakes_only_at_the_park_ceiling() {
        // One scheduled document whose loop already finished (`loop=1`, one
        // pass done): nothing will ever be due. A week of virtual idle may
        // only cost the bounded heartbeat, never a spin.
        let graph = graph();
        let finished = spec(&[("interval", "10m"), ("loop", "1")]);
        let mut state = reconcile(None, &finished, 0);
        state.passes_completed = 1;
        state.last_fired_due_secs = Some(0);
        graph.set_state("doc", Some(state));
        graph.set_document("doc", Some(document("doc", finished)));
        assert_eq!(graph.earliest_wake(), None);
        let week = 7 * 86_400;
        let stop = Arc::new(AtomicBool::new(false));
        let clock = VirtualClock::new(0, week, Arc::clone(&stop));
        let stats = run_scheduler_loop(&graph, &clock, &stop, |_| {
            panic!("an exhausted loop never fires")
        });
        assert_eq!(stats.wakeups, week / MAX_SCHEDULER_PARK_SECS);
        assert_eq!(stats.dispatched, 0);
    }

    #[test]
    fn idle_graph_parks_at_the_max_park_and_never_spins() {
        let graph = graph();
        let stop = Arc::new(AtomicBool::new(false));
        let clock = VirtualClock::new(0, 3_600, Arc::clone(&stop));
        let stats = run_scheduler_loop(&graph, &clock, &stop, |_| panic!("nothing is scheduled"));
        assert_eq!(stats.wakeups, 3_600 / MAX_SCHEDULER_PARK_SECS);
        assert_eq!(stats.productive_wakeups, 0);
    }

    #[test]
    fn an_in_flight_document_cannot_spin_the_loop() {
        // The worker never reports back (a wedged dispatch): the loop must keep
        // parking instead of re-dispatching or busy-waiting.
        let graph = graph_with_states(1, &[("10m", 600)]);
        let stop = Arc::new(AtomicBool::new(false));
        let clock = VirtualClock::new(0, 3_600, Arc::clone(&stop));
        let mut dispatched = 0;
        let stats = run_scheduler_loop(&graph, &clock, &stop, |_| dispatched += 1);
        assert_eq!(
            dispatched, 1,
            "dispatched once, never again while in flight"
        );
        assert!(
            stats.wakeups <= 3_600 / MAX_SCHEDULER_PARK_SECS + 1,
            "{stats:?}"
        );
    }

    #[test]
    fn a_worker_reporting_back_still_due_cannot_spin_the_loop() {
        // A buggy worker that releases the document without advancing its
        // state: the document stays due. The loop may retry it at most once per
        // second and must park between retries.
        let graph = graph_with_states(1, &[("10m", 600)]);
        let stop = Arc::new(AtomicBool::new(false));
        let horizon = 60;
        let clock = VirtualClock::new(0, horizon, Arc::clone(&stop));
        let graph_ref = &graph;
        let stats = run_scheduler_loop(graph_ref, &clock, &stop, |due| {
            graph_ref.release(&due.key);
        });
        assert!(stats.dispatched <= horizon + 1, "{stats:?}");
        assert!(stats.wakeups <= 2 * horizon + 2, "{stats:?}");
        assert_eq!(
            clock.parks.lock().len() as u64,
            horizon,
            "one park per virtual second"
        );
    }

    #[test]
    fn spec_edit_recomputes_the_earliest_wake_and_notifies_the_waker() {
        let graph = graph();
        let key = "doc";
        let slow = spec(&[("interval", "1h")]);
        let mut state = reconcile(None, &slow, 0);
        state.last_fired_due_secs = Some(0);
        graph.set_state(key, Some(state));
        graph.set_document(key, Some(document(key, slow)));
        assert_eq!(graph.earliest_wake(), Some(3_600));
        let before = graph.waker().generation();
        // The operator edits the marker: a faster cadence re-anchors at "now".
        let fast = spec(&[("interval", "5m")]);
        let state = reconcile(graph.state(key), &fast, 100);
        graph.set_state(key, Some(state));
        graph.set_document(key, Some(document(key, fast)));
        assert_eq!(graph.earliest_wake(), Some(400));
        assert_ne!(
            graph.waker().generation(),
            before,
            "the parked loop is woken"
        );
        let woken = graph.waker().generation();
        // Re-publishing the same spec is not a change.
        graph.set_document(key, Some(document(key, spec(&[("interval", "5m")]))));
        assert_eq!(graph.waker().generation(), woken, "equal specs do not wake");
        graph.set_document(key, None);
        assert_eq!(graph.earliest_wake(), None, "unscheduling removes the wake");
    }

    #[test]
    fn a_parked_thread_wakes_on_reschedule_without_waiting_out_its_deadline() {
        // Real threads and the real waker; the clock is frozen so only the
        // graph change can end the park.
        struct FrozenClock;
        impl ScheduleClock for FrozenClock {
            fn now_secs(&self) -> u64 {
                1_000
            }
            fn park(&self, waker: &ScheduleWaker, observed: u64, _until: Option<u64>, _max: u64) {
                waker.wait(observed, std::time::Duration::from_secs(30));
            }
        }
        let graph = Arc::new(graph());
        let key = "doc";
        let slow = spec(&[("interval", "1h")]);
        let mut state = reconcile(None, &slow, 1_000);
        state.last_fired_due_secs = Some(1_000);
        graph.set_state(key, Some(state));
        graph.set_document(key, Some(document(key, slow)));
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = std::sync::mpsc::channel();
        let runner = {
            let graph = Arc::clone(&graph);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                run_scheduler_loop(&graph, &FrozenClock, &stop, |due| {
                    let _ = sender.send(due);
                })
            })
        };
        // A new schedule appears for another document, due now.
        let other = spec(&[("interval", "10m")]);
        graph.set_document("other", Some(document("other", other)));
        let due = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the loop must wake on the graph change, not its 1h deadline");
        assert_eq!(due.key, "other");
        stop.store(true, Ordering::SeqCst);
        graph.waker().notify();
        let stats = runner.join().unwrap();
        assert!(stats.wakeups <= 3, "{stats:?}");
    }
}
