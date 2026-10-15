//! # Module: op_capture_io (`#qnodemerge4wire`)
//!
//! Supply side of the op-capture / evented-reflection merge (`#qnodemerge4`).
//! The *consumer* (`agent_doc_merge::crdt::merge_with_editor_ops` + `EditorOp` /
//! `replay_editor_ops`) already trusts the editor's *real* operations over a
//! Myers diff-guess when those ops replay onto the merge base exactly. This
//! module is the durable plumbing that gets those ops from the editor to the
//! merge through the typed Lazily state backbone:
//!
//! - **Persistence** — `EditorOpCaptureCheckpointed` is a complete, typed state
//!   fact containing the ordered ops and exact `base_hash`. The former
//!   `editor_op_captures` table is inert legacy data, never migration input.
//! - **Append discipline** — recording an op against a *different* base than
//!   the projected base starts a fresh monotonic epoch. A live controller
//!   serializes read/derive/append; cold startup performs the same transition in
//!   one SQLite transaction.
//! - **Consume + clear** — once a merge has loaded the ops for its base they are
//!   cleared with `EditorOpCaptureCleared`, so op capture and cycle phase are one
//!   atomically projectable model.
//!
//! ## Agentic Contracts
//! - `record_editor_op` is append-only within an epoch and resets the epoch when
//!   `base_hash` changes; it never silently drops an op.
//! - `record_editor_ops_retaining` (the editor FFI path) retains a burst whose
//!   append failed transiently and re-sends it, in per-document order and with
//!   its original nonce, until the controller acknowledges it; the controller
//!   deduplicates re-sends by nonce (`#editoropscheckpointfail`).
//! - `editor_ops_for_base` returns `Some(ops)` **only** when the stored
//!   `base_hash` matches the hash of the supplied base text; otherwise `None`.
//! - `clear_op_capture` is idempotent — clearing an absent epoch is `Ok(())`.
//! - `consume_replayed_editor_op_epoch` decides "is the replayed epoch
//!   consumed?" from the state: a clear that errors while the epoch reads back
//!   absent is consumed, so a loaded host's missed reply is not a failure.
//! - `agent_doc_hash::content_hash` is the single source of the base-hash
//!   function shared by the producer (editor, via FFI) and the consumer (merge).
//!
//! Wiring point (`#qnodemerge4wire` part 3): `merge::merge_contents_crdt_with_ops`
//! loads ops via `editor_ops_for_base`, passes them to `merge_with_editor_ops`,
//! and clears the epoch after the merge. No per-document file or bespoke table
//! is on this hot path.

use agent_doc_hash::content_hash;
use agent_doc_merge::crdt::EditorOp;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Editor op capture must never monopolize the authoritative state ledger, or an
/// editor thread, when a controller owns the write lock: the first append
/// attempt is bounded by this budget. A burst that misses it is not dropped —
/// [`record_editor_ops_retaining`] retains it and the background drainer
/// re-sends it with [`RETAINED_EDITOR_OPS_RETRY_TIMEOUT`]
/// (`#editoropscheckpointfail`).
const EDITOR_OP_CAPTURE_BUSY_TIMEOUT: Duration = Duration::from_millis(250);
/// A remote editor projection cannot be installed until its operator-op epoch
/// is durably closed. Unlike optional typing evidence, this is a causal fence:
/// a short controller queue spike must retain the projection, but it must not
/// turn the ordinary replay path into a permanent retry storm.
const EDITOR_PROJECTION_FENCE_TIMEOUT: Duration = Duration::from_secs(3);

/// Per-document memo of the last `current_base_hash` result, keyed by the
/// durable-baseline content hash that produced it (`#qbasehashmemo`). No
/// filesystem projection participates in the typing hot path.
fn base_hash_cache() -> &'static Mutex<HashMap<PathBuf, (String, String)>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (String, String)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Count of full (cache-miss) base-hash recomputations, for memoization tests.
pub(crate) static BASE_HASH_RECOMPUTES: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
fn base_hash_recomputes_by_doc_for_tests() -> &'static Mutex<HashMap<PathBuf, u64>> {
    static COUNTS: OnceLock<Mutex<HashMap<PathBuf, u64>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn record_base_hash_recompute_for_tests(doc: &Path) {
    *base_hash_recomputes_by_doc_for_tests()
        .lock()
        .entry(doc.to_path_buf())
        .or_insert(0) += 1;
}

#[cfg(test)]
fn base_hash_recomputes_for_doc_for_tests(doc: &Path) -> u64 {
    let key = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    base_hash_recomputes_by_doc_for_tests()
        .lock()
        .get(&key)
        .copied()
        .unwrap_or(0)
}

/// The per-document operation epoch stored in `state.db`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpCaptureState {
    /// Monotonic Lazily epoch. A base change or explicit clear advances it.
    pub epoch: u64,
    /// SHA256 hex of the buffer text the ops were captured against — the merge
    /// base they replay onto. The merge trusts the ops only when its resolved
    /// base hashes to this value.
    pub base_hash: String,
    /// Ordered editor operations, in the sequence the editor performed them.
    pub ops: Vec<EditorOp>,
    /// Last-update wall-clock (ms since epoch), best-effort, for GC.
    pub updated_ms: u64,
}

fn state_db_identity(doc: &Path) -> Result<(PathBuf, String, String)> {
    let canonical = doc.canonicalize()?;
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
    let project_root = agent_doc_fs::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok((project_root, hash, canonical.to_string_lossy().into_owned()))
}

fn event_nonce() -> String {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        now_nanos(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn decode_capture(
    capture: &agent_doc_state_backbone::EditorOpCaptureProjection,
) -> Result<OpCaptureState> {
    let ops = serde_json::from_str::<Vec<EditorOp>>(&capture.ops_json)
        .context("decode Lazily editor-op epoch")?;
    Ok(OpCaptureState {
        epoch: capture.epoch,
        base_hash: capture.base_hash.clone(),
        ops,
        updated_ms: capture.updated_ms,
    })
}

fn captures_from_document_projection(
    document: Option<&agent_doc_state_backbone::DocumentStateProjection>,
) -> Result<(u64, Option<OpCaptureState>, Option<OpCaptureState>)> {
    let generation = document
        .map(|document| document.document.editor_op_capture_generation)
        .unwrap_or_default();
    Ok((
        generation,
        document
            .and_then(|document| document.document.editor_op_capture.as_ref())
            .map(decode_capture)
            .transpose()?,
        document
            .and_then(|document| document.document.last_editor_op_capture.as_ref())
            .map(decode_capture)
            .transpose()?,
    ))
}

fn load_cold_projected_captures(
    project_root: &Path,
    document_hash: &str,
) -> Result<(u64, Option<OpCaptureState>, Option<OpCaptureState>)> {
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let rows =
        agent_doc_sqlite::state_store::load_state_events_from_db(&conn, Some(document_hash))?;
    let mut ledger = agent_doc_state_backbone::EventLedger::new();
    for row in rows {
        let event: agent_doc_state_backbone::StateEvent =
            serde_json::from_str(&row.payload_json)
                .with_context(|| format!("decode state event {}", row.event_id))?;
        ledger.append(event);
    }
    let projection = ledger.project();
    captures_from_document_projection(projection.document(document_hash))
}

fn load_projected_captures(
    doc: &Path,
    project_root: &Path,
    document_hash: &str,
) -> Result<(u64, Option<OpCaptureState>, Option<OpCaptureState>)> {
    if agent_doc_state_wire::in_controller_request()
        && let Some(document) = agent_doc_state_wire::local_document_projection(document_hash)
    {
        return captures_from_document_projection(document.as_ref());
    }
    let controller_socket = agent_doc_controller::paths::socket_path(project_root);
    if controller_socket.exists() && !agent_doc_state_wire::in_controller_request() {
        let request = serde_json::json!({
            "command": "document_state_projection",
            "file": doc,
        });
        match agent_doc_state_wire::send_ndjson_request_to_actor(
            &controller_socket,
            &request,
            EDITOR_OP_CAPTURE_BUSY_TIMEOUT,
        ) {
            Ok(raw) => {
                #[derive(serde::Deserialize)]
                struct ProjectionEnvelope {
                    ok: bool,
                    data: Option<agent_doc_state_backbone::DocumentStateProjection>,
                    error: Option<String>,
                }
                let envelope: ProjectionEnvelope =
                    serde_json::from_str(&raw).context("decode editor-op state projection")?;
                if !envelope.ok {
                    anyhow::bail!(
                        "editor-op state projection rejected: {}",
                        envelope.error.as_deref().unwrap_or("unknown error")
                    );
                }
                return captures_from_document_projection(envelope.data.as_ref());
            }
            Err(agent_doc_state_wire::ActorRequestError::Connect(_))
            | Err(agent_doc_state_wire::ActorRequestError::Timeout(_))
                if !controller_socket.exists() =>
            {
                // The actor disappeared between discovery and connect. With no
                // live owner, cold hydration from the typed event ledger below
                // is the single state-backbone path.
            }
            Err(err) => return Err(err).context("read Lazily editor-op projection"),
        }
    }

    load_cold_projected_captures(project_root, document_hash)
}

/// Load the operation epoch for `doc`, if present.
///
/// The typed Lazily projection is authoritative. Missing typed state remains
/// missing; the former bespoke table is never read or imported.
pub fn load_op_capture(doc: &Path) -> Result<Option<OpCaptureState>> {
    let (project_root, document_hash, _) = state_db_identity(doc)?;
    load_projected_captures(doc, &project_root, &document_hash).map(|(_, active, _)| active)
}

/// Load the newest durable operation checkpoint, including an epoch that has
/// already been consumed and cleared.
///
/// A controller running an older native library may omit the retained field
/// from its wire projection. In that transition case, the append-only ledger is
/// replayed directly; it remains the durable authority for historical evidence.
pub fn load_last_op_capture(doc: &Path) -> Result<Option<OpCaptureState>> {
    let (project_root, document_hash, _) = state_db_identity(doc)?;
    let (_, _, retained) = load_projected_captures(doc, &project_root, &document_hash)?;
    if retained.is_some() {
        return Ok(retained);
    }
    load_cold_projected_captures(&project_root, &document_hash).map(|(_, _, retained)| retained)
}

/// True when there are **pending** captured editor ops for `doc` — live
/// keystrokes the editor reported that a merge has not yet consumed+cleared.
///
/// This is the liveness anchor the CRDT merge base uses to tell a **live
/// deletion** (the overlay shrank because the user just deleted a line; the ops
/// are still pending) apart from a **genuinely stale** overlay (an older
/// committed subset; no pending ops). The lineage-free `from_markdown` overlay
/// leaves no causal history to compare, so the two are content-identical
/// (overlay ⊆ baseline) — only the still-uncleared op-capture row marks the
/// divergence as live. After a committed cycle `merge_contents_crdt_with_ops`
/// clears the row, so a truly stale overlay reads `false` here and still GCs.
///
/// Over-reporting is the safe direction: a lingering non-empty row at worst
/// *preserves* a stale overlay (delayed GC), never discards a live edit.
pub fn has_pending_editor_ops(doc: &Path) -> bool {
    matches!(load_op_capture(doc), Ok(Some(capture)) if !capture.ops.is_empty())
}

/// Record one editor op for `doc`, captured against the buffer whose text hashes
/// to `base_hash`.
///
/// Appends within the current epoch when `base_hash` matches the stored one;
/// otherwise starts a fresh epoch (the prior ops were captured against a base
/// that no longer applies, so they are discarded).
pub fn record_editor_op(doc: &Path, base_hash: &str, op: EditorOp) -> Result<()> {
    record_editor_ops(doc, base_hash, vec![op])
}

/// Refresh the activity time of an already-active editor-op epoch without
/// changing its base, epoch, or operations.
///
/// An editor may drain a burst next to a document snapshot and then discover
/// that the document advanced before handoff. The burst remains live and is
/// requeued, but no new operation can be appended until the next stable quiet
/// boundary. Refreshing the complete checkpoint makes that retryable activity
/// visible to controller admission (notably Compact Exchange) without dropping
/// or duplicating any captured operation.
pub fn refresh_op_capture_activity(doc: &Path) -> Result<bool> {
    let (project_root, document_hash, canonical_path) = state_db_identity(doc)?;
    let updated_ms = now_millis();
    let nonce = event_nonce();
    let controller_socket = agent_doc_controller::paths::socket_path(&project_root);
    if controller_socket.exists() && !agent_doc_state_wire::in_controller_request() {
        let payload = serde_json::json!({
            "action": "touch",
            "updated_ms": updated_ms,
            "event_nonce": nonce,
        });
        let request = serde_json::json!({
            "command": "editor_op_capture_update",
            "file": doc,
            "diagnostic_payload": serde_json::to_string(&payload)?,
        });
        match agent_doc_state_wire::send_ndjson_request_to_actor(
            &controller_socket,
            &request,
            EDITOR_OP_CAPTURE_BUSY_TIMEOUT,
        ) {
            Ok(raw) => {
                #[derive(serde::Deserialize)]
                struct UpdateEnvelope {
                    ok: bool,
                    data: Option<bool>,
                    error: Option<String>,
                }
                let envelope: UpdateEnvelope =
                    serde_json::from_str(&raw).context("decode editor-op activity response")?;
                if !envelope.ok {
                    anyhow::bail!(
                        "Lazily editor-op activity refresh rejected: {}",
                        envelope.error.as_deref().unwrap_or("unknown error")
                    );
                }
                return Ok(envelope.data.unwrap_or(false));
            }
            Err(agent_doc_state_wire::ActorRequestError::Connect(_))
                if !controller_socket.exists() => {}
            Err(err) => return Err(err).context("refresh Lazily editor-op activity"),
        }
    }

    let mut conn = agent_doc_sqlite::state_store::open_state_db_with_timeout(
        &project_root,
        EDITOR_OP_CAPTURE_BUSY_TIMEOUT,
    )?;
    let tx = conn.transaction()?;
    let rows = agent_doc_sqlite::state_store::load_state_events_from_db(&tx, Some(&document_hash))?;
    let mut ledger = agent_doc_state_backbone::EventLedger::new();
    for row in rows {
        ledger.append(serde_json::from_str(&row.payload_json)?);
    }
    let projection = ledger.project();
    let Some(active) = projection
        .document(&document_hash)
        .and_then(|document| document.document.editor_op_capture.as_ref())
    else {
        return Ok(false);
    };
    let event = agent_doc_state_backbone::StateEvent::new(
        format!(
            "editor-op-capture-activity:{document_hash}:{}:{nonce}",
            active.epoch
        ),
        agent_doc_state_backbone::StateFact::EditorOpCaptureCheckpointed {
            document_hash: document_hash.clone(),
            canonical_path,
            epoch: active.epoch,
            base_hash: active.base_hash.clone(),
            ops_json: active.ops_json.clone(),
            updated_ms,
        },
    );
    let payload_json = serde_json::to_string(&event)?;
    agent_doc_sqlite::state_store::insert_state_event_in_db(
        &tx,
        &agent_doc_sqlite::state_store::StateEventInsert {
            event_id: &event.event_id,
            document_hash: event.document_hash(),
            domain: event.domain().label(),
            fact_type: event.fact.label(),
            payload_json: &payload_json,
        },
    )?;
    tx.commit()?;
    agent_doc_state_wire::mark_local_state_db_dirty();
    Ok(true)
}

/// One editor-op burst with a stable identity.
///
/// The `nonce` and `updated_ms` are fixed when the burst is first handed over,
/// so every re-send of a retained batch is byte-identical and the controller's
/// applied-nonce ledger can recognise it (`#editoropscheckpointfail`).
#[derive(Debug, Clone)]
struct EditorOpsBatch {
    base_hash: String,
    ops: Vec<EditorOp>,
    updated_ms: u64,
    nonce: String,
}

/// Why one append attempt did not record its batch.
#[derive(Debug)]
enum AppendAttemptError {
    /// The batch may succeed later: the controller did not answer in time, was
    /// momentarily unreachable, or the cold store was busy. A timed-out request
    /// may even have been applied late; the controller deduplicates re-sends by
    /// nonce, so retrying is always safe.
    Transient(anyhow::Error),
    /// The batch can never be recorded as sent (the controller rejected it or
    /// it is malformed). Retrying would only repeat the rejection.
    Rejected(anyhow::Error),
}

impl AppendAttemptError {
    fn into_error(self) -> anyhow::Error {
        match self {
            Self::Transient(err) | Self::Rejected(err) => err,
        }
    }
}

/// Record an ordered editor-op burst in one bounded state-ledger transaction.
///
/// A quiet-period editor report can contain hundreds of keystrokes. Persisting
/// those one at a time repeatedly opened SQLite, deserialized and serialized the
/// growing JSON vector, and held up controller reads. This batch boundary keeps
/// the same epoch semantics while doing that work exactly once per drained burst.
///
/// This is the strict single-attempt form: `Ok(())` means the batch is durably
/// recorded and any error means it is not (or, for a controller timeout, not
/// *known* to be). Editor producers use [`record_editor_ops_retaining`] instead,
/// which never drops a transiently failed burst.
pub fn record_editor_ops(doc: &Path, base_hash: &str, ops: Vec<EditorOp>) -> Result<()> {
    if ops.is_empty() {
        return Ok(());
    }
    let batch = EditorOpsBatch {
        base_hash: base_hash.to_string(),
        ops,
        updated_ms: now_millis(),
        nonce: event_nonce(),
    };
    append_editor_ops_batch(doc, &batch, EDITOR_OP_CAPTURE_BUSY_TIMEOUT)
        .map_err(AppendAttemptError::into_error)
}

/// One append attempt for `batch` with the given controller response budget.
fn append_editor_ops_batch(
    doc: &Path,
    batch: &EditorOpsBatch,
    response_timeout: Duration,
) -> std::result::Result<(), AppendAttemptError> {
    let (project_root, document_hash, canonical_path) =
        state_db_identity(doc).map_err(AppendAttemptError::Rejected)?;
    let ops_json = serde_json::to_string(&batch.ops)
        .context("encode editor-op batch")
        .map_err(AppendAttemptError::Rejected)?;
    let controller_socket = agent_doc_controller::paths::socket_path(&project_root);
    if controller_socket.exists() && !agent_doc_state_wire::in_controller_request() {
        let payload = serde_json::json!({
            "action": "append",
            "base_hash": batch.base_hash,
            "ops_json": ops_json,
            "updated_ms": batch.updated_ms,
            "event_nonce": batch.nonce,
        });
        let request = serde_json::json!({
            "command": "editor_op_capture_update",
            "file": doc,
            "diagnostic_payload": payload.to_string(),
        });
        match agent_doc_state_wire::send_ndjson_request_to_actor(
            &controller_socket,
            &request,
            response_timeout,
        ) {
            Ok(raw) => {
                #[derive(serde::Deserialize)]
                struct UpdateEnvelope {
                    ok: bool,
                    error: Option<String>,
                }
                let envelope: UpdateEnvelope = serde_json::from_str(&raw)
                    .context("decode editor-op append response")
                    .map_err(AppendAttemptError::Rejected)?;
                if !envelope.ok {
                    return Err(AppendAttemptError::Rejected(anyhow::anyhow!(
                        "Lazily editor-op append rejected: {}",
                        envelope.error.as_deref().unwrap_or("unknown error")
                    )));
                }
                return Ok(());
            }
            Err(agent_doc_state_wire::ActorRequestError::Connect(_))
                if !controller_socket.exists() => {}
            Err(err) => {
                // `#editoropscheckpointfail`: a timeout here is the common case —
                // the controller's state-ingress lock was held by a durable reload
                // for longer than the response budget. The request is still queued
                // and will usually be applied late, so this is never final.
                return Err(AppendAttemptError::Transient(
                    anyhow::Error::new(err).context("append Lazily editor-op checkpoint"),
                ));
            }
        }
    }

    append_editor_ops_batch_cold(doc, &project_root, &document_hash, canonical_path, batch)
        .context("append cold editor-op checkpoint")
        .map_err(AppendAttemptError::Transient)
}

/// Controller-less (or in-controller) append: one SQLite transaction.
fn append_editor_ops_batch_cold(
    doc: &Path,
    project_root: &Path,
    document_hash: &str,
    canonical_path: String,
    batch: &EditorOpsBatch,
) -> Result<()> {
    let mut conn = agent_doc_sqlite::state_store::open_state_db_with_timeout(
        project_root,
        EDITOR_OP_CAPTURE_BUSY_TIMEOUT,
    )?;
    let local_current = agent_doc_state_wire::in_controller_request()
        .then(|| load_projected_captures(doc, project_root, document_hash))
        .transpose()?;
    let tx = conn.transaction()?;
    let (generation, existing) = match local_current {
        Some((generation, active, _)) => (generation, active),
        None => {
            let rows =
                agent_doc_sqlite::state_store::load_state_events_from_db(&tx, Some(document_hash))?;
            let mut ledger = agent_doc_state_backbone::EventLedger::new();
            for row in rows {
                ledger.append(serde_json::from_str(&row.payload_json)?);
            }
            let projection = ledger.project();
            let (generation, active, _) =
                captures_from_document_projection(projection.document(document_hash))?;
            (generation, active)
        }
    };
    let (epoch, mut complete_ops) = match existing {
        Some(existing) if existing.base_hash == batch.base_hash => (existing.epoch, existing.ops),
        _ => (generation.saturating_add(1), Vec::new()),
    };
    complete_ops.extend(batch.ops.iter().cloned());
    let event = agent_doc_state_backbone::StateEvent::new(
        format!(
            "editor-op-capture-checkpoint:{document_hash}:{epoch}:{}",
            batch.nonce
        ),
        agent_doc_state_backbone::StateFact::EditorOpCaptureCheckpointed {
            document_hash: document_hash.to_string(),
            canonical_path,
            epoch,
            base_hash: batch.base_hash.clone(),
            ops_json: serde_json::to_string(&complete_ops)?,
            updated_ms: batch.updated_ms,
        },
    );
    let payload_json = serde_json::to_string(&event)?;
    agent_doc_sqlite::state_store::insert_state_event_in_db(
        &tx,
        &agent_doc_sqlite::state_store::StateEventInsert {
            event_id: &event.event_id,
            document_hash: event.document_hash(),
            domain: event.domain().label(),
            fact_type: event.fact.label(),
            payload_json: &payload_json,
        },
    )?;
    tx.commit()?;
    agent_doc_state_wire::mark_local_state_db_dirty();
    Ok(())
}

/// How [`record_editor_ops_retaining`] disposed of a burst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorOpsRecordOutcome {
    /// Durably recorded by this call.
    Recorded,
    /// Not yet recorded; retained in order and re-sent by the binary's
    /// background drainer until it is (`#editoropscheckpointfail`).
    Retained {
        /// Batches now retained for this document, including this one.
        pending_batches: usize,
        /// Why this burst was retained (full error chain, or ordering).
        cause: String,
    },
}

/// Most bursts one document may retain before a new one is refused outright.
/// Typing produces a few bursts per second, so this is many minutes of an
/// unreachable controller — far beyond a durable-reload stall or a recycle.
const RETAINED_EDITOR_OPS_LIMIT: usize = 4096;
/// Response budget for a drainer re-send. The drainer runs off every editor
/// thread, so it can afford to wait out a controller ingress stall.
const RETAINED_EDITOR_OPS_RETRY_TIMEOUT: Duration = Duration::from_secs(3);
const RETAINED_EDITOR_OPS_BACKOFF_MIN: Duration = Duration::from_millis(50);
const RETAINED_EDITOR_OPS_BACKOFF_MAX: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct RetainedEditorOpsBatch {
    batch: EditorOpsBatch,
    retained_at: std::time::Instant,
    attempts: u32,
    next_attempt_at: std::time::Instant,
}

#[derive(Debug, Default)]
struct RetainedEditorOps {
    /// Per-document FIFO of bursts not yet known to be recorded.
    queues: HashMap<PathBuf, std::collections::VecDeque<RetainedEditorOpsBatch>>,
    /// Documents with an append attempt in flight (a caller's first attempt or
    /// a drainer re-send). At most one per document, so bursts reach the
    /// controller strictly in typing order.
    in_flight: std::collections::HashSet<PathBuf>,
}

struct RetainedEditorOpsStore {
    state: Mutex<RetainedEditorOps>,
    changed: parking_lot::Condvar,
}

fn retained_editor_ops() -> &'static RetainedEditorOpsStore {
    static STORE: OnceLock<RetainedEditorOpsStore> = OnceLock::new();
    STORE.get_or_init(|| RetainedEditorOpsStore {
        state: Mutex::new(RetainedEditorOps::default()),
        changed: parking_lot::Condvar::new(),
    })
}

fn retained_key(doc: &Path) -> PathBuf {
    doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf())
}

fn retry_backoff(attempts: u32) -> Duration {
    RETAINED_EDITOR_OPS_BACKOFF_MIN
        .saturating_mul(1u32 << attempts.min(16))
        .min(RETAINED_EDITOR_OPS_BACKOFF_MAX)
}

/// Record an editor-op burst without ever silently losing it
/// (`#editoropscheckpointfail`).
///
/// The first attempt keeps the editor's short response budget. When it fails
/// transiently — in practice a controller whose state-ingress lock is held by
/// a durable reload past that budget — the burst is retained in a per-document
/// FIFO and re-sent with the same nonce by a background drainer until the
/// controller acknowledges it. While a document has a retained or in-flight
/// burst, later bursts queue behind it, so the epoch keeps typing order.
///
/// Returns `Err` only when the burst can never be recorded: the controller
/// rejected it, or the per-document retention bound is exhausted.
pub fn record_editor_ops_retaining(
    doc: &Path,
    base_hash: &str,
    ops: Vec<EditorOp>,
) -> Result<EditorOpsRecordOutcome> {
    if ops.is_empty() {
        return Ok(EditorOpsRecordOutcome::Recorded);
    }
    let batch = EditorOpsBatch {
        base_hash: base_hash.to_string(),
        ops,
        updated_ms: now_millis(),
        nonce: event_nonce(),
    };
    let key = retained_key(doc);
    let store = retained_editor_ops();
    {
        let mut state = store.state.lock();
        let busy = state.in_flight.contains(&key)
            || state
                .queues
                .get(&key)
                .is_some_and(|queue| !queue.is_empty());
        if busy {
            let queue = state.queues.entry(key.clone()).or_default();
            if queue.len() >= RETAINED_EDITOR_OPS_LIMIT {
                anyhow::bail!(
                    "editor-op retention bound exhausted: {} bursts already retained for {}",
                    queue.len(),
                    key.display()
                );
            }
            let now = std::time::Instant::now();
            queue.push_back(RetainedEditorOpsBatch {
                batch,
                retained_at: now,
                attempts: 0,
                next_attempt_at: now,
            });
            let pending_batches = queue.len();
            drop(state);
            ensure_retained_editor_ops_drainer();
            store.changed.notify_all();
            return Ok(EditorOpsRecordOutcome::Retained {
                pending_batches,
                cause: "ordered behind an unacknowledged editor-op burst".to_string(),
            });
        }
        state.in_flight.insert(key.clone());
    }

    let attempt = append_editor_ops_batch(doc, &batch, EDITOR_OP_CAPTURE_BUSY_TIMEOUT);
    let mut state = store.state.lock();
    state.in_flight.remove(&key);
    let outcome = match attempt {
        Ok(()) => Ok(EditorOpsRecordOutcome::Recorded),
        Err(AppendAttemptError::Rejected(err)) => Err(err),
        Err(AppendAttemptError::Transient(err)) => {
            let queue = state.queues.entry(key.clone()).or_default();
            let now = std::time::Instant::now();
            // Front, not back: bursts typed while this attempt was in flight
            // were queued behind it and must stay behind it.
            queue.push_front(RetainedEditorOpsBatch {
                batch,
                retained_at: now,
                attempts: 1,
                next_attempt_at: now + retry_backoff(0),
            });
            Ok(EditorOpsRecordOutcome::Retained {
                pending_batches: queue.len(),
                cause: format!("{err:#}"),
            })
        }
    };
    let wake_drainer = state
        .queues
        .get(&key)
        .is_some_and(|queue| !queue.is_empty());
    drop(state);
    if wake_drainer {
        ensure_retained_editor_ops_drainer();
        store.changed.notify_all();
    }
    outcome
}

/// Bursts retained for `doc` that the controller has not yet acknowledged.
pub fn retained_editor_ops_pending(doc: &Path) -> usize {
    let key = retained_key(doc);
    let state = retained_editor_ops().state.lock();
    state.queues.get(&key).map_or(0, |queue| queue.len())
        + usize::from(state.in_flight.contains(&key))
}

/// Block until `doc` has no retained or in-flight burst, or `timeout` elapses.
/// Returns whether the document drained.
pub fn wait_for_retained_editor_ops(doc: &Path, timeout: Duration) -> bool {
    let key = retained_key(doc);
    let deadline = std::time::Instant::now() + timeout;
    let store = retained_editor_ops();
    let mut state = store.state.lock();
    loop {
        let pending = state.in_flight.contains(&key)
            || state
                .queues
                .get(&key)
                .is_some_and(|queue| !queue.is_empty());
        if !pending {
            return true;
        }
        if store.changed.wait_until(&mut state, deadline).timed_out() {
            return false;
        }
    }
}

fn ensure_retained_editor_ops_drainer() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        if let Err(err) = std::thread::Builder::new()
            .name("agent-doc-editor-ops-retry".to_string())
            .spawn(run_retained_editor_ops_drainer)
        {
            eprintln!("[op-capture] failed to start retained editor-op drainer: {err}");
        }
    });
}

/// Pick the next ready head burst, mark its document in flight, and return it.
fn next_retained_editor_ops(
    state: &mut RetainedEditorOps,
    now: std::time::Instant,
) -> std::result::Result<(PathBuf, EditorOpsBatch), Option<std::time::Instant>> {
    let mut earliest: Option<std::time::Instant> = None;
    let mut ready: Option<PathBuf> = None;
    for (key, queue) in &state.queues {
        if state.in_flight.contains(key) {
            continue;
        }
        let Some(head) = queue.front() else { continue };
        if head.next_attempt_at <= now {
            ready = Some(key.clone());
            break;
        }
        earliest = Some(earliest.map_or(head.next_attempt_at, |e| e.min(head.next_attempt_at)));
    }
    let Some(key) = ready else {
        return Err(earliest);
    };
    let batch = state.queues[&key]
        .front()
        .expect("ready queue has a head")
        .batch
        .clone();
    state.in_flight.insert(key.clone());
    Ok((key, batch))
}

fn run_retained_editor_ops_drainer() {
    let store = retained_editor_ops();
    loop {
        let (key, batch) = {
            let mut state = store.state.lock();
            loop {
                match next_retained_editor_ops(&mut state, std::time::Instant::now()) {
                    Ok(next) => break next,
                    Err(Some(at)) => {
                        store.changed.wait_until(&mut state, at);
                    }
                    Err(None) => store.changed.wait(&mut state),
                }
            }
        };
        let attempt = append_editor_ops_batch(&key, &batch, RETAINED_EDITOR_OPS_RETRY_TIMEOUT);
        let mut state = store.state.lock();
        let queue = state.queues.entry(key.clone()).or_default();
        let head_matches = queue
            .front()
            .is_some_and(|head| head.batch.nonce == batch.nonce);
        if !head_matches {
            // Unreachable while only this drainer and first attempts touch
            // queue heads; never pop a different burst on a stale result.
            state.in_flight.remove(&key);
            drop(state);
            store.changed.notify_all();
            continue;
        }
        let summary = agent_doc_merge::crdt::summarize_editor_ops_for_log(&batch.ops);
        let base = batch
            .base_hash
            .get(..12)
            .unwrap_or(&batch.base_hash)
            .to_string();
        let line = match attempt {
            Ok(()) => {
                let head = queue.pop_front().expect("head checked above");
                Some(format!(
                    "editor_ops_recorded count={} base={base} transaction=batch {summary} retained_retry=true attempts={} retained_ms={} #qbasehashmemo #editoropscheckpointfail",
                    batch.ops.len(),
                    head.attempts + 1,
                    head.retained_at.elapsed().as_millis(),
                ))
            }
            Err(AppendAttemptError::Rejected(err)) => {
                let head = queue.pop_front().expect("head checked above");
                Some(format!(
                    "editor_ops_record_failed count={} transaction=batch retained_retry=true attempts={} error={err:#} #qbasehashmemo #editoropscheckpointfail",
                    batch.ops.len(),
                    head.attempts + 1,
                ))
            }
            Err(AppendAttemptError::Transient(_)) => {
                let head = queue.front_mut().expect("head checked above");
                head.next_attempt_at = std::time::Instant::now() + retry_backoff(head.attempts);
                head.attempts = head.attempts.saturating_add(1);
                None
            }
        };
        if queue.is_empty() {
            state.queues.remove(&key);
        }
        drop(state);
        // The document stays in flight until its receipt is written, so a
        // waiter that sees it drained also sees the receipt. Logging happens
        // outside the store lock: `log_op` may itself query the controller.
        if let Some(line) = line {
            agent_doc_ops_log_io::log_op(&key, &line);
        }
        store.state.lock().in_flight.remove(&key);
        store.changed.notify_all();
    }
}

/// Compute the base hash the editor reporters must stamp on captured ops so the
/// merge will accept them (`#qnodemerge4wire`).
///
/// This MUST return `content_hash` of the **exact same base text**
/// `merge_contents_crdt_with_ops` resolves at write time — the
/// persisted CRDT merge base decoded to text — otherwise the merge's
/// [`editor_ops_for_base`] gate rejects every op (`capture.base_hash !=
/// content_hash(base_text)`) and the feature silently no-ops. The base both
/// sides diverge from is the committed snapshot content (the same content
/// preflight saves as the next write's baseline at a synced point), so the caller
/// supplies the same CRDT merge-base resolver the write path uses. With no
/// snapshot/CRDT state yet this is the empty-text hash, matching the merge's
/// `None => String::new()` base.
pub fn current_base_hash_with<R>(doc: &Path, resolve_base_state: R) -> Result<String>
where
    R: FnOnce(&Path, &str) -> Result<Vec<u8>>,
{
    let baseline = agent_doc_snapshot_io::load_document_baseline(doc)?.unwrap_or_default();
    current_base_hash_from_baseline_with(doc, &baseline, resolve_base_state)
}

/// [`current_base_hash_with`] for a caller that already holds the merge baseline.
///
/// `#basehashforbiddenstatedb`: an embedded editor host may not open `state.db`
/// at all (`forbid_state_db_connections_for_process`), so the baseline load in
/// [`current_base_hash_with`] cannot run inside a JetBrains IDE — it fails
/// closed, and every captured operator burst is then refused with
/// `base_hash_unavailable`. Such a host reads the same baseline from the project
/// controller's projection plane and hands it here, so both worlds share one
/// hash computation and one cache.
pub fn current_base_hash_from_baseline_with<R>(
    doc: &Path,
    baseline: &str,
    resolve_base_state: R,
) -> Result<String>
where
    R: FnOnce(&Path, &str) -> Result<Vec<u8>>,
{
    let fingerprint = content_hash(baseline);
    let cache_key = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());

    if let Some((cached_fp, cached_hash)) = base_hash_cache().lock().get(&cache_key)
        && *cached_fp == fingerprint
    {
        return Ok(cached_hash.clone());
    }

    BASE_HASH_RECOMPUTES.fetch_add(1, Ordering::Relaxed);
    #[cfg(test)]
    record_base_hash_recompute_for_tests(&cache_key);
    let base_state = resolve_base_state(doc, baseline)?;
    let base_text = agent_doc_merge::crdt::CrdtDoc::decode_state(&base_state)
        .map(|d| d.to_text())
        .unwrap_or_default();
    let hash = content_hash(&base_text);

    base_hash_cache()
        .lock()
        .insert(cache_key, (fingerprint, hash.clone()));
    Ok(hash)
}

/// Return the captured ops for `doc` **only** when they were captured against a
/// base whose text matches `base_text` (by hash). A mismatch (stale/advanced
/// base, missed event) returns `None` so the merge falls back to the diff-guess.
pub fn editor_ops_for_base(doc: &Path, base_text: &str) -> Result<Option<Vec<EditorOp>>> {
    let Some(capture) = load_op_capture(doc)? else {
        return Ok(None);
    };
    if capture.ops.is_empty() {
        return Ok(None);
    }
    if capture.base_hash == content_hash(base_text) {
        Ok(Some(capture.ops))
    } else {
        Ok(None)
    }
}

/// Return exact-base editor ops across a non-operator projection boundary.
///
/// The active epoch is authoritative while present. Once a compare-and-swap
/// projection fences that epoch, its durable retained checkpoint remains the
/// same operator lineage evidence for retries against the original base.
pub fn editor_ops_for_base_or_retained(
    doc: &Path,
    base_text: &str,
) -> Result<Option<Vec<EditorOp>>> {
    if let Some(ops) = editor_ops_for_base(doc, base_text)? {
        return Ok(Some(ops));
    }
    let Some(capture) = load_last_op_capture(doc)? else {
        return Ok(None);
    };
    if capture.ops.is_empty() || capture.base_hash != content_hash(base_text) {
        return Ok(None);
    }
    Ok(Some(capture.ops))
}

/// Replay the newest durable editor operation checkpoint when, and only when,
/// it was captured against `base_text` exactly.
pub fn last_editor_text_for_base(doc: &Path, base_text: &str) -> Result<Option<String>> {
    let Some(capture) = load_last_op_capture(doc)? else {
        return Ok(None);
    };
    if capture.ops.is_empty() || capture.base_hash != content_hash(base_text) {
        return Ok(None);
    }
    Ok(agent_doc_merge::crdt::replay_editor_ops(
        base_text,
        &capture.ops,
    ))
}

/// Reconstruct the newest operator-authored document cut for `base_text`.
///
/// The active epoch is preferred. The retained checkpoint is a fallback for an
/// epoch already fenced by a non-operator projection. Neither source is
/// evidence unless its base hash matches `base_text` exactly.
pub fn operator_text_for_base(doc: &Path, base_text: &str) -> Result<Option<String>> {
    if let Some(ops) = editor_ops_for_base(doc, base_text)?
        && let Some(text) = agent_doc_merge::crdt::replay_editor_ops(base_text, &ops)
    {
        return Ok(Some(text));
    }
    last_editor_text_for_base(doc, base_text)
}

/// `#unstrikelost`: queue items struck in `base_text` that the operator's OWN
/// captured editor ops un-struck, keyed for
/// [`agent_doc_merge::crdt::with_operator_rearmed_queue_items`].
///
/// The active epoch is preferred; the newest retained checkpoint covers an epoch
/// a non-operator projection already closed. Ops captured against any other base
/// are not evidence, so the set is empty and the merge keeps the
/// anti-resurrection `Live < Struck` join.
///
/// This form maps an unreadable epoch to "no evidence". A caller whose outcome
/// re-strikes an operator unstrike on empty evidence must use
/// [`try_operator_rearmed_queue_item_keys`] and fail closed instead
/// (`#unstrikelost`, unreadable evidence).
pub fn operator_rearmed_queue_item_keys(
    doc: &Path,
    base_text: &str,
) -> std::collections::HashSet<String> {
    try_operator_rearmed_queue_item_keys(doc, base_text).unwrap_or_default()
}

/// [`operator_rearmed_queue_item_keys`], keeping "the evidence could not be
/// read" (a dead or wedged controller, an unreadable `state.db`) distinct from
/// "there is no evidence" (`#unstrikelost`, unreadable evidence).
pub fn try_operator_rearmed_queue_item_keys(
    doc: &Path,
    base_text: &str,
) -> Result<std::collections::HashSet<String>> {
    Ok(operator_text_for_base(doc, base_text)?
        .map(|cut| agent_doc_merge::crdt::operator_rearmed_queue_item_keys(base_text, &cut))
        .unwrap_or_default())
}

/// `#retypestruck`: text the operator typed or pasted in the ACTIVE editor-op
/// epoch, as contiguous runs (see
/// [`agent_doc_merge::crdt::editor_ops_inserted_runs`]).
///
/// Unlike [`operator_text_for_base`] this is base-independent: the epoch's base
/// is routinely not the committed snapshot (an earlier operator edit or an agent
/// projection advanced it), and exact-base replay then yields no evidence at
/// all. Only the active epoch is read. It opens after the last non-operator
/// projection, so its keystrokes postdate every strike the agent projected; a
/// retained (closed) checkpoint may hold the keystrokes that typed a head the
/// agent has since answered, and is therefore never evidence of a re-type.
pub fn active_operator_inserted_runs(doc: &Path) -> Result<Vec<String>> {
    let Some(capture) = load_op_capture(doc)? else {
        return Ok(Vec::new());
    };
    Ok(agent_doc_merge::crdt::editor_ops_inserted_runs(
        &capture.ops,
    ))
}

/// Clear the active Lazily op-capture epoch. Idempotent at the public boundary:
/// clearing an absent epoch succeeds while retaining a monotonic clear marker.
pub fn clear_op_capture(doc: &Path) -> Result<()> {
    clear_op_capture_with_timeout(doc, EDITOR_OP_CAPTURE_BUSY_TIMEOUT)
}

/// Close the active editor-op epoch before a controller-owned projection is
/// handed to an editor host.
///
/// This is still bounded and fail-closed, but it has a larger budget than
/// optional typing capture because the visible delivery cannot safely proceed
/// without the fence.
pub fn clear_op_capture_for_editor_projection(doc: &Path) -> Result<()> {
    clear_op_capture_with_timeout(doc, EDITOR_PROJECTION_FENCE_TIMEOUT)
}

/// Clear attempts [`consume_replayed_editor_op_epoch`] makes before it reports
/// [`ReplayedEpochConsumption::Failed`].
pub const REPLAYED_EPOCH_CLEAR_ATTEMPTS: u32 = 3;

/// How consuming a replayed editor-op epoch after a successful write ended.
#[derive(Debug)]
pub enum ReplayedEpochConsumption {
    /// The epoch is gone. `outcome` is the ops-log label: the clear itself was
    /// acknowledged (`cleared_after_successful_write`), or the clear reported
    /// an error while the state read back shows the epoch absent
    /// (`already_absent_after_clear_error`, with that `error`).
    Consumed {
        outcome: &'static str,
        attempt: u32,
        error: Option<String>,
    },
    /// Every attempt failed and the epoch is still projected.
    Failed { attempts: u32, error: anyhow::Error },
}

/// Consume the editor-op epoch a write just replayed and applied.
///
/// `#loadbudgettests` / `#compactclearidempotent`: the answer is read from the
/// state, never from whether one bounded transport round trip beat the clock.
/// An actor request can miss its budget on a loaded host while the actor still
/// applies it, so a clear error is followed by a state read; an absent epoch
/// is consumed. Each clear uses the causal-fence budget
/// ([`clear_op_capture_for_editor_projection`]), not the typing hot-path one:
/// an applied cut whose epoch stays open is replayed again by the next cycle.
pub fn consume_replayed_editor_op_epoch(doc: &Path) -> ReplayedEpochConsumption {
    consume_replayed_editor_op_epoch_with(
        doc,
        clear_op_capture_for_editor_projection,
        |doc| matches!(load_op_capture(doc), Ok(None)),
        std::thread::sleep,
    )
}

fn consume_replayed_editor_op_epoch_with(
    doc: &Path,
    mut clear: impl FnMut(&Path) -> Result<()>,
    mut epoch_absent: impl FnMut(&Path) -> bool,
    mut backoff: impl FnMut(Duration),
) -> ReplayedEpochConsumption {
    let mut last_err = None;
    for attempt in 1..=REPLAYED_EPOCH_CLEAR_ATTEMPTS {
        match clear(doc) {
            Ok(()) => {
                return ReplayedEpochConsumption::Consumed {
                    outcome: "cleared_after_successful_write",
                    attempt,
                    error: None,
                };
            }
            Err(err) => {
                if epoch_absent(doc) {
                    return ReplayedEpochConsumption::Consumed {
                        outcome: "already_absent_after_clear_error",
                        attempt,
                        error: Some(err.to_string()),
                    };
                }
                last_err = Some(err);
                if attempt < REPLAYED_EPOCH_CLEAR_ATTEMPTS {
                    backoff(Duration::from_millis(50 * u64::from(attempt)));
                }
            }
        }
    }
    ReplayedEpochConsumption::Failed {
        attempts: REPLAYED_EPOCH_CLEAR_ATTEMPTS,
        error: last_err.expect("every failed attempt records its error"),
    }
}

fn clear_op_capture_with_timeout(doc: &Path, timeout: Duration) -> Result<()> {
    let (project_root, document_hash, _) = state_db_identity(doc)?;
    let nonce = event_nonce();
    let controller_socket = agent_doc_controller::paths::socket_path(&project_root);
    if controller_socket.exists() && !agent_doc_state_wire::in_controller_request() {
        let payload = serde_json::json!({
            "action": "clear",
            "event_nonce": nonce,
        });
        let request = serde_json::json!({
            "command": "editor_op_capture_update",
            "file": doc,
            "diagnostic_payload": serde_json::to_string(&payload)?,
        });
        match agent_doc_state_wire::send_ndjson_request_to_actor(
            &controller_socket,
            &request,
            timeout,
        ) {
            Ok(raw) => {
                #[derive(serde::Deserialize)]
                struct UpdateEnvelope {
                    ok: bool,
                    error: Option<String>,
                }
                let envelope: UpdateEnvelope =
                    serde_json::from_str(&raw).context("decode editor-op clear response")?;
                if !envelope.ok {
                    anyhow::bail!(
                        "Lazily editor-op clear rejected: {}",
                        envelope.error.as_deref().unwrap_or("unknown error")
                    );
                }
                return Ok(());
            }
            Err(agent_doc_state_wire::ActorRequestError::Connect(_))
                if !controller_socket.exists() => {}
            Err(err) => return Err(err).context("clear Lazily editor-op epoch"),
        }
    }

    let mut conn =
        agent_doc_sqlite::state_store::open_state_db_with_timeout(&project_root, timeout)?;
    let local_generation = agent_doc_state_wire::in_controller_request()
        .then(|| load_projected_captures(doc, &project_root, &document_hash))
        .transpose()?
        .map(|(generation, _, _)| generation);
    let tx = conn.transaction()?;
    let generation = match local_generation {
        Some(generation) => generation,
        None => {
            let rows = agent_doc_sqlite::state_store::load_state_events_from_db(
                &tx,
                Some(&document_hash),
            )?;
            let mut ledger = agent_doc_state_backbone::EventLedger::new();
            for row in rows {
                ledger.append(serde_json::from_str(&row.payload_json)?);
            }
            ledger
                .project()
                .document(&document_hash)
                .map(|document| document.document.editor_op_capture_generation)
                .unwrap_or_default()
        }
    };
    let epoch = generation.saturating_add(1);
    let event = agent_doc_state_backbone::StateEvent::new(
        format!("editor-op-capture-cleared:{document_hash}:{epoch}:{nonce}"),
        agent_doc_state_backbone::StateFact::EditorOpCaptureCleared {
            document_hash: document_hash.clone(),
            epoch,
        },
    );
    let payload_json = serde_json::to_string(&event)?;
    agent_doc_sqlite::state_store::insert_state_event_in_db(
        &tx,
        &agent_doc_sqlite::state_store::StateEventInsert {
            event_id: &event.event_id,
            document_hash: event.document_hash(),
            domain: event.domain().label(),
            fact_type: event.fact.label(),
            payload_json: &payload_json,
        },
    )?;
    tx.commit()?;
    agent_doc_state_wire::mark_local_state_db_dirty();
    Ok(())
}

/// Garbage-collect projected epochs whose `updated_ms` is older than
/// `max_age_secs`.
pub fn gc_op_captures(project_root: &Path, max_age_secs: u64) -> Result<usize> {
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let cutoff_ms = now_millis().saturating_sub(max_age_secs.saturating_mul(1000));
    let rows = agent_doc_sqlite::state_store::load_state_events_from_db(&conn, None)?;
    let mut ledger = agent_doc_state_backbone::EventLedger::new();
    for row in rows {
        ledger.append(serde_json::from_str(&row.payload_json)?);
    }
    let stale_paths = ledger
        .project()
        .documents
        .values()
        .filter_map(|document| document.document.editor_op_capture.as_ref())
        .filter(|capture| capture.updated_ms == 0 || capture.updated_ms < cutoff_ms)
        .map(|capture| PathBuf::from(&capture.canonical_path))
        .collect::<Vec<_>>();
    drop(conn);
    let mut cleared = 0;
    for path in stale_paths {
        match clear_op_capture(&path) {
            Ok(()) => cleared += 1,
            Err(err) => eprintln!(
                "[op-capture] warning: failed to GC Lazily epoch for {}: {err:#}",
                path.display()
            ),
        }
    }
    Ok(cleared)
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_doc() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();
        (dir, doc)
    }

    #[test]
    fn current_base_hash_matches_merge_gate_so_stamped_ops_are_accepted() {
        // `#qnodemerge4wire` keystone invariant: ops the editor reporters stamp
        // with `current_base_hash()` MUST pass the write-time merge's
        // `editor_ops_for_base` gate (`base_hash == content_hash(base_text)`),
        // otherwise every capture is silently rejected and the feature no-ops.
        let (_dir, doc) = setup_doc();
        let base_text = "# base\n\n## section\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base_text).encode_state();

        let base_hash =
            current_base_hash_with(&doc, |_doc, _baseline| Ok(base_state.clone())).unwrap();
        record_editor_op(
            &doc,
            &base_hash,
            EditorOp::Insert {
                offset: 0,
                text: "x".into(),
            },
        )
        .unwrap();

        // Resolve the SAME base text `merge_contents_crdt_with_ops` resolves.
        let base_text = agent_doc_merge::crdt::CrdtDoc::decode_state(&base_state)
            .map(|d| d.to_text())
            .unwrap_or_default();

        assert_eq!(
            base_hash,
            content_hash(&base_text),
            "current_base_hash must equal the merge's base_text hash"
        );
        let ops = editor_ops_for_base(&doc, &base_text).unwrap();
        assert!(
            ops.is_some(),
            "ops stamped with current_base_hash must pass the merge gate"
        );
        assert_eq!(ops.unwrap().len(), 1);
    }

    #[test]
    fn current_base_hash_is_empty_text_hash_without_snapshot() {
        // No snapshot/CRDT base yet → empty-text hash, matching the merge's
        // `None => String::new()` base so a first-edit capture still aligns.
        let (_dir, doc) = setup_doc();
        assert_eq!(
            current_base_hash_with(&doc, |_doc, _baseline| {
                Ok(agent_doc_merge::crdt::CrdtDoc::from_text("").encode_state())
            })
            .unwrap(),
            content_hash("")
        );
    }

    #[test]
    fn current_base_hash_is_memoized_until_base_changes() {
        // `#qbasehashmemo`: a typing burst leaves the snapshot + overlay
        // unchanged, so repeated `current_base_hash` calls (one per keystroke)
        // must be served from the memo without rebuilding the full-document CRDT
        // merge base. The per-document recompute counter avoids parallel-test
        // noise from other temp documents.
        let (_dir, doc) = setup_doc();
        let base_text = "# base\n\n## section\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base_text).encode_state();

        let before = base_hash_recomputes_for_doc_for_tests(&doc);
        let h1 = current_base_hash_with(&doc, |_doc, _baseline| Ok(base_state.clone())).unwrap();
        let after_first = base_hash_recomputes_for_doc_for_tests(&doc);
        assert_eq!(after_first, before + 1, "first call must recompute once");

        // Simulate a typing burst: many calls against the same unchanged base.
        for _ in 0..50 {
            assert_eq!(
                current_base_hash_with(&doc, |_doc, _baseline| Ok(base_state.clone())).unwrap(),
                h1
            );
        }
        assert_eq!(
            base_hash_recomputes_for_doc_for_tests(&doc),
            after_first,
            "repeated calls against an unchanged base must be cache hits"
        );

        // Changing the base (a real write boundary) must invalidate the memo.
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "# base\n\n## section\n\nmore\n",
            |_, _| {},
        )
        .unwrap();
        let base_state_2 =
            agent_doc_merge::crdt::CrdtDoc::from_text("# base\n\n## section\n\nmore\n")
                .encode_state();
        let h2 = current_base_hash_with(&doc, |_doc, _baseline| Ok(base_state_2.clone())).unwrap();
        assert_ne!(h1, h2, "a changed snapshot must yield a fresh base hash");
        assert!(
            base_hash_recomputes_for_doc_for_tests(&doc) > after_first,
            "a changed base must trigger a recompute"
        );
    }

    #[test]
    fn record_then_load_roundtrip() {
        let (_dir, doc) = setup_doc();
        let base = "hello world\n";
        let h = content_hash(base);
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 5,
                text: "!".to_string(),
            },
        )
        .unwrap();
        let capture = load_op_capture(&doc).unwrap().unwrap();
        assert_eq!(capture.base_hash, h);
        assert_eq!(capture.epoch, 1);
        assert_eq!(capture.ops.len(), 1);
    }

    #[test]
    fn record_appends_within_same_epoch() {
        let (_dir, doc) = setup_doc();
        let h = content_hash("base\n");
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 0,
                text: "a".into(),
            },
        )
        .unwrap();
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 1,
                text: "b".into(),
            },
        )
        .unwrap();
        let capture = load_op_capture(&doc).unwrap().unwrap();
        assert_eq!(capture.epoch, 1, "same-base append stays in one epoch");
        assert_eq!(capture.ops.len(), 2);
    }

    #[test]
    fn record_batch_preserves_order_and_appends_in_one_epoch() {
        let (_dir, doc) = setup_doc();
        let h = content_hash("base\n");
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 0,
                text: "prefix".into(),
            },
        )
        .unwrap();
        record_editor_ops(
            &doc,
            &h,
            vec![
                EditorOp::Delete { offset: 2, len: 3 },
                EditorOp::Insert {
                    offset: 2,
                    text: "replacement".into(),
                },
            ],
        )
        .unwrap();

        let capture = load_op_capture(&doc).unwrap().unwrap();
        assert_eq!(
            capture.ops,
            vec![
                EditorOp::Insert {
                    offset: 0,
                    text: "prefix".into(),
                },
                EditorOp::Delete { offset: 2, len: 3 },
                EditorOp::Insert {
                    offset: 2,
                    text: "replacement".into(),
                },
            ],
        );
    }

    #[test]
    fn record_against_new_base_resets_epoch() {
        let (_dir, doc) = setup_doc();
        let h1 = content_hash("base one\n");
        record_editor_op(
            &doc,
            &h1,
            EditorOp::Insert {
                offset: 0,
                text: "x".into(),
            },
        )
        .unwrap();
        let first_epoch = load_op_capture(&doc).unwrap().unwrap().epoch;
        let h2 = content_hash("base two\n");
        record_editor_op(
            &doc,
            &h2,
            EditorOp::Insert {
                offset: 0,
                text: "y".into(),
            },
        )
        .unwrap();
        let capture = load_op_capture(&doc).unwrap().unwrap();
        assert_eq!(capture.base_hash, h2);
        assert_eq!(capture.epoch, first_epoch + 1);
        assert_eq!(capture.ops.len(), 1, "new base must start a fresh epoch");
    }

    #[test]
    fn editor_ops_for_base_matches_only_on_hash() {
        let (_dir, doc) = setup_doc();
        let base = "the base text\n";
        let h = content_hash(base);
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 0,
                text: "z".into(),
            },
        )
        .unwrap();
        assert!(editor_ops_for_base(&doc, base).unwrap().is_some());
        assert!(
            editor_ops_for_base(&doc, "a different base\n")
                .unwrap()
                .is_none(),
            "ops captured against a different base must not be trusted"
        );
    }

    #[test]
    fn editor_ops_for_base_none_when_empty_or_absent() {
        let (_dir, doc) = setup_doc();
        assert!(editor_ops_for_base(&doc, "anything").unwrap().is_none());
    }

    /// `#loadbudgettests`: a clear whose round trip misses its budget after the
    /// actor applied it (what a loaded host produces) is still a consumed
    /// epoch. The fault is injected as a clear that lands and then errors, so
    /// the outcome does not depend on host timing.
    #[test]
    fn consume_replayed_epoch_reads_state_after_a_clear_that_landed_but_errored() {
        let (_dir, doc) = setup_doc();
        let h = content_hash("b\n");
        record_editor_op(&doc, &h, EditorOp::Delete { offset: 0, len: 1 }).unwrap();
        let mut backoffs = Vec::new();
        let outcome = consume_replayed_editor_op_epoch_with(
            &doc,
            |doc| {
                clear_op_capture(doc)?;
                anyhow::bail!("injected: actor applied the clear, reply missed its budget")
            },
            |doc| matches!(load_op_capture(doc), Ok(None)),
            |delay| backoffs.push(delay),
        );
        match outcome {
            ReplayedEpochConsumption::Consumed {
                outcome,
                attempt,
                error,
            } => {
                assert_eq!(outcome, "already_absent_after_clear_error");
                assert_eq!(attempt, 1);
                assert!(error.unwrap().contains("injected"));
            }
            other => panic!("a landed clear must read as consumed: {other:?}"),
        }
        assert!(
            backoffs.is_empty(),
            "no retry once the state shows it absent"
        );
        assert!(load_op_capture(&doc).unwrap().is_none());
    }

    #[test]
    fn consume_replayed_epoch_fails_while_the_epoch_is_still_projected() {
        let (_dir, doc) = setup_doc();
        let h = content_hash("b\n");
        record_editor_op(&doc, &h, EditorOp::Delete { offset: 0, len: 1 }).unwrap();
        let mut clears = 0;
        let mut backoffs = Vec::new();
        let outcome = consume_replayed_editor_op_epoch_with(
            &doc,
            |_| {
                clears += 1;
                anyhow::bail!("injected: clear never reached the actor")
            },
            |doc| matches!(load_op_capture(doc), Ok(None)),
            |delay| backoffs.push(delay),
        );
        assert!(
            matches!(
                outcome,
                ReplayedEpochConsumption::Failed { attempts, .. }
                    if attempts == REPLAYED_EPOCH_CLEAR_ATTEMPTS
            ),
            "an epoch still projected must keep reporting failure: {outcome:?}"
        );
        assert_eq!(clears, REPLAYED_EPOCH_CLEAR_ATTEMPTS);
        assert_eq!(
            backoffs.len() as u32,
            REPLAYED_EPOCH_CLEAR_ATTEMPTS - 1,
            "backoff runs between attempts only"
        );
        assert!(load_op_capture(&doc).unwrap().is_some());
    }

    #[test]
    fn consume_replayed_epoch_reports_an_acknowledged_clear() {
        let (_dir, doc) = setup_doc();
        let h = content_hash("b\n");
        record_editor_op(&doc, &h, EditorOp::Delete { offset: 0, len: 1 }).unwrap();
        let outcome = consume_replayed_editor_op_epoch(&doc);
        assert!(
            matches!(
                outcome,
                ReplayedEpochConsumption::Consumed {
                    outcome: "cleared_after_successful_write",
                    attempt: 1,
                    error: None,
                }
            ),
            "{outcome:?}"
        );
        assert!(load_op_capture(&doc).unwrap().is_none());
    }

    #[test]
    fn clear_is_idempotent() {
        let (_dir, doc) = setup_doc();
        clear_op_capture(&doc).unwrap();
        let h = content_hash("b\n");
        record_editor_op(&doc, &h, EditorOp::Delete { offset: 0, len: 1 }).unwrap();
        assert!(load_op_capture(&doc).unwrap().is_some());
        clear_op_capture(&doc).unwrap();
        assert!(load_op_capture(&doc).unwrap().is_none());
        let retained = load_last_op_capture(&doc)
            .unwrap()
            .expect("cleared checkpoint remains durable recovery evidence");
        assert_eq!(retained.base_hash, h);
        assert_eq!(
            editor_ops_for_base_or_retained(&doc, "b\n").unwrap(),
            Some(retained.ops.clone()),
            "bounded projection retries retain the exact-base operation lineage"
        );
        assert_eq!(
            last_editor_text_for_base(&doc, "b\n").unwrap(),
            Some("\n".to_string())
        );
        assert!(
            last_editor_text_for_base(&doc, "different\n")
                .unwrap()
                .is_none(),
            "retained operations must not replay onto a different base"
        );
        clear_op_capture(&doc).unwrap();
        assert_eq!(
            load_last_op_capture(&doc).unwrap(),
            Some(retained),
            "later idempotent clears must not erase retained evidence"
        );
    }

    #[test]
    fn activity_refresh_preserves_the_pending_epoch_and_operations() {
        let (_dir, doc) = setup_doc();
        let base = "b\n";
        let base_hash = content_hash(base);
        record_editor_op(
            &doc,
            &base_hash,
            EditorOp::Insert {
                offset: 0,
                text: "a".into(),
            },
        )
        .unwrap();
        let before = load_op_capture(&doc).unwrap().expect("active capture");

        assert!(refresh_op_capture_activity(&doc).unwrap());
        let after = load_op_capture(&doc)
            .unwrap()
            .expect("activity refresh retains the capture");
        assert_eq!(after.epoch, before.epoch);
        assert_eq!(after.base_hash, before.base_hash);
        assert_eq!(after.ops, before.ops);
        assert!(after.updated_ms >= before.updated_ms);
    }

    /// A fake project controller for `#editoropscheckpointfail`.
    ///
    /// It mirrors the real `editor_op_capture_update` handler's shape: one
    /// writer lock serializes appends, already-applied nonces are acknowledged
    /// without a second append, and the first request is held past the editor's
    /// response budget the way a `refresh_memory` durable reload holds the
    /// state-ingress lock (observed: `state_event_ingress_slow
    /// site=append_apply_state_event fact=editor_op_capture_checkpointed
    /// wait_ms=1165`). The stalled request is still applied — late.
    #[cfg(unix)]
    struct FakeController {
        /// Every append request in arrival order: (nonce, ops_json).
        received: std::sync::Arc<parking_lot::Mutex<Vec<(String, String)>>>,
        /// Appends actually applied, in apply order: (nonce, ops_json).
        applied: std::sync::Arc<parking_lot::Mutex<Vec<(String, String)>>>,
    }

    #[cfg(unix)]
    impl FakeController {
        fn spawn(project_root: &Path, stall_first: Duration, reject: bool) -> Self {
            use std::io::{BufRead, Write};
            let listener = std::os::unix::net::UnixListener::bind(
                project_root.join(".agent-doc/controller.sock"),
            )
            .unwrap();
            let received = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
            let applied = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
            let writes = std::sync::Arc::new(parking_lot::Mutex::new(0usize));
            let (received_c, applied_c) = (received.clone(), applied.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { return };
                    let (received, applied, writes) =
                        (received_c.clone(), applied_c.clone(), writes.clone());
                    std::thread::spawn(move || {
                        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                        if request["command"] != "editor_op_capture_update" {
                            // Ops-log tagging reads the live projection; this
                            // fake owns no document state.
                            let mut stream = stream;
                            let _ = writeln!(
                                stream,
                                "{}",
                                serde_json::json!({"ok": true, "data": null})
                            );
                            return;
                        }
                        let payload: serde_json::Value =
                            serde_json::from_str(request["diagnostic_payload"].as_str().unwrap())
                                .unwrap();
                        let nonce = payload["event_nonce"].as_str().unwrap().to_string();
                        let ops_json = payload["ops_json"].as_str().unwrap().to_string();
                        let mut served = writes.lock();
                        received.lock().push((nonce.clone(), ops_json.clone()));
                        if *served == 0 {
                            std::thread::sleep(stall_first);
                        }
                        *served += 1;
                        let response = if reject {
                            serde_json::json!({"ok": false, "error": "decode editor-op capture update"})
                        } else {
                            let mut applied = applied.lock();
                            let fresh = !applied.iter().any(|(seen, _)| *seen == nonce);
                            if fresh {
                                applied.push((nonce, ops_json));
                            }
                            serde_json::json!({"ok": true, "data": fresh})
                        };
                        drop(served);
                        let mut stream = stream;
                        let _ = writeln!(stream, "{response}");
                    });
                }
            });
            Self { received, applied }
        }
    }

    #[cfg(unix)]
    fn typed(index: usize) -> EditorOp {
        EditorOp::Insert {
            offset: index,
            text: index.to_string(),
        }
    }

    /// `#editoropscheckpointfail` keystone: a typing burst that starts while the
    /// controller is stalled past the editor's 250ms response budget must be
    /// retained, re-sent, and recorded exactly once each, in typing order.
    /// Before the fix the first burst logged `editor_ops_record_failed ...
    /// append Lazily editor-op checkpoint` and was dropped from op provenance
    /// (or, on the controller side, applied late behind the editor's back).
    #[cfg(unix)]
    #[test]
    fn typing_burst_behind_a_stalled_controller_is_retained_and_recorded_once_in_order() {
        let (dir, doc) = setup_doc();
        let controller = FakeController::spawn(dir.path(), Duration::from_millis(400), false);
        let h = content_hash("base\n");

        let first = record_editor_ops_retaining(&doc, &h, vec![typed(0)]).unwrap();
        match &first {
            EditorOpsRecordOutcome::Retained { cause, .. } => assert!(
                cause.contains("append Lazily editor-op checkpoint") && cause.contains("timed out"),
                "the retained cause must carry the full timeout chain: {cause}"
            ),
            other => panic!("a stalled first append must be retained, got {other:?}"),
        }
        for index in 1..5 {
            let outcome = record_editor_ops_retaining(&doc, &h, vec![typed(index)]).unwrap();
            assert!(
                matches!(outcome, EditorOpsRecordOutcome::Retained { .. }),
                "burst {index} must queue behind the unacknowledged burst, got {outcome:?}"
            );
        }

        assert!(
            wait_for_retained_editor_ops(&doc, Duration::from_secs(20)),
            "the retry drainer must deliver every retained burst"
        );
        assert_eq!(retained_editor_ops_pending(&doc), 0);

        let applied: Vec<Vec<EditorOp>> = controller
            .applied
            .lock()
            .iter()
            .map(|(_, ops_json)| serde_json::from_str(ops_json).unwrap())
            .collect();
        assert_eq!(
            applied,
            (0..5).map(|index| vec![typed(index)]).collect::<Vec<_>>(),
            "every burst applied exactly once, in typing order"
        );
        let received = controller.received.lock().clone();
        let first_nonce = &controller.applied.lock()[0].0;
        assert_eq!(
            received
                .iter()
                .filter(|(nonce, _)| nonce == first_nonce)
                .count(),
            2,
            "the stalled burst is re-sent with its original nonce, and that re-send is deduplicated"
        );

        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert_eq!(
            log.lines()
                .filter(|line| line.contains("editor_ops_recorded")
                    && line.contains("retained_retry=true"))
                .count(),
            5,
            "each delivered burst leaves a recorded receipt: {log}"
        );
    }

    /// The strict single-attempt form still reports the stall, with the whole
    /// chain (`#editoropscheckpointfail`: the pre-0.35.487 log carried only the
    /// outer context, which is why this root cause was invisible).
    #[cfg(unix)]
    #[test]
    fn strict_append_against_a_stalled_controller_reports_the_timeout_chain() {
        let (dir, doc) = setup_doc();
        let _controller = FakeController::spawn(dir.path(), Duration::from_millis(400), false);
        let err = record_editor_ops(&doc, &content_hash("base\n"), vec![typed(0)])
            .expect_err("a stalled controller cannot acknowledge within the editor budget");
        let chain = format!("{err:#}");
        assert!(
            chain.contains("append Lazily editor-op checkpoint")
                && chain.contains("timed out waiting for Lazily actor"),
            "{chain}"
        );
    }

    /// A definitive controller rejection is not retained: retrying would only
    /// repeat it, and the caller must see the failure.
    #[cfg(unix)]
    #[test]
    fn rejected_append_is_reported_not_retained() {
        let (dir, doc) = setup_doc();
        let _controller = FakeController::spawn(dir.path(), Duration::ZERO, true);
        let err = record_editor_ops_retaining(&doc, &content_hash("base\n"), vec![typed(0)])
            .expect_err("a rejected append must fail");
        assert!(format!("{err:#}").contains("rejected"), "{err:#}");
        assert_eq!(retained_editor_ops_pending(&doc), 0);
    }

    #[test]
    fn visible_editor_projection_fence_outlives_optional_capture_budget() {
        assert!(EDITOR_PROJECTION_FENCE_TIMEOUT > EDITOR_OP_CAPTURE_BUSY_TIMEOUT);
        assert_eq!(EDITOR_PROJECTION_FENCE_TIMEOUT, Duration::from_secs(3));
    }

    #[test]
    fn legacy_state_row_is_never_runtime_authority() {
        let (_dir, doc) = setup_doc();
        let (project_root, document_hash, canonical_path) = state_db_identity(&doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&project_root).unwrap();
        agent_doc_sqlite::state_store::upsert_editor_op_capture_in_db(
            &conn,
            &agent_doc_sqlite::state_store::EditorOpCaptureRecord {
                document_hash: document_hash.clone(),
                canonical_path,
                base_hash: "base".to_string(),
                ops_json: "{not valid json".to_string(),
                updated_at_ms: now_millis(),
            },
        )
        .unwrap();
        assert!(load_op_capture(&doc).unwrap().is_none());
        assert!(
            agent_doc_sqlite::state_store::load_editor_op_capture_from_db(&conn, &document_hash)
                .unwrap()
                .is_some(),
            "ordinary reads must neither import nor delete inert legacy data",
        );
    }

    #[test]
    fn gc_removes_stale_and_zero_timestamp_rows() {
        let (dir, doc) = setup_doc();
        // Fresh epoch (current timestamp) — should survive a generous max_age.
        let h = content_hash("b\n");
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 0,
                text: "a".into(),
            },
        )
        .unwrap();
        let removed = gc_op_captures(dir.path(), 3600).unwrap();
        assert_eq!(removed, 0, "fresh row must not be GC'd");
        // Supersede it with a zero-timestamp Lazily checkpoint (no usable
        // epoch freshness) — should be reaped with a typed clear fact.
        let capture = load_op_capture(&doc).unwrap().unwrap();
        let (project_root, document_hash, canonical_path) = state_db_identity(&doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&project_root).unwrap();
        let epoch = capture.epoch + 1;
        let event = agent_doc_state_backbone::StateEvent::new(
            format!("test-zero-timestamp:{document_hash}:{epoch}"),
            agent_doc_state_backbone::StateFact::EditorOpCaptureCheckpointed {
                document_hash: document_hash.clone(),
                canonical_path,
                epoch,
                base_hash: capture.base_hash,
                ops_json: serde_json::to_string(&capture.ops).unwrap(),
                updated_ms: 0,
            },
        );
        let payload_json = serde_json::to_string(&event).unwrap();
        agent_doc_sqlite::state_store::insert_state_event_in_db(
            &conn,
            &agent_doc_sqlite::state_store::StateEventInsert {
                event_id: &event.event_id,
                document_hash: event.document_hash(),
                domain: event.domain().label(),
                fact_type: event.fact.label(),
                payload_json: &payload_json,
            },
        )
        .unwrap();
        let removed = gc_op_captures(dir.path(), 3600).unwrap();
        assert_eq!(removed, 1);
        assert!(load_op_capture(&doc).unwrap().is_none());
    }

    #[test]
    fn gc_missing_dir_is_zero() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(gc_op_captures(dir.path(), 60).unwrap(), 0);
    }
}
