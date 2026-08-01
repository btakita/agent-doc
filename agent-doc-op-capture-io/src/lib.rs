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
//! - `editor_ops_for_base` returns `Some(ops)` **only** when the stored
//!   `base_hash` matches the hash of the supplied base text; otherwise `None`.
//! - `clear_op_capture` is idempotent — clearing an absent epoch is `Ok(())`.
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

/// Editor op capture is optional evidence for the CRDT merge. It must never
/// monopolize the authoritative state ledger when a controller owns the write
/// lock: the merge can safely fall back to its diff path if this bounded write
/// cannot be recorded.
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

/// Record an ordered editor-op burst in one bounded state-ledger transaction.
///
/// A quiet-period editor report can contain hundreds of keystrokes. Persisting
/// those one at a time repeatedly opened SQLite, deserialized and serialized the
/// growing JSON vector, and held up controller reads. This batch boundary keeps
/// the same epoch semantics while doing that work exactly once per drained burst.
pub fn record_editor_ops(doc: &Path, base_hash: &str, ops: Vec<EditorOp>) -> Result<()> {
    if ops.is_empty() {
        return Ok(());
    }
    let (project_root, document_hash, canonical_path) = state_db_identity(doc)?;
    let ops_json = serde_json::to_string(&ops)?;
    let updated_ms = now_millis();
    let nonce = event_nonce();
    let controller_socket = agent_doc_controller::paths::socket_path(&project_root);
    if controller_socket.exists() && !agent_doc_state_wire::in_controller_request() {
        let payload = serde_json::json!({
            "action": "append",
            "base_hash": base_hash,
            "ops_json": ops_json,
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
                    error: Option<String>,
                }
                let envelope: UpdateEnvelope =
                    serde_json::from_str(&raw).context("decode editor-op append response")?;
                if !envelope.ok {
                    anyhow::bail!(
                        "Lazily editor-op append rejected: {}",
                        envelope.error.as_deref().unwrap_or("unknown error")
                    );
                }
                return Ok(());
            }
            Err(agent_doc_state_wire::ActorRequestError::Connect(_))
                if !controller_socket.exists() => {}
            Err(err) => return Err(err).context("append Lazily editor-op checkpoint"),
        }
    }

    let mut conn = agent_doc_sqlite::state_store::open_state_db_with_timeout(
        &project_root,
        EDITOR_OP_CAPTURE_BUSY_TIMEOUT,
    )?;
    let local_current = agent_doc_state_wire::in_controller_request()
        .then(|| load_projected_captures(doc, &project_root, &document_hash))
        .transpose()?;
    let tx = conn.transaction()?;
    let (generation, existing) = match local_current {
        Some((generation, active, _)) => (generation, active),
        None => {
            let rows = agent_doc_sqlite::state_store::load_state_events_from_db(
                &tx,
                Some(&document_hash),
            )?;
            let mut ledger = agent_doc_state_backbone::EventLedger::new();
            for row in rows {
                ledger.append(serde_json::from_str(&row.payload_json)?);
            }
            let projection = ledger.project();
            let (generation, active, _) =
                captures_from_document_projection(projection.document(&document_hash))?;
            (generation, active)
        }
    };
    let (epoch, mut complete_ops) = match existing {
        Some(existing) if existing.base_hash == base_hash => (existing.epoch, existing.ops),
        _ => (generation.saturating_add(1), Vec::new()),
    };
    complete_ops.extend(ops);
    let event = agent_doc_state_backbone::StateEvent::new(
        format!("editor-op-capture-checkpoint:{document_hash}:{epoch}:{nonce}"),
        agent_doc_state_backbone::StateFact::EditorOpCaptureCheckpointed {
            document_hash: document_hash.clone(),
            canonical_path,
            epoch,
            base_hash: base_hash.to_string(),
            ops_json: serde_json::to_string(&complete_ops)?,
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
    Ok(())
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
    let fingerprint = content_hash(&baseline);
    let cache_key = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());

    if let Some((cached_fp, cached_hash)) = base_hash_cache().lock().get(&cache_key)
        && *cached_fp == fingerprint
    {
        return Ok(cached_hash.clone());
    }

    BASE_HASH_RECOMPUTES.fetch_add(1, Ordering::Relaxed);
    #[cfg(test)]
    record_base_hash_recompute_for_tests(&cache_key);
    let base_state = resolve_base_state(doc, &baseline)?;
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
