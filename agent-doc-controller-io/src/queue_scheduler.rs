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
//! row for that `(document, queue_id)`, and the ordinary drain (supervisor idle
//! watch, in-session loop, subagent fan-out) — which treats every head of a
//! closed window as held — dispatches the heads through its existing paths.
//!
//! Durability: every transition is a `BEGIN IMMEDIATE` read-modify-write of
//! that queue's row, re-applying the pure transition to the freshly loaded
//! state, so a restarted, hot-updated, or hand-over controller neither refires
//! a tick nor double-counts a pass.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;

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
    fn restore_loop_items(
        &self,
        file: &Path,
        queue_id: &str,
        roster: &[String],
        source_content: &str,
    ) -> Result<usize>;
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

    fn restore_loop_items(
        &self,
        file: &Path,
        queue_id: &str,
        roster: &[String],
        source_content: &str,
    ) -> Result<usize> {
        crate::project_controller::runtime_effects()?.restore_queue_loop_items(
            file,
            queue_id,
            roster,
            source_content,
        )
    }

    fn now_secs(&self) -> u64 {
        agent_doc_log_time::current_epoch_secs()
    }
}

// ----- durable state (`queue_document_state`, queue-ID-addressed kind) -----

fn state_identity(file: &Path) -> Result<Option<(PathBuf, String, String)>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    Ok(Some((root, hash, canonical.to_string_lossy().into_owned())))
}

/// The graph/state key of a `(document, queue_id)` schedule.
pub fn schedule_key(file: &Path, queue_id: &str) -> Result<String> {
    Ok(format!(
        "{}:{queue_id}",
        agent_doc_hash::path_hash(file)
            .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?
    ))
}

fn state_kind(queue_id: &str) -> String {
    if queue_id == agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID {
        QUEUE_SCHEDULE_STATE_KIND.to_string()
    } else {
        format!("{QUEUE_SCHEDULE_STATE_KIND}:{queue_id}")
    }
}

/// The document's durable schedule state (`None` when it has never been
/// admitted, or is outside an agent-doc project).
pub fn load_schedule_state(file: &Path) -> Result<Option<QueueScheduleState>> {
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("read schedule document {}", file.display()))?;
    let Some(schedule) = queue_schedule::identified_queue_schedules(&text)?
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    load_schedule_state_for_queue(file, &schedule.queue_id)
}

pub fn load_schedule_state_for_queue(
    file: &Path,
    queue_id: &str,
) -> Result<Option<QueueScheduleState>> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(None);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    load_from_conn(&conn, &document_hash, queue_id)
}

fn load_from_conn(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
    queue_id: &str,
) -> Result<Option<QueueScheduleState>> {
    agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        conn,
        document_hash,
        &state_kind(queue_id),
    )?
    .map(|record| serde_json::from_str(&record.payload_json).context("decode queue schedule state"))
    .transpose()
}

/// Rolling-upgrade bridge: the document-global scheduler stored the first
/// schedule under the legacy `queue_schedule` kind even when its queue had an
/// explicit ID. A one-queue rename has one possible successor and adopts the
/// row atomically. A multi-queue split is ambiguous, so it discards the legacy
/// row instead of assigning an open pass/roster to an unrelated sibling.
fn migrate_legacy_state(file: &Path, queue_id: &str, now: u64, queue_count: usize) -> Result<bool> {
    if queue_id == agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID {
        return Ok(false);
    }
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(false);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .context("begin queue schedule migration")?;
    let result = (|| {
        if load_from_conn(&conn, &document_hash, queue_id)?.is_some() {
            return Ok(false);
        }
        let Some(legacy) = load_from_conn(
            &conn,
            &document_hash,
            agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID,
        )?
        else {
            return Ok(false);
        };
        let discarded = queue_count != 1;
        if !discarded {
            let mut legacy = legacy;
            legacy.owner_queue_id = Some(queue_id.to_string());
            agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
                &conn,
                &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                    document_hash: document_hash.clone(),
                    state_kind: state_kind(queue_id),
                    canonical_path,
                    payload_json: serde_json::to_string(&legacy)
                        .context("serialize migrated queue schedule state")?,
                    updated_at_secs: now,
                },
            )?;
        }
        agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
            &conn,
            &document_hash,
            QUEUE_SCHEDULE_STATE_KIND,
        )?;
        Ok(discarded)
    })();
    match result {
        Ok(discarded) => {
            conn.execute_batch("COMMIT")?;
            Ok(discarded)
        }
        Err(err) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(err)
        }
    }
}

/// Bind a row to its keyed queue. A pre-provenance row in a multi-queue
/// document is ambiguous: releases before the owner field could have copied
/// document-global counters or an open roster into the first scheduled queue.
/// Restart it from the current queue rather than letting sibling progress gate,
/// restore, or exhaust this queue.
fn bind_queue_owner(
    mut state: QueueScheduleState,
    spec: &agent_doc_queue::queue_schedule::QueueScheduleSpec,
    queue_id: &str,
    queue_count: usize,
    now: u64,
    preexisting: bool,
) -> (QueueScheduleState, bool) {
    if state.owner_queue_id.as_deref() == Some(queue_id) {
        return (state, false);
    }
    let ambiguous = preexisting
        && match state.owner_queue_id.as_deref() {
            Some(owner) => owner != queue_id,
            None => queue_count > 1,
        };
    if ambiguous {
        state = QueueScheduleState::new(spec, now);
    }
    state.owner_queue_id = Some(queue_id.to_string());
    (state, ambiguous)
}

/// Apply `mutate` to the freshly loaded row in one immediate transaction and
/// persist its result (`None` deletes the row). An unchanged state is not
/// rewritten.
fn mutate_schedule_state<T>(
    file: &Path,
    queue_id: &str,
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
        let kind = state_kind(queue_id);
        let before = load_from_conn(&conn, &document_hash, queue_id)?;
        let (after, value) = mutate(before.clone());
        if after != before {
            match &after {
                None => agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
                    &conn,
                    &document_hash,
                    &kind,
                )
                .map(|_| ())?,
                Some(state) => agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
                    &conn,
                    &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                        document_hash: document_hash.clone(),
                        state_kind: kind,
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
    let mut files = agent_doc_sqlite::state_store::list_queue_document_state_family_from_db(
        &conn,
        QUEUE_SCHEDULE_STATE_KIND,
    )?
    .into_iter()
    .map(|record| PathBuf::from(record.canonical_path))
    .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    Ok(files)
}

fn stored_queue_ids(file: &Path) -> Result<Vec<String>> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(Vec::new());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    Ok(
        agent_doc_sqlite::state_store::list_queue_document_state_family_from_db(
            &conn,
            QUEUE_SCHEDULE_STATE_KIND,
        )?
        .into_iter()
        .filter(|record| record.document_hash == document_hash)
        .filter_map(|record| {
            if record.state_kind == QUEUE_SCHEDULE_STATE_KIND {
                Some(agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID.to_string())
            } else {
                record
                    .state_kind
                    .strip_prefix(&format!("{QUEUE_SCHEDULE_STATE_KIND}:"))
                    .map(str::to_string)
            }
        })
        .collect(),
    )
}

// ----- the worker -----

#[derive(Debug)]
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
        let text = self.io.current_text(file)?;
        let schedules = queue_schedule::identified_queue_schedules(&text)?;
        let components = agent_doc_element::element::parse(&text)?;
        let queue_count = agent_doc_queue::queue_graph::QueueGraph::parse(&text, &components)
            .map_err(anyhow::Error::new)?
            .blocks()
            .len();
        let now = self.io.now_secs();
        if let Some(first) = schedules.first()
            && migrate_legacy_state(file, &first.queue_id, now, queue_count)?
        {
            self.io.log(
                file,
                &format!(
                    "queue_schedule_legacy_state_discarded queue_id={} reason=ambiguous_multi_queue_owner",
                    first.queue_id
                ),
            );
        }
        let declared = schedules
            .iter()
            .map(|schedule| schedule.queue_id.clone())
            .collect::<HashSet<_>>();

        let mut known = stored_queue_ids(file)?.into_iter().collect::<HashSet<_>>();
        for key in self.graph.scheduled_keys() {
            if let Some(document) = self.graph.document(&key)
                && document.file == file
            {
                known.insert(document.queue_id);
            }
        }
        for queue_id in known.difference(&declared) {
            let key = schedule_key(file, queue_id)?;
            mutate_schedule_state(file, queue_id, now, |_| (None, ()))?;
            self.graph.set_document(&key, None);
            self.graph.set_state(&key, None);
            self.io
                .log(file, &format!("queue_schedule_removed queue_id={queue_id}"));
        }

        for schedule in schedules {
            let queue_id = schedule.queue_id.clone();
            let key = schedule_key(file, &queue_id)?;
            if !schedule.is_valid() {
                self.graph.set_document(&key, None);
                self.graph.set_state(&key, None);
                self.io.log(
                    file,
                    &format!(
                        "queue_schedule_invalid queue_id={queue_id} issues={}",
                        schedule
                            .issues
                            .iter()
                            .map(|issue| format!("{}:{}", issue.attr, issue.message))
                            .collect::<Vec<_>>()
                            .join("|")
                    ),
                );
                continue;
            }
            let spec = schedule.spec.expect("valid schedule has a spec");
            let live = queue_schedule::live_heads_for_queue(&text, &queue_id)?;
            let completed = queue_schedule::completed_heads_for_queue(&text, &queue_id)?;
            let (state, outcome, admitted, repaired) =
                mutate_schedule_state(file, &queue_id, now, |prev| {
                    let admitted = prev.is_none();
                    let state = queue_schedule::reconcile(prev, &spec, now);
                    let (state, repaired) =
                        bind_queue_owner(state, &spec, &queue_id, queue_count, now, !admitted);
                    let (state, outcome) = queue_schedule::observe_with_completed(
                        state, &spec, &live, &completed, now,
                    );
                    (Some(state.clone()), (state, outcome, admitted, repaired))
                })?;
            if repaired {
                self.io.log(
                    file,
                    &format!(
                        "queue_schedule_legacy_state_reset queue_id={queue_id} reason=ambiguous_multi_queue_owner"
                    ),
                );
            }
            if admitted {
                self.io.log(
                    file,
                    &format!(
                        "queue_schedule_admitted queue_id={queue_id} cadence={} loop={:?}",
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
                    queue_id: queue_id.clone(),
                    spec,
                }),
            );
            if let ObserveOutcome::Completed { pass, restore } = outcome {
                self.io.log(
                    file,
                    &format!(
                        "queue_schedule_pass_completed queue_id={queue_id} pass={pass} restore_items={}",
                        restore.len()
                    ),
                );
                if !restore.is_empty() {
                    self.restore(&key, file, &queue_id, &restore, &text);
                }
            }
        }
        Ok(())
    }

    fn restore(
        &self,
        key: &str,
        file: &Path,
        queue_id: &str,
        roster: &[String],
        source_content: &str,
    ) {
        let restored = self
            .io
            .restore_loop_items(file, queue_id, roster, source_content);
        let now = self.io.now_secs();
        let applied = restored.is_ok();
        let committed = mutate_schedule_state(file, queue_id, now, |prev| {
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
                self.io.log(
                    file,
                    &format!("queue_schedule_loop_restored queue_id={queue_id} items={count}"),
                );
                self.graph.set_state(key, state);
            }
            (Err(err), Ok(state)) => {
                self.io.log(
                    file,
                    &format!(
                        "queue_schedule_loop_restore_failed queue_id={queue_id} backoff_until={:?} error={err:#}",
                        state.as_ref().and_then(|state| state.backoff_until_secs)
                    ),
                );
                self.graph.set_state(key, state);
            }
            (_, Err(err)) => self.io.log(
                file,
                &format!("queue_schedule_state_write_failed queue_id={queue_id} error={err:#}"),
            ),
        }
    }

    fn log_tick(
        &self,
        due: &DueSchedule,
        state: &QueueScheduleState,
        spec: &queue_schedule::QueueScheduleSpec,
        decision: FireDecision,
        live_heads: usize,
    ) {
        let next_run = queue_schedule::wake_deadline(state, spec, self.graph.tz());
        self.io.log(
            &due.file,
            &format!(
                "queue_schedule_tick queue_id={} decision={} pass={} passes_completed={} live_heads={} coalesced_ticks={} next_run={}",
                due.queue_id,
                serde_json::to_string(&decision)
                    .unwrap_or_default()
                    .trim_matches('"'),
                state
                    .current_pass()
                    .map_or_else(|| "-".to_string(), |pass| pass.to_string()),
                state.passes_completed,
                live_heads,
                state.coalesced_ticks,
                next_run.map_or_else(
                    || "never".to_string(),
                    agent_doc_log_time::format_log_timestamp,
                ),
            ),
        );
    }

    /// Run one due tick (or a pending restore) for `due`.
    pub fn run_due(&self, due: &DueSchedule) {
        let state = match self.try_run_due(due) {
            Ok(state) => state,
            Err(err) => {
                let now = self.io.now_secs();
                let state = mutate_schedule_state(&due.file, &due.queue_id, now, |prev| {
                    let next = prev.map(|state| queue_schedule::record_failure(state, now));
                    (next.clone(), next)
                });
                self.io.log(
                    &due.file,
                    &format!(
                        "queue_schedule_tick_failed queue_id={} error={err:#}",
                        due.queue_id
                    ),
                );
                match state {
                    Ok(state) => state,
                    Err(write_err) => {
                        self.io.log(
                            &due.file,
                            &format!(
                                "queue_schedule_state_write_failed queue_id={} error={write_err:#}",
                                due.queue_id
                            ),
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
        let schedule = queue_schedule::identified_queue_schedules(&text)?
            .into_iter()
            .find(|schedule| schedule.queue_id == due.queue_id);
        let Some(schedule) = schedule else {
            self.graph.set_document(&due.key, None);
            self.io.log(
                &due.file,
                &format!("queue_schedule_removed queue_id={}", due.queue_id),
            );
            return Ok(None);
        };
        if !schedule.is_valid() {
            self.graph.set_document(&due.key, None);
            self.io.log(
                &due.file,
                &format!("queue_schedule_invalid queue_id={}", due.queue_id),
            );
            return Ok(None);
        }
        let spec = schedule.spec.expect("valid schedule has a spec");
        if let Some(state) = load_schedule_state_for_queue(&due.file, &due.queue_id)?
            && let Some(roster) = state.restore_pending.clone()
        {
            self.restore(&due.key, &due.file, &due.queue_id, &roster, &text);
            return Ok(self.graph.state(&due.key));
        }
        let now = self.io.now_secs();
        let facts = FireFacts {
            live_heads: queue_schedule::live_heads_for_queue(&text, &due.queue_id)?,
            completed_heads: queue_schedule::completed_heads_for_queue(&text, &due.queue_id)?,
            paused: queue_schedule::queue_held_by_control_for_queue(&text, &due.queue_id)?,
        };
        let tz = self.graph.tz();
        let (state, decision) = mutate_schedule_state(&due.file, &due.queue_id, now, |prev| {
            let state = queue_schedule::reconcile(prev, &spec, now);
            let (state, decision) = queue_schedule::fire(state, &spec, now, &facts, tz);
            let state = if decision == FireDecision::NotDue {
                state
            } else {
                queue_schedule::record_success(state)
            };
            (Some(state.clone()), (state, decision))
        })?;
        if decision == FireDecision::Recovered {
            self.restore(
                &due.key,
                &due.file,
                &due.queue_id,
                &facts.completed_heads,
                &text,
            );
            let restored_state = self.graph.state(&due.key);
            if let Some(restored_state) = restored_state.as_ref() {
                // Log the post-restore state. Before GH #348 this ran while
                // `restore_pending` had no backoff and rendered epoch zero as
                // the next run even though the restore immediately followed.
                self.log_tick(due, restored_state, &spec, decision, facts.live_heads.len());
            }
            return Ok(restored_state);
        }
        if decision != FireDecision::NotDue {
            self.log_tick(due, &state, &spec, decision, facts.live_heads.len());
        }
        Ok(Some(state))
    }
}

// ----- process-wide scheduler -----

struct RunningScheduler {
    sender: Sender<Job>,
    graph: Arc<QueueScheduleGraph>,
    stop: Arc<AtomicBool>,
    worker_thread: JoinHandle<()>,
    scheduler_thread: JoinHandle<()>,
}

impl RunningScheduler {
    fn disconnected_reason(&self) -> Option<&'static str> {
        if self.worker_thread.is_finished() {
            Some("queue-schedule worker thread exited")
        } else if self.scheduler_thread.is_finished() {
            Some("queue-schedule loop thread exited")
        } else {
            None
        }
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.graph.waker().notify();
        self.worker_thread.thread().unpark();
        self.scheduler_thread.thread().unpark();
    }
}

#[derive(Default)]
struct ProcessScheduler {
    running: Option<RunningScheduler>,
    documents: HashSet<PathBuf>,
}

enum EnsureScheduler {
    Existing,
    Started,
    Replaced(String),
}

/// Receipt for one realtime refresh enqueue.
///
/// A recovered enqueue names the failed observation that forced replacement;
/// callers no longer have to infer that a closed channel was silently ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "queue refresh enqueue receipts expose scheduler disconnection and recovery"]
pub enum QueueRefreshEnqueueReceipt {
    SchedulerNotRunning,
    Enqueued,
    Recovered {
        reason: String,
        refreshed_documents: usize,
    },
}

impl ProcessScheduler {
    fn replace_with(&mut self, spawn: &mut impl FnMut() -> Result<RunningScheduler>) -> Result<()> {
        if let Some(stale) = self.running.take() {
            stale.stop();
        }
        self.running = Some(spawn().context("failed to spawn replacement queue scheduler")?);
        Ok(())
    }

    fn ensure_with(
        &mut self,
        spawn: &mut impl FnMut() -> Result<RunningScheduler>,
    ) -> Result<EnsureScheduler> {
        match self
            .running
            .as_ref()
            .and_then(RunningScheduler::disconnected_reason)
        {
            Some(reason) => {
                let reason = reason.to_string();
                self.replace_with(spawn)?;
                Ok(EnsureScheduler::Replaced(reason))
            }
            None if self.running.is_some() => Ok(EnsureScheduler::Existing),
            None => {
                self.replace_with(spawn)?;
                Ok(EnsureScheduler::Started)
            }
        }
    }

    fn sorted_documents(&self) -> Vec<PathBuf> {
        let mut documents = self.documents.iter().cloned().collect::<Vec<_>>();
        documents.sort();
        documents
    }

    fn send_refreshes(&self, documents: &[PathBuf]) -> std::result::Result<usize, String> {
        let running = self
            .running
            .as_ref()
            .expect("send_refreshes requires a running scheduler");
        for file in documents {
            running
                .sender
                .send(Job::Refresh(file.clone()))
                .map_err(|err| err.to_string())?;
        }
        Ok(documents.len())
    }

    fn recover_and_refresh_all(
        &mut self,
        reason: String,
        spawn: &mut impl FnMut() -> Result<RunningScheduler>,
    ) -> Result<QueueRefreshEnqueueReceipt> {
        self.replace_with(spawn).with_context(|| {
            format!("queue refresh enqueue failed ({reason}); scheduler replacement failed")
        })?;
        let documents = self.sorted_documents();
        let refreshed_documents = self.send_refreshes(&documents).map_err(|replacement_error| {
            anyhow::anyhow!(
                "queue refresh enqueue failed ({reason}); replacement enqueue failed: {replacement_error}"
            )
        })?;
        Ok(QueueRefreshEnqueueReceipt::Recovered {
            reason,
            refreshed_documents,
        })
    }

    fn enqueue_refresh_with(
        &mut self,
        file: &Path,
        spawn: &mut impl FnMut() -> Result<RunningScheduler>,
    ) -> Result<QueueRefreshEnqueueReceipt> {
        if self.running.is_none() {
            return Ok(QueueRefreshEnqueueReceipt::SchedulerNotRunning);
        }
        self.documents.insert(file.to_path_buf());
        match self.ensure_with(spawn)? {
            EnsureScheduler::Replaced(reason) => {
                let documents = self.sorted_documents();
                match self.send_refreshes(&documents) {
                    Ok(refreshed_documents) => Ok(QueueRefreshEnqueueReceipt::Recovered {
                        reason,
                        refreshed_documents,
                    }),
                    Err(replacement_error) => self.recover_and_refresh_all(
                        format!("{reason}; first replacement enqueue failed: {replacement_error}"),
                        spawn,
                    ),
                }
            }
            EnsureScheduler::Existing | EnsureScheduler::Started => {
                match self.send_refreshes(&[file.to_path_buf()]) {
                    Ok(_) => Ok(QueueRefreshEnqueueReceipt::Enqueued),
                    Err(reason) => self.recover_and_refresh_all(reason, spawn),
                }
            }
        }
    }

    fn start_with(
        &mut self,
        documents: Vec<PathBuf>,
        spawn: &mut impl FnMut() -> Result<RunningScheduler>,
    ) -> Result<()> {
        for file in &documents {
            self.documents.insert(file.clone());
        }
        let ensured = self.ensure_with(spawn)?;
        let refreshes = if matches!(ensured, EnsureScheduler::Existing) {
            documents
        } else {
            self.sorted_documents()
        };
        if let Err(reason) = self.send_refreshes(&refreshes) {
            let receipt = self.recover_and_refresh_all(reason, spawn)?;
            surface_recovery_receipt(&receipt);
        } else if let EnsureScheduler::Replaced(reason) = ensured {
            let receipt = QueueRefreshEnqueueReceipt::Recovered {
                reason,
                refreshed_documents: refreshes.len(),
            };
            surface_recovery_receipt(&receipt);
        }
        Ok(())
    }
}

fn process_scheduler() -> &'static Mutex<ProcessScheduler> {
    static RUNNING: OnceLock<Mutex<ProcessScheduler>> = OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(ProcessScheduler::default()))
}

fn region_effect_installed() -> &'static OnceLock<()> {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    &INSTALLED
}

fn surface_recovery_receipt(receipt: &QueueRefreshEnqueueReceipt) {
    if let QueueRefreshEnqueueReceipt::Recovered {
        reason,
        refreshed_documents,
    } = receipt
    {
        eprintln!(
            "[queue-schedule] WARNING: replaced disconnected scheduler after `{reason}`; refreshed_documents={refreshed_documents}"
        );
    }
}

/// Note that `file`'s queue may have changed (commit frontier, watch-daemon
/// disk signal). A no-op when no scheduler runs in this process. A disconnected
/// scheduler is replaced and every known scheduled document is refreshed.
pub fn note_document_changed(file: &Path) -> Result<QueueRefreshEnqueueReceipt> {
    let receipt = process_scheduler()
        .lock()
        .enqueue_refresh_with(file, &mut || spawn_scheduler(Arc::new(ProductionIo)))?;
    surface_recovery_receipt(&receipt);
    Ok(receipt)
}

/// Start this process's single queue scheduler (idempotent) and enqueue a
/// refresh of every document with a durable schedule under `project_root`.
pub fn start_queue_scheduler(project_root: &Path) -> Result<()> {
    let documents = scheduled_documents(project_root).with_context(|| {
        format!(
            "could not list scheduled documents under {}",
            project_root.display()
        )
    })?;
    process_scheduler()
        .lock()
        .start_with(documents, &mut || spawn_scheduler(Arc::new(ProductionIo)))?;
    region_effect_installed().get_or_init(install_region_effect);
    Ok(())
}

fn spawn_scheduler(io: Arc<dyn QueueScheduleIo>) -> Result<RunningScheduler> {
    // `#stategraphjoin`: the scheduler joins the controller process's shared
    // scope (the relay's queue-region Source lives there too).
    let graph = Arc::new(QueueScheduleGraph::new_in(
        agent_doc_document_realtime::editor_process_scope(),
        Arc::new(SystemLocalOffset),
    ));
    let (sender, receiver) = std::sync::mpsc::channel::<Job>();
    let worker = QueueScheduleWorker::new(Arc::clone(&graph), io);
    let worker_thread = std::thread::Builder::new()
        .name("agent-doc-queue-schedule-worker".to_string())
        .spawn(move || run_worker(&worker, &receiver))
        .context("failed to start queue-schedule worker")?;
    let loop_sender = sender.clone();
    let loop_graph = Arc::clone(&graph);
    let stop = Arc::new(AtomicBool::new(false));
    let loop_stop = Arc::clone(&stop);
    let scheduler_thread = std::thread::Builder::new()
        .name("agent-doc-queue-scheduler".to_string())
        .spawn(move || {
            // The loop lives as long as the controller process.
            run_scheduler_loop(&loop_graph, &SystemScheduleClock, &loop_stop, |due| {
                if loop_sender.send(Job::Due(due.clone())).is_err() {
                    loop_graph.release(&due.key);
                }
            });
        })
        .context("failed to start queue-schedule loop")?;
    Ok(RunningScheduler {
        sender,
        graph,
        stop,
        worker_thread,
        scheduler_thread,
    })
}

/// The worker loop: due ticks first, refreshes coalesced per document. A
/// refresh reconciles every queue schedule in that document independently.
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
            if (hint || was_hinted)
                && let Err(err) = note_document_changed(&file)
            {
                eprintln!(
                    "[queue-schedule] WARNING: could not enqueue realtime refresh for {}: {err:#}",
                    file.display()
                );
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
        fn restore_loop_items(
            &self,
            _file: &Path,
            queue_id: &str,
            roster: &[String],
            source_content: &str,
        ) -> Result<usize> {
            if self.fail_restore.load(std::sync::atomic::Ordering::SeqCst) {
                anyhow::bail!("editor unavailable");
            }
            self.restores.lock().push(roster.to_vec());
            let mut text = self.text.lock();
            if let Some(updated) =
                queue_schedule::plan_loop_restore_for_queue(source_content, queue_id, roster)?
            {
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
        due_for(file, agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID)
    }

    fn due_for(file: &Path, queue_id: &str) -> DueSchedule {
        DueSchedule {
            key: schedule_key(file, queue_id).unwrap(),
            file: file.to_path_buf(),
            queue_id: queue_id.to_string(),
        }
    }

    fn parked_scheduler(sender: Sender<Job>) -> RunningScheduler {
        let graph = Arc::new(QueueScheduleGraph::new(Arc::new(FixedOffset(0))));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_thread = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::SeqCst) {
                std::thread::park_timeout(std::time::Duration::from_millis(10));
            }
        });
        let scheduler_stop = Arc::clone(&stop);
        let scheduler_thread = std::thread::spawn(move || {
            while !scheduler_stop.load(Ordering::SeqCst) {
                std::thread::park_timeout(std::time::Duration::from_millis(10));
            }
        });
        RunningScheduler {
            sender,
            graph,
            stop,
            worker_thread,
            scheduler_thread,
        }
    }

    fn scheduler_with_finished_loop(sender: Sender<Job>) -> RunningScheduler {
        let graph = Arc::new(QueueScheduleGraph::new(Arc::new(FixedOffset(0))));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_thread = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::SeqCst) {
                std::thread::park_timeout(std::time::Duration::from_millis(10));
            }
        });
        let scheduler_thread = std::thread::spawn(|| {});
        while !scheduler_thread.is_finished() {
            std::thread::yield_now();
        }
        RunningScheduler {
            sender,
            graph,
            stop,
            worker_thread,
            scheduler_thread,
        }
    }

    #[test]
    fn a_disconnected_worker_is_replaced_and_the_refresh_is_retried() {
        let (dead_sender, dead_receiver) = std::sync::mpsc::channel();
        drop(dead_receiver);
        let (replacement_sender, replacement_receiver) = std::sync::mpsc::channel();
        let mut replacement = Some(parked_scheduler(replacement_sender));
        let mut state = ProcessScheduler {
            running: Some(parked_scheduler(dead_sender)),
            ..ProcessScheduler::default()
        };
        let file = PathBuf::from("/sample/project/plan.md");

        let receipt = state
            .enqueue_refresh_with(&file, &mut || {
                Ok(replacement.take().expect("one replacement scheduler"))
            })
            .unwrap();

        match receipt {
            QueueRefreshEnqueueReceipt::Recovered {
                reason,
                refreshed_documents,
            } => {
                assert!(reason.contains("sending on a closed channel"), "{reason}");
                assert_eq!(refreshed_documents, 1);
            }
            other => panic!("expected recovered enqueue receipt, got {other:?}"),
        }
        match replacement_receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("replacement received the retried refresh")
        {
            Job::Refresh(received) => assert_eq!(received, file),
            Job::Due(_) => panic!("replacement received a due job instead of the refresh"),
        }
        state.running.take().unwrap().stop();
    }

    #[test]
    fn a_finished_scheduler_loop_is_replaced_before_the_refresh() {
        let (stale_sender, stale_receiver) = std::sync::mpsc::channel();
        let (replacement_sender, replacement_receiver) = std::sync::mpsc::channel();
        let mut replacement = Some(parked_scheduler(replacement_sender));
        let mut state = ProcessScheduler {
            running: Some(scheduler_with_finished_loop(stale_sender)),
            ..ProcessScheduler::default()
        };
        let file = PathBuf::from("/sample/project/plan.md");

        let receipt = state
            .enqueue_refresh_with(&file, &mut || {
                Ok(replacement.take().expect("one replacement scheduler"))
            })
            .unwrap();

        assert_eq!(
            receipt,
            QueueRefreshEnqueueReceipt::Recovered {
                reason: "queue-schedule loop thread exited".to_string(),
                refreshed_documents: 1,
            }
        );
        assert!(stale_receiver.try_recv().is_err());
        match replacement_receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("replacement received the refresh")
        {
            Job::Refresh(received) => assert_eq!(received, file),
            Job::Due(_) => panic!("replacement received a due job instead of the refresh"),
        }
        state.running.take().unwrap().stop();
    }

    #[test]
    fn a_failed_replacement_surfaces_the_refresh_enqueue_error() {
        let (dead_sender, dead_receiver) = std::sync::mpsc::channel();
        drop(dead_receiver);
        let mut state = ProcessScheduler {
            running: Some(parked_scheduler(dead_sender)),
            ..ProcessScheduler::default()
        };

        let error = state
            .enqueue_refresh_with(Path::new("/sample/project/plan.md"), &mut || {
                anyhow::bail!("replacement refused")
            })
            .unwrap_err();
        let rendered = format!("{error:#}");

        assert!(
            rendered.contains("sending on a closed channel"),
            "{rendered}"
        );
        assert!(rendered.contains("replacement refused"), "{rendered}");
        assert!(state.running.is_none());
    }

    #[test]
    fn immediate_and_hourly_queues_have_independent_state_and_passes() {
        let content = concat!(
            "# Plan\n\n",
            "<!-- agent:queue id=main go -->\n",
            "- ship now\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues interval=1h loop -->\n",
            "- poll issues\n",
            "<!-- /agent:queue -->\n",
        );
        let (_dir, file) = project(content);
        let io = FakeIo::new(content);
        let worker = worker(&io);
        worker.refresh(&file);

        assert!(
            load_schedule_state_for_queue(&file, "main")
                .unwrap()
                .is_none()
        );
        let issues = load_schedule_state_for_queue(&file, "issues")
            .unwrap()
            .expect("issues schedule admitted");
        assert!(!issues.window_open());
        assert_eq!(
            worker.graph().scheduled_keys(),
            vec![schedule_key(&file, "issues").unwrap()]
        );

        worker.run_due(&due_for(&file, "issues"));
        assert!(
            load_schedule_state_for_queue(&file, "issues")
                .unwrap()
                .unwrap()
                .window_open()
        );
        assert!(
            load_schedule_state_for_queue(&file, "main")
                .unwrap()
                .is_none()
        );

        *io.text.lock() = content.replace("- poll issues", "- ~~poll issues~~");
        worker.refresh(&file);
        let issues = load_schedule_state_for_queue(&file, "issues")
            .unwrap()
            .unwrap();
        assert_eq!(issues.passes_completed, 1);
        assert!(io.text.lock().contains("id=main go -->\n- ship now\n"));
        assert!(
            io.text
                .lock()
                .contains("id=issues interval=1h loop -->\n- poll issues\n")
        );
    }

    #[test]
    fn two_scheduled_queues_keep_independent_deadlines_and_passes() {
        let content = concat!(
            "# Plan\n\n",
            "<!-- agent:queue id=main interval=5m loop -->\n",
            "- ship now\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues interval=1h loop -->\n",
            "- poll issues\n",
            "<!-- /agent:queue -->\n",
        );
        let (_dir, file) = project(content);
        let io = FakeIo::new(content);
        let worker = worker(&io);
        worker.refresh(&file);

        worker.run_due(&due_for(&file, "main"));
        assert!(
            load_schedule_state_for_queue(&file, "main")
                .unwrap()
                .unwrap()
                .window_open()
        );
        assert!(
            !load_schedule_state_for_queue(&file, "issues")
                .unwrap()
                .unwrap()
                .window_open()
        );
        assert_eq!(worker.graph().earliest_wake(), Some(1_000));

        worker.run_due(&due_for(&file, "issues"));
        assert!(
            load_schedule_state_for_queue(&file, "issues")
                .unwrap()
                .unwrap()
                .window_open()
        );
        assert_eq!(worker.graph().earliest_wake(), Some(1_300));

        *io.text.lock() = content.replace("- ship now", "- ~~ship now~~");
        worker.refresh(&file);
        assert_eq!(
            load_schedule_state_for_queue(&file, "main")
                .unwrap()
                .unwrap()
                .passes_completed,
            1
        );
        assert_eq!(
            load_schedule_state_for_queue(&file, "issues")
                .unwrap()
                .unwrap()
                .passes_completed,
            0
        );
        assert!(
            io.text
                .lock()
                .contains("id=main interval=5m loop -->\n- ship now\n")
        );
        assert!(
            io.text
                .lock()
                .contains("id=issues interval=1h loop -->\n- poll issues\n")
        );
    }

    #[test]
    fn explicit_queue_adopts_legacy_state_without_resetting_progress() {
        let content = doc("id=issues interval=1h loop", "- poll issues\n");
        let (_dir, file) = project(&content);
        let spec = queue_schedule::queue_schedule_spec(&content).unwrap();
        let mut legacy = queue_schedule::reconcile(None, &spec, 900);
        legacy.passes_completed = 2;
        mutate_schedule_state(
            &file,
            agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID,
            900,
            |_| (Some(legacy), ()),
        )
        .unwrap();

        let io = FakeIo::new(&content);
        worker(&io).refresh(&file);

        assert!(
            load_schedule_state_for_queue(&file, agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            load_schedule_state_for_queue(&file, "issues")
                .unwrap()
                .unwrap()
                .passes_completed,
            2
        );
        assert_eq!(
            load_schedule_state_for_queue(&file, "issues")
                .unwrap()
                .unwrap()
                .owner_queue_id
                .as_deref(),
            Some("issues")
        );
    }

    #[test]
    fn multi_queue_split_discards_ambiguous_legacy_open_pass() {
        let content = concat!(
            "# Plan\n\n",
            "<!-- agent:queue id=main go -->\n",
            "- ship now\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues interval=1h loop -->\n",
            "- poll issues\n",
            "<!-- /agent:queue -->\n",
        );
        let (_dir, file) = project(content);
        let spec = queue_schedule::queue_schedule_spec(content).unwrap();
        let mut legacy = QueueScheduleState::new(&spec, 900);
        legacy.window = queue_schedule::ScheduleWindow::Open {
            pass: 1,
            opened_secs: 900,
            roster: vec!["ship now".into(), "poll issues".into()],
            ran: Vec::new(),
        };
        mutate_schedule_state(
            &file,
            agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID,
            900,
            |_| (Some(legacy), ()),
        )
        .unwrap();

        let io = FakeIo::new(content);
        let worker = worker(&io);
        worker.refresh(&file);

        assert!(
            load_schedule_state_for_queue(&file, agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID)
                .unwrap()
                .is_none()
        );
        let issues = load_schedule_state_for_queue(&file, "issues")
            .unwrap()
            .expect("issues schedule admitted from its own queue");
        assert!(!issues.window_open());
        assert_eq!(issues.owner_queue_id.as_deref(), Some("issues"));
        assert!(io.logs.lock().iter().any(|line| line.contains(
            "queue_schedule_legacy_state_discarded queue_id=issues reason=ambiguous_multi_queue_owner"
        )));

        worker.run_due(&due_for(&file, "issues"));
        let issues = load_schedule_state_for_queue(&file, "issues")
            .unwrap()
            .unwrap();
        assert_eq!(
            issues.window,
            queue_schedule::ScheduleWindow::Open {
                pass: 1,
                opened_secs: 1_000,
                roster: vec!["poll issues".into()],
                ran: Vec::new(),
            }
        );
    }

    #[test]
    fn refresh_repairs_pre_provenance_multi_queue_pass_before_it_can_stay_busy() {
        let content = concat!(
            "# Plan\n\n",
            "<!-- agent:queue id=main go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues interval=1h loop -->\n",
            "- poll issues\n",
            "<!-- /agent:queue -->\n",
        );
        let (_dir, file) = project(content);
        let spec = queue_schedule::queue_schedule_spec(content).unwrap();
        let mut contaminated = QueueScheduleState::new(&spec, 900);
        contaminated.window = queue_schedule::ScheduleWindow::Open {
            pass: 1,
            opened_secs: 700,
            roster: vec!["old main task".into(), "poll issues".into()],
            ran: Vec::new(),
        };
        contaminated.last_fired_due_secs = Some(900);
        contaminated.passes_completed = 7;
        mutate_schedule_state(&file, "issues", 900, |_| (Some(contaminated), ())).unwrap();

        let io = FakeIo::new(content);
        let worker = worker(&io);
        worker.refresh(&file);

        let repaired = load_schedule_state_for_queue(&file, "issues")
            .unwrap()
            .unwrap();
        assert!(!repaired.window_open());
        assert_eq!(repaired.last_fired_due_secs, None);
        assert_eq!(repaired.passes_completed, 0);
        assert_eq!(repaired.owner_queue_id.as_deref(), Some("issues"));
        assert_eq!(worker.graph().earliest_wake(), Some(1_000));
        assert!(io.logs.lock().iter().any(|line| line.contains(
            "queue_schedule_legacy_state_reset queue_id=issues reason=ambiguous_multi_queue_owner"
        )));

        worker.run_due(&due_for(&file, "issues"));
        let opened = load_schedule_state_for_queue(&file, "issues")
            .unwrap()
            .unwrap();
        assert_eq!(
            opened.window,
            queue_schedule::ScheduleWindow::Open {
                pass: 1,
                opened_secs: 1_000,
                roster: vec!["poll issues".into()],
                ran: Vec::new(),
            }
        );
    }

    #[test]
    fn invalid_edit_holds_only_that_queue_and_preserves_its_progress() {
        let valid = concat!(
            "# Plan\n\n",
            "<!-- agent:queue id=main go -->\n",
            "- ship now\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues interval=1h loop -->\n",
            "- poll issues\n",
            "<!-- /agent:queue -->\n",
        );
        let (_dir, file) = project(valid);
        let io = FakeIo::new(valid);
        let worker = worker(&io);
        worker.refresh(&file);
        worker.run_due(&due_for(&file, "issues"));
        let before = load_schedule_state_for_queue(&file, "issues")
            .unwrap()
            .unwrap();

        *io.text.lock() = valid.replace("interval=1h", "interval='1 hour'");
        worker.refresh(&file);

        assert_eq!(
            load_schedule_state_for_queue(&file, "issues")
                .unwrap()
                .unwrap(),
            before
        );
        assert!(worker.graph().scheduled_keys().is_empty());
        assert!(
            load_schedule_state_for_queue(&file, "main")
                .unwrap()
                .is_none()
        );
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

    /// GH #327 + `#loopnoresurrect` on a multi-queue document. Every binary
    /// drain keeps a `loop` queue's struck row, so the pass-4 shape (struck,
    /// never seen claimed) restores the item. A row that is claimed and then
    /// erased with no struck copy was removed by the operator (deleted, or cut
    /// into `agent:run`): the pass completes and restores nothing, and the
    /// sibling queue's struck copy is never a restore source.
    #[test]
    fn loop_pass_restores_a_struck_item_and_never_an_erased_one() {
        let queues = |issues_body: &str| {
            format!(
                concat!(
                    "<!-- agent:queue subagents id=main go -->\n",
                    "- ~~#upgrade~~\n",
                    "<!-- /agent:queue -->\n\n",
                    "<!-- agent:queue loop interval=\"1h\" id=issues -->\n",
                    "{}",
                    "<!-- /agent:queue -->\n",
                ),
                issues_body
            )
        };
        let content = queues("- #upgrade\n");
        let (_dir, file) = project(&content);
        let io = FakeIo::new(&content);
        let worker = worker(&io);
        worker.refresh(&file);
        let due = due_for(&file, "issues");

        // Pass 1 (the pass-4 shape): struck without ever being seen claimed.
        worker.run_due(&due);
        assert!(
            load_schedule_state_for_queue(&file, "issues")
                .unwrap()
                .unwrap()
                .window_open()
        );
        *io.text.lock() = queues("- ~~#upgrade~~\n");
        worker.refresh(&file);
        let state = load_schedule_state_for_queue(&file, "issues")
            .unwrap()
            .unwrap();
        assert_eq!(state.passes_completed, 1);
        assert_eq!(
            io.restores.lock().last(),
            Some(&vec!["#upgrade".to_string()])
        );
        assert_eq!(*io.text.lock(), content, "pass 1 restored the struck item");

        // Pass 2: claimed, then the row is removed outright.
        *io.now.lock() = 1_000 + 3_600;
        worker.run_due(&due);
        *io.text.lock() = queues("- 🚧 #upgrade\n");
        worker.refresh(&file);
        *io.text.lock() = queues("");
        worker.refresh(&file);
        let state = load_schedule_state_for_queue(&file, "issues")
            .unwrap()
            .unwrap();
        assert_eq!(state.passes_completed, 2);
        assert_eq!(state.restore_pending, None);
        assert_eq!(
            *io.text.lock(),
            queues(""),
            "pass 2 did not resurrect the removed row"
        );
        assert_eq!(io.restores.lock().len(), 1);
    }

    /// GH #335: if the one post-pass restore write was lost, the next due
    /// tick rebuilds the loop roster from its struck rows instead of leaving
    /// the queue in `empty_poll` forever.
    #[test]
    fn idle_loop_tick_recovers_struck_rows_when_restore_pending_was_lost() {
        let struck = doc("interval=1h loop", "- ~~#upgrade~~\n");
        let live = doc("interval=1h loop", "- #upgrade\n");
        let (_dir, file) = project(&struck);
        let io = FakeIo::new(&struck);
        let worker = worker(&io);
        worker.refresh(&file);
        mutate_schedule_state(
            &file,
            agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID,
            1_000,
            |prev| {
                let mut state = prev.expect("schedule admitted");
                state.passes_completed = 9;
                state.window = queue_schedule::ScheduleWindow::Closed;
                state.restore_pending = None;
                (Some(state), ())
            },
        )
        .unwrap();

        worker.run_due(&due(&file));

        let state = load_schedule_state(&file).unwrap().unwrap();
        assert_eq!(*io.text.lock(), live);
        assert_eq!(state.passes_completed, 9);
        assert_eq!(state.restore_pending, None);
        assert_eq!(
            state.window,
            queue_schedule::ScheduleWindow::Open {
                pass: 10,
                opened_secs: 1_000,
                roster: vec!["#upgrade".into()],
                ran: Vec::new(),
            }
        );
        assert_eq!(
            io.restores.lock().as_slice(),
            &[vec![String::from("#upgrade")]]
        );
        assert!(io.logs.lock().iter().any(|line| {
            line.contains("queue_schedule_tick") && line.contains("decision=recovered")
        }));
    }

    /// GH #348: a failed self-heal still sees the old struck row. That row is
    /// the pending restore's input, not proof that the recovered pass ran.
    #[test]
    fn failed_idle_recovery_neither_completes_the_new_pass_nor_logs_epoch_deadline() {
        let struck = doc("interval=1h loop", "- ~~#upgrade~~\n");
        let (_dir, file) = project(&struck);
        let io = FakeIo::new(&struck);
        let worker = worker(&io);
        worker.refresh(&file);
        mutate_schedule_state(
            &file,
            agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID,
            1_000,
            |prev| {
                let mut state = prev.expect("schedule admitted");
                state.passes_completed = 14;
                state.window = queue_schedule::ScheduleWindow::Closed;
                state.restore_pending = None;
                (Some(state), ())
            },
        )
        .unwrap();
        io.fail_restore
            .store(true, std::sync::atomic::Ordering::SeqCst);

        worker.run_due(&due(&file));
        worker.refresh(&file);

        let state = load_schedule_state(&file).unwrap().unwrap();
        assert_eq!(state.passes_completed, 14);
        assert_eq!(
            state.current_pass(),
            Some(15),
            "the recovered pass remains open until its roster is visibly restored"
        );
        assert_eq!(state.restore_pending, Some(vec!["#upgrade".into()]));
        assert_eq!(state.backoff_until_secs, Some(1_030));
        let logs = io.logs.lock();
        assert!(logs.iter().all(|line| {
            !line.contains("queue_schedule_pass_completed queue_id=default pass=15")
        }));
        let recovered = logs
            .iter()
            .find(|line| line.contains("decision=recovered"))
            .expect("recovered tick log");
        assert!(
            recovered.contains("next_run=1970-01-01T00:17:10Z"),
            "{recovered}"
        );
        assert!(!recovered.contains("next_run=1970-01-01T00:00:00Z"));
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
        *io.text.lock() = doc("loop", "- ~~a~~\n");
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
