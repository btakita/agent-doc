//! The project controller's queue scheduler (`interval=`, `time=`, `loop`).
//!
//! One scheduler per controller process: one loop thread
//! (`agent-doc-queue-scheduler`) running
//! [`agent_doc_queue::queue_schedule_runtime::run_scheduler_loop`] over one
//! Lazily [`QueueScheduleGraph`], and one background worker thread
//! (`agent-doc-queue-schedule-worker`) that does every piece of IO. The loop
//! thread only parks and enqueues; it never reads a document, touches
//! `state.db`, or writes the queue.
//!
//! Ingress is reactive. An Effect on the relay's queue-region Source
//! (`agent_doc_crdt_relay_io::queue_region_graph`) enqueues a refresh when a
//! live document's queue region changes; the controller's commit frontier and
//! watch-daemon disk signals call [`note_document_changed`] for detached
//! documents. A refresh re-reads the document, re-parses the schedule, folds
//! the live heads into the open pass, and publishes the result into the graph,
//! whose earliest-wake `Computed` re-parks the loop.
//!
//! A due tick never runs work itself: it opens a pass window in the durable
//! `queue_schedule` row, and the ordinary drain (supervisor idle watch,
//! in-session loop, subagent fan-out) — which treats every head of a closed
//! window as held — dispatches the heads through its existing paths.
//!
//! Durability: every transition is a `BEGIN IMMEDIATE` read-modify-write of
//! the row, re-applying the pure transition to the freshly loaded state, so a
//! restarted, hot-updated, or hand-over controller neither refires a tick nor
//! double-counts a pass.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, OnceLock};

use agent_doc_queue::queue_schedule::{
    self, FireDecision, FireFacts, LocalOffset, ObserveOutcome, QUEUE_SCHEDULE_STATE_KIND,
    QueueScheduleState,
};
use agent_doc_queue::queue_schedule_runtime::{
    DueSchedule, QueueScheduleGraph, ScheduledDocument, SystemScheduleClock, run_scheduler_loop,
};
use anyhow::{Context, Result};
use parking_lot::Mutex;

/// Effects the worker performs. Production reads the relay/disk and writes
/// through the binary's queue write port; tests substitute fakes.
pub trait QueueScheduleIo: Send + Sync + 'static {
    /// The document's authoritative current text.
    fn current_text(&self, file: &Path) -> Result<String>;
    /// Put a completed `loop` pass's items back into the queue. Returns how
    /// many items were restored.
    fn restore_loop_items(&self, file: &Path, roster: &[String]) -> Result<usize>;
    fn now_secs(&self) -> u64;
    fn log(&self, file: &Path, line: &str) {
        agent_doc_ops_log_io::log_op(file, line);
    }
}

/// The platform's local zone, via `localtime_r` (UTC where unavailable).
pub struct SystemLocalOffset;

impl LocalOffset for SystemLocalOffset {
    #[cfg(unix)]
    fn utc_offset_secs(&self, epoch_secs: i64) -> i64 {
        let time: libc::time_t = epoch_secs as libc::time_t;
        // SAFETY: `tm` is a plain C struct that `localtime_r` fully initializes
        // on success; on failure we never read it.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::localtime_r(&time, &mut tm) };
        if result.is_null() { 0 } else { tm.tm_gmtoff }
    }

    #[cfg(not(unix))]
    fn utc_offset_secs(&self, _epoch_secs: i64) -> i64 {
        0
    }
}

struct ProductionIo;

impl QueueScheduleIo for ProductionIo {
    fn current_text(&self, file: &Path) -> Result<String> {
        match agent_doc_crdt_relay_io::current_text_for_file(file) {
            Ok(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => Ok(text),
            Ok(agent_doc_crdt_relay_io::CurrentText::Detached) => std::fs::read_to_string(file)
                .with_context(|| format!("queue schedule: read {}", file.display())),
            Ok(_) => anyhow::bail!(
                "queue schedule: the live editor authority for {} is not available",
                file.display()
            ),
            Err(err) => Err(err),
        }
    }

    fn restore_loop_items(&self, file: &Path, roster: &[String]) -> Result<usize> {
        crate::project_controller::runtime_effects()?.restore_queue_loop_items(file, roster)
    }

    fn now_secs(&self) -> u64 {
        agent_doc_log_time::current_epoch_secs()
    }
}

// ----- durable state (`queue_document_state`, kind `queue_schedule`) -----

fn state_identity(file: &Path) -> Result<Option<(PathBuf, String, String)>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    Ok(Some((root, hash, canonical.to_string_lossy().into_owned())))
}

/// The graph/state key of a document (the same hash its claims use).
pub fn schedule_key(file: &Path) -> Result<String> {
    agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))
}

/// The document's durable schedule state (`None` when it has never been
/// admitted, or is outside an agent-doc project).
pub fn load_schedule_state(file: &Path) -> Result<Option<QueueScheduleState>> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(None);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    load_from_conn(&conn, &document_hash)
}

fn load_from_conn(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
) -> Result<Option<QueueScheduleState>> {
    agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        conn,
        document_hash,
        QUEUE_SCHEDULE_STATE_KIND,
    )?
    .map(|record| serde_json::from_str(&record.payload_json).context("decode queue schedule state"))
    .transpose()
}

/// Apply `mutate` to the freshly loaded row in one immediate transaction and
/// persist its result (`None` deletes the row). An unchanged state is not
/// rewritten.
fn mutate_schedule_state<T>(
    file: &Path,
    now: u64,
    mutate: impl FnOnce(Option<QueueScheduleState>) -> (Option<QueueScheduleState>, T),
) -> Result<T> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        anyhow::bail!(
            "{} is not inside an agent-doc project (no .agent-doc root); a queue schedule needs state.db",
            file.display()
        );
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .context("begin queue schedule transaction")?;
    let result = (|| {
        let before = load_from_conn(&conn, &document_hash)?;
        let (after, value) = mutate(before.clone());
        if after != before {
            match &after {
                None => agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
                    &conn,
                    &document_hash,
                    QUEUE_SCHEDULE_STATE_KIND,
                )
                .map(|_| ())?,
                Some(state) => agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
                    &conn,
                    &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                        document_hash: document_hash.clone(),
                        state_kind: QUEUE_SCHEDULE_STATE_KIND.to_string(),
                        canonical_path,
                        payload_json: serde_json::to_string(state)
                            .context("serialize queue schedule state")?,
                        updated_at_secs: now,
                    },
                )?,
            }
        }
        Ok(value)
    })();
    match result {
        Ok(value) => {
            conn.execute_batch("COMMIT")
                .context("commit queue schedule transaction")?;
            Ok(value)
        }
        Err(err) => {
            if let Err(rollback) = conn.execute_batch("ROLLBACK") {
                eprintln!("[queue-schedule] WARNING: rollback failed after {err:#}: {rollback}");
            }
            Err(err)
        }
    }
}

/// Every document with a durable schedule row under `project_root`.
pub fn scheduled_documents(project_root: &Path) -> Result<Vec<PathBuf>> {
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    Ok(
        agent_doc_sqlite::state_store::list_queue_document_state_from_db(
            &conn,
            QUEUE_SCHEDULE_STATE_KIND,
        )?
        .into_iter()
        .map(|record| PathBuf::from(record.canonical_path))
        .collect(),
    )
}

// ----- the worker -----

enum Job {
    Refresh(PathBuf),
    Due(DueSchedule),
}

/// The worker's handle on the graph and its effects.
pub struct QueueScheduleWorker {
    graph: Arc<QueueScheduleGraph>,
    io: Arc<dyn QueueScheduleIo>,
}

impl QueueScheduleWorker {
    pub fn new(graph: Arc<QueueScheduleGraph>, io: Arc<dyn QueueScheduleIo>) -> Self {
        Self { graph, io }
    }

    pub fn graph(&self) -> &Arc<QueueScheduleGraph> {
        &self.graph
    }

    /// Re-read `file` and fold it into the schedule: admit, reschedule,
    /// unschedule, or complete the open pass.
    pub fn refresh(&self, file: &Path) {
        if let Err(err) = self.try_refresh(file) {
            self.io.log(
                file,
                &format!("queue_schedule_refresh_failed error={err:#}"),
            );
        }
    }

    fn try_refresh(&self, file: &Path) -> Result<()> {
        let key = schedule_key(file)?;
        let text = self.io.current_text(file)?;
        let Some(spec) = queue_schedule::queue_schedule_spec(&text) else {
            if self.graph.document(&key).is_some()
                || self.graph.state(&key).is_some()
                || load_schedule_state(file)?.is_some()
            {
                mutate_schedule_state(file, self.io.now_secs(), |_| (None, ()))?;
                self.graph.set_document(&key, None);
                self.graph.set_state(&key, None);
                self.io.log(file, "queue_schedule_removed");
            }
            return Ok(());
        };
        let now = self.io.now_secs();
        let live = queue_schedule::live_heads(&text);
        let (state, outcome, admitted) = mutate_schedule_state(file, now, |prev| {
            let admitted = prev.is_none();
            let state = queue_schedule::reconcile(prev, &spec, now);
            let (state, outcome) = queue_schedule::observe(state, &spec, &live, now);
            (Some(state.clone()), (state, outcome, admitted))
        })?;
        if admitted {
            self.io.log(
                file,
                &format!(
                    "queue_schedule_admitted cadence={} loop={:?}",
                    spec.cadence_fingerprint(),
                    spec.loop_limit
                ),
            );
        }
        self.graph.set_state(&key, Some(state));
        self.graph.set_document(
            &key,
            Some(ScheduledDocument {
                file: file.to_path_buf(),
                spec,
            }),
        );
        if let ObserveOutcome::Completed { pass, restore } = outcome {
            self.io.log(
                file,
                &format!(
                    "queue_schedule_pass_completed pass={pass} restore_items={}",
                    restore.len()
                ),
            );
            if !restore.is_empty() {
                self.restore(&key, file, &restore);
            }
        }
        Ok(())
    }

    fn restore(&self, key: &str, file: &Path, roster: &[String]) {
        let restored = self.io.restore_loop_items(file, roster);
        let now = self.io.now_secs();
        let applied = restored.is_ok();
        let committed = mutate_schedule_state(file, now, |prev| {
            let next = prev.map(|state| {
                if applied {
                    queue_schedule::restore_applied(state)
                } else {
                    queue_schedule::record_failure(state, now)
                }
            });
            (next.clone(), next)
        });
        match (&restored, committed) {
            (Ok(count), Ok(state)) => {
                self.io
                    .log(file, &format!("queue_schedule_loop_restored items={count}"));
                self.graph.set_state(key, state);
            }
            (Err(err), Ok(state)) => {
                self.io.log(
                    file,
                    &format!(
                        "queue_schedule_loop_restore_failed backoff_until={:?} error={err:#}",
                        state.as_ref().and_then(|state| state.backoff_until_secs)
                    ),
                );
                self.graph.set_state(key, state);
            }
            (_, Err(err)) => self.io.log(
                file,
                &format!("queue_schedule_state_write_failed error={err:#}"),
            ),
        }
    }

    /// Run one due tick (or a pending restore) for `due`.
    pub fn run_due(&self, due: &DueSchedule) {
        let state = match self.try_run_due(due) {
            Ok(state) => state,
            Err(err) => {
                let now = self.io.now_secs();
                let state = mutate_schedule_state(&due.file, now, |prev| {
                    let next = prev.map(|state| queue_schedule::record_failure(state, now));
                    (next.clone(), next)
                });
                self.io.log(
                    &due.file,
                    &format!("queue_schedule_tick_failed error={err:#}"),
                );
                match state {
                    Ok(state) => state,
                    Err(write_err) => {
                        self.io.log(
                            &due.file,
                            &format!("queue_schedule_state_write_failed error={write_err:#}"),
                        );
                        // Keep the in-memory state but back it off so the loop
                        // cannot redispatch immediately.
                        self.graph
                            .state(&due.key)
                            .map(|state| queue_schedule::record_failure(state, now))
                    }
                }
            }
        };
        self.graph.complete(&due.key, state);
    }

    fn try_run_due(&self, due: &DueSchedule) -> Result<Option<QueueScheduleState>> {
        let text = self.io.current_text(&due.file)?;
        let Some(spec) = queue_schedule::queue_schedule_spec(&text) else {
            mutate_schedule_state(&due.file, self.io.now_secs(), |_| (None, ()))?;
            self.graph.set_document(&due.key, None);
            self.io.log(&due.file, "queue_schedule_removed");
            return Ok(None);
        };
        if let Some(state) = load_schedule_state(&due.file)?
            && let Some(roster) = state.restore_pending.clone()
        {
            self.restore(&due.key, &due.file, &roster);
            return Ok(self.graph.state(&due.key));
        }
        let now = self.io.now_secs();
        let facts = FireFacts {
            live_heads: queue_schedule::live_heads(&text),
            paused: queue_schedule::queue_held_by_control(&text),
        };
        let tz = self.graph.tz();
        let (state, decision) = mutate_schedule_state(&due.file, now, |prev| {
            let state = queue_schedule::reconcile(prev, &spec, now);
            let (state, decision) = queue_schedule::fire(state, &spec, now, &facts, tz);
            let state = if decision == FireDecision::NotDue {
                state
            } else {
                queue_schedule::record_success(state)
            };
            (Some(state.clone()), (state, decision))
        })?;
        if decision != FireDecision::NotDue {
            let next_run = queue_schedule::wake_deadline(&state, &spec, tz);
            self.io.log(
                &due.file,
                &format!(
                    "queue_schedule_tick decision={} pass={} passes_completed={} live_heads={} coalesced_ticks={} next_run={}",
                    serde_json::to_string(&decision)
                        .unwrap_or_default()
                        .trim_matches('"'),
                    state
                        .current_pass()
                        .map_or_else(|| "-".to_string(), |pass| pass.to_string()),
                    state.passes_completed,
                    facts.live_heads.len(),
                    state.coalesced_ticks,
                    next_run.map_or_else(
                        || "never".to_string(),
                        agent_doc_log_time::format_log_timestamp
                    ),
                ),
            );
        }
        Ok(Some(state))
    }
}

// ----- process-wide scheduler -----

struct RunningScheduler {
    sender: Mutex<Sender<Job>>,
}

fn running() -> &'static OnceLock<RunningScheduler> {
    static RUNNING: OnceLock<RunningScheduler> = OnceLock::new();
    &RUNNING
}

/// Note that `file`'s queue may have changed (commit frontier, watch-daemon
/// disk signal). A no-op when no scheduler runs in this process.
pub fn note_document_changed(file: &Path) {
    if let Some(running) = running().get() {
        let _ = running.sender.lock().send(Job::Refresh(file.to_path_buf()));
    }
}

/// Start this process's single queue scheduler (idempotent) and enqueue a
/// refresh of every document with a durable schedule under `project_root`.
pub fn start_queue_scheduler(project_root: &Path) {
    let mut started_now = false;
    let running = running().get_or_init(|| {
        started_now = true;
        spawn_scheduler(Arc::new(ProductionIo))
    });
    if !started_now {
        // Another root in the same process: hydrate it too.
        hydrate(&running.sender.lock(), project_root);
        return;
    }
    hydrate(&running.sender.lock(), project_root);
    install_region_effect();
}

fn hydrate(sender: &Sender<Job>, project_root: &Path) {
    match scheduled_documents(project_root) {
        Ok(files) => {
            for file in files {
                let _ = sender.send(Job::Refresh(file));
            }
        }
        Err(err) => eprintln!(
            "[queue-schedule] WARNING: could not list scheduled documents under {}: {err:#}",
            project_root.display()
        ),
    }
}

fn spawn_scheduler(io: Arc<dyn QueueScheduleIo>) -> RunningScheduler {
    // `#stategraphjoin`: the scheduler joins the controller process's shared
    // scope (the relay's queue-region Source lives there too).
    let graph = Arc::new(QueueScheduleGraph::new_in(
        agent_doc_document_realtime::editor_process_scope(),
        Arc::new(SystemLocalOffset),
    ));
    let (sender, receiver) = std::sync::mpsc::channel::<Job>();
    let worker = QueueScheduleWorker::new(Arc::clone(&graph), io);
    if let Err(err) = std::thread::Builder::new()
        .name("agent-doc-queue-schedule-worker".to_string())
        .spawn(move || run_worker(&worker, &receiver))
    {
        eprintln!("[queue-schedule] failed to start worker: {err}");
    }
    let loop_sender = sender.clone();
    let loop_graph = Arc::clone(&graph);
    if let Err(err) = std::thread::Builder::new()
        .name("agent-doc-queue-scheduler".to_string())
        .spawn(move || {
            // The loop lives as long as the controller process.
            let never_stop = AtomicBool::new(false);
            run_scheduler_loop(&loop_graph, &SystemScheduleClock, &never_stop, |due| {
                if loop_sender.send(Job::Due(due.clone())).is_err() {
                    loop_graph.release(&due.key);
                }
            });
        })
    {
        eprintln!("[queue-schedule] failed to start scheduler loop: {err}");
    }
    RunningScheduler {
        sender: Mutex::new(sender),
    }
}

/// The worker loop: due ticks first, refreshes coalesced per document.
fn run_worker(worker: &QueueScheduleWorker, receiver: &Receiver<Job>) {
    while let Ok(job) = receiver.recv() {
        let mut refreshes: Vec<PathBuf> = Vec::new();
        let mut dues: Vec<DueSchedule> = Vec::new();
        let mut push = |job: Job| match job {
            Job::Refresh(file) => {
                if !refreshes.contains(&file) {
                    refreshes.push(file);
                }
            }
            Job::Due(due) => dues.push(due),
        };
        push(job);
        while let Ok(job) = receiver.try_recv() {
            push(job);
        }
        for file in refreshes {
            worker.refresh(&file);
        }
        for due in dues {
            worker.run_due(&due);
        }
    }
}

/// Last region hash, schedule hint, and file the region Effect saw per document.
type SeenRegion = (Option<u64>, bool, PathBuf);

/// Subscribe the scheduler to the relay's queue-region Source. The Effect
/// only enqueues; it runs inside the relay's hub operation, so it must never
/// block or do IO. It reads nothing but the region Source, so the only thing
/// that reruns it is a queue-region change.
///
/// A region change is forwarded when the region mentions a schedule attribute
/// now, or did at the previous observation (so removing the attributes, or the
/// whole queue, still reaches the worker and unschedules the document).
fn install_region_effect() {
    let (ctx, regions) = agent_doc_crdt_relay_io::queue_region_graph();
    let seen: Mutex<std::collections::HashMap<String, SeenRegion>> = Mutex::new(Default::default());
    let effect = ctx.effect(move |ctx| {
        for key in regions.keys(ctx) {
            let observation = regions.observe(ctx, &key).flatten();
            let mut seen = seen.lock();
            let previous = seen.get(&key).cloned();
            let (hash, hint, file) = match observation {
                Some(observation) => (
                    Some(observation.region_hash),
                    observation.schedule_hint,
                    observation.file,
                ),
                None => match &previous {
                    Some((_, _, file)) => (None, false, file.clone()),
                    None => continue,
                },
            };
            if previous
                .as_ref()
                .is_some_and(|(prev_hash, _, _)| *prev_hash == hash)
            {
                continue;
            }
            let was_hinted = previous
                .as_ref()
                .is_some_and(|(_, prev_hint, _)| *prev_hint);
            seen.insert(key, (hash, hint, file.clone()));
            drop(seen);
            if hint || was_hinted {
                note_document_changed(&file);
            }
        }
    });
    // The subscription lives as long as the controller process.
    let _ = effect;
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::queue_schedule::FixedOffset;

    struct FakeIo {
        text: Mutex<String>,
        now: Mutex<u64>,
        restores: Mutex<Vec<Vec<String>>>,
        fail_restore: AtomicBool,
        logs: Mutex<Vec<String>>,
    }

    impl FakeIo {
        fn new(text: &str) -> Arc<Self> {
            Arc::new(Self {
                text: Mutex::new(text.to_string()),
                now: Mutex::new(1_000),
                restores: Mutex::new(Vec::new()),
                fail_restore: AtomicBool::new(false),
                logs: Mutex::new(Vec::new()),
            })
        }
    }

    impl QueueScheduleIo for FakeIo {
        fn current_text(&self, _file: &Path) -> Result<String> {
            Ok(self.text.lock().clone())
        }
        fn restore_loop_items(&self, _file: &Path, roster: &[String]) -> Result<usize> {
            if self.fail_restore.load(std::sync::atomic::Ordering::SeqCst) {
                anyhow::bail!("editor unavailable");
            }
            self.restores.lock().push(roster.to_vec());
            let mut text = self.text.lock();
            if let Some(updated) = queue_schedule::plan_loop_restore(&text, roster)? {
                *text = updated;
            }
            Ok(roster.len())
        }
        fn now_secs(&self) -> u64 {
            *self.now.lock()
        }
        fn log(&self, _file: &Path, line: &str) {
            self.logs.lock().push(line.to_string());
        }
    }

    fn project(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, content).unwrap();
        (dir, doc)
    }

    fn doc(attrs: &str, body: &str) -> String {
        format!("# Plan\n\n<!-- agent:queue {attrs} -->\n{body}<!-- /agent:queue -->\n")
    }

    fn worker(io: &Arc<FakeIo>) -> QueueScheduleWorker {
        QueueScheduleWorker::new(
            Arc::new(QueueScheduleGraph::new(Arc::new(FixedOffset(0)))),
            Arc::clone(io) as Arc<dyn QueueScheduleIo>,
        )
    }

    fn due(file: &Path) -> DueSchedule {
        DueSchedule {
            key: schedule_key(file).unwrap(),
            file: file.to_path_buf(),
        }
    }

    #[test]
    fn refresh_admits_the_schedule_durably_and_holds_heads_until_a_tick() {
        let content = doc("interval=10m", "- check CI\n");
        let (_dir, file) = project(&content);
        let io = FakeIo::new(&content);
        let worker = worker(&io);
        worker.refresh(&file);
        let state = load_schedule_state(&file).unwrap().expect("admitted");
        assert!(!state.window_open());
        assert_eq!(
            queue_schedule::held_heads(&content, Some(&state)),
            vec!["check CI"]
        );
        assert_eq!(
            worker.graph().earliest_wake(),
            Some(1_000),
            "first tick is immediate"
        );
        let due = worker.graph().take_due(1_000);
        assert_eq!(due.len(), 1);
        worker.run_due(&due[0]);
        let state = load_schedule_state(&file).unwrap().unwrap();
        assert!(state.window_open(), "the tick opened a pass window");
        assert!(queue_schedule::held_heads(&content, Some(&state)).is_empty());
        assert_eq!(worker.graph().earliest_wake(), Some(1_600));
    }

    #[test]
    fn restart_neither_refires_a_tick_nor_double_counts_a_pass() {
        let content = doc("interval=10m loop=3", "- a\n");
        let (_dir, file) = project(&content);
        let io = FakeIo::new(&content);
        let first = worker(&io);
        first.refresh(&file);
        first.run_due(&due(&file));
        // The pass drains: the head is consumed.
        *io.text.lock() = doc("interval=10m loop=3", "- ~~a~~\n");
        first.refresh(&file);
        let state = load_schedule_state(&file).unwrap().unwrap();
        assert_eq!(state.passes_completed, 1);
        assert_eq!(io.restores.lock().len(), 1, "the loop item was restored");
        assert_eq!(*io.text.lock(), doc("interval=10m loop=3", "- a\n"));

        // A hot update: a brand-new controller (fresh graph) loads the row and
        // replays the same wake at the same instant.
        let second = worker(&io);
        second.refresh(&file);
        second.run_due(&due(&file));
        let replayed = load_schedule_state(&file).unwrap().unwrap();
        assert_eq!(replayed.passes_completed, 1, "no double count");
        assert!(!replayed.window_open(), "the fired tick is not refired");
        assert_eq!(second.graph().earliest_wake(), Some(1_600));
    }

    #[test]
    fn loop_three_stops_after_three_passes_and_keeps_its_items() {
        let content = doc("interval=10m loop=3", "- a\n- b\n");
        let (_dir, file) = project(&content);
        let io = FakeIo::new(&content);
        let worker = worker(&io);
        worker.refresh(&file);
        for pass in 1..=3u64 {
            *io.now.lock() = 1_000 + (pass - 1) * 600;
            worker.run_due(&due(&file));
            assert!(load_schedule_state(&file).unwrap().unwrap().window_open());
            *io.text.lock() = doc("interval=10m loop=3", "- ~~a~~\n- ~~b~~\n");
            worker.refresh(&file);
            assert_eq!(*io.text.lock(), content, "pass {pass} restored the items");
        }
        let state = load_schedule_state(&file).unwrap().unwrap();
        assert_eq!(state.passes_completed, 3);
        *io.now.lock() = 1_000 + 3 * 600;
        worker.run_due(&due(&file));
        let state = load_schedule_state(&file).unwrap().unwrap();
        assert!(!state.window_open(), "a fourth pass never opens");
        assert_eq!(worker.graph().earliest_wake(), None);
        assert_eq!(
            queue_schedule::held_heads(&content, Some(&state)),
            vec!["a", "b"],
            "the items stay in the queue, held"
        );
    }

    #[test]
    fn a_failed_restore_backs_off_and_retries() {
        let content = doc("loop", "- a\n");
        let (_dir, file) = project(&content);
        let io = FakeIo::new(&content);
        let worker = worker(&io);
        worker.refresh(&file);
        worker.run_due(&due(&file));
        io.fail_restore
            .store(true, std::sync::atomic::Ordering::SeqCst);
        *io.text.lock() = doc("loop", "");
        worker.refresh(&file);
        let state = load_schedule_state(&file).unwrap().unwrap();
        assert!(state.restore_pending.is_some());
        assert_eq!(state.failures, 1);
        assert_eq!(worker.graph().earliest_wake(), Some(1_000 + 30));
        io.fail_restore
            .store(false, std::sync::atomic::Ordering::SeqCst);
        *io.now.lock() = 1_030;
        worker.run_due(&due(&file));
        let state = load_schedule_state(&file).unwrap().unwrap();
        assert!(state.restore_pending.is_none());
        assert_eq!(state.failures, 0);
        assert_eq!(*io.text.lock(), content);
    }

    #[test]
    fn removing_the_schedule_attributes_unschedules_the_document() {
        let content = doc("interval=10m", "- a\n");
        let (_dir, file) = project(&content);
        let io = FakeIo::new(&content);
        let worker = worker(&io);
        worker.refresh(&file);
        assert!(load_schedule_state(&file).unwrap().is_some());
        *io.text.lock() = doc("go", "- a\n");
        worker.refresh(&file);
        assert!(load_schedule_state(&file).unwrap().is_none());
        assert_eq!(worker.graph().earliest_wake(), None);
    }

    #[test]
    fn editing_interval_reschedules_in_realtime_and_keeps_the_pass_count() {
        let content = doc("interval=1h loop", "- a\n");
        let (_dir, file) = project(&content);
        let io = FakeIo::new(&content);
        let worker = worker(&io);
        worker.refresh(&file);
        worker.run_due(&due(&file));
        *io.text.lock() = doc("interval=1h loop", "- ~~a~~\n");
        worker.refresh(&file);
        assert_eq!(
            load_schedule_state(&file)
                .unwrap()
                .unwrap()
                .passes_completed,
            1
        );
        assert_eq!(worker.graph().earliest_wake(), Some(1_000 + 3_600));
        let woken = worker.graph().waker().generation();
        // The operator edits the marker at t=1500; the refresh the region
        // Effect enqueues re-anchors the cadence at the edit.
        *io.now.lock() = 1_500;
        *io.text.lock() = doc("interval=5m loop", "- a\n");
        worker.refresh(&file);
        assert_eq!(worker.graph().earliest_wake(), Some(1_500 + 300));
        assert_ne!(
            worker.graph().waker().generation(),
            woken,
            "the parked loop is woken"
        );
        let state = load_schedule_state(&file).unwrap().unwrap();
        assert_eq!(
            state.passes_completed, 1,
            "an edit keeps the durable pass count"
        );
        // Editing `loop` alone keeps the cadence anchor.
        *io.text.lock() = doc("interval=5m loop=4", "- a\n");
        *io.now.lock() = 1_600;
        worker.refresh(&file);
        assert_eq!(worker.graph().earliest_wake(), Some(1_500 + 300));
    }

    #[test]
    fn paused_queue_skips_the_tick() {
        let content = doc("interval=10m pause", "- a\n");
        let (_dir, file) = project(&content);
        let io = FakeIo::new(&content);
        let worker = worker(&io);
        worker.refresh(&file);
        worker.run_due(&due(&file));
        let state = load_schedule_state(&file).unwrap().unwrap();
        assert!(!state.window_open());
        assert!(
            io.logs
                .lock()
                .iter()
                .any(|line| line.contains("decision=skipped_paused")),
            "{:?}",
            io.logs.lock()
        );
    }

    #[test]
    fn region_observation_changes_only_with_the_queue_region() {
        let file = Path::new("/project/plan.md");
        let base = format!("intro\n{}", doc("interval=10m", "- a\n"));
        let first = agent_doc_crdt_relay_io::queue_region_observation(file, &base).unwrap();
        assert!(first.schedule_hint);
        let typed_outside = format!("{base}\nmore exchange text\n");
        assert_eq!(
            agent_doc_crdt_relay_io::queue_region_observation(file, &typed_outside),
            Some(first.clone()),
            "typing outside the queue is not a change"
        );
        let edited = format!("intro\n{}", doc("interval=5m", "- a\n"));
        assert_ne!(
            agent_doc_crdt_relay_io::queue_region_observation(file, &edited),
            Some(first)
        );
        assert!(
            !agent_doc_crdt_relay_io::queue_region_observation(file, &doc("go", "- a\n"))
                .unwrap()
                .schedule_hint
        );
    }

    #[test]
    fn system_offset_is_sane() {
        let offset = SystemLocalOffset.utc_offset_secs(1_700_000_000);
        assert!(offset.abs() <= 14 * 3_600, "{offset}");
    }
}
