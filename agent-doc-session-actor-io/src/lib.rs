//! # Module: session_actor
//!
//! ## Spec
//! - Freezes the phase-1 single-owner session-actor contract while introducing
//!   the phase-2 durable actor projection.
//! - Ownership generations are monotonic per document session and start at `1`.
//! - A new authoritative generation is recorded whenever a new `agent-doc start`
//!   session begins or an existing document session is rebound to a different pane.
//! - Legacy generation inference is delegated to the pure supervisor lifecycle
//!   API, which remains backward-compatible with logs that only have
//!   `session_start` lines and no explicit generation markers.
//! - The Project Controller owns the live actor Source, keyed by canonical
//!   document path. SQLite hydrates that Source on cold start and durably records
//!   accepted transitions. Legacy JSON actor/session projections are not read as
//!   bootstrap authority.
//!
//! ## Agentic Contracts
//! - The controller actor Source is authoritative for the latest generation
//!   while the controller is live.
//! - Store updates must be monotonic and fail closed on generation regressions.
//! - Legacy logs with at least one `session_start` but no explicit generation
//!   markers infer the current generation from the number of starts.
//!
//! ## Evals
//! - `project_binding_stores_initial_generation`
//! - `project_binding_advances_generation_on_new_pane`
//! - `record_session_start_persists_authoritative_record`

use agent_doc_harness::{document_harness_from_content, normalize_harness_name};
use agent_doc_supervisor::{OwnershipGeneration, infer_latest_generation_from_content};
use anyhow::Result;
use std::path::{Path, PathBuf};

use agent_doc_controller::actor::{ActorLastTransition, ActorRecord, ActorState};
use agent_doc_sqlite::state_store;

type ActorStore = std::collections::BTreeMap<String, ActorRecord>;

#[derive(Debug, Clone)]
struct ActorRecordUpdate<'a> {
    session_id: &'a str,
    pane_id: &'a str,
    window_id: &'a str,
    harness: &'a str,
    state: ActorState,
    caller: &'a str,
    reason: &'a str,
    generation: u64,
    expected_prior_generation: Option<u64>,
}

fn log_path(file: &Path, session_id: &str) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(None);
    };
    Ok(Some(
        root.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
    ))
}

fn timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn document_path_from_base_dir(base_dir: &Path, file: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn canonical_document_id(file: &Path) -> Result<Option<String>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    Ok(Some(canonical.to_string_lossy().to_string()))
}

pub fn canonical_document_id_in(base_dir: &Path, file: &str) -> String {
    canonical_document_id(&document_path_from_base_dir(base_dir, file))
        .ok()
        .flatten()
        .unwrap_or_else(|| tmux_router::registry::canonical_registry_key_in(base_dir, file))
}

fn load_store_in(base_dir: &Path) -> Result<ActorStore> {
    let conn = state_store::open_state_db(base_dir)?;
    state_store::load_actor_store_from_db(&conn)
}

fn load_actor_record_by_document_id(
    base_dir: &Path,
    document_id: &str,
) -> Result<Option<ActorRecord>> {
    let conn = state_store::open_state_db(base_dir)?;
    state_store::load_actor_record_from_db(&conn, document_id)
}

fn store_actor_record_in(
    base_dir: &Path,
    expected_prior_generation: Option<u64>,
    record: &ActorRecord,
) -> Result<ActorRecord> {
    let mut conn = state_store::open_state_db(base_dir)?;
    state_store::store_actor_record_tx(&mut conn, expected_prior_generation, record, None, None)?;
    Ok(record.clone())
}

/// `#closeout-recovery-state-machine` debug API: load every actor record in the
/// project store, keyed by canonical document id. Exposed for state-drift
/// investigation across all actors (`agent-doc session debug`), where each
/// record is cross-referenced with its document's cycle phase and closeout
/// recovery classification.
pub fn load_all_records_in(base_dir: &Path) -> Result<ActorStore> {
    load_store_in(base_dir)
}

fn legacy_generation_for_document(file: &Path, session_id_hint: Option<&str>) -> Result<u64> {
    let Some(canonical) = file.canonicalize().ok() else {
        return Ok(0);
    };
    let Some(session_id) = session_id_hint
        .map(ToOwned::to_owned)
        .or_else(|| agent_doc_frontmatter_io::session::read_session_id(&canonical))
    else {
        return Ok(0);
    };
    let Some(path) = log_path(&canonical, &session_id)? else {
        return Ok(0);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(0);
    };
    Ok(infer_latest_generation_from_content(&content))
}

fn current_generation_in(
    base_dir: &Path,
    file: &str,
    session_id_hint: Option<&str>,
) -> Result<u64> {
    let document_id = canonical_document_id_in(base_dir, file);
    let store = load_store_in(base_dir)?;
    if let Some(record) = store.get(&document_id) {
        return Ok(record.generation);
    }
    legacy_generation_for_document(
        &document_path_from_base_dir(base_dir, file),
        session_id_hint,
    )
}

fn store_record_in(
    base_dir: &Path,
    file: &str,
    update: ActorRecordUpdate<'_>,
) -> Result<ActorRecord> {
    let document_id = canonical_document_id_in(base_dir, file);
    let current_record = load_actor_record_by_document_id(base_dir, &document_id)?;
    let prior_generation = current_record
        .as_ref()
        .map(|record| record.generation)
        .unwrap_or_else(|| {
            current_generation_in(base_dir, file, Some(update.session_id)).unwrap_or(0)
        });
    if let Some(expected) = update.expected_prior_generation
        && prior_generation != expected
    {
        anyhow::bail!(
            "actor generation compare-and-swap failed for {}: expected {}, found {}",
            document_id,
            expected,
            prior_generation
        );
    }
    if update.generation < prior_generation {
        anyhow::bail!(
            "actor generation regression for {}: attempted {}, current {}",
            document_id,
            update.generation,
            prior_generation
        );
    }
    let record = ActorRecord {
        document_id: document_id.clone(),
        session_id: update.session_id.to_string(),
        generation: update.generation,
        pane_id: update.pane_id.to_string(),
        window_id: update.window_id.to_string(),
        harness: normalize_harness_name(update.harness),
        state: update.state,
        last_transition: ActorLastTransition {
            caller: update.caller.to_string(),
            reason: update.reason.to_string(),
            timestamp: timestamp_secs(),
            prior_generation,
            new_generation: update.generation,
        },
    };
    let stored = store_actor_record_in(base_dir, update.expected_prior_generation, &record)?;
    Ok(stored)
}

pub fn detect_document_harness_in(base_dir: &Path, file: &str) -> String {
    let document_path = document_path_from_base_dir(base_dir, file);
    if let Ok(content) = std::fs::read_to_string(&document_path)
        && let Some(harness) = document_harness_from_content(&content)
    {
        return harness;
    }
    normalize_harness_name(&agent_doc_model_tier::detect_harness())
}

pub fn load_record_in(base_dir: &Path, file: &str) -> Result<Option<ActorRecord>> {
    let document_id = canonical_document_id_in(base_dir, file);
    Ok(load_store_in(base_dir)?.get(&document_id).cloned())
}

pub fn project_binding_in(
    base_dir: &Path,
    file: &str,
    session_id: &str,
    pane_id: &str,
    window_id: &str,
    caller: &str,
    reason: &str,
) -> Result<OwnershipGeneration> {
    let prior_generation = current_generation_in(base_dir, file, Some(session_id))?;
    let generation = load_record_in(base_dir, file)?
        .filter(|record| record.pane_id == pane_id)
        .map(|record| record.generation)
        .unwrap_or_else(|| prior_generation.saturating_add(1).max(1));
    let harness = detect_document_harness_in(base_dir, file);
    let _ = store_record_in(
        base_dir,
        file,
        ActorRecordUpdate {
            session_id,
            pane_id,
            window_id,
            harness: &harness,
            state: ActorState::Ready,
            caller,
            reason,
            generation,
            expected_prior_generation: Some(prior_generation),
        },
    )?;
    Ok(OwnershipGeneration {
        prior_generation,
        new_generation: generation,
    })
}

pub fn record_session_start_direct(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    window_id: &str,
    generation: u64,
) -> Result<ActorRecord> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(base_dir) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        anyhow::bail!(
            "failed to locate project root for actor record {}",
            canonical.display()
        );
    };
    let harness = detect_document_harness_in(&base_dir, &canonical.to_string_lossy());
    store_record_in(
        &base_dir,
        &canonical.to_string_lossy(),
        ActorRecordUpdate {
            session_id,
            pane_id,
            window_id,
            harness: &harness,
            state: ActorState::Starting,
            caller: "start",
            reason: "session_start",
            generation,
            expected_prior_generation: Some(generation.saturating_sub(1)),
        },
    )
}

#[allow(dead_code)] // Compatibility API for direct callers; supervisor path uses controller IPC.
pub fn record_session_start(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    window_id: &str,
    generation: u64,
) -> Result<ActorRecord> {
    record_session_start_direct(file, session_id, pane_id, window_id, generation)
}

/// Rewrite the harness identity on the authoritative actor record after an in-loop
/// `agent:` switch spawned a fresh harness (`#actorharnessrecordwriteback`).
///
/// `transition_state_in` deliberately CARRIES FORWARD `current.harness` on every
/// lifecycle transition, so once a record says `codex` it keeps saying `codex` for
/// the life of the generation — even after the supervisor tore that child down and
/// spawned `claude` in its place. Route then compares the stale stored harness
/// against the frontmatter-resolved one and defers to a boundary restart that has
/// already run. Only the supervisor knows which harness is ACTUALLY live, so it
/// calls this at the moment of the swap; the document's frontmatter is not a safe
/// substitute (it flips the instant the operator edits `agent:`, before any restart).
///
/// Pane/generation/state are preserved — this is an identity correction on the
/// existing generation, not a rebind.
pub fn set_record_harness_in(
    base_dir: &Path,
    document_id: &str,
    session_id: &str,
    pane_id: &str,
    harness: &str,
) -> Result<ActorRecord> {
    let Some(current) = load_record_in(base_dir, document_id)? else {
        anyhow::bail!("missing authoritative actor record for {}", document_id);
    };
    if current.session_id != session_id {
        anyhow::bail!(
            "stale actor harness writeback for {}: session {} no longer owns generation {} (current session {})",
            document_id,
            session_id,
            current.generation,
            current.session_id
        );
    }
    if current.pane_id != pane_id {
        anyhow::bail!(
            "stale actor harness writeback for {}: pane {} no longer owns generation {} (current pane {})",
            document_id,
            pane_id,
            current.generation,
            current.pane_id
        );
    }
    let normalized = normalize_harness_name(harness);
    if normalized == current.harness {
        return Ok(current);
    }
    store_record_in(
        base_dir,
        document_id,
        ActorRecordUpdate {
            session_id,
            pane_id,
            window_id: &current.window_id,
            harness: &normalized,
            state: current.state,
            caller: "supervisor",
            reason: "agent_harness_switch_writeback",
            generation: current.generation,
            expected_prior_generation: Some(current.generation),
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub fn transition_state_in(
    base_dir: &Path,
    document_id: &str,
    session_id: &str,
    pane_id: &str,
    expected_generation: Option<u64>,
    state: ActorState,
    caller: &str,
    reason: &str,
) -> Result<ActorRecord> {
    let Some(current) = load_record_in(base_dir, document_id)? else {
        anyhow::bail!("missing authoritative actor record for {}", document_id);
    };
    if current.session_id != session_id {
        anyhow::bail!(
            "stale actor transition for {}: session {} no longer owns generation {} (current session {})",
            document_id,
            session_id,
            current.generation,
            current.session_id
        );
    }
    if let Some(expected) = expected_generation
        && current.generation != expected
        && current.pane_id != pane_id
    {
        anyhow::bail!(
            "stale actor transition for {}: generation {} pane {} no longer current (current generation {} pane {})",
            document_id,
            expected,
            pane_id,
            current.generation,
            current.pane_id
        );
    }
    if current.pane_id != pane_id {
        anyhow::bail!(
            "stale actor transition for {}: pane {} no longer owns generation {} (current pane {})",
            document_id,
            pane_id,
            current.generation,
            current.pane_id
        );
    }
    let harness = if current.harness.trim().is_empty() {
        detect_document_harness_in(base_dir, document_id)
    } else {
        current.harness.clone()
    };
    store_record_in(
        base_dir,
        document_id,
        ActorRecordUpdate {
            session_id,
            pane_id,
            window_id: &current.window_id,
            harness: &harness,
            state,
            caller,
            reason,
            generation: current.generation,
            expected_prior_generation: Some(current.generation),
        },
    )
}

pub fn transition_state_direct(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    expected_generation: Option<u64>,
    state: ActorState,
    caller: &str,
    reason: &str,
) -> Result<ActorRecord> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(base_dir) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        anyhow::bail!(
            "failed to locate project root for actor record {}",
            canonical.display()
        );
    };
    let file_key = canonical.to_string_lossy().to_string();
    transition_state_in(
        &base_dir,
        &file_key,
        session_id,
        pane_id,
        expected_generation,
        state,
        caller,
        reason,
    )
}

/// Path-based wrapper for [`set_record_harness_in`] (`#actorharnessrecordwriteback`).
pub fn set_record_harness_direct(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    harness: &str,
) -> Result<ActorRecord> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(base_dir) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        anyhow::bail!(
            "failed to locate project root for actor record {}",
            canonical.display()
        );
    };
    let file_key = canonical.to_string_lossy().to_string();
    set_record_harness_in(&base_dir, &file_key, session_id, pane_id, harness)
}

#[allow(dead_code)] // Compatibility API for direct callers; supervisor path uses controller IPC.
pub fn transition_state(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    state: ActorState,
    caller: &str,
    reason: &str,
) -> Result<ActorRecord> {
    transition_state_direct(file, session_id, pane_id, None, state, caller, reason)
}

pub fn infer_latest_generation(file: &Path, session_id: &str) -> Result<u64> {
    let actor_generation = file
        .canonicalize()
        .ok()
        .and_then(|canonical| {
            agent_doc_project_root_io::project_root_containing(&canonical).and_then(|root| {
                load_record_in(&root, &canonical.to_string_lossy())
                    .ok()
                    .flatten()
                    .map(|record| record.generation)
            })
        })
        .unwrap_or(0);
    // The controller actor record is the authoritative committed generation: it
    // only advances through the SQLite CAS in `store_actor_record_tx`. The
    // session log's `session_start` / `ownership_transition` lines, by contrast,
    // are written OPTIMISTICALLY in `start/run.rs` BEFORE the controller commits
    // the start — so a start that loses the controller CAS still appends its
    // intended (un-committed) generation to the log. Maxing the actor record
    // with the log would then feed the *next* start a prior generation higher
    // than the controller actually holds; the controller CAS expects
    // `start_generation - 1 == committed`, so it rejects again, appends a yet
    // higher generation to the log, and the divergence runs away — the start
    // wedges forever with `compare-and-swap failed: expected <log>, found <db>`.
    // When a controller actor record exists it is therefore the sole authority;
    // the log is consulted only as a bootstrap/legacy fallback before the
    // control plane has any record for this document.
    if actor_generation > 0 {
        return Ok(actor_generation);
    }
    let Some(path) = log_path(file, session_id)? else {
        return Ok(0);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(0);
    };
    Ok(infer_latest_generation_from_content(&content))
}

pub fn next_generation(file: &Path, session_id: &str) -> Result<OwnershipGeneration> {
    let prior_generation = infer_latest_generation(file, session_id)?;
    Ok(OwnershipGeneration {
        prior_generation,
        new_generation: prior_generation.saturating_add(1).max(1),
    })
}

// ===========================================================================
// #pcpc1 — Session-actor message runtime
// ===========================================================================
//
// Everything above is the *data*/CAS layer: free functions guarded by SQLite
// compare-and-swap. That makes ownership monotonic across processes, but it
// does NOT serialize the concurrent in-process writers (write submit, closeout,
// queue-head selection, lifecycle transitions) that race on a single document
// today — they are only ordered by advisory `flock` + per-call CAS.
//
// This section turns the session actor from a pure data struct into a real
// in-process actor: a per-document mailbox (command channel) drained on a
// single owning thread, so every document-scoped op for one document executes
// in one serialized order. The SQLite CAS stays the durable cross-process
// backstop — the mailbox serializes the *same-process* writers, CAS still
// fails closed on a stale generation from another process.
//
// Phase 1 (this commit) establishes the runtime + a registry that funnels all
// callers for one document into one mailbox, and proves serialization with a
// deterministic SimWorld test. `#pcpc3` routes the concrete write/stream/
// finalize/repair + supervisor idle/file-watch disk writes through
// `DocumentActor::submit`; phase 1 keeps the existing call sites unchanged so
// the runtime can land independently shippable.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};

use agent_doc_document_realtime::session_ops::SessionOpKind;

/// State the owner thread carries across the ops it serializes for one
/// document. Jobs receive `&mut ActorContext` so #pcpc3 can thread the in-flight
/// write/closeout state through the single owner without re-locking.
pub struct ActorContext {
    pub doc_id: String,
    pub base_dir: PathBuf,
    /// Count of ops the owner thread has drained. Plain (non-atomic) on
    /// purpose: it is only ever touched on the single owner thread, so a racing
    /// writer would be a serialization bug the SimWorld test detects.
    processed: u64,
    /// Kind of the op currently/most-recently drained — a #pcpc3 observability
    /// hook so the serialized owner can branch write/closeout/queue/lifecycle.
    last_op: Option<SessionOpKind>,
}

impl ActorContext {
    /// Number of ops this actor has drained so far. Monotonic, owner-thread only.
    pub fn processed(&self) -> u64 {
        self.processed
    }

    /// Kind of the op the owner thread is currently draining.
    pub fn last_op(&self) -> Option<SessionOpKind> {
        self.last_op
    }
}

/// A job plus its bookkeeping, delivered over the mailbox. The closure captures
/// its own typed reply channel so `submit::<R>` can return an arbitrary `R`
/// while the envelope itself stays a single non-generic type.
struct Envelope {
    kind: SessionOpKind,
    run: Box<dyn FnOnce(&mut ActorContext) + Send>,
}

/// A per-document in-process actor: a mailbox (`Sender<Envelope>`) feeding a
/// single owner thread that drains commands in arrival order. Cloneable handle
/// semantics come from wrapping it in `Arc` inside the registry; the owner
/// thread lives until the last sender is dropped.
pub struct DocumentActor {
    doc_id: String,
    tx: Option<Sender<Envelope>>,
    handle: Option<JoinHandle<()>>,
}

impl DocumentActor {
    /// Spawn the owner thread for `doc_id`, rooted at `base_dir`.
    pub fn spawn(doc_id: impl Into<String>, base_dir: impl Into<PathBuf>) -> DocumentActor {
        let doc_id = doc_id.into();
        let base_dir = base_dir.into();
        let (tx, rx) = mpsc::channel::<Envelope>();
        let actor_doc = doc_id.clone();
        let handle = thread::Builder::new()
            .name(format!("session-actor:{doc_id}"))
            .spawn(move || run_owner_loop(actor_doc, base_dir, rx))
            .expect("spawn session-actor owner thread");
        DocumentActor {
            doc_id,
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    /// Canonical document id this actor owns.
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }

    /// Enqueue `job` without blocking on its result. Returns the reply receiver
    /// so a caller can pipeline several ops and collect later. The job runs on
    /// the owner thread, serialized after every previously enqueued op.
    pub fn enqueue<R, F>(&self, kind: SessionOpKind, job: F) -> Result<Receiver<R>>
    where
        R: Send + 'static,
        F: FnOnce(&mut ActorContext) -> R + Send + 'static,
    {
        let tx = self
            .tx
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("session actor for {} is shutting down", self.doc_id))?;
        let (reply_tx, reply_rx) = mpsc::channel::<R>();
        let envelope = Envelope {
            kind,
            run: Box::new(move |ctx| {
                let out = job(ctx);
                if reply_tx.send(out).is_err() {
                    // Never swallow silently: the caller dropped its receiver
                    // before the op finished. Harmless for fire-and-forget ops,
                    // but log so a lost closeout reply is debuggable.
                    eprintln!(
                        "[session-actor] {} op {:?} reply dropped before delivery",
                        ctx.doc_id, kind
                    );
                }
            }),
        };
        tx.send(envelope).map_err(|_| {
            anyhow::anyhow!("session actor for {} is no longer running", self.doc_id)
        })?;
        Ok(reply_rx)
    }

    /// Enqueue fire-and-forget document work without manufacturing a reply
    /// receiver. Reactive Effects use this path after an exact target has been
    /// admitted: convergence may be slow, but the projection thread must not
    /// wait and an intentionally absent receiver must not be logged as a lost
    /// closeout reply.
    pub fn enqueue_detached<F>(&self, kind: SessionOpKind, job: F) -> Result<()>
    where
        F: FnOnce(&mut ActorContext) + Send + 'static,
    {
        let tx = self
            .tx
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("session actor for {} is shutting down", self.doc_id))?;
        tx.send(Envelope {
            kind,
            run: Box::new(job),
        })
        .map_err(|_| anyhow::anyhow!("session actor for {} is no longer running", self.doc_id))
    }

    /// Enqueue `job` and block until the owner thread runs it, returning its
    /// result. This is the serialized critical-section entrypoint #pcpc3 routes
    /// write/closeout/queue-head/lifecycle ops through.
    pub fn submit<R, F>(&self, kind: SessionOpKind, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce(&mut ActorContext) -> R + Send + 'static,
    {
        let reply_rx = self.enqueue(kind, job)?;
        reply_rx.recv().map_err(|_| {
            anyhow::anyhow!("session actor for {} dropped before replying", self.doc_id)
        })
    }
}

impl Drop for DocumentActor {
    fn drop(&mut self) {
        // Drop the sender first so the owner loop sees the channel close and
        // returns, then join. Order matters: joining before dropping `tx` would
        // deadlock because the loop only exits once every sender is gone.
        self.tx.take();
        if let Some(handle) = self.handle.take()
            && let Err(panic) = handle.join()
        {
            eprintln!(
                "[session-actor] owner thread for {} panicked: {:?}",
                self.doc_id, panic
            );
        }
    }
}

fn run_owner_loop(doc_id: String, base_dir: PathBuf, rx: Receiver<Envelope>) {
    let mut ctx = ActorContext {
        doc_id,
        base_dir,
        processed: 0,
        last_op: None,
    };
    while let Ok(envelope) = rx.recv() {
        ctx.processed = ctx.processed.saturating_add(1);
        ctx.last_op = Some(envelope.kind);
        (envelope.run)(&mut ctx);
    }
}

/// Process-wide registry of per-document actors. Every caller for a given
/// canonical document id is funneled to the one mailbox, which is what makes
/// the serialization hold across the whole process (not just within one call
/// site). #pcpc3 looks the actor up here before routing a disk write.
pub struct SessionActorRuntime {
    actors: Mutex<HashMap<String, Arc<DocumentActor>>>,
}

impl SessionActorRuntime {
    fn new() -> SessionActorRuntime {
        SessionActorRuntime {
            actors: Mutex::new(HashMap::new()),
        }
    }

    /// Get the actor for `doc_id`, spawning it (rooted at `base_dir`) on first
    /// use. Subsequent calls with the same id return the same mailbox.
    pub fn actor_for(&self, doc_id: &str, base_dir: &Path) -> Arc<DocumentActor> {
        let mut actors = self.actors.lock();
        actors
            .entry(doc_id.to_string())
            .or_insert_with(|| Arc::new(DocumentActor::spawn(doc_id, base_dir.to_path_buf())))
            .clone()
    }

    /// Drop the actor for `doc_id` if present, returning whether one existed.
    /// The owner thread is joined when the last `Arc` is dropped.
    pub fn shutdown(&self, doc_id: &str) -> bool {
        let mut actors = self.actors.lock();
        actors.remove(doc_id).is_some()
    }
}

/// The process-wide session-actor runtime singleton.
pub fn runtime() -> &'static SessionActorRuntime {
    static RUNTIME: OnceLock<SessionActorRuntime> = OnceLock::new();
    RUNTIME.get_or_init(SessionActorRuntime::new)
}

/// Convenience: resolve the actor for `file` under `base_dir` (keyed by the
/// canonical document id) from the global runtime.
pub fn document_actor_in(base_dir: &Path, file: &str) -> Arc<DocumentActor> {
    let doc_id = canonical_document_id_in(base_dir, file);
    runtime().actor_for(&doc_id, base_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn seed_project_file(dir: &tempfile::TempDir, rel: &str, content: &str) -> PathBuf {
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join(rel);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&file, content).unwrap();
        file
    }

    #[test]
    fn project_binding_stores_initial_generation() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-1\nagent: codex\n---\nBody\n",
        );

        let generations = project_binding_in(
            tmp.path(),
            &file.to_string_lossy(),
            "session-1",
            "%41",
            "@1",
            "route",
            "dispatch_bind",
        )
        .unwrap();

        assert_eq!(
            generations,
            OwnershipGeneration {
                prior_generation: 0,
                new_generation: 1
            }
        );
        let record = load_record_in(tmp.path(), &file.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(record.generation, 1);
        assert_eq!(record.pane_id, "%41");
        assert_eq!(record.state, ActorState::Ready);
        assert_eq!(record.harness, "codex");
    }

    #[test]
    fn infer_latest_generation_ignores_optimistic_log_ahead_of_actor_record() {
        // Regression for the runaway `compare-and-swap failed: expected <log>,
        // found <db>` start wedge (#startgenlogdrift). The session log's
        // `session_start generation=N` lines are written optimistically in
        // `start/run.rs` BEFORE the controller commits a start, so a start that
        // loses the controller CAS still inflates the log past the committed
        // actor generation. The committed controller actor record must stay the
        // sole authority — letting the log drag the inferred generation above it
        // would feed the next start an un-committable prior generation and wedge
        // the CAS forever.
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/efs.md",
            "---\nagent_doc_session: efs\nagent: codex\n---\nBody\n",
        );

        // Commit a controller actor at generation 1 (the last *successful* start).
        project_binding_in(
            tmp.path(),
            &file.to_string_lossy(),
            "efs",
            "%44",
            "@1",
            "start",
            "session_start",
        )
        .unwrap();
        let record = load_record_in(tmp.path(), &file.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(record.generation, 1);

        // Simulate failed starts that climbed the optimistic session log to 11
        // without ever committing past generation 1.
        let log = tmp.path().join(".agent-doc/logs/efs.log");
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(
            &log,
            "[1] session_start file=tasks/efs.md pane=%44 session=efs generation=1\n\
             [2] session_start file=tasks/efs.md pane=%45 session=efs generation=11\n",
        )
        .unwrap();

        // Inference trusts the committed actor record (1), not the inflated log
        // (11), so the next start hands the controller a committable prior.
        assert_eq!(infer_latest_generation(&file, "efs").unwrap(), 1);
        let generations = next_generation(&file, "efs").unwrap();
        assert_eq!(generations.prior_generation, 1);
        assert_eq!(generations.new_generation, 2);
    }

    #[test]
    fn load_all_records_returns_every_bound_actor() {
        // `#closeout-recovery-state-machine`: the debug API surfaces every actor
        // in the project store, keyed by canonical document id.
        let tmp = tempfile::TempDir::new().unwrap();
        let a = seed_project_file(
            &tmp,
            "tasks/a.md",
            "---\nagent_doc_session: s-a\nagent: codex\n---\nA\n",
        );
        let b = seed_project_file(
            &tmp,
            "tasks/b.md",
            "---\nagent_doc_session: s-b\nagent: claude\n---\nB\n",
        );
        project_binding_in(
            tmp.path(),
            &a.to_string_lossy(),
            "s-a",
            "%10",
            "@1",
            "route",
            "dispatch_bind",
        )
        .unwrap();
        project_binding_in(
            tmp.path(),
            &b.to_string_lossy(),
            "s-b",
            "%20",
            "@1",
            "route",
            "dispatch_bind",
        )
        .unwrap();

        let store = load_all_records_in(tmp.path()).unwrap();
        assert_eq!(store.len(), 2, "expected both actors: {store:?}");
        let panes: std::collections::BTreeSet<&str> =
            store.values().map(|r| r.pane_id.as_str()).collect();
        assert!(panes.contains("%10") && panes.contains("%20"), "{panes:?}");
    }

    #[test]
    fn project_binding_advances_generation_on_new_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-2\nagent: codex\n---\nBody\n",
        );

        project_binding_in(
            tmp.path(),
            &file.to_string_lossy(),
            "session-2",
            "%41",
            "@1",
            "route",
            "dispatch_bind",
        )
        .unwrap();
        let generations = project_binding_in(
            tmp.path(),
            &file.to_string_lossy(),
            "session-2",
            "%52",
            "@2",
            "sync",
            "recover_owner",
        )
        .unwrap();

        assert_eq!(
            generations,
            OwnershipGeneration {
                prior_generation: 1,
                new_generation: 2
            }
        );
        let record = load_record_in(tmp.path(), &file.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(record.generation, 2);
        assert_eq!(record.pane_id, "%52");
        assert_eq!(record.window_id, "@2");
    }

    #[test]
    fn record_session_start_persists_authoritative_record() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-3\nagent: codex\n---\nBody\n",
        );

        let record = record_session_start(&file, "session-3", "%61", "@7", 1).unwrap();

        assert_eq!(record.generation, 1);
        assert_eq!(record.state, ActorState::Starting);
        assert_eq!(record.last_transition.prior_generation, 0);
        assert_eq!(record.last_transition.new_generation, 1);
        assert_eq!(record.harness, "codex");
    }

    #[test]
    fn binding_a_pane_evicts_other_documents_bound_to_it() {
        // "1 pane = 1 document": a new document taking a pane must clear the
        // previous document's binding in the same actor-store write.
        let tmp = tempfile::TempDir::new().unwrap();
        let doc_a = seed_project_file(
            &tmp,
            "tasks/a.md",
            "---\nagent_doc_session: sess-a\nagent: codex\n---\nBody\n",
        );
        let doc_b = seed_project_file(
            &tmp,
            "tasks/b.md",
            "---\nagent_doc_session: sess-b\nagent: codex\n---\nBody\n",
        );

        record_session_start(&doc_a, "sess-a", "%1", "@1", 1).unwrap();
        let a_before = load_record_in(tmp.path(), &doc_a.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(a_before.pane_id, "%1");
        assert_ne!(a_before.state, ActorState::Closed);

        let b_after = record_session_start(&doc_b, "sess-b", "%1", "@1", 1)
            .expect("the incoming document should recover by taking the pane");
        assert_eq!(b_after.pane_id, "%1");
        assert_eq!(b_after.state, ActorState::Starting);

        let a_after = load_record_in(tmp.path(), &doc_a.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(
            a_after.state,
            ActorState::Closed,
            "document A must lose the pane when B takes it"
        );
        assert_eq!(a_after.pane_id, "");
        assert_eq!(a_after.window_id, "");
        assert!(
            a_after
                .last_transition
                .reason
                .contains("evicted_cross_document_pane owner="),
            "unexpected transition: {:?}",
            a_after.last_transition
        );
    }

    #[test]
    fn set_record_harness_persists_switch_and_survives_later_transitions() {
        // `#actorharnessrecordwriteback`: the live repro. The operator flips
        // `agent: codex` -> `agent: claude`, the supervisor spawns claude fresh, and
        // route must stop reading `codex` off the persisted record. Before the fix the
        // record kept saying `codex` forever, because `transition_state_*` carries the
        // stored harness forward on every later lifecycle transition.
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-hsw\nagent: codex\n---\nBody\n",
        );

        let started = record_session_start(&file, "session-hsw", "%30", "@8", 1).unwrap();
        assert_eq!(started.harness, "codex");

        let switched = set_record_harness_direct(&file, "session-hsw", "%30", "claude").unwrap();
        // Stored normalized, so route's normalized `expected_harness` compares equal.
        assert_eq!(switched.harness, "claude-code");
        // An identity correction, never a rebind.
        assert_eq!(switched.generation, 1);
        assert_eq!(switched.pane_id, "%30");
        assert_eq!(switched.state, started.state);

        // The regression that actually bit: a later lifecycle transition must carry
        // the NEW harness forward, not resurrect the old one.
        let after = transition_state(
            &file,
            "session-hsw",
            "%30",
            ActorState::Ready,
            "supervisor",
            "idle_pane_reconcile",
        )
        .unwrap();
        assert_eq!(after.harness, "claude-code");
        assert_eq!(after.generation, 1);
    }

    #[test]
    fn set_record_harness_is_a_noop_for_the_same_harness_and_rejects_stale_owners() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-hsw2\nagent: codex\n---\nBody\n",
        );
        record_session_start(&file, "session-hsw2", "%30", "@8", 1).unwrap();

        // Same harness (raw vs normalized spelling) must not churn the record.
        let unchanged = set_record_harness_direct(&file, "session-hsw2", "%30", "codex").unwrap();
        assert_eq!(unchanged.harness, "codex");
        assert_eq!(
            unchanged.last_transition.reason, "session_start",
            "no-op writeback must not rewrite the transition record"
        );

        // A pane or session that no longer owns the generation must not be able to
        // rewrite the harness out from under the real owner.
        assert!(set_record_harness_direct(&file, "session-hsw2", "%99", "claude").is_err());
        assert!(set_record_harness_direct(&file, "other-session", "%30", "claude").is_err());
    }

    #[test]
    fn transition_state_updates_same_generation_without_rebinding() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-4\nagent: codex\n---\nBody\n",
        );

        record_session_start(&file, "session-4", "%71", "@8", 1).unwrap();
        let record = transition_state(
            &file,
            "session-4",
            "%71",
            ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        assert_eq!(record.generation, 1);
        assert_eq!(record.state, ActorState::Ready);
        assert_eq!(record.last_transition.prior_generation, 1);
        assert_eq!(record.last_transition.new_generation, 1);
        assert_eq!(record.last_transition.caller, "supervisor");
        assert_eq!(record.last_transition.reason, "prompt_ready");
    }

    #[test]
    fn transition_state_preserves_generation_for_supervisor_lifecycle_updates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-4b\nagent: codex\n---\nBody\n",
        );

        record_session_start(&file, "session-4b", "%72", "@8", 1).unwrap();

        let waiting = transition_state(
            &file,
            "session-4b",
            "%72",
            ActorState::WaitingInput,
            "supervisor",
            "clean_exit_prompt",
        )
        .unwrap();
        assert_eq!(waiting.generation, 1);
        assert_eq!(waiting.state, ActorState::WaitingInput);
        assert_eq!(waiting.last_transition.prior_generation, 1);
        assert_eq!(waiting.last_transition.new_generation, 1);
        assert_eq!(waiting.last_transition.reason, "clean_exit_prompt");

        let blocked = transition_state(
            &file,
            "session-4b",
            "%72",
            ActorState::Blocked,
            "supervisor",
            "supervisor_halted",
        )
        .unwrap();
        assert_eq!(blocked.generation, 1);
        assert_eq!(blocked.state, ActorState::Blocked);
        assert_eq!(blocked.last_transition.prior_generation, 1);
        assert_eq!(blocked.last_transition.new_generation, 1);
        assert_eq!(blocked.last_transition.reason, "supervisor_halted");

        let closed = transition_state(
            &file,
            "session-4b",
            "%72",
            ActorState::Closed,
            "supervisor",
            "user_quit_clean_exit",
        )
        .unwrap();
        assert_eq!(closed.generation, 1);
        assert_eq!(closed.state, ActorState::Closed);
        assert_eq!(closed.last_transition.prior_generation, 1);
        assert_eq!(closed.last_transition.new_generation, 1);
        assert_eq!(closed.last_transition.reason, "user_quit_clean_exit");
    }

    #[test]
    fn transition_state_rejects_stale_pane_updates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-5\nagent: codex\n---\nBody\n",
        );

        record_session_start(&file, "session-5", "%81", "@9", 1).unwrap();
        let err = transition_state(
            &file,
            "session-5",
            "%82",
            ActorState::Closed,
            "supervisor",
            "session_end",
        )
        .expect_err("stale pane must not update authoritative actor state");

        assert!(
            format!("{err}").contains("no longer owns generation"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn record_session_start_rejects_stale_generation_after_new_owner_rebind() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-6\nagent: codex\n---\nBody\n",
        );

        record_session_start(&file, "session-6", "%91", "@10", 1).unwrap();
        project_binding_in(
            tmp.path(),
            &file.to_string_lossy(),
            "session-6",
            "%92",
            "@11",
            "sync",
            "recover_owner",
        )
        .unwrap();

        let err = record_session_start(&file, "session-6", "%91", "@10", 1)
            .expect_err("stale generation must not overwrite the newer authoritative owner");
        assert!(
            format!("{err}").contains("compare-and-swap failed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn transition_state_rejects_stale_session_updates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-7\nagent: codex\n---\nBody\n",
        );

        record_session_start(&file, "session-7", "%101", "@12", 1).unwrap();
        let err = transition_state(
            &file,
            "session-7-stale",
            "%101",
            ActorState::Closed,
            "supervisor",
            "session_end",
        )
        .expect_err("stale session updates must not mutate the authoritative record");

        assert!(
            format!("{err}").contains("current session"),
            "unexpected error: {err}"
        );
    }

    // ===================================================================
    // #pcpc1 — Session-actor message runtime (SimWorld determinism)
    // ===================================================================

    // `Arc`/`Mutex` come in via `use super::*`; only the extras are new here.
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn session_actor_runtime_serializes_single_producer_in_fifo_order() {
        // SimWorld: one producer pipelines N ops without blocking, then
        // collects. A single owner thread must drain them in submission order
        // (FIFO), and `processed` must equal N — the deterministic baseline the
        // concurrent test builds on.
        let tmp = tempfile::TempDir::new().unwrap();
        let actor = DocumentActor::spawn("doc-fifo", tmp.path().to_path_buf());

        let observed: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        let mut acks = Vec::new();
        const N: u64 = 64;
        for seq in 0..N {
            let observed = observed.clone();
            let rx = actor
                .enqueue(SessionOpKind::WriteSubmit, move |ctx| {
                    observed.lock().push(seq);
                    ctx.processed()
                })
                .unwrap();
            acks.push(rx);
        }

        // Acks come back in submission order, each reporting a strictly
        // increasing processed count (proves single-owner serialization).
        let mut prev = 0u64;
        for (i, rx) in acks.into_iter().enumerate() {
            let processed = rx.recv().unwrap();
            assert_eq!(
                processed,
                i as u64 + 1,
                "op {i} ran out of order: processed={processed}"
            );
            assert!(processed > prev, "processed counter regressed");
            prev = processed;
        }

        assert_eq!(
            *observed.lock(),
            (0..N).collect::<Vec<_>>(),
            "single-producer ops must execute in FIFO order"
        );
    }

    #[test]
    fn session_actor_runtime_serializes_concurrent_producers_without_overlap() {
        // SimWorld: M producer threads each blocking-submit one op to the SAME
        // document actor at once. The owner thread must execute them strictly
        // one at a time. We detect any overlap with a re-entrancy guard and
        // assert every op ran exactly once — the serialization invariant that
        // holds regardless of OS scheduling (the deterministic part), while the
        // SQLite CAS remains the cross-process backstop.
        let tmp = tempfile::TempDir::new().unwrap();
        let actor = Arc::new(DocumentActor::spawn(
            "doc-concurrent",
            tmp.path().to_path_buf(),
        ));

        let in_critical = Arc::new(AtomicBool::new(false));
        let overlap_detected = Arc::new(AtomicBool::new(false));
        let ran = Arc::new(AtomicUsize::new(0));

        const M: usize = 32;
        let barrier = Arc::new(Barrier::new(M));
        let mut threads = Vec::new();
        for _ in 0..M {
            let actor = actor.clone();
            let barrier = barrier.clone();
            let in_critical = in_critical.clone();
            let overlap_detected = overlap_detected.clone();
            let ran = ran.clone();
            threads.push(thread::spawn(move || {
                barrier.wait();
                actor
                    .submit(SessionOpKind::Closeout, move |_ctx| {
                        // Entering the critical section: nobody else may be in it.
                        if in_critical.swap(true, Ordering::SeqCst) {
                            overlap_detected.store(true, Ordering::SeqCst);
                        }
                        // Widen the window so a non-serialized runtime would race.
                        thread::sleep(std::time::Duration::from_millis(1));
                        ran.fetch_add(1, Ordering::SeqCst);
                        in_critical.store(false, Ordering::SeqCst);
                    })
                    .unwrap();
            }));
        }
        for t in threads {
            t.join().unwrap();
        }

        assert!(
            !overlap_detected.load(Ordering::SeqCst),
            "session actor ran two ops concurrently — not serialized"
        );
        assert_eq!(
            ran.load(Ordering::SeqCst),
            M,
            "every concurrently-submitted op must run exactly once"
        );
    }

    #[test]
    fn session_actor_runtime_registry_funnels_one_document_to_one_mailbox() {
        // SimWorld: two lookups for the same canonical document id return the
        // same owner thread (so cross-call-site writers are serialized), and a
        // different document gets a distinct owner.
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let rt = SessionActorRuntime::new();

        let a1 = rt.actor_for("doc-A", tmp.path());
        let a2 = rt.actor_for("doc-A", tmp.path());
        let b1 = rt.actor_for("doc-B", tmp.path());

        assert!(
            Arc::ptr_eq(&a1, &a2),
            "same document id must resolve to one shared actor"
        );
        assert!(
            !Arc::ptr_eq(&a1, &b1),
            "different documents must get distinct actors"
        );

        // The shared mailbox serializes ops submitted via either handle.
        let order: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let o1 = order.clone();
        let r1 = a1
            .enqueue(SessionOpKind::QueueHead, move |_| o1.lock().push(1))
            .unwrap();
        let o2 = order.clone();
        let r2 = a2
            .enqueue(SessionOpKind::QueueHead, move |_| o2.lock().push(2))
            .unwrap();
        r1.recv().unwrap();
        r2.recv().unwrap();
        assert_eq!(*order.lock(), vec![1, 2]);

        assert!(rt.shutdown("doc-A"));
        assert!(rt.shutdown("doc-B"));
        assert!(!rt.shutdown("doc-A"), "second shutdown must report absent");
    }
}
