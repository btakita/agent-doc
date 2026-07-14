//! Live wiring of the CRDT relay/commit-barrier into the finalize + disk paths
//! (`#crdtauth4` cutover).
//!
//! The state-vector sync primitive (`agent_doc_merge::crdt_sync`), the authority
//! state machine ([`agent_doc_document_realtime::crdt_authority`]), and the relay
//! hub ([`agent_doc_document_realtime::crdt_relay`]) were built and tested as
//! standalone modules. This
//! module is the **live cutover**: it routes the real `finalize` commit point and
//! the real `.yrs` load/merge call-sites through the authority-gated barrier,
//! while keeping the headless / [`CrdtAuthority::GitAuthoritative`] path
//! byte-for-byte unchanged.
//!
//! ## Authority gate is load-bearing
//!
//! Every entry point here resolves the document's [`CrdtAuthority`] first (cheaply,
//! per-document, fail-safe to `GitAuthoritative`) via the durable reliable-sync
//! liveness projection:
//!
//! - [`CrdtAuthority::GitAuthoritative`] (**Detached** — no live editor): every
//!   entry point is a **no-op** that returns the trivially-ready / unchanged
//!   result. The CRDT is ephemeral, git is the source of truth, and none of the
//!   live-replica machinery runs. This is most dogfooding traffic and it is
//!   provably unchanged (see the tests at the bottom).
//! - [`CrdtAuthority::MultiReplica`] (**EditorAttached** — a live editor plugin):
//!   the commit barrier flushes the currently-live editor replicas to a consistent
//!   cut before the snapshot is committed, and the disk `.yrs` is treated as a
//!   write-through recovery projection only (in-memory wins).
//!
//! ## Per-document isolation (`#xdocsuper1/3`)
//!
//! The hub registry is keyed by the document hash
//! ([`agent_doc_fs::document_state_hash`]). Each document gets its own independent
//! [`RelayHub`]; a hub for one document can never observe or flush another
//! document's replicas. This is the same per-document isolation the hosting-epoch
//! backbone enforces, applied to the live relay layer.
//!
//! ## Scope of this cutover
//!
//! - **Wired:** the finalize commit barrier ([`commit_barrier_for_file`]), the
//!   disk-demotion reconcile at the live load seam
//!   ([`reconcile_disk_projection_for_file`]), supervisor-restart recovery of the
//!   canonical replica from the disk projection ([`recover_hub_from_disk`]), and
//!   the per-document hub registry ([`with_hub`]).
//! - **Wired:** editor-replica lifecycle and delta transport through the
//!   supervisor IPC family (`replica_register`, `replica_update`, `replica_pull`,
//!   `replica_ack`, `replica_deregister`). Fan-out is target-owned: peer updates
//!   remain queued until the target editor applies them to its FFI replica/buffer
//!   and ACKs the delivery. The commit barrier refuses a MultiReplica closeout
//!   while any live target has unacknowledged delivery.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use agent_doc_document_realtime::crdt_authority::CrdtAuthority;
use agent_doc_document_realtime::crdt_relay::{
    AwarenessState, DiskChangeOutcome, PendingReplicaUpdate, RelayHub, ReplicaDeliverySnapshot,
    mint_client_id,
};
use agent_doc_document_realtime::watch_authority::{
    WatchAction, WatchDelivery, decide_watch_action,
};
use lazily::DurableOutbox;

/// Stable event kinds written to `.agent-doc/crdt-replica-events/`.
/// Strings are a wire encoding only; producers select a closed enum variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrdtReplicaEventReason {
    RequestFullState,
    Fanout,
    ResponseCellAdd,
    CpcWrite,
    Rebootstrap,
    AckReplay,
    AckRecoveryForceRefresh,
}

impl CrdtReplicaEventReason {
    pub const fn token(self) -> &'static str {
        match self {
            Self::RequestFullState => "request_full_state",
            Self::Fanout => "fanout",
            Self::ResponseCellAdd => "response_cell_add",
            Self::CpcWrite => "cpc_write",
            Self::Rebootstrap => "rebootstrap",
            Self::AckReplay => "ack_replay",
            Self::AckRecoveryForceRefresh => "ack_recovery_force_refresh",
        }
    }
}

impl std::fmt::Display for CrdtReplicaEventReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.token())
    }
}

fn reliable_sync_database_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".agent-doc")
        .join("reliable_sync_outbox.db")
}

fn restore_durable_liveness(file: &Path, document_hash: &str) -> Result<()> {
    let Some(project_root) = agent_doc_fs::find_project_root(file) else {
        return Ok(());
    };
    let database_path = reliable_sync_database_path(&project_root);
    let snapshot = agent_doc_sqlite::reliable_sync_inbox::load(&database_path)?;
    let mut batches = snapshot
        .liveness
        .iter()
        .map(|record| {
            serde_json::from_str::<Vec<agent_doc_reliable_sync_io::liveness::LivenessOp>>(
                &record.ops_json,
            )
            .with_context(|| {
                format!(
                    "decode durable reliable-sync liveness source={} epoch={}",
                    record.source_key, record.epoch
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if database_path.exists() {
        let outbox = agent_doc_sqlite::reliable_sync_outbox::SqliteOutbox::open(
            &database_path,
            document_hash.to_string(),
        )?;
        for (epoch, message) in outbox.replay_from(0) {
            if let Some(decoded) =
                agent_doc_reliable_sync_io::liveness::decode_liveness_frame(&message)
            {
                batches.push(decoded.with_context(|| {
                    format!(
                        "decode pending reliable-sync liveness document_hash={document_hash} epoch={epoch}"
                    )
                })?);
            }
        }
    }
    let mut plane = agent_doc_reliable_sync_io::global_liveness_plane()
        .lock()
        .map_err(|_| anyhow::anyhow!("reliable-sync liveness plane mutex poisoned"))?;
    for batch in &batches {
        plane.restore_liveness(batch);
    }
    for cursor in &snapshot.cursors {
        plane.restore_cursor(&cursor.document_hash, cursor.ack_through);
    }
    Ok(())
}

/// Default-on editor liveness authority shared by relay, controller, closeout,
/// repair, and write convergence. Cold readers rebuild the receiver journal and
/// retained sender suffix; only the explicit authority-off rollback reads the
/// historical plugin-owner lease.
pub fn reliable_sync_editor_live_for_file(file: &Path) -> bool {
    let file_string = file.to_string_lossy();
    if !agent_doc_reliable_sync_io::authority_enabled() {
        return agent_doc_plugin_owner::live_editor_endpoint_attached(&file_string);
    }
    if let Some(live) = agent_doc_reliable_sync_io::plane_editor_live_for_path(&file_string) {
        return live;
    }
    let document_hash = agent_doc_hash::document_id_for_path(file);
    if let Err(error) = restore_durable_liveness(file, &document_hash) {
        eprintln!(
            "[reliable-sync] durable authority hydration failed for {}: {error:#}",
            file.display()
        );
    }
    if let Some(live) = agent_doc_reliable_sync_io::plane_editor_live_for_path(&file_string) {
        return live;
    }
    false
}

/// Resolve CRDT authority from the shared durable reliable-sync liveness plane.
pub fn crdt_authority_for_file(file: &Path) -> CrdtAuthority {
    if reliable_sync_editor_live_for_file(file) {
        CrdtAuthority::MultiReplica
    } else {
        CrdtAuthority::GitAuthoritative
    }
}

fn authority_for_file(file: &str) -> CrdtAuthority {
    crdt_authority_for_file(Path::new(file))
}

/// The canonical replica's reserved yrs client-id for every per-document hub. The
/// CPC/controller-owned canonical replica is the hub authority; editor replicas
/// mint their own ids via [`mint_client_id`] and can never collide with this
/// reserved id (`RelayHub::register` rejects it).
const CANONICAL_CLIENT_ID: u64 = 1;
#[cfg(test)]
const EDITOR_SYNC_SETTLE_MS: u64 = 75;
#[cfg(test)]
const EDITOR_SYNC_TIMEOUT_MS: u64 = 150;
const DOCUMENT_MODEL_ENSURE_POLL_MS: u64 = 25;
#[cfg(test)]
const DOCUMENT_MODEL_ENSURE_TIMEOUT_MS: u64 = 150;
#[cfg(not(test))]
const DOCUMENT_MODEL_ENSURE_TIMEOUT_MS: u64 = 5_000;
// `#missingreplicarecycle`: an editor that holds the document but registered no
// CRDT replica gets only this brief window to answer the publish request before
// the model is recycled from the durable projection. Kept well under the 750ms
// controller read timeout so a stale/half-synced editor cannot wedge the
// single-threaded controller (a full-timeout wait per read starves every other
// reader). `EditorSyncPending` — a hub that already exists mid-flush — still uses
// the full [`DOCUMENT_MODEL_ENSURE_TIMEOUT_MS`].
#[cfg(test)]
const DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS: u64 = 60;
#[cfg(not(test))]
const DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS: u64 = 400;
// `send_publish_live_buffer` can spend 3s connecting and 6s waiting for a
// receipt. Keep the cross-process lock fresh across that whole window so a slow
// or wedged editor listener cannot cause competing recovery attempts.
const DOCUMENT_MODEL_ENSURE_LOCK_STALE_MS: u64 = 12_000;

/// Process-global per-document relay-hub registry, keyed by document hash.
///
/// Per-document isolation (`#xdocsuper1/3`): each document's replicas live in
/// their own hub; there is no shared canonical replica across documents.
fn hub_registry() -> &'static Mutex<HashMap<String, RelayHub>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, RelayHub>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Run `f` against the per-document [`RelayHub`] for `file`, creating an empty hub
/// on first contact. This is the single entry point for the live relay layer:
/// register/deregister editor replicas, deliver deltas, and drive the commit
/// barrier all go through here so per-document isolation is structural.
///
/// Returns the closure's result. Does NOT consult authority — callers that must
/// gate on `EditorAttached` should resolve [`authority_for_file`] first (the
/// finalize/disk entry points below do).
pub fn with_hub<T>(file: &Path, f: impl FnOnce(&mut RelayHub) -> T) -> Result<T> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    let hub = registry
        .entry(hash)
        .or_insert_with(|| RelayHub::new(CANONICAL_CLIENT_ID));
    Ok(f(hub))
}

/// Run `f` against an already-allocated per-document hub. Unlike
/// [`with_hub_seeded_from_file`], this never creates a hub from disk: callers use
/// it when disk is a recovery projection and an absent hub means the live model is
/// not available.
fn with_existing_hub<T>(file: &Path, f: impl FnOnce(&mut RelayHub) -> T) -> Result<Option<T>> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    Ok(registry.get_mut(&hash).map(f))
}

/// [`with_hub`] for live file-backed authority paths. A newly allocated hub must
/// start from the current document text, not an empty CRDT, or the first editor
/// delta can be applied at a clamped offset and later overwrite the buffer.
fn with_hub_seeded_from_file<T>(file: &Path, f: impl FnOnce(&mut RelayHub) -> T) -> Result<T> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    {
        let mut registry = hub_registry()
            .lock()
            .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
        if let Some(hub) = registry.get_mut(&hash) {
            return Ok(f(hub));
        }
    }
    let seed_text = std::fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("failed to seed relay hub from {}: {e}", file.display()))?;
    let seeded_hub = RelayHub::from_text(CANONICAL_CLIENT_ID, &seed_text);
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    let hub = registry.entry(hash).or_insert(seeded_hub);
    Ok(f(hub))
}

/// Whether a relay hub has been allocated for `doc_hash` (test-only assertion
/// helper, e.g. proving the Detached path allocates no hub).
pub fn hub_is_allocated_for_test(doc_hash: &str) -> bool {
    hub_registry()
        .lock()
        .map(|registry| registry.contains_key(doc_hash))
        .unwrap_or(false)
}

/// Live document text resolved from the CRDT relay authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentText {
    /// No live editor owns the document; callers may use git/disk authority.
    Detached,
    /// A live editor owns the document, but no relay replica has registered.
    EditorAttachedMissingReplica,
    /// The relay has live replicas, but could not reach a consistent canonical
    /// cut. Callers must retry instead of reading disk as a substitute.
    EditorSyncPending,
    /// The relay canonical text after flushing hub-side live replicas.
    Current {
        text: String,
        live_editors: usize,
        delivery_converged: bool,
    },
}

/// Return the current operator-visible document text from the live CRDT relay.
///
/// This is the replacement read authority for the old live-buffer sidecar hot
/// path: when an editor is attached, disk is only a recovery projection and the
/// caller must either use the relay canonical text or retry when the relay has
/// not registered/converged yet.
pub fn current_text_for_file(file: &Path) -> Result<CurrentText> {
    let authority = authority_for_file(&file.display().to_string());
    current_text_for_file_with_authority(file, authority)
}

/// [`current_text_for_file`] without flushing live editor ops into the canonical
/// replica.
pub fn current_text_for_file_nonblocking(file: &Path) -> Result<CurrentText> {
    let authority = authority_for_file(&file.display().to_string());
    current_text_for_file_with_authority_nonblocking(file, authority)
}

/// [`current_text_for_file`] with an explicitly-resolved authority for tests and
/// callers that already hold the authority decision.
pub fn current_text_for_file_with_authority(
    file: &Path,
    authority: CrdtAuthority,
) -> Result<CurrentText> {
    current_text_for_file_with_authority_inner(file, authority, false, true)
}

/// [`current_text_for_file_with_authority`] without flushing live editor ops.
///
/// This is for latency-sensitive observation paths that need a cheap CPC state
/// proof. If a hub exists but is not already a consistent cut, it reports
/// [`CurrentText::EditorSyncPending`] instead of driving the commit barrier.
pub fn current_text_for_file_with_authority_nonblocking(
    file: &Path,
    authority: CrdtAuthority,
) -> Result<CurrentText> {
    current_text_for_file_with_authority_inner(file, authority, false, false)
}

/// Resolve current text after a publish-live-buffer request has already had a
/// bounded chance to restore the live relay model.
///
/// This keeps the first read strict: while an editor owns the document, the
/// binary must ask the editor/controller to republish before it uses the durable
/// `.yrs` recovery projection. The projection remains restart recovery input,
/// not markdown/disk authority.
pub fn current_text_for_file_with_authority_recovering_projection(
    file: &Path,
    authority: CrdtAuthority,
) -> Result<CurrentText> {
    current_text_for_file_with_authority_inner(file, authority, true, true)
}

fn current_text_for_file_with_authority_inner(
    file: &Path,
    authority: CrdtAuthority,
    recover_missing_from_projection: bool,
    flush_barrier: bool,
) -> Result<CurrentText> {
    if !authority.editor_attached() {
        return Ok(CurrentText::Detached);
    }

    let hash = agent_doc_fs::document_state_hash(file)?;
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    if !registry.contains_key(&hash) && recover_missing_from_projection {
        drop(registry);
        recover_missing_hub_from_durable_projection(file, &hash)?;
        registry = hub_registry()
            .lock()
            .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    }
    let Some(hub) = registry.get_mut(&hash) else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_current_text_unavailable file={} authority=multi_replica reason=missing_replica doc_hash={} process_pid={}",
                file.display(),
                hash,
                std::process::id(),
            ),
        );
        return Ok(CurrentText::EditorAttachedMissingReplica);
    };

    let ready = if flush_barrier {
        hub.commit_barrier_under_authority(authority)?
    } else {
        hub.commit_barrier_ready()?
    };
    let delivery_converged = hub.delivery_converged();
    if !ready {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_current_text_unavailable file={} authority=multi_replica reason=sync_pending live_editors={} delivery_converged={}",
                file.display(),
                hub.live_count(),
                delivery_converged,
            ),
        );
        return Ok(CurrentText::EditorSyncPending);
    }

    let text = hub.canonical_text();
    let live_editors = hub.live_count();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_current_text file={} authority=multi_replica len={} hash={} live_editors={} delivery_converged={}",
            file.display(),
            text.len(),
            agent_doc_hash::content_hash(&text),
            live_editors,
            delivery_converged,
        ),
    );
    Ok(CurrentText::Current {
        text,
        live_editors,
        delivery_converged,
    })
}

fn recover_missing_hub_from_durable_projection(file: &Path, hash: &str) -> Result<bool> {
    let projection = match agent_doc_snapshot_io::load_crdt(file) {
        Ok(Some(projection)) => projection,
        Ok(None) => return Ok(false),
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_current_text_projection_recovery_failed file={} authority=multi_replica doc_hash={} reason=load_crdt_error error={} recovery=continue_missing_replica",
                    file.display(),
                    hash,
                    format!("{err:#}").replace('\n', "\\n"),
                ),
            );
            return Ok(false);
        }
    };
    match recover_hub_from_disk(file, &projection)
        .or_else(|err| recover_hub_from_legacy_markdown_projection(file, hash, &projection, err))
    {
        Ok(()) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_current_text_projection_recovered file={} authority=multi_replica doc_hash={} bytes={} process_pid={}",
                    file.display(),
                    hash,
                    projection.len(),
                    std::process::id(),
                ),
            );
            Ok(true)
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_current_text_projection_recovery_failed file={} authority=multi_replica doc_hash={} reason=recover_projection_error error={} recovery=continue_missing_replica",
                    file.display(),
                    hash,
                    format!("{err:#}").replace('\n', "\\n"),
                ),
            );
            Ok(false)
        }
    }
}

fn recover_hub_from_legacy_markdown_projection(
    file: &Path,
    hash: &str,
    projection: &[u8],
    original_err: anyhow::Error,
) -> Result<()> {
    let text = match std::str::from_utf8(projection) {
        Ok(text) if looks_like_legacy_markdown_projection(text) => text,
        _ => return Err(original_err),
    };
    let mut hub = RelayHub::new(CANONICAL_CLIENT_ID);
    let editor = mint_client_id("agent-doc:legacy-markdown-projection");
    hub.register(editor)?;
    hub.apply_local(editor, 0, 0, text)?;
    let repaired_projection = hub.projection_bytes();
    agent_doc_snapshot_io::save_crdt(file, &repaired_projection)?;
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    registry.entry(hash.to_string()).or_insert(hub);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_current_text_legacy_markdown_projection_repaired file={} authority=multi_replica doc_hash={} text_len={} repaired_bytes={} recovery=rewrote_legacy_text_projection",
            file.display(),
            hash,
            text.len(),
            repaired_projection.len(),
        ),
    );
    Ok(())
}

fn looks_like_legacy_markdown_projection(text: &str) -> bool {
    text.starts_with("---\n")
        || text.starts_with("# ")
        || text.contains("<!-- agent:")
        || text.contains("<!-- patch:exchange -->")
}

/// Ensure the live document model is usable before a hot-path read gives up on
/// editor authority.
///
/// This is intentionally narrower than the commit barrier: it does not treat
/// markdown or live-buffer sidecars as authoritative. When the editor owns the
/// document but the relay is missing or not converged, it asks the editor to
/// republish/register its live buffer via the read-only `publish_live_buffer` IPC
/// path, waits for a bounded interval, and only then may restore the in-memory
/// relay hub from the durable `.yrs` restart projection. Callers should surface
/// this failure instead of the raw "missing replica" state so startup/reconcile
/// is the final contract, not the pre-recovery observation.
pub fn ensure_document_model(file: &Path, source: &str) -> Result<CurrentText> {
    let authority = authority_for_file(&file.display().to_string());
    let first = current_text_for_file_with_authority(file, authority)?;
    ensure_document_model_with_current_text_recovery_observer(
        file,
        source,
        first,
        || current_text_for_file_with_authority(file, authority),
        || current_text_for_file_with_authority_recovering_projection(file, authority),
    )
}

/// Ensure the live document model using a caller-supplied current-text observer.
///
/// This keeps the single bounded publish/retry guard in the relay crate while
/// allowing controller clients to request an editor publish outside the
/// controller RPC handler and then poll CPC-owned relay state through the
/// controller. The observer must read relay state only; it must not treat disk or
/// live-buffer sidecars as fallback authority while an editor is attached.
pub fn ensure_document_model_with_current_text_observer(
    file: &Path,
    source: &str,
    first: CurrentText,
    observe_current_text: impl FnMut() -> Result<CurrentText>,
) -> Result<CurrentText> {
    ensure_document_model_with_current_text_observer_inner(
        file,
        source,
        first,
        observe_current_text,
        None,
    )
}

/// [`ensure_document_model_with_current_text_observer`] plus a final recovery
/// observer that may use the durable CRDT projection after publish/retry timed
/// out.
pub fn ensure_document_model_with_current_text_recovery_observer(
    file: &Path,
    source: &str,
    first: CurrentText,
    observe_current_text: impl FnMut() -> Result<CurrentText>,
    mut observe_recovery_current_text: impl FnMut() -> Result<CurrentText>,
) -> Result<CurrentText> {
    ensure_document_model_with_current_text_observer_inner(
        file,
        source,
        first,
        observe_current_text,
        Some(&mut observe_recovery_current_text),
    )
}

fn ensure_document_model_with_current_text_observer_inner(
    file: &Path,
    source: &str,
    first: CurrentText,
    mut observe_current_text: impl FnMut() -> Result<CurrentText>,
    mut observe_recovery_current_text: Option<&mut dyn FnMut() -> Result<CurrentText>>,
) -> Result<CurrentText> {
    if matches!(first, CurrentText::Detached | CurrentText::Current { .. }) {
        return Ok(first);
    }

    let first_label = current_text_label(&first);
    let mut ensure_guard = match acquire_document_model_ensure_attempt(file, source, first_label)? {
        DocumentModelEnsureAdmission::Run(guard) => guard,
        DocumentModelEnsureAdmission::Suppressed(suppression) => {
            return suppressed_document_model_ensure_result(file, source, suppression);
        }
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "document_model_ensure_start file={} source={} initial_state={}",
            file.display(),
            source,
            first_label,
        ),
    );
    let ensure_timeout_ms = if matches!(first, CurrentText::EditorAttachedMissingReplica) {
        DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS
    } else {
        DOCUMENT_MODEL_ENSURE_TIMEOUT_MS
    };
    if let Err(err) = request_document_model_live_buffer_publish_with_timeout(
        file,
        source,
        std::time::Duration::from_millis(ensure_timeout_ms),
    ) {
        ensure_guard.record_failure(first_label);
        return Err(err);
    }

    // `#missingreplicarecycle`: bound how long we wait before recycling the
    // document model from the durable projection. `EditorAttachedMissingReplica`
    // means the durable reliable-sync open set says the editor holds the document,
    // but no CRDT
    // replica is registered at all — the registry has no hub for this doc. A
    // healthy editor registers its replica on open, so a *persistently* missing
    // replica is a stale/half-synced editor (e.g. a JetBrains buffer that
    // keeps its durable Open fact live, yet never publishes a replica). Waiting the
    // full publish timeout on every read then wedges the single-threaded controller
    // and times out every other reader (idle-watch reads give up at 750ms). Give
    // such an editor only a brief window to answer the publish request above, then
    // recycle from the durable `.yrs` projection — the last-known relay canonical,
    // a valid CRDT authority (not disk). If the editor does come alive its
    // `register_replica` merges into the recovered hub
    // (`with_hub_seeded_from_file` adds to the existing hub, never clobbers it).
    // `EditorSyncPending` keeps the full window: there a hub already exists with the
    // editor's un-flushed ops that are worth waiting for.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ensure_timeout_ms);
    let mut last_label = first_label;
    let mut last_observer_error: Option<String> = None;
    loop {
        if std::time::Instant::now() >= deadline {
            if let Some(observer) = observe_recovery_current_text.as_mut() {
                match observer() {
                    Ok(current @ (CurrentText::Detached | CurrentText::Current { .. })) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "document_model_ensure_ready file={} source={} initial_state={} final_state={} recovery=durable_projection_after_publish_timeout",
                                file.display(),
                                source,
                                first_label,
                                current_text_label(&current),
                            ),
                        );
                        ensure_guard.record_success();
                        return Ok(current);
                    }
                    Ok(
                        current @ (CurrentText::EditorAttachedMissingReplica
                        | CurrentText::EditorSyncPending),
                    ) => {
                        last_label = current_text_label(&current);
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "document_model_ensure_projection_recovery_not_ready file={} source={} initial_state={} final_state={} recovery=retry_without_disk_write",
                                file.display(),
                                source,
                                first_label,
                                last_label,
                            ),
                        );
                    }
                    Err(err) => {
                        let detail = format!("{err:#}")
                            .replace('\n', " | ")
                            .chars()
                            .take(240)
                            .collect::<String>();
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "document_model_ensure_projection_recovery_error file={} source={} initial_state={} last_state={} error={} recovery=retry_without_disk_write",
                                file.display(),
                                source,
                                first_label,
                                last_label,
                                detail,
                            ),
                        );
                        last_observer_error = Some(detail);
                    }
                }
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_model_ensure_failed file={} source={} initial_state={} final_state={} timeout_ms={} last_observer_error={} recovery=retry_without_disk_write",
                    file.display(),
                    source,
                    first_label,
                    last_label,
                    DOCUMENT_MODEL_ENSURE_TIMEOUT_MS,
                    last_observer_error.as_deref().unwrap_or("none"),
                ),
            );
            ensure_guard.record_failure(last_label);
            anyhow::bail!(
                "document model startup/reconciliation failed for {}: editor authority stayed in {last_label} after a bounded publish-live-buffer request; disk remained non-authoritative and was not read as a fallback; last_observer_error={}; recovery=retry_without_disk_write; reload or save the editor buffer, then retry",
                file.display(),
                last_observer_error.as_deref().unwrap_or("none")
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(
            DOCUMENT_MODEL_ENSURE_POLL_MS,
        ));
        let current = match observe_current_text() {
            Ok(current) => current,
            Err(err) => {
                let detail = format!("{err:#}")
                    .replace('\n', " | ")
                    .chars()
                    .take(240)
                    .collect::<String>();
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "document_model_ensure_observer_error file={} source={} initial_state={} last_state={} error={} recovery=retry_until_deadline",
                        file.display(),
                        source,
                        first_label,
                        last_label,
                        detail,
                    ),
                );
                last_observer_error = Some(detail);
                continue;
            }
        };
        match current {
            CurrentText::Detached | CurrentText::Current { .. } => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "document_model_ensure_ready file={} source={} initial_state={} final_state={}",
                        file.display(),
                        source,
                        first_label,
                        current_text_label(&current),
                    ),
                );
                ensure_guard.record_success();
                return Ok(current);
            }
            CurrentText::EditorAttachedMissingReplica | CurrentText::EditorSyncPending => {
                last_label = current_text_label(&current);
            }
        }
    }
}

/// Fail fast when another process is actively attempting document-model recovery
/// for the same editor-attached document.
///
/// This is intentionally public for resolver entry points that would otherwise
/// perform a noisy current-text probe before reaching [`ensure_document_model`].
pub fn defer_if_document_model_ensure_suppressed(file: &Path, source: &str) -> Result<()> {
    let authority = authority_for_file(&file.display().to_string());
    if authority.editor_attached()
        && let Some(suppression) = existing_document_model_ensure_in_progress(file, source)?
    {
        suppressed_document_model_ensure_result(file, source, suppression)?;
    }
    Ok(())
}

fn current_text_label(current: &CurrentText) -> &'static str {
    match current {
        CurrentText::Detached => "detached",
        CurrentText::EditorAttachedMissingReplica => "editor_attached_model_missing",
        CurrentText::EditorSyncPending => "editor_sync_pending",
        CurrentText::Current { .. } => "current",
    }
}

#[derive(Debug, Clone)]
struct DocumentModelEnsurePaths {
    lock_path: PathBuf,
}

struct DocumentModelEnsureGuard {
    lock_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct DocumentModelEnsureSuppression {
    reason: &'static str,
    state: &'static str,
}

impl DocumentModelEnsureGuard {
    fn record_failure(&mut self, _state: &str) {}

    fn record_success(&mut self) {}
}

impl Drop for DocumentModelEnsureGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

enum DocumentModelEnsureAdmission {
    Run(DocumentModelEnsureGuard),
    Suppressed(DocumentModelEnsureSuppression),
}

fn document_model_ensure_paths(file: &Path) -> Result<DocumentModelEnsurePaths> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
    let dir = project_root
        .join(".agent-doc")
        .join("document-model-ensure");
    std::fs::create_dir_all(&dir)?;
    Ok(DocumentModelEnsurePaths {
        lock_path: dir.join(format!("{hash}.lock")),
    })
}

fn existing_document_model_ensure_in_progress(
    file: &Path,
    source: &str,
) -> Result<Option<DocumentModelEnsureSuppression>> {
    let paths = document_model_ensure_paths(file)?;
    if let Some(state) =
        fresh_document_model_ensure_marker(&paths.lock_path, DOCUMENT_MODEL_ENSURE_LOCK_STALE_MS)
    {
        let suppression = DocumentModelEnsureSuppression {
            reason: "in_progress",
            state,
        };
        log_document_model_ensure_suppressed(file, source, suppression);
        return Ok(Some(suppression));
    }
    Ok(None)
}

fn acquire_document_model_ensure_attempt(
    file: &Path,
    source: &str,
    initial_state: &'static str,
) -> Result<DocumentModelEnsureAdmission> {
    let paths = document_model_ensure_paths(file)?;
    for _ in 0..2 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths.lock_path)
        {
            Ok(mut lock) => {
                let _ = lock.write_all(initial_state.as_bytes());
                return Ok(DocumentModelEnsureAdmission::Run(
                    DocumentModelEnsureGuard {
                        lock_path: paths.lock_path,
                    },
                ));
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                if let Some(state) = fresh_document_model_ensure_marker(
                    &paths.lock_path,
                    DOCUMENT_MODEL_ENSURE_LOCK_STALE_MS,
                ) {
                    let suppression = DocumentModelEnsureSuppression {
                        reason: "in_progress",
                        state,
                    };
                    log_document_model_ensure_suppressed(file, source, suppression);
                    return Ok(DocumentModelEnsureAdmission::Suppressed(suppression));
                }
                let _ = std::fs::remove_file(&paths.lock_path);
            }
            Err(err) => return Err(err.into()),
        }
    }
    let state = "editor_attached_model_missing";
    let suppression = DocumentModelEnsureSuppression {
        reason: "lock_contention",
        state,
    };
    log_document_model_ensure_suppressed(file, source, suppression);
    Ok(DocumentModelEnsureAdmission::Suppressed(suppression))
}

fn fresh_document_model_ensure_marker(path: &Path, ttl_ms: u64) -> Option<&'static str> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = modified.elapsed().unwrap_or_default();
    if age > std::time::Duration::from_millis(ttl_ms) {
        return None;
    }
    let state = std::fs::read_to_string(path).unwrap_or_default();
    Some(label_to_current_text_state(state.trim()))
}

fn label_to_current_text_state(label: &str) -> &'static str {
    match label {
        "editor_sync_pending" => "editor_sync_pending",
        _ => "editor_attached_model_missing",
    }
}

fn log_document_model_ensure_suppressed(
    file: &Path,
    source: &str,
    suppression: DocumentModelEnsureSuppression,
) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "document_model_ensure_suppressed file={} source={} reason={} state={} lock_stale_ms={} recovery=retry_without_disk_write",
            file.display(),
            source,
            suppression.reason,
            suppression.state,
            DOCUMENT_MODEL_ENSURE_LOCK_STALE_MS,
        ),
    );
}

fn suppressed_document_model_ensure_result(
    file: &Path,
    source: &str,
    suppression: DocumentModelEnsureSuppression,
) -> Result<CurrentText> {
    anyhow::bail!(
        "document model startup/reconciliation for {} suppressed duplicate publish-live-buffer request from {source}; reason={}; editor authority stayed in {}; disk remained non-authoritative and was not read as a fallback; recovery=retry_without_disk_write; retry after the active recovery attempt finishes, or reload/save the editor buffer",
        file.display(),
        suppression.reason,
        suppression.state,
    );
}

pub fn request_document_model_live_buffer_publish_with_timeout(
    file: &Path,
    source: &str,
    timeout: std::time::Duration,
) -> Result<()> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
    let path_str = canonical.to_string_lossy().to_string();
    let doc_hash =
        agent_doc_fs::document_state_hash(&canonical).unwrap_or_else(|e| format!("hash_error:{e}"));
    let listener_active = agent_doc_ipc_io::is_listener_active(&project_root);
    let (transport, publish_result) = if listener_active {
        match agent_doc_ipc_io::send_publish_live_buffer_with_timeout(
            &project_root,
            &path_str,
            timeout,
        ) {
            Ok(true) => ("editor_ipc", Ok(true)),
            Ok(false) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "document_model_ensure_publish_socket_unavailable file={} canonical={} source={} transport=editor_ipc project_root={} listener_active={} doc_hash={} action=file_signal_fallback process_pid={}",
                        file.display(),
                        canonical.display(),
                        source,
                        project_root.display(),
                        listener_active,
                        doc_hash,
                        std::process::id(),
                    ),
                );
                (
                    "file_signal_after_socket_unavailable",
                    agent_doc_ipc_io::send_publish_live_buffer_file_signal(
                        &project_root,
                        &path_str,
                    ),
                )
            }
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "document_model_ensure_publish_socket_error file={} canonical={} source={} transport=editor_ipc project_root={} listener_active={} doc_hash={} action=file_signal_fallback process_pid={} error={}",
                        file.display(),
                        canonical.display(),
                        source,
                        project_root.display(),
                        listener_active,
                        doc_hash,
                        std::process::id(),
                        err.to_string().replace(char::is_whitespace, "_"),
                    ),
                );
                (
                    "file_signal_after_socket_error",
                    agent_doc_ipc_io::send_publish_live_buffer_file_signal(
                        &project_root,
                        &path_str,
                    ),
                )
            }
        }
    } else {
        (
            "file_signal",
            agent_doc_ipc_io::send_publish_live_buffer_file_signal(&project_root, &path_str),
        )
    };
    match publish_result {
        Ok(true) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_model_ensure_publish_requested file={} canonical={} source={} transport={} project_root={} listener_active={} doc_hash={} process_pid={}",
                    file.display(),
                    canonical.display(),
                    source,
                    transport,
                    project_root.display(),
                    listener_active,
                    doc_hash,
                    std::process::id(),
                ),
            );
            Ok(())
        }
        Ok(false) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_model_ensure_publish_unavailable file={} canonical={} source={} transport={} project_root={} listener_active={} doc_hash={} process_pid={} recovery=continue_to_projection_recovery",
                    file.display(),
                    canonical.display(),
                    source,
                    transport,
                    project_root.display(),
                    listener_active,
                    doc_hash,
                    std::process::id(),
                ),
            );
            Ok(())
        }
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_model_ensure_publish_error file={} canonical={} source={} transport={} project_root={} listener_active={} doc_hash={} process_pid={} error={} recovery=continue_to_projection_recovery",
                    file.display(),
                    canonical.display(),
                    source,
                    transport,
                    project_root.display(),
                    listener_active,
                    doc_hash,
                    std::process::id(),
                    e,
                ),
            );
            Ok(())
        }
    }
}

/// The outcome of a live editor-replica IPC delta relayed through the
/// per-document hub (`#crdtauth5`).
#[derive(Debug, Clone)]
pub struct FanOut {
    /// The minted yrs client-id of the origin editor replica.
    pub origin: u64,
    /// The incremental update fanned out (only the new op(s)).
    pub update: Vec<u8>,
    /// The currently-live OTHER replicas that received `update`.
    pub targets: Vec<u64>,
    /// The canonical converged text length (chars) after integrating — for
    /// diagnostics / ops.log only.
    pub canonical_len: usize,
}

/// Result of a CPC-authored CRDT write into the controller-owned canonical
/// replica. Disk materialization may use this result as proof that the document
/// file is a projection of the relay, not a separate editor-authoritative path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpcRelayWrite {
    pub applied: bool,
    pub content_len: usize,
    pub content_hash: String,
    pub update_bytes: usize,
    pub targets: usize,
    pub live_editors: usize,
    pub delivery_converged: bool,
}

/// Result of one body-aware assistant response cell added to the canonical
/// realtime document. `content` is the exact post-operation canonical projection
/// used by snapshot/commit materialization; it is not a second write request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseCellRelayWrite {
    pub applied: bool,
    pub cell_id: String,
    pub content: String,
    pub content_hash: String,
    pub update_bytes: usize,
    pub targets: usize,
    pub live_editors: usize,
    pub delivery_converged: bool,
}

/// Pending updates plus delivery state for one editor replica.
#[derive(Debug, Clone)]
pub struct ReplicaPull {
    pub client_id: u64,
    pub updates: Vec<PendingReplicaUpdate>,
    pub delivery: ReplicaDeliverySnapshot,
}

/// Register an editor replica with the document's per-document hub on the live
/// IPC path (`#crdtauth5`, plan phase 5), authority-gated.
///
/// - [`CrdtAuthority::GitAuthoritative`] (**Detached**): refused — `Ok(None)`,
///   and NO hub is allocated. A document with no live editor has no
///   multi-replica session to join; the headless control-plane path is
///   untouched.
/// - [`CrdtAuthority::MultiReplica`] (**EditorAttached**): mints a stable
///   client-id from `identity`, registers it in the per-document hub
///   (bootstrapping it from canonical), and returns
///   `Some((client_id, canonical_bootstrap_state))` so the editor's FFI node
///   starts converged.
///
/// A client-id collision (already registered, or canonical-id collision) is a
/// hard error per the plan's unique-stable-client-id rule.
/// #live-editor-reactive: mark this document open in the process-local reactive
/// `editor_open_docs` authority. Called from the editor replica lifecycle (register /
/// reconnect / update) — an explicit, in-process event, so the reactive authority stays
/// truthful **without any filesystem read**. Every document that reaches the CRDT relay is
/// an agent-doc session document, so `is_agent_doc` is true.
fn mark_editor_open_docs_open(file: &Path) {
    agent_doc_document_realtime::editor_open_docs::editor_open_docs()
        .mark_open(&file.display().to_string(), true);
}

/// #live-editor-reactive: mark this document closed in the reactive authority. Called on
/// an explicit editor `replica_deregister` (the editor detached / closed the buffer). A
/// controller recycle sends no deregister, so it leaves this compatibility projection
/// untracked; the resolver restores authority from durable reliable-sync state.
fn mark_editor_open_docs_closed(file: &Path) {
    agent_doc_document_realtime::editor_open_docs::editor_open_docs()
        .mark_closed(&file.display().to_string());
}

/// Seed the legacy process-exit watcher projection from the reliable-sync open pids.
/// Durable hydration runs first, so a controller recycle does not need a lease read.
/// The plugin-owner pid is consulted only in the explicit authority rollback mode.
fn mark_editor_attach_open(file: &Path) {
    let doc = file.display().to_string();
    if agent_doc_reliable_sync_io::authority_enabled() {
        let _ = reliable_sync_editor_live_for_file(file);
        let document_hash = agent_doc_hash::document_id_for_path(file);
        let pids = agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .ok()
            .map(|plane| {
                plane
                    .projection()
                    .open_pids(&document_hash)
                    .into_iter()
                    .filter(|pid| plane.projection().pid_alive(*pid))
                    .filter_map(|pid| u32::try_from(pid).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for pid in pids {
            agent_doc_document_realtime::editor_attach::editor_attach().attach(&doc, pid);
        }
    } else if let Some(lease) = agent_doc_plugin_owner::read_plugin_owner_lease(&doc) {
        agent_doc_document_realtime::editor_attach::editor_attach().attach(&doc, lease.pid);
    }
}

/// Re-seed the reactive editor-attach watcher projection **only if** the
/// document is not already reactively attached. Called on the high-frequency editor
/// `replica_update` path so durable hydration is not repeated per keystroke;
/// it fires only to recover after a controller recycle (in-memory state lost) or a stale
/// exit mark, matching the phantom-heal reattach next to it. A genuine update proves the
/// editor is alive, so re-asserting `alive` is correct.
fn reseed_editor_attach_if_needed(file: &Path) {
    let doc = file.display().to_string();
    if !agent_doc_document_realtime::editor_attach::editor_attach().is_attached(&doc) {
        mark_editor_attach_open(file);
    }
}

/// #s4b: mark this document detached in the reactive editor-attach authority on an
/// explicit editor `replica_deregister` (mirrors [`mark_editor_open_docs_closed`]). A
/// controller recycle sends no deregister, so durable reliable-sync hydration restores it.
fn mark_editor_attach_closed(file: &Path) {
    agent_doc_document_realtime::editor_attach::editor_attach().detach(&file.display().to_string());
}

pub fn register_replica_for_file(file: &Path, identity: &str) -> Result<Option<(u64, Vec<u8>)>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = mint_client_id(identity);
    let bootstrap = with_hub_seeded_from_file(file, |hub| {
        if hub.is_registered(client_id) {
            // Idempotent re-register (e.g. an editor reconnect that re-announces
            // the same stable identity): reconnect/sync the existing mirror, then
            // return the current canonical bootstrap state.
            hub.reconnect(client_id)
                .map(|()| hub.canonical_encoded_state())
        } else {
            hub.register(client_id)
                .map(|()| hub.canonical_encoded_state())
        }
    })??;
    // Editor attach is an explicit event → drive the reactive open-docs authority.
    mark_editor_open_docs_open(file);
    // Seed the legacy process-exit watcher from durable reliable-sync open pids.
    mark_editor_attach_open(file);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_replica_register file={} authority=multi_replica client_id={} bootstrap_bytes={}",
            file.display(),
            client_id,
            bootstrap.len(),
        ),
    );
    Ok(Some((client_id, bootstrap)))
}

/// Deregister an editor replica from the document's hub on the live IPC path
/// (editor/IDE closed the document). Authority-gated like
/// [`register_replica_for_file`]: `Ok(false)` (no hub touched) under Detached;
/// `Ok(true)` when a live-attached hub dropped the mirror.
pub fn deregister_replica_for_file(file: &Path, identity: &str) -> Result<bool> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(false);
    }
    let client_id = mint_client_id(identity);
    let removed = with_hub_seeded_from_file(file, |hub| hub.deregister(client_id))?;
    // Editor detach is an explicit event → mark the reactive open-docs authority closed.
    // (A controller recycle sends no deregister; durable reliable-sync hydration
    // remains the authority source rather than this compatibility projection.)
    mark_editor_open_docs_closed(file);
    // Explicit close updates the legacy process-exit watcher projection. The durable
    // reliable-sync Close fact independently drives the hot-path authority gate.
    mark_editor_attach_closed(file);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_replica_deregister file={} authority=multi_replica client_id={} removed={}",
            file.display(),
            client_id,
            removed,
        ),
    );
    Ok(removed)
}

/// Relay a **raw encoded yrs update** from an editor replica through the
/// document's per-document hub: integrate it into the canonical replica and fan
/// the missing delta out to every OTHER live replica's hub-side mirror
/// (`#crdtauth5`, plan phase 5), authority-gated.
///
/// - [`CrdtAuthority::GitAuthoritative`] (**Detached**): refused — `Ok(None)`,
///   no hub allocated. The headless path never fans deltas.
/// - [`CrdtAuthority::MultiReplica`] (**EditorAttached**): applies the editor's
///   op, integrates canonical, broadcasts, and returns the [`FanOut`] (per-target
///   delta + canonical text length) so the IPC handler can relay the delta back
///   out over the socket to the peers' FFI nodes.
///
/// Per-document isolation is structural: the update only ever reaches THIS
/// document's hub (keyed by [`agent_doc_fs::document_state_hash`]) — `#xdocsuper1/3`.
pub fn relay_replica_update_for_file(
    file: &Path,
    identity: &str,
    update: &[u8],
) -> Result<Option<FanOut>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = mint_client_id(identity);
    // Zero-member relay auto-heal: after a
    // controller/supervisor recycle (`#statedbgc`), the in-process `hub_registry`
    // restarts empty and the hub is rebuilt from the durable `.yrs` projection with
    // ONLY the canonical replica (`live_count() == 0`). The editor is genuinely
    // still open — its FFI replica keeps shipping `replica_update`s for its stable
    // client-id — but the hub no longer knows that client, so the relay used to
    // hard-fail with "replica {id} is not registered", leaving the editor a phantom
    // (zero live replicas) forever until it was re-opened. Since the caller proved
    // `editor_attached()` and provided the editor's stable `identity`, re-register
    // the dropped replica (seeded from the recovered canonical) before relaying. The
    // editor's update — encoded as "everything a fresh peer is missing" — then
    // integrates idempotently and `live_count()` returns to 1, healing the phantom
    // on the editor's next edit with no plugin round-trip.
    let mut reattached = false;
    let packet = with_hub_seeded_from_file(file, |hub| {
        if !hub.is_registered(client_id) {
            hub.register(client_id)?;
            reattached = true;
        }
        hub.relay_update(client_id, update)
    })??;
    // An editor update proves the doc is open → keep the reactive open-docs authority
    // truthful (also re-seeds it after a recycle-driven phantom-heal reattach).
    mark_editor_open_docs_open(file);
    // Re-seed the legacy process-exit watcher only if not already attached
    // (recovers after a recycle/stale-exit without a per-update durable-state read).
    reseed_editor_attach_if_needed(file);
    if reattached {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_reattach_on_update file={} authority=multi_replica client_id={} recovery=reregister_dropped_replica",
                file.display(),
                client_id,
            ),
        );
        // `#reattach-adopt`: the member was just re-seeded from the recovered canonical,
        // which may carry drift the operator already deleted (`#sy71`) that this
        // incremental update's union-merge cannot remove. Ask the editor to re-announce
        // its FULL authoritative state (adopt frame) so the controller REPLACES the
        // drifted canonical. Additive + regression-safe: the union-merge above already
        // kept the editor's edits; the adopt corrects the drift when it lands, and once
        // the canonical is clean subsequent updates fold onto it with nothing to re-add.
        if let Err(err) =
            signal_crdt_replica_event(file, CrdtReplicaEventReason::RequestFullState, 0)
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_replica_event_signal_failed file={} reason=request_full_state error={err}",
                    file.display(),
                ),
            );
        }
    }
    let canonical_len =
        with_hub_seeded_from_file(file, |hub| hub.canonical_text().chars().count())?;
    if !packet.targets.is_empty()
        && !packet.update.is_empty()
        && let Err(err) =
            signal_crdt_replica_event(file, CrdtReplicaEventReason::Fanout, packet.targets.len())
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_event_signal_failed file={} reason=fanout error={err}",
                file.display(),
            ),
        );
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_replica_fanout file={} authority=multi_replica origin={} targets={} update_bytes={} canonical_len={}",
            file.display(),
            packet.origin,
            packet.targets.len(),
            packet.update.len(),
            canonical_len,
        ),
    );
    Ok(Some(FanOut {
        origin: packet.origin,
        update: packet.update,
        targets: packet.targets,
        canonical_len,
    }))
}

/// Apply a CPC-authored full-document update through the CRDT relay.
///
/// This is the controller→editor direction for recovered/finalized writes. It
/// refuses to create a relay hub from disk while an editor is attached, and it
/// only mutates the canonical replica when the caller's `expected_current`
/// byte-matches the current CPC canonical text after the live-editor commit
/// barrier has flushed inbound editor ops. That baseline check is the guard that
/// keeps unsaved editor-buffer changes from being overwritten by a stale binary
/// recovery response.
/// Apply a CPC full-document replace against an already-resolved relay `hub`.
///
/// Shared by the first-attempt and durable-projection-recovery paths of
/// [`apply_cpc_write_for_file`] so both enforce the identical commit-barrier and
/// `expected_current` baseline guards. Fails closed (`retry_crdt_merge`) when the
/// hub canonical diverges from `expected_current`, so recovering a hub from the
/// durable projection can never overwrite unsaved editor state that the caller
/// did not compact against.
fn apply_cpc_write_on_hub(
    hub: &mut RelayHub,
    file: &Path,
    authority: CrdtAuthority,
    expected_current: &str,
    content: &str,
) -> Result<CpcRelayWrite> {
    let ready = hub.commit_barrier_under_authority(authority)?;
    if !ready {
        anyhow::bail!(
            "CPC relay write refused for {}: editor_sync_pending; disk is a non-authoritative projection",
            file.display()
        );
    }
    let canonical = hub.canonical_text();
    if canonical != expected_current {
        anyhow::bail!(
            "CPC relay write refused for {}: expected_hash={} current_hash={} recovery=retry_crdt_merge",
            file.display(),
            agent_doc_hash::content_hash(expected_current),
            agent_doc_hash::content_hash(&canonical)
        );
    }
    let before_hash = agent_doc_hash::content_hash(&canonical);
    let packet = hub.apply_canonical_replace(expected_current, content)?;
    let applied = before_hash != agent_doc_hash::content_hash(content);
    let targets = packet.targets.len();
    Ok(CpcRelayWrite {
        applied,
        content_len: content.len(),
        content_hash: agent_doc_hash::content_hash(content),
        update_bytes: packet.update.len(),
        targets,
        live_editors: hub.live_count(),
        // An empty delivery set proves convergence only for an idempotent
        // no-op. If canonical content advanced while a durable editor owner
        // has no registered replica, nobody has observed that frontier yet.
        delivery_converged: hub.delivery_converged() && (!applied || targets > 0),
    })
}

fn apply_response_cell_on_hub(
    hub: &mut RelayHub,
    file: &Path,
    authority: CrdtAuthority,
    response: &str,
) -> Result<ResponseCellRelayWrite> {
    let ready = hub.commit_barrier_under_authority(authority)?;
    if !ready {
        anyhow::bail!(
            "response cell add refused for {}: editor_sync_pending",
            file.display()
        );
    }
    let canonical = hub.canonical_text();
    let outcome = agent_doc_merge::response_cell::add_response_cell(&canonical, response)?;
    let (update_bytes, targets) = if outcome.applied {
        let packet = hub.apply_canonical_replace(&canonical, &outcome.content)?;
        (packet.update.len(), packet.targets.len())
    } else {
        (0, 0)
    };
    // The state-backbone fact is appended only after this operation returns, so
    // make the CRDT mutation restart-durable first.  Persisting while the hub is
    // locked also prevents two concurrent response cells from writing recovery
    // projections out of canonical order.
    agent_doc_snapshot_io::save_crdt(file, &hub.projection_bytes())?;
    Ok(ResponseCellRelayWrite {
        applied: outcome.applied,
        cell_id: outcome.cell_id,
        content_hash: agent_doc_hash::content_hash(&outcome.content),
        content: outcome.content,
        update_bytes,
        targets,
        live_editors: hub.live_count(),
        delivery_converged: hub.delivery_converged(),
    })
}

/// Apply one idempotent assistant-response cell directly to the controller's
/// canonical CRDT document. The semantic operation is evaluated while the hub is
/// locked and after its inbound barrier, so concurrent operator text is part of
/// the apply-time canonical instead of a stale caller-provided baseline.
pub fn add_response_cell_for_file(
    file: &Path,
    response: &str,
    source: &str,
) -> Result<Option<ResponseCellRelayWrite>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }

    let Some(result) = with_existing_hub(file, |hub| {
        apply_response_cell_on_hub(hub, file, authority, response)
    })?
    else {
        // A durable projection can lag an attached editor that has not yet
        // re-registered after controller restart. Reconstructing the response
        // operation from that projection could drop live typing or document
        // components. Defer to the existing IPC retry path until the live
        // canonical relay model is present.
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_response_cell_add_deferred file={} source={} reason=missing_live_canonical_model recovery=wait_for_editor_replica",
                file.display(),
                source,
            ),
        );
        return Ok(None);
    };
    let result = result?;

    if result.targets > 0
        && result.update_bytes > 0
        && let Err(err) = signal_crdt_replica_event(
            file,
            CrdtReplicaEventReason::ResponseCellAdd,
            result.targets,
        )
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_event_signal_failed file={} reason=response_cell_add error={err}",
                file.display()
            ),
        );
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_response_cell_add file={} source={} cell_id={} applied={} content_hash={} update_bytes={} targets={} live_editors={} delivery_converged={}",
            file.display(),
            source,
            result.cell_id,
            result.applied,
            result.content_hash,
            result.update_bytes,
            result.targets,
            result.live_editors,
            result.delivery_converged,
        ),
    );
    Ok(Some(result))
}

pub fn apply_cpc_write_for_file(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
) -> Result<Option<CpcRelayWrite>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    // The document-op plane feeds the canonical independently of the transient
    // registered-replica count. A zero-live hub is therefore still a current CRDT
    // authority; demoting it to disk here would reintroduce the frozen-canonical
    // wedge this path used to work around. The CAS below remains the safety gate.
    // First attempt against an already-registered live relay hub. When the editor
    // is attached but this process has no registered replica (a transient gap after
    // a controller recycle / editor restart, or the FFI replica dropped), the hub
    // is absent and `with_existing_hub` returns `None`.
    let result = if let Some(result) = with_existing_hub(file, |hub| {
        apply_cpc_write_on_hub(hub, file, authority, expected_current, content)
    })? {
        result
    } else {
        // Recover the hub from the durable `.yrs` projection before failing —
        // symmetric with the read path
        // ([`current_text_for_file_with_authority_recovering_projection`]). The
        // projection is the last-known relay canonical, not raw disk, so this does
        // not smuggle a non-authoritative disk image in: the `expected_current`
        // baseline check inside [`apply_cpc_write_on_hub`] still fails closed with
        // `retry_crdt_merge` if the recovered canonical diverges from what the
        // caller compacted against. Without this, a compact/CPC write hard-fails
        // the whole operation (observed: JB `Compact Exchange` →
        // `crdt_cpc_write ... no registered replica yet`, #cpcwritemissingreplica).
        let hash = agent_doc_fs::document_state_hash(file)?;
        let recovered = recover_missing_hub_from_durable_projection(file, &hash)?;
        match if recovered {
            with_existing_hub(file, |hub| {
                apply_cpc_write_on_hub(hub, file, authority, expected_current, content)
            })?
        } else {
            None
        } {
            Some(result) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "crdt_cpc_write_recovered_missing_replica file={} source={} authority=multi_replica doc_hash={} recovery=durable_projection",
                        file.display(),
                        source,
                        hash,
                    ),
                );
                result
            }
            None => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "crdt_cpc_write_deferred file={} source={} authority=multi_replica reason=missing_relay_model recovered_projection={} recovery=publish_live_buffer_register_crdt",
                        file.display(),
                        source,
                        recovered,
                    ),
                );
                anyhow::bail!(
                    "CPC relay write unavailable for {}; editor is the current authority but the CRDT relay has no registered replica yet",
                    file.display()
                );
            }
        }
    };
    let result = result?;
    if result.targets > 0
        && result.update_bytes > 0
        && let Err(err) =
            signal_crdt_replica_event(file, CrdtReplicaEventReason::CpcWrite, result.targets)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_event_signal_failed file={} reason=cpc_write error={err}",
                file.display(),
            ),
        );
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_cpc_write file={} source={} authority=multi_replica applied={} content_len={} content_hash={} update_bytes={} targets={} live_editors={} delivery_converged={}",
            file.display(),
            source,
            result.applied,
            result.content_len,
            result.content_hash,
            result.update_bytes,
            result.targets,
            result.live_editors,
            result.delivery_converged,
        ),
    );
    Ok(Some(result))
}

/// Pull supervisor-to-editor updates queued for this replica. The returned
/// updates remain pending until [`ack_replica_update_for_file`] confirms the
/// editor applied them.
pub fn pull_replica_updates_for_file(file: &Path, identity: &str) -> Result<Option<ReplicaPull>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = mint_client_id(identity);
    let updates = with_hub_seeded_from_file(file, |hub| hub.pending_updates(client_id))??;
    let delivery = with_hub_seeded_from_file(file, |hub| {
        hub.delivery_snapshot()
            .into_iter()
            .find(|entry| entry.client_id == client_id)
    })?
    .ok_or_else(|| anyhow::anyhow!("replica {client_id} is not registered"))?;
    // Only log a pull that actually delivers work or advances the ack frontier.
    // The editor replica forwarder polls this ~4×/second while attached; logging
    // every empty steady-state poll floods ops.log (observed growing it to
    // ~800MB and starving the session) without recording anything actionable
    // (#crdtpullspam).
    if !updates.is_empty() || delivery.current_generation != delivery.last_ack_generation {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_pull file={} authority=multi_replica client_id={} updates={} current_generation={} last_ack_generation={}",
                file.display(),
                client_id,
                updates.len(),
                delivery.current_generation,
                delivery.last_ack_generation,
            ),
        );
    }
    Ok(Some(ReplicaPull {
        client_id,
        updates,
        delivery,
    }))
}

/// D2 delivery: if the editor `identity` was flagged for a **replace-capable
/// re-bootstrap** (an out-of-band deletion the additive CRDT delta cannot
/// express), return the corrected canonical text and clear the flag. `Ok(None)`
/// when nothing is pending or the doc is not editor-attached. The editor may
/// replace its buffer only after proving the visible editor buffer and local
/// native replica still match the expected baseline; otherwise it republishes the
/// editor buffer through the relay and lets operator text win.
pub fn pull_rebootstrap_for_file(file: &Path, identity: &str) -> Result<Option<String>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = mint_client_id(identity);
    let text = with_hub_seeded_from_file(file, |hub| {
        if hub.pending_rebootstrap_members().contains(&client_id) {
            let text = hub.rebootstrap_text();
            hub.clear_rebootstrap(client_id);
            Some(text)
        } else {
            None
        }
    })?;
    if text.is_some() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_rebootstrap_pull file={} authority=multi_replica identity={} action=replace_buffer",
                file.display(),
                identity,
            ),
        );
    }
    Ok(text)
}

/// ACK one pulled update after the editor applied it to the local document
/// replica/buffer.
pub fn ack_replica_update_for_file(
    file: &Path,
    identity: &str,
    patch_id: &str,
    generation: u64,
) -> Result<Option<bool>> {
    ack_replica_update_for_file_with_content_hash(file, identity, patch_id, generation, None)
}

/// Hash-qualified variant of [`ack_replica_update_for_file`]. Current editor
/// plugins send the visible editor text hash so a handled CRDT generation cannot
/// be mistaken for actual convergence. `None` is accepted only for compatibility
/// with an older plugin during the crash-safe install handoff.
pub fn ack_replica_update_for_file_with_content_hash(
    file: &Path,
    identity: &str,
    patch_id: &str,
    generation: u64,
    applied_content_hash: Option<&str>,
) -> Result<Option<bool>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = mint_client_id(identity);
    let acknowledged = with_hub_seeded_from_file(file, |hub| {
        hub.ack_delivery_with_content_hash(client_id, patch_id, generation, applied_content_hash)
    })??;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_replica_ack file={} authority=multi_replica client_id={} patch_id={} generation={} content_hash={} acknowledged={}",
            file.display(),
            client_id,
            patch_id,
            generation,
            applied_content_hash.unwrap_or("legacy-unverified"),
            acknowledged,
        ),
    );
    Ok(Some(acknowledged))
}

/// Push an ephemeral awareness/presence update for an editor replica through the
/// document's hub (`#crdtauth5`). Authority-gated; presence is NOT part of the
/// document CRDT, never persisted, never committed. Returns the deterministic
/// presence snapshot of all live replicas for fan-out, or `None` under Detached.
pub fn set_replica_awareness_for_file(
    file: &Path,
    identity: &str,
    state: AwarenessState,
) -> Result<Option<Vec<(u64, AwarenessState)>>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = mint_client_id(identity);
    let snapshot = with_hub_seeded_from_file(file, |hub| {
        hub.set_awareness(client_id, state);
        hub.awareness_snapshot()
    })?;
    Ok(Some(snapshot))
}

/// Recover the per-document canonical replica from a durable disk recovery
/// projection on supervisor restart (plan phase 6). At most one flush is lost;
/// live editors re-sync newer ops when they re-register. The disk `.yrs` is a
/// recovery input only, never authority.
///
/// Idempotent on an existing hub: if a live hub for the document already exists,
/// the stale disk projection is reconciled into it (in-memory wins) rather than
/// replacing it.
pub fn recover_hub_from_disk(file: &Path, projection: &[u8]) -> Result<()> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    {
        let registry = hub_registry()
            .lock()
            .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
        if let Some(existing) = registry.get(&hash) {
            // A live hub already holds the authority — disk is recovery-only, so
            // reconcile the projection into it (in-memory wins) instead of clobbering.
            existing.reconcile_disk_projection(projection)?;
            return Ok(());
        }
    }
    let hub = RelayHub::recover_from_projection(CANONICAL_CLIENT_ID, projection)?;
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    registry.entry(hash).or_insert(hub);
    Ok(())
}

/// Result of refreshing the durable CRDT recovery projection before a process
/// recycle/reload boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableProjectionCheckpoint {
    /// No live editor owns the document. Git/disk authority is already durable and
    /// the relay projection is intentionally untouched.
    Detached,
    /// The foreground path did not have a ready live model to checkpoint. A
    /// background repair request was recorded; the turn/recycle hot path should
    /// continue without treating the stale `.yrs` projection as authoritative.
    Deferred { reason: String },
    /// A live editor relay was flushed to `.agent-doc/crdt/<hash>.yrs`.
    Checkpointed {
        bytes: usize,
        changed: bool,
        live_editors: usize,
        text_len: usize,
        text_hash: String,
    },
}

/// Flush the live relay's canonical replica to the durable `.yrs` recovery
/// projection before a recycle/reload tears down the process that owns the hub.
///
/// This is **not** the closeout hot path and the sidecar is not authority. It is
/// a bounded pre-recycle checkpoint: under detached/headless authority it skips
/// without allocating a hub; under editor authority it requires a live, converged
/// document model and writes the recovery projection from the in-memory canonical
/// replica in one serialized sidecar-projection instruction.
pub fn checkpoint_durable_projection_for_file(
    file: &Path,
    source: &str,
) -> Result<DurableProjectionCheckpoint> {
    checkpoint_durable_projection_for_file_with_mode(
        file,
        source,
        DurableProjectionMode::Foreground,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DurableProjectionMode {
    Foreground,
    Background,
}

fn checkpoint_durable_projection_for_file_with_mode(
    file: &Path,
    source: &str,
    mode: DurableProjectionMode,
) -> Result<DurableProjectionCheckpoint> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_durable_checkpoint_skipped file={} source={} authority=git reason=detached",
                file.display(),
                source,
            ),
        );
        return Ok(DurableProjectionCheckpoint::Detached);
    }

    let current = match mode {
        DurableProjectionMode::Foreground => current_text_for_file_with_authority(file, authority)?,
        DurableProjectionMode::Background => ensure_document_model(file, source)?,
    };
    let (live_editors, delivery_converged) = match current {
        CurrentText::Detached => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_durable_checkpoint_skipped file={} source={} authority=git reason=authority_flipped_detached",
                    file.display(),
                    source,
                ),
            );
            return Ok(DurableProjectionCheckpoint::Detached);
        }
        CurrentText::Current {
            live_editors,
            delivery_converged,
            ..
        } => (live_editors, delivery_converged),
        CurrentText::EditorAttachedMissingReplica | CurrentText::EditorSyncPending => {
            return defer_or_fail_durable_projection_checkpoint(
                file,
                source,
                mode,
                current_text_label(&current),
            );
        }
    };
    if !delivery_converged {
        return defer_or_fail_durable_projection_checkpoint(
            file,
            source,
            mode,
            &format!("delivery_not_converged live_editors={live_editors}"),
        );
    }

    let Some((projection, canonical_text)) =
        with_existing_hub(file, |hub| (hub.projection_bytes(), hub.canonical_text()))?
    else {
        return defer_or_fail_durable_projection_checkpoint(
            file,
            source,
            mode,
            "missing_hub_after_ready_state",
        );
    };
    let path = agent_doc_fs::crdt_path_for(file)?;
    let changed =
        agent_doc_snapshot_io::with_crdt_lock_labeled(file, "durable_recycle_checkpoint", || {
            agent_doc_snapshot_io::write_crdt_state_file_if_changed(&path, &projection)
        })?;
    let text_len = canonical_text.len();
    let text_hash = agent_doc_hash::content_hash(&canonical_text);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_durable_checkpoint file={} source={} authority=multi_replica bytes={} changed={} live_editors={} delivery_converged={} text_len={} text_hash={} path={}",
            file.display(),
            source,
            projection.len(),
            changed,
            live_editors,
            delivery_converged,
            text_len,
            text_hash,
            path.display(),
        ),
    );
    Ok(DurableProjectionCheckpoint::Checkpointed {
        bytes: projection.len(),
        changed,
        live_editors,
        text_len,
        text_hash,
    })
}

fn defer_or_fail_durable_projection_checkpoint(
    file: &Path,
    source: &str,
    mode: DurableProjectionMode,
    reason: &str,
) -> Result<DurableProjectionCheckpoint> {
    match mode {
        DurableProjectionMode::Foreground => {
            defer_durable_projection_checkpoint(file, source, reason)?;
            Ok(DurableProjectionCheckpoint::Deferred {
                reason: reason.to_string(),
            })
        }
        DurableProjectionMode::Background => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_durable_checkpoint_background_blocked file={} source={} reason={}",
                    file.display(),
                    source,
                    reason,
                ),
            );
            anyhow::bail!(
                "CRDT durable checkpoint background repair blocked for {} before {source}: {reason}",
                file.display()
            );
        }
    }
}

#[derive(Debug, Clone)]
struct DurableProjectionRepairPaths {
    pending_path: PathBuf,
    lock_path: PathBuf,
}

struct DurableProjectionRepairGuard {
    lock_path: PathBuf,
}

impl Drop for DurableProjectionRepairGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

fn durable_projection_repair_paths(file: &Path) -> Result<DurableProjectionRepairPaths> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
    let dir = project_root.join(".agent-doc").join("crdt-repair");
    std::fs::create_dir_all(&dir)?;
    Ok(DurableProjectionRepairPaths {
        pending_path: dir.join(format!("{hash}.json")),
        lock_path: dir.join(format!("{hash}.lock")),
    })
}

fn defer_durable_projection_checkpoint(file: &Path, source: &str, reason: &str) -> Result<()> {
    record_durable_projection_repair_request(file, source, reason)?;
    spawn_durable_projection_repair(file, source, reason);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_durable_checkpoint_deferred file={} source={} reason={} recovery=background_yrs_repair",
            file.display(),
            source,
            reason,
        ),
    );
    Ok(())
}

fn record_durable_projection_repair_request(file: &Path, source: &str, reason: &str) -> Result<()> {
    let paths = durable_projection_repair_paths(file)?;
    let requested_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let body = format!(
        "{{\"file\":\"{}\",\"source\":\"{}\",\"reason\":\"{}\",\"requested_at_secs\":{requested_at}}}",
        json_string_escape(&file.display().to_string()),
        json_string_escape(source),
        json_string_escape(reason),
    );
    std::fs::write(&paths.pending_path, body).with_context(|| {
        format!(
            "failed to write CRDT durable projection repair request {}",
            paths.pending_path.display()
        )
    })?;
    Ok(())
}

fn json_string_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn clear_durable_projection_repair_request(file: &Path) {
    if let Ok(paths) = durable_projection_repair_paths(file) {
        let _ = std::fs::remove_file(paths.pending_path);
    }
}

fn acquire_durable_projection_repair_guard(
    file: &Path,
) -> Result<Option<DurableProjectionRepairGuard>> {
    let paths = durable_projection_repair_paths(file)?;
    const STALE_LOCK_MS: u64 = 30_000;
    if let Some(metadata) = std::fs::metadata(&paths.lock_path).ok()
        && let Ok(modified) = metadata.modified()
        && modified.elapsed().unwrap_or_default() <= std::time::Duration::from_millis(STALE_LOCK_MS)
    {
        return Ok(None);
    }
    let _ = std::fs::remove_file(&paths.lock_path);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&paths.lock_path)
    {
        Ok(mut lock) => {
            let _ = lock.write_all(std::process::id().to_string().as_bytes());
            Ok(Some(DurableProjectionRepairGuard {
                lock_path: paths.lock_path,
            }))
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn spawn_durable_projection_repair(file: &Path, source: &str, reason: &str) {
    let file = file.to_path_buf();
    let source = source.to_string();
    let reason = reason.to_string();
    let Some(guard) = acquire_durable_projection_repair_guard(&file)
        .inspect_err(|err| {
            agent_doc_ops_log_io::log_op(
                &file,
                &format!(
                    "crdt_durable_checkpoint_background_spawn_skipped file={} source={} reason={} error={:?}",
                    file.display(),
                    source,
                    reason,
                    err.to_string(),
                ),
            );
        })
        .ok()
        .flatten()
    else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("agent-doc-crdt-repair".to_string())
        .spawn(move || {
            let _guard = guard;
            let background_source = format!("{source}:background");
            match checkpoint_durable_projection_for_file_with_mode(
                &file,
                &background_source,
                DurableProjectionMode::Background,
            ) {
                Ok(DurableProjectionCheckpoint::Checkpointed { .. })
                | Ok(DurableProjectionCheckpoint::Detached) => {
                    clear_durable_projection_repair_request(&file);
                    agent_doc_ops_log_io::log_op(
                        &file,
                        &format!(
                            "crdt_durable_checkpoint_background_repaired file={} source={} original_reason={}",
                            file.display(),
                            background_source,
                            reason,
                        ),
                    );
                }
                Ok(DurableProjectionCheckpoint::Deferred { reason: deferred }) => {
                    agent_doc_ops_log_io::log_op(
                        &file,
                        &format!(
                            "crdt_durable_checkpoint_background_deferred file={} source={} original_reason={} deferred_reason={}",
                            file.display(),
                            background_source,
                            reason,
                            deferred,
                        ),
                    );
                }
                Err(err) => {
                    agent_doc_ops_log_io::log_op(
                        &file,
                        &format!(
                            "crdt_durable_checkpoint_background_failed file={} source={} original_reason={} error={:?}",
                            file.display(),
                            background_source,
                            reason,
                            err.to_string(),
                        ),
                    );
                }
            }
        });
}

/// The **authority-gated commit barrier** at the live finalize commit point
/// (`#crdtauth4`, plan phase 4).
///
/// This replaces the fragile patch-ack quiescence proof for the EditorAttached
/// path: before the snapshot is committed to git, every currently-live editor
/// replica is flushed into the canonical replica on a **consistent cut**, so a
/// commit can only snapshot a state that provably holds every live editor's last
/// ops. It is a checkpoint, **not a global lock** — a slow / disconnected editor
/// is excluded from the cut (and contributes on reconnect), so finalize never
/// blocks forever on a stalled editor.
///
/// Returns whether a snapshot is safe to commit:
/// - [`CrdtAuthority::GitAuthoritative`] (**Detached**): trivially `true`, no hub
///   work, no allocation of a hub — the headless commit path is unchanged.
/// - [`CrdtAuthority::MultiReplica`] (**EditorAttached**): drives the per-document
///   hub's commit barrier and returns its consistent-cut result.
///
/// Under editor authority, unresolved delivery is a failed commit barrier. A
/// closeout may retry once the editor buffer reaches disk, but it must not mark a
/// turn committed from stale disk while a live editor has newer text.
pub fn commit_barrier_for_file(file: &Path) -> bool {
    let file_str = file.display().to_string();
    let authority = authority_for_file(&file_str);
    commit_barrier_for_file_with_authority(file, authority)
}

/// [`commit_barrier_for_file`] with an explicitly-resolved authority — the
/// deterministically-testable core. Callers that already hold a resolved
/// [`CrdtAuthority`] (e.g. from a backbone projection) should use this to avoid a
/// second authority hydration/read.
pub fn commit_barrier_for_file_with_authority(file: &Path, authority: CrdtAuthority) -> bool {
    commit_barrier_for_file_with_authority_and_delivery(file, authority, true)
}

/// Commit barrier for a semantic response cell whose CRDT projection and
/// `ResponseCellAdded` fact are already durable in the realtime backbone.
///
/// Durability does not make a stale visible editor safe. The response cell must
/// reach the same outbound delivery frontier as every other attached write
/// before closeout can commit it.
pub fn commit_barrier_for_durable_response_cell(file: &Path) -> bool {
    let file_str = file.display().to_string();
    let authority = authority_for_file(&file_str);
    commit_barrier_for_file_with_authority_and_delivery(file, authority, true)
}

fn commit_barrier_for_file_with_authority_and_delivery(
    file: &Path,
    authority: CrdtAuthority,
    require_delivery_convergence: bool,
) -> bool {
    if !authority.editor_attached() {
        // Detached / headless: the CRDT is ephemeral, git is the source of truth,
        // and there are no live editor replicas to flush. The barrier is trivially
        // satisfied and NO hub is touched — the headless path is byte-for-byte
        // unchanged.
        return true;
    }
    match with_existing_hub(file, |hub| {
        // `#staleinmem` — out-of-band baseline reconcile, BEFORE flushing live
        // editors into the canonical for the commit cut. This compares the real
        // document file to the relay's last committed baseline; it never creates
        // a relay hub from disk and never consults live-buffer sidecars.
        if let Ok(on_disk) = std::fs::read_to_string(file) {
            match hub.reconcile_canonical_against_baseline(&on_disk) {
                Ok(true) => agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "crdt_canonical_rebuilt_from_baseline file={} authority=multi_replica disk_len={}",
                        file.display(),
                        on_disk.len()
                    ),
                ),
                Ok(false) => {}
                Err(e) => agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "crdt_canonical_baseline_reconcile_error file={} error={}",
                        file.display(),
                        e
                    ),
                ),
            }
        }
        hub.commit_barrier_under_authority(authority)
            .map(|ready| (ready, hub.delivery_converged(), hub.live_count()))
    }) {
        Ok(Some(Ok((ready, delivery_converged, live_editors)))) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_commit_barrier file={} authority=multi_replica ready={} delivery_required={} delivery_converged={} live_editors={}",
                    file.display(),
                    ready,
                    require_delivery_convergence,
                    delivery_converged,
                    live_editors,
                ),
            );
            ready && (!require_delivery_convergence || delivery_converged)
        }
        Ok(Some(Err(e))) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_commit_barrier_error file={} authority=multi_replica error={}",
                    file.display(),
                    e
                ),
            );
            false
        }
        Ok(None) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_commit_barrier_deferred file={} authority=multi_replica reason=missing_relay_model recovery=publish_live_buffer_register_crdt",
                    file.display(),
                ),
            );
            false
        }
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_commit_barrier_registry_error file={} error={}",
                    file.display(),
                    e
                ),
            );
            false
        }
    }
}

/// Record the just-committed on-disk content as this document's hub baseline so a
/// later out-of-band disk correction (a `git checkout HEAD` / `reset` recovery the
/// hub did not author) is detectable at the next commit barrier (`#staleinmem`).
/// Call right after a successful git commit.
///
/// - [`CrdtAuthority::GitAuthoritative`] (**Detached**): no-op — there is no live
///   canonical replica / hub to mark, and no hub is allocated.
/// - [`CrdtAuthority::MultiReplica`] (**EditorAttached**): records the baseline on
///   an already-allocated hub. Does NOT allocate a hub — a document that never
///   engaged the multi-replica path is left untouched.
pub fn record_committed_baseline_for_file(file: &Path) {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return;
    }
    let on_disk = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_record_committed_baseline_read_error file={} error={}",
                    file.display(),
                    e
                ),
            );
            return;
        }
    };
    let hash = match agent_doc_fs::document_state_hash(file) {
        Ok(h) => h,
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_record_committed_baseline_hash_error file={} error={}",
                    file.display(),
                    e
                ),
            );
            return;
        }
    };
    match hub_registry().lock() {
        Ok(mut registry) => {
            if let Some(hub) = registry.get_mut(&hash) {
                hub.record_committed_baseline(&on_disk);
            }
        }
        Err(e) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_record_committed_baseline_registry_error file={} error={}",
                file.display(),
                e
            ),
        ),
    }
}

/// The **authority-gated disk-demotion reconcile** at the live `.yrs` load seam
/// (`#crdtauth4`, plan phase 6).
///
/// Under [`CrdtAuthority::MultiReplica`] the in-memory canonical replica is the
/// authority and the disk `.yrs` is a write-through **recovery projection only**
/// ([`agent_doc_document_realtime::crdt_relay::DISK_IS_RECOVERY_PROJECTION_ONLY`]):
/// a (possibly stale)
/// disk projection is reconciled INTO the live replica, which can only add ops the
/// live replica genuinely lost (a crash gap) and can never regress live text —
/// in-memory wins. Returns `Some(changed)` where `changed` is whether the disk
/// held ops the live replica was missing.
///
/// Under [`CrdtAuthority::GitAuthoritative`] there is no live in-memory authority
/// to reconcile against — disk demotion does not apply, and the existing
/// baseline-wins load path
/// (`agent_doc_snapshot_io::crdt_merge_base_state_with`, which already
/// discards a stale `.yrs` whose markdown projection does not match the cycle
/// baseline) is left to run unchanged. Returns `None` (no live reconcile
/// performed).
/// Force the live canonical replica for `file` to `text`, unconditionally
/// (`#jb-compact-commit-stale-relay-canonical`). Authority-gated like
/// [`reconcile_disk_projection_for_file`]: when no editor is attached (headless)
/// there is no live canonical to converge and the caller's disk/snapshot write is
/// already authoritative, so this returns `Ok(None)`. When a relay hub exists it
/// adopts `text` as the canonical and returns `Ok(Some(changed))`.
///
/// The single caller is the authoritative-compaction commit: with durable Open
/// authority but zero registered relay members, the commit's
/// `try_resolve_current_document_content` keeps
/// editor authority and returns the FROZEN pre-compact canonical, so the commit
/// lands pre-compact content in HEAD and Compact Exchange leaves the summary
/// uncommitted. Converging the lazily canonical to the compacted content first
/// makes that read resolve the compacted document.
pub fn adopt_authoritative_text_for_file(file: &Path, text: &str) -> Result<Option<bool>> {
    let file_str = file.display().to_string();
    let authority = authority_for_file(&file_str);
    if !authority.editor_attached() {
        return Ok(None);
    }
    let Some(changed) = with_existing_hub(file, |hub| hub.adopt_authoritative_text(text))? else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_adopt_authoritative_text_deferred file={} authority=multi_replica reason=missing_relay_model",
                file.display(),
            ),
        );
        return Ok(None);
    };
    let changed = changed?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_adopt_authoritative_text file={} authority=multi_replica changed={} content_len={} content_hash={}",
            file.display(),
            changed,
            text.len(),
            agent_doc_hash::content_hash(text),
        ),
    );
    Ok(Some(changed))
}

/// Adopt an editor's authoritative **full lazily state** as the relay canonical for
/// `file`, lineage-intact — the reattach acute-wedge fix (`#reattach-adopt`).
///
/// `full_state` is the editor's whole `ReplicaState::encode_state()` (`serde_json`
/// `Vec<TextOp>`). Use this (NOT [`apply_document_op_delta_for_file`], which
/// union-merges) when the editor re-announces after its registration lapsed: the
/// recovered canonical may carry drift the operator already deleted (`#sy71`) with no
/// counterpart delete op, so a fold would keep it — adoption replaces the drifted
/// canonical with the editor's authoritative state, dropping the drift while keeping
/// the editor's `OpId` lineage. Returns `Ok(Some(text_changed))` when a live relay
/// model exists, `Ok(None)` headless. The FFI/plugin must send the editor's full
/// state (not an incremental delta) on reattach for this to be correct.
pub fn adopt_editor_full_state_for_file(file: &Path, full_state: &[u8]) -> Result<Option<bool>> {
    let Some(changed) = with_existing_hub(file, |hub| {
        let before = hub.canonical_text();
        hub.adopt_editor_full_state(full_state)
            .map(|_| hub.canonical_text() != before)
    })?
    else {
        return Ok(None);
    };
    let changed = changed?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_editor_full_state_adopted file={} authority=multi_replica changed={} state_len={} recovery=reattach_drop_drift",
            file.display(),
            changed,
            full_state.len(),
        ),
    );
    Ok(Some(changed))
}

/// Adopt the editor's authoritative **TEXT** as the relay canonical for `file` — the
/// bounded, runaway-safe reattach path (`#reattach-adopt`). `text` is the editor's document
/// text (`O(text)`, not the op-log). Rebuilds the canonical from text with a self-echo guard
/// (`RelayHub::adopt_editor_text`). Returns `Ok(Some(changed))` when a hub exists.
pub fn adopt_editor_text_for_file(file: &Path, text: &str) -> Result<Option<bool>> {
    let changed = with_hub(file, |hub| {
        let before = hub.canonical_text();
        hub.adopt_editor_text(text)
            .map(|_| hub.canonical_text() != before)
    })??;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_editor_text_adopted file={} authority=multi_replica changed={} text_len={} recovery=reattach_drop_drift_bounded",
            file.display(),
            changed,
            text.len(),
        ),
    );
    Ok(Some(changed))
}

/// Fold a durably-replicated document-op **delta frame** into the relay canonical
/// for `file` — the `#docop-plane` P2 ingest that keeps the canonical un-frozen when
/// a connected editor's CRDT member registration has lapsed (`live_editors == 0`).
///
/// `delta` is the `serde_json` `Vec<lazily::TextOp>` body a plugin pushes over the
/// reliable-sync RPC (a `document_op` frame payload / `ReplicaState::diff` output).
/// Returns `Ok(Some(text_changed))` when a live relay model exists, `Ok(None)` when
/// there is no hub (headless — nothing to feed). Safe regardless of `live_editors`:
/// applying the delta is idempotent + commutative, so a duplicate / out-of-order /
/// stale frame converges rather than corrupting the canonical.
pub fn apply_document_op_delta_for_file(file: &Path, delta: &[u8]) -> Result<Option<bool>> {
    let Some(changed) = with_existing_hub(file, |hub| {
        let before = hub.canonical_text();
        hub.apply_document_op_delta(delta)
            .map(|_| hub.canonical_text() != before)
    })?
    else {
        return Ok(None);
    };
    let changed = changed?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_document_op_delta_applied file={} authority=multi_replica changed={} delta_len={}",
            file.display(),
            changed,
            delta.len(),
        ),
    );
    Ok(Some(changed))
}

pub fn reconcile_disk_projection_for_file(file: &Path, projection: &[u8]) -> Result<Option<bool>> {
    let file_str = file.display().to_string();
    let authority = authority_for_file(&file_str);
    if !authority.editor_attached() {
        // Headless: no live canonical replica is authoritative. The
        // baseline-wins load path in snapshot.rs already handles stale disk.
        return Ok(None);
    }
    let Some(changed) = with_existing_hub(file, |hub| hub.reconcile_disk_projection(projection))?
    else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_disk_demotion_reconcile_deferred file={} authority=multi_replica reason=missing_relay_model",
                file.display(),
            ),
        );
        return Ok(None);
    };
    let changed = changed?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_disk_demotion_reconcile file={} authority=multi_replica disk_added_ops={}",
            file.display(),
            changed
        ),
    );
    Ok(Some(changed))
}

/// Route a settled out-of-band disk change into the live canonical replica — the
/// in-process host seam the controller watcher calls when it observes a
/// `FileWatchChangeObserved` for a document (`plan-crdt-scramble-and-disk-propagation.md`
/// Phase C1). Mirrors [`reconcile_disk_projection_for_file`]: authority-gated,
/// fail-open sync barrier, then the hub method.
///
/// Under [`CrdtAuthority::GitAuthoritative`] (no live editor) there is no live
/// canonical replica to reconcile against — disk is already authoritative and the
/// headless load path owns it — so this returns `Ok(None)`. Under
/// [`CrdtAuthority::MultiReplica`] the disk text is routed through
/// [`RelayHub::apply_disk_change`], returning `Ok(Some(outcome))`.
///
/// The editor-side propagation of a `RebuiltFromDisk` correction still needs the
/// replace-capable delivery (Phase D2) — this seam integrates the change into the
/// canonical replica and reports the outcome; it does not yet push a deletion into
/// the live editor buffer.
pub fn apply_disk_change_for_file(file: &Path, on_disk: &str) -> Result<Option<DiskChangeOutcome>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        // Headless: no live canonical replica; the baseline-wins load path owns
        // stale disk. Nothing to reconcile here.
        return Ok(None);
    }
    let Some(outcome) = with_existing_hub(file, |hub| hub.apply_disk_change(on_disk))? else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_disk_change_reconcile_deferred file={} authority=multi_replica reason=missing_relay_model",
                file.display(),
            ),
        );
        return Ok(None);
    };
    let outcome = outcome?;
    if matches!(outcome, DiskChangeOutcome::RebuiltFromDisk { live_members } if live_members > 0)
        && let Err(err) = signal_crdt_replica_event(file, CrdtReplicaEventReason::Rebootstrap, 0)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_event_signal_failed file={} reason=rebootstrap error={err}",
                file.display(),
            ),
        );
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_disk_change_reconcile file={} authority=multi_replica outcome={outcome:?}",
            file.display(),
        ),
    );
    Ok(Some(outcome))
}

/// Wake editor replicas that watch `.agent-doc/crdt-replica-events/` so they can
/// drain queued CRDT deliveries from the controller without a fixed pull loop.
pub fn signal_crdt_replica_event(
    file: &Path,
    reason: CrdtReplicaEventReason,
    targets: usize,
) -> Result<()> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let path = agent_doc_fs::crdt_replica_event_path_for(&canonical)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create CRDT replica event dir {}",
                parent.display()
            )
        })?;
    }
    let doc_hash = agent_doc_fs::document_state_hash(&canonical)?;
    let signaled_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let body = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "doc_hash": doc_hash,
        "reason": reason.token(),
        "targets": targets,
        "signaled_at_ms": signaled_at_ms,
        "process_pid": std::process::id(),
    });
    std::fs::write(&path, serde_json::to_vec(&body)?)
        .with_context(|| format!("failed to write CRDT replica event {}", path.display()))?;
    Ok(())
}

/// Producer (C1b, controller watch side): drop a disk-change-reconcile marker
/// for `file` so the CPC/controller consumer reconciles the change into the
/// canonical replica at its next safe boundary. This is a robust cross-process
/// signal (a file marker, mirroring `recycle_request`) — it needs no supervisor
/// socket or session resolution and survives degraded IPC. The marker is a
/// signal only; the consumer re-reads the current disk text so a change that
/// lands after this call is still picked up.
pub fn request_disk_change_reconcile(file: &Path) -> Result<()> {
    let path = agent_doc_fs::disk_change_request_path_for(file)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create disk-change-request dir {}",
                parent.display()
            )
        })?;
    }
    let requested_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let body = format!("{{\"requested_at_secs\":{requested_at}}}");
    std::fs::write(&path, body)
        .with_context(|| format!("failed to write disk-change-request {}", path.display()))?;
    Ok(())
}

/// Whether a disk-change-reconcile marker is pending for `file`.
pub fn disk_change_request_pending(file: &Path) -> bool {
    agent_doc_fs::disk_change_request_path_for(file)
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Consumer (C1b, supervisor idle-loop side): if a disk-change-reconcile marker
/// is pending for `file`, re-read the current disk text, route it into the
/// canonical replica via [`apply_disk_change_for_file`], clear the marker, and
/// return the outcome. Returns `Ok(None)` when no marker is pending. The marker is
/// always cleared once observed (even on a headless / no-op reconcile) so the
/// signal is consumed exactly once.
pub fn consume_disk_change_reconcile(file: &Path) -> Result<Option<DiskChangeOutcome>> {
    let marker = agent_doc_fs::disk_change_request_path_for(file)?;
    if !marker.exists() {
        return Ok(None);
    }
    let on_disk = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read disk text for reconcile {}", file.display()))?;
    let outcome = apply_disk_change_for_file(file, &on_disk)?;
    // Clear the marker whether or not a live hub reconciled — the signal is spent.
    if let Err(e) = std::fs::remove_file(&marker)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[crdt] warning: failed to clear disk-change-request {}: {e}",
            marker.display()
        );
    }
    Ok(outcome)
}

/// Daemon-facing gate (C1b producer entry): given a settled watch `delivery` for
/// `file`, decide via [`decide_watch_action`] whether the change should be routed
/// to the canonical replica and, if so, drop the reconcile marker. Returns the
/// chosen [`WatchAction`]. Editor-attached changes (`ReconcileIntoCanonical` /
/// `DeferForEditSettle`) drop a marker; headless changes (`ApplyAsDiskAuthority`,
/// the disk-authority load path owns them) and non-changes drop none. This keeps
/// headless documents — the common case — from accumulating markers no supervisor
/// would consume.
pub fn route_disk_change_signal(file: &Path, delivery: &WatchDelivery) -> Result<WatchAction> {
    let authority = authority_for_file(&file.display().to_string());
    // The supervisor's own reconcile barrier handles an in-flight editor edit, so
    // the daemon does not need the live edit epoch here — pass `false` and let the
    // consumer's bounded fail-open barrier settle it.
    let action = decide_watch_action(delivery, authority, false);
    if matches!(
        action,
        WatchAction::ReconcileIntoCanonical | WatchAction::DeferForEditSettle
    ) {
        request_disk_change_reconcile(file)?;
    }
    Ok(action)
}

#[cfg(test)]
fn settle_or_flush_editor_sync_barrier(file: &Path, reason: &str) -> bool {
    let file_str = file.display().to_string();
    let outcome = agent_doc_debounce::await_editor_sync_barrier(
        &file_str,
        EDITOR_SYNC_SETTLE_MS,
        EDITOR_SYNC_TIMEOUT_MS,
    );
    let in_flight = outcome
        .statuses
        .iter()
        .filter(|status| status.in_flight)
        .count();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "editor_sync_barrier file={} reason={} outcome={:?} statuses={} in_flight={} typing_recent={}",
            file.display(),
            reason,
            outcome.kind,
            outcome.statuses.len(),
            in_flight,
            outcome.typing_recent
        ),
    );
    if outcome.kind != agent_doc_debounce::EditorSyncBarrierKind::TimedOut {
        return true;
    }

    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "editor_sync_barrier_flush_skipped file={} reason={} cause=canonicalize_error error={}",
                    file.display(),
                    reason,
                    e
                ),
            );
            return false;
        }
    };
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
    let patch_id = uuid::Uuid::new_v4().to_string();
    let path_str = canonical.to_string_lossy().to_string();
    // `#vscodepublishparity` — mirror `converge.rs`'s
    // `live_buffer_delivery_missing_operator_text_authority_after_refresh`: when no
    // socket listener owns this project (the VS Code / pluginless / file-IPC editor
    // case, which runs no socket), fall back to the publish-live-buffer FILE signal
    // those editors watch instead of skipping the editor-sync-barrier flush entirely.
    // Skipping (the old `cause=no_ipc_listener` early return) left VS Code sessions
    // silently missing this live-buffer publish even though the sibling converge path
    // already fell back to the file signal.
    let listener_active = agent_doc_ipc_io::is_listener_active(&project_root);
    let (transport, publish_result) = if listener_active {
        (
            "editor_ipc",
            agent_doc_ipc_io::send_publish_live_buffer(&project_root, &path_str),
        )
    } else {
        (
            "file_signal",
            agent_doc_ipc_io::send_publish_live_buffer_file_signal(&project_root, &path_str),
        )
    };
    match publish_result {
        Ok(true) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "editor_sync_barrier_live_buffer_publish_requested file={} reason={} transport={} patch_id={}",
                    file.display(),
                    reason,
                    transport,
                    patch_id
                ),
            );
        }
        Ok(false) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "editor_sync_barrier_live_buffer_publish_not_acked file={} reason={} transport={} patch_id={}",
                    file.display(),
                    reason,
                    transport,
                    patch_id
                ),
            );
            return false;
        }
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "editor_sync_barrier_live_buffer_publish_error file={} reason={} transport={} patch_id={} error={}",
                    file.display(),
                    reason,
                    transport,
                    patch_id,
                    e
                ),
            );
            return false;
        }
    }

    let current = match current_text_for_file(file) {
        Ok(current) => current,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "editor_sync_barrier_after_publish_current_unavailable file={} reason={} error={}",
                    file.display(),
                    reason,
                    err
                ),
            );
            return false;
        }
    };
    let ready = matches!(
        current,
        CurrentText::Detached
            | CurrentText::Current {
                delivery_converged: true,
                ..
            }
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "editor_sync_barrier_after_publish_current file={} reason={} state={} ready={}",
            file.display(),
            reason,
            current_text_label(&current),
            ready
        ),
    );
    ready
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    /// A throwaway tracked document under a temp project root so `doc_hash` and the
    /// per-document keying resolve against a real path.
    fn temp_doc(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        // `.agent-doc/` marks the project root for `find_project_root`.
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "# {name}\n\nbody").unwrap();
        (dir, path)
    }

    fn seed_live_reliable_sync_open(file: &str) {
        let pid = std::process::id();
        let document_hash = agent_doc_hash::document_id_for_path(Path::new(file));
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .unwrap()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: pid.into(),
                tag: format!("test-editor-{pid}:{file}"),
            }]);
    }

    #[test]
    fn durable_response_cell_waits_for_outbound_editor_ack() {
        let (_dir, doc) = temp_doc("durable-response-cell.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange patch=append -->\n❯ operator prompt\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let identity = "intellij:durable-response-cell";
        register_replica_for_file(&doc, identity)
            .unwrap()
            .expect("live editor should register with the relay");

        let response = "### Re: operator prompt — gpt-5\n\nDone.";
        let first = add_response_cell_for_file(&doc, response, "test")
            .unwrap()
            .expect("editor-attached response add should use the relay");
        assert!(first.applied);
        assert_eq!(first.live_editors, 1);
        assert_eq!(first.targets, 1);
        assert!(!first.delivery_converged);
        assert!(first.content.contains(response));

        assert!(
            !commit_barrier_for_file_with_authority(&doc, CrdtAuthority::MultiReplica),
            "generic writes still require outbound editor acknowledgement"
        );
        assert!(
            !commit_barrier_for_durable_response_cell(&doc),
            "a durable response cell still requires visible editor acknowledgement"
        );

        let pull = pull_replica_updates_for_file(&doc, identity)
            .unwrap()
            .expect("live editor should receive the response delivery");
        assert!(!pull.updates.is_empty());
        for update in pull.updates {
            assert_eq!(
                ack_replica_update_for_file_with_content_hash(
                    &doc,
                    identity,
                    &update.patch_id,
                    update.generation,
                    Some(&update.expected_content_hash),
                )
                .unwrap(),
                Some(true),
            );
        }
        assert!(
            commit_barrier_for_durable_response_cell(&doc),
            "the response cell may commit after the visible editor ACK frontier converges"
        );

        let projection = agent_doc_snapshot_io::load_crdt(&doc)
            .unwrap()
            .expect("response add should durably checkpoint the canonical projection");
        let recovered =
            RelayHub::recover_from_projection(CANONICAL_CLIENT_ID, &projection).unwrap();
        assert!(recovered.canonical_text().contains(response));

        let replay = add_response_cell_for_file(&doc, response, "test-replay")
            .unwrap()
            .expect("replay should still use the relay");
        assert!(!replay.applied);
        assert_eq!(replay.cell_id, first.cell_id);
        assert_eq!(replay.content, first.content);
    }

    #[test]
    fn register_replica_seeds_fresh_hub_from_current_document_text() {
        let (_dir, doc) = temp_doc("seed-register.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let on_disk = std::fs::read_to_string(&doc).unwrap();

        let (client_id, bootstrap) = register_replica_for_file(&doc, "intellij:seed")
            .unwrap()
            .expect("editor-attached register should return a bootstrap");
        let replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap).unwrap();
        assert_eq!(
            replica.text(),
            on_disk,
            "a first live editor must not attach to an empty canonical replica"
        );
        with_hub(&doc, |hub| {
            assert_eq!(hub.canonical_text(), on_disk);
            assert_eq!(hub.member_text(client_id).unwrap(), on_disk);
        })
        .unwrap();
    }

    #[test]
    fn cpc_relay_write_requires_current_canonical_baseline() {
        let (_dir, doc) = temp_doc("cpc-baseline.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        register_replica_for_file(&doc, "intellij:cpc-baseline")
            .unwrap()
            .expect("editor replica should attach");

        let err = apply_cpc_write_for_file(
            &doc,
            "stale baseline\n",
            "stale baseline\n### Re: no — gpt-5\n\nNo.\n",
            "test_cpc_relay",
        )
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("recovery=retry_crdt_merge"),
            "stale baseline must fail closed before relay mutation: {err:#}"
        );
        with_hub(&doc, |hub| {
            assert!(hub.canonical_text().contains("body"));
            assert_eq!(
                hub.pending_updates(mint_client_id("intellij:cpc-baseline"))
                    .unwrap()
                    .len(),
                0
            );
        })
        .unwrap();
    }

    #[test]
    fn cpc_relay_write_zero_live_editors_keeps_doc_op_canonical_authority() {
        // The document-op plane feeds canonical independently of relay-member
        // liveness. Zero live members must not demote an existing hub to disk.
        let (_dir, doc) = temp_doc("cpc-stale-lease.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        register_replica_for_file(&doc, "intellij:cpc-stale")
            .unwrap()
            .expect("editor replica should attach");
        let baseline = match current_text_for_file(&doc).unwrap() {
            CurrentText::Current { text, .. } => text,
            other => panic!("expected relay current text, got {other:?}"),
        };
        // The member goes offline while the durable Open fact remains live.
        let client_id = mint_client_id("intellij:cpc-stale");
        with_hub(&doc, |hub| {
            assert!(
                hub.disconnect(client_id),
                "disconnect should mark member offline"
            );
            assert_eq!(
                hub.live_count(),
                0,
                "editor disconnected -> zero live editors"
            );
        })
        .unwrap();

        let next = format!("{baseline}\n### Re: current — gpt-5\n\nCanonical authority.\n");
        let result = apply_cpc_write_for_file(&doc, &baseline, &next, "test_cpc_relay")
            .expect("zero-live canonical write should pass its CAS")
            .expect("an existing doc-op canonical must not demote to disk");
        assert!(result.applied);
        assert_eq!(result.live_editors, 0);
        with_hub(&doc, |hub| assert_eq!(hub.canonical_text(), next)).unwrap();
    }

    #[test]
    fn cpc_relay_write_queues_editor_pull_without_file_ipc_sidecar() {
        let (_dir, doc) = temp_doc("cpc-write.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        register_replica_for_file(&doc, "intellij:cpc-write")
            .unwrap()
            .expect("editor replica should attach");
        let current = match current_text_for_file(&doc).unwrap() {
            CurrentText::Current { text, .. } => text,
            other => panic!("expected relay current text, got {other:?}"),
        };
        let next = format!("{current}\n### Re: relay — gpt-5\n\nRecovered via relay.\n");

        let result = apply_cpc_write_for_file(&doc, &current, &next, "test_cpc_relay")
            .unwrap()
            .expect("attached CPC relay write should apply");
        assert!(result.applied);
        assert_eq!(result.targets, 1);
        assert!(!result.delivery_converged);
        with_hub(&doc, |hub| {
            assert_eq!(hub.canonical_text(), next);
            let pending = hub
                .pending_updates(mint_client_id("intellij:cpc-write"))
                .unwrap();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].origin, CANONICAL_CLIENT_ID);
        })
        .unwrap();
    }

    #[test]
    fn cpc_relay_write_recovers_missing_replica_from_durable_projection() {
        // Editor attached (authority) but this process has NO registered relay
        // replica — the transient gap after a controller recycle / editor restart
        // that made JB `Compact Exchange` hard-fail with
        // `crdt_cpc_write ... no registered replica yet` (#cpcwritemissingreplica).
        // With a durable `.yrs` projection on disk, the write must recover the hub
        // from it (symmetric with the read path) and apply, rather than aborting.
        let (_dir, doc) = temp_doc("cpc-missing-replica.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        register_replica_for_file(&doc, "intellij:cpc-recover")
            .unwrap()
            .expect("editor replica should attach");
        let current = match current_text_for_file(&doc).unwrap() {
            CurrentText::Current { text, .. } => text,
            other => panic!("expected relay current text, got {other:?}"),
        };
        // Persist the durable projection, then evict the in-process hub to model the
        // missing-replica state a recycle/restart leaves behind.
        checkpoint_durable_projection_for_file(&doc, "test_missing_replica").unwrap();
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert!(
            hub_registry().lock().unwrap().remove(&hash).is_some(),
            "test setup should evict the live hub"
        );
        assert!(
            agent_doc_snapshot_io::load_crdt(&doc).unwrap().is_some(),
            "durable projection must exist for recovery"
        );

        let next = format!("{current}\n### Re: recovered — gpt-5\n\nAfter recycle.\n");
        let result = apply_cpc_write_for_file(&doc, &current, &next, "test_cpc_relay")
            .unwrap()
            .expect("missing-replica CPC write should recover from projection and apply");
        assert!(result.applied);
        with_hub(&doc, |hub| assert_eq!(hub.canonical_text(), next)).unwrap();
    }

    #[test]
    fn relay_update_reattaches_dropped_replica_after_recycle() {
        // Phantom-editor heal: a JB editor was open (durable fact + live replica), then the
        // controller/supervisor recycled (`#statedbgc`). The in-process hub restarts
        // empty and is rebuilt from the durable projection with ONLY the canonical
        // replica (`live_count()==0`) — but the editor is still open and its FFI
        // replica keeps shipping `replica_update`s for its stable client-id. The relay
        // must re-register the dropped replica (not hard-fail "not registered"), apply
        // the editor's update, and return `live_count()` to 1 — healing the phantom on
        // the editor's next edit with no plugin round-trip.
        let (_dir, doc) = temp_doc("relay-reattach.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let identity = "intellij:reattach";
        let (client_id, bootstrap) = register_replica_for_file(&doc, identity)
            .unwrap()
            .expect("editor replica should attach");

        // The editor makes a local edit against its FFI replica and would ship the
        // encoded state to the hub.
        let replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap).unwrap();
        let base_chars = replica.text().chars().count() as u32;
        replica.apply_local_edit(
            base_chars,
            0,
            "\n### Re: after recycle — gpt-5\n\nEDITOR EDIT\n",
        );
        let editor_update = replica.encode_state();

        // Model the recycle: persist the durable projection, then evict the live hub
        // so the next hub access recovers a canonical-only hub with zero live editors.
        checkpoint_durable_projection_for_file(&doc, "test_reattach").unwrap();
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert!(
            hub_registry().lock().unwrap().remove(&hash).is_some(),
            "test setup should evict the live hub"
        );

        let fan_out = relay_replica_update_for_file(&doc, identity, &editor_update)
            .expect("a dropped-replica update must re-register and relay, not fail closed")
            .expect("editor-attached authority must relay the reattached update");
        assert_eq!(fan_out.origin, client_id);
        with_hub(&doc, |hub| {
            assert_eq!(
                hub.live_count(),
                1,
                "re-registering the dropped replica must restore one live editor"
            );
            assert!(
                hub.canonical_text().contains("EDITOR EDIT"),
                "the editor's post-recycle edit must integrate into the canonical text"
            );
        })
        .unwrap();
    }

    #[test]
    fn cpc_relay_write_without_projection_still_fails_closed_on_missing_replica() {
        // Missing replica AND no durable projection to recover from: the write must
        // still fail closed with the actionable "no registered replica yet" error
        // rather than fabricating a hub from raw disk.
        let (_dir, doc) = temp_doc("cpc-no-projection.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert!(
            !hub_registry().lock().unwrap().contains_key(&hash),
            "no hub should be allocated yet"
        );
        assert!(
            agent_doc_snapshot_io::load_crdt(&doc).unwrap().is_none(),
            "no durable projection should exist"
        );

        let err =
            apply_cpc_write_for_file(&doc, "baseline\n", "baseline\nmore\n", "test_cpc_relay")
                .unwrap_err();
        assert!(
            format!("{err:#}").contains("no registered replica yet"),
            "must fail closed without a projection to recover from: {err:#}"
        );
    }

    #[test]
    fn detached_commit_barrier_is_a_trivial_noop() {
        // Detached / GitAuthoritative: the barrier is trivially ready and NO hub is
        // allocated for the document — the headless commit path is untouched.
        let (_dir, doc) = temp_doc("detached.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert!(commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::GitAuthoritative
        ));
        let registry = hub_registry().lock().unwrap();
        assert!(
            !registry.contains_key(&hash),
            "the Detached path must not allocate a relay hub"
        );
    }

    #[test]
    fn detached_durable_checkpoint_skips_without_allocating_hub() {
        let (_dir, doc) = temp_doc("detached-checkpoint.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();

        let outcome = checkpoint_durable_projection_for_file(&doc, "test_detached").unwrap();

        assert_eq!(outcome, DurableProjectionCheckpoint::Detached);
        assert!(
            !hub_registry().lock().unwrap().contains_key(&hash),
            "detached checkpoint must not create a relay hub"
        );
        assert!(
            agent_doc_snapshot_io::load_crdt(&doc).unwrap().is_none(),
            "detached checkpoint must not materialize a CRDT sidecar"
        );
    }

    #[test]
    fn detached_current_text_is_a_noop_and_allocates_no_hub() {
        let (_dir, doc) = temp_doc("detached-current.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let current = current_text_for_file_with_authority(&doc, CrdtAuthority::GitAuthoritative)
            .expect("detached current text should not fail");
        assert_eq!(current, CurrentText::Detached);
        assert!(
            !hub_is_allocated_for_test(&hash),
            "detached current-text reads must not seed a relay hub from disk"
        );
    }

    #[test]
    fn editor_attached_current_text_reads_relay_canonical_after_flush() {
        let (_dir, doc) = temp_doc("attached-current.md");
        let editor = mint_client_id("intellij:attached-current");
        with_hub(&doc, |hub| {
            hub.register(editor).unwrap();
            hub.local_edit(editor, 0, 0, "LIVE ").unwrap();
            assert!(
                !hub.canonical_text().starts_with("LIVE "),
                "the local editor op starts outside canonical"
            );
        })
        .unwrap();

        let current = current_text_for_file_with_authority(&doc, CrdtAuthority::MultiReplica)
            .expect("attached current text should read relay canonical");
        match current {
            CurrentText::Current {
                text, live_editors, ..
            } => {
                assert!(text.starts_with("LIVE "), "relay current text: {text:?}");
                assert_eq!(live_editors, 1);
            }
            other => panic!("expected relay current text, got {other:?}"),
        }
    }

    #[test]
    fn editor_attached_current_text_without_replica_does_not_read_disk() {
        let (_dir, doc) = temp_doc("attached-missing-current.md");
        let current = current_text_for_file_with_authority(&doc, CrdtAuthority::MultiReplica)
            .expect("missing replica is a legal relay state");
        assert_eq!(current, CurrentText::EditorAttachedMissingReplica);
    }

    #[test]
    fn editor_attached_projection_recovery_requires_explicit_recovery_read() {
        let (_dir, doc) = temp_doc("attached-projection-recovery.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let editor = mint_client_id("intellij:projection-recovery");
        prior.register(editor).unwrap();
        prior.apply_local(editor, 0, 0, "durable recovery").unwrap();
        agent_doc_snapshot_io::save_crdt(&doc, &prior.projection_bytes()).unwrap();
        hub_registry().lock().unwrap().remove(&hash);

        let strict = current_text_for_file_with_authority(&doc, CrdtAuthority::MultiReplica)
            .expect("strict read should not fail");
        assert_eq!(strict, CurrentText::EditorAttachedMissingReplica);
        assert!(
            !hub_is_allocated_for_test(&hash),
            "strict current-text reads must not restore from the recovery projection"
        );

        let recovered = current_text_for_file_with_authority_recovering_projection(
            &doc,
            CrdtAuthority::MultiReplica,
        )
        .expect("explicit recovery read should restore the relay hub");
        match recovered {
            CurrentText::Current {
                text, live_editors, ..
            } => {
                assert_eq!(text, "durable recovery");
                assert_eq!(live_editors, 0, "editors re-register after recovery");
            }
            other => panic!("expected recovered relay current text, got {other:?}"),
        }
    }

    #[test]
    fn nonblocking_current_text_does_not_flush_pending_editor_ops() {
        let (_dir, doc) = temp_doc("attached-nonblocking-current.md");
        let editor = mint_client_id("intellij:nonblocking-current");
        with_hub_seeded_from_file(&doc, |hub| {
            hub.register(editor).unwrap();
            hub.local_edit(editor, 0, 0, "LIVE ").unwrap();
            assert!(
                !hub.canonical_text().starts_with("LIVE "),
                "test setup should leave pending editor ops outside the canonical cut"
            );
        })
        .unwrap();

        let observed =
            current_text_for_file_with_authority_nonblocking(&doc, CrdtAuthority::MultiReplica)
                .expect("nonblocking read should not fail");
        assert_eq!(observed, CurrentText::EditorSyncPending);
        with_existing_hub(&doc, |hub| {
            assert!(
                !hub.canonical_text().starts_with("LIVE "),
                "nonblocking current-text read must not flush editor ops"
            );
        })
        .unwrap()
        .expect("hub should still exist");

        let flushed = current_text_for_file_with_authority(&doc, CrdtAuthority::MultiReplica)
            .expect("strict read should still flush the barrier");
        match flushed {
            CurrentText::Current { text, .. } => assert!(text.starts_with("LIVE ")),
            other => panic!("expected strict read to return current text, got {other:?}"),
        }
    }

    #[test]
    fn projection_recovery_repairs_legacy_markdown_sidecar() {
        let legacy_markdown = "---\ntitle: legacy\n---\n\n<!-- agent:exchange -->\nBody\n";
        let (_dir, doc) = temp_doc("attached-legacy-markdown-projection.md");
        agent_doc_snapshot_io::save_crdt(&doc, legacy_markdown.as_bytes()).unwrap();

        let recovered = current_text_for_file_with_authority_recovering_projection(
            &doc,
            CrdtAuthority::MultiReplica,
        )
        .expect("explicit recovery read should repair legacy markdown sidecar");

        match recovered {
            CurrentText::Current { text, .. } => assert_eq!(text, legacy_markdown),
            other => panic!("expected markdown projection recovery, got {other:?}"),
        }
        let repaired = agent_doc_snapshot_io::load_crdt(&doc)
            .unwrap()
            .expect("repaired projection should be persisted");
        assert_ne!(repaired, legacy_markdown.as_bytes());
        let rebuilt = RelayHub::recover_from_projection(CANONICAL_CLIENT_ID, &repaired).unwrap();
        assert_eq!(rebuilt.canonical_text(), legacy_markdown);
    }

    #[test]
    fn ensure_document_model_recovers_projection_after_publish_timeout() {
        let (_dir, doc) = temp_doc("ensure-model-projection-recovery.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let editor = mint_client_id("intellij:ensure-projection-recovery");
        prior.register(editor).unwrap();
        prior
            .apply_local(editor, 0, 0, "projection after publish")
            .unwrap();
        agent_doc_snapshot_io::save_crdt(&doc, &prior.projection_bytes()).unwrap();
        hub_registry().lock().unwrap().remove(&hash);

        let poll_count = Arc::new(Mutex::new(0usize));
        let poll_count_for_observer = Arc::clone(&poll_count);
        let current = ensure_document_model_with_current_text_recovery_observer(
            &doc,
            "test_projection_recovery",
            CurrentText::EditorAttachedMissingReplica,
            || {
                *poll_count_for_observer.lock().unwrap() += 1;
                Ok(CurrentText::EditorAttachedMissingReplica)
            },
            || {
                current_text_for_file_with_authority_recovering_projection(
                    &doc,
                    CrdtAuthority::MultiReplica,
                )
            },
        )
        .expect("ensure should fall back to durable projection after publish timeout");

        assert!(
            *poll_count.lock().unwrap() > 0,
            "ensure should poll the strict observer before recovery"
        );
        match current {
            CurrentText::Current {
                text, live_editors, ..
            } => {
                assert_eq!(text, "projection after publish");
                assert_eq!(live_editors, 0);
            }
            other => panic!("expected projection-backed current text, got {other:?}"),
        }
    }

    #[test]
    fn ensure_document_model_recycles_missing_replica_within_short_window() {
        let (_dir, doc) = temp_doc("ensure-model-missing-replica-recycle.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let editor = mint_client_id("intellij:missing-replica-recycle");
        prior.register(editor).unwrap();
        prior
            .apply_local(editor, 0, 0, "recycled from projection")
            .unwrap();
        agent_doc_snapshot_io::save_crdt(&doc, &prior.projection_bytes()).unwrap();
        hub_registry().lock().unwrap().remove(&hash);

        let poll_count = Arc::new(Mutex::new(0usize));
        let poll_count_for_observer = Arc::clone(&poll_count);
        // The editor never registers a replica (stale/half-synced): the strict
        // observer stays at EditorAttachedMissingReplica forever.
        let current = ensure_document_model_with_current_text_recovery_observer(
            &doc,
            "test_missing_replica_recycle",
            CurrentText::EditorAttachedMissingReplica,
            || {
                *poll_count_for_observer.lock().unwrap() += 1;
                Ok(CurrentText::EditorAttachedMissingReplica)
            },
            || {
                current_text_for_file_with_authority_recovering_projection(
                    &doc,
                    CrdtAuthority::MultiReplica,
                )
            },
        )
        .expect("missing-replica ensure should recycle the model from the durable projection");

        match current {
            CurrentText::Current { text, .. } => assert_eq!(text, "recycled from projection"),
            other => panic!("expected projection-recycled current text, got {other:?}"),
        }
        // `#missingreplicarecycle`: the missing-replica case uses the short window
        // (`DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS` = 60ms in test /
        // `DOCUMENT_MODEL_ENSURE_POLL_MS` = 25ms → ~2-3 polls), well under the full
        // `DOCUMENT_MODEL_ENSURE_TIMEOUT_MS` (150ms → ~6 polls), so a stale editor
        // cannot block the single-threaded controller for the full window.
        let polls = *poll_count.lock().unwrap();
        assert!(
            (1..=4).contains(&polls),
            "missing-replica ensure should poll only within the short window, got {polls}"
        );
    }

    #[test]
    fn ensure_document_model_recovers_compacted_exchange_projection_after_publish_timeout() {
        let (_dir, doc) = temp_doc("ensure-model-compact-exchange-recovery.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let old_blocks = (0..8)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(4)
                )
            })
            .collect::<String>();
        let kept_block = "### Re: kept - gpt-5\n\nKept response.\n";
        let pre_compact = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}{kept_block}<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        let compacted = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\nCompacted content:\n- Archived 8 response topic(s): archived 0; archived 1; archived 2; 5 more\n- Prior summary/context: compacted prior responses\n{kept_block}<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &pre_compact).unwrap();

        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let editor = mint_client_id("intellij:compact-exchange-recovery");
        prior.register(editor).unwrap();
        prior.apply_local(editor, 0, 0, &compacted).unwrap();
        agent_doc_snapshot_io::save_crdt(&doc, &prior.projection_bytes()).unwrap();
        hub_registry().lock().unwrap().remove(&hash);

        let current = ensure_document_model_with_current_text_recovery_observer(
            &doc,
            "test_compact_exchange_projection_recovery",
            CurrentText::EditorAttachedMissingReplica,
            || Ok(CurrentText::EditorAttachedMissingReplica),
            || {
                current_text_for_file_with_authority_recovering_projection(
                    &doc,
                    CrdtAuthority::MultiReplica,
                )
            },
        )
        .expect("ensure should recover compacted exchange from durable projection");

        match current {
            CurrentText::Current {
                text, live_editors, ..
            } => {
                assert_eq!(text, compacted);
                assert_eq!(live_editors, 0, "editors re-register after recovery");
                assert!(text.contains("### Session Summary"));
                assert!(text.contains(kept_block));
                assert!(
                    !text.contains("Archived response body."),
                    "archived response bodies must not be re-expanded from stale disk"
                );
            }
            other => panic!("expected compacted projection current text, got {other:?}"),
        }
    }

    #[test]
    fn ensure_document_model_recovers_projection_after_publish_transport_failure() {
        let (dir, doc) = temp_doc("ensure-model-publish-transport-failure.md");
        let canonical = doc.canonicalize().unwrap();
        let file_str = canonical.to_string_lossy().to_string();
        seed_live_reliable_sync_open(&file_str);
        let hash = agent_doc_fs::document_state_hash(&canonical).unwrap();
        let compacted = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\nCompacted exchange body.\n<!-- /agent:exchange -->\n";

        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let editor = mint_client_id("intellij:publish-transport-failure");
        prior.register(editor).unwrap();
        prior.apply_local(editor, 0, 0, compacted).unwrap();
        agent_doc_snapshot_io::save_crdt(&canonical, &prior.projection_bytes()).unwrap();
        hub_registry().lock().unwrap().remove(&hash);

        std::fs::write(dir.path().join(".agent-doc").join("patches"), "not a dir").unwrap();

        let current = ensure_document_model(&canonical, "test_publish_transport_failure")
            .expect("publish transport failure should continue to durable projection recovery");

        match current {
            CurrentText::Current {
                text, live_editors, ..
            } => {
                assert_eq!(text, compacted);
                assert_eq!(live_editors, 0);
                assert!(text.contains("### Session Summary"));
            }
            other => panic!("expected projection-backed current text, got {other:?}"),
        }

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("document_model_ensure_publish_error")
                && log.contains("recovery=continue_to_projection_recovery")
                && log.contains("document_model_ensure_ready")
                && log.contains("recovery=durable_projection_after_publish_timeout"),
            "failed publish transport should be audited and then recovered from projection:\n{log}"
        );
    }

    #[test]
    fn ensure_document_model_recovers_after_delayed_replica_registration() {
        let (_dir, doc) = temp_doc("ensure-model-register.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let doc_for_register = doc.clone();
        let register = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            register_replica_for_file(&doc_for_register, "intellij:ensure-model")
                .expect("delayed register should not fail")
                .expect("editor-attached register should allocate model")
        });

        let current = ensure_document_model(&doc, "test_ensure_model")
            .expect("ensure should observe the delayed registered model");
        let (client_id, _bootstrap) = register.join().unwrap();
        match current {
            CurrentText::Current {
                text, live_editors, ..
            } => {
                assert!(text.contains("ensure-model-register.md"));
                assert_eq!(live_editors, 1);
                with_hub(&doc, |hub| {
                    assert_eq!(hub.live_count(), 1);
                    assert!(hub.is_registered(client_id));
                })
                .unwrap();
            }
            other => panic!("expected current model after ensure, got {other:?}"),
        }
    }

    #[test]
    fn ensure_document_model_falls_back_to_file_signal_after_socket_rejects_publish() {
        let (dir, doc) = temp_doc("ensure-model-socket-reject.md");
        let canonical = doc.canonicalize().unwrap();
        let file_str = canonical.to_string_lossy().to_string();
        seed_live_reliable_sync_open(&file_str);

        let root = dir.path().to_path_buf();
        let root_for_listener = root.clone();
        let server = thread::spawn(move || {
            agent_doc_ipc_io::start_listener(&root_for_listener, |_msg| {
                Some(serde_json::json!({"type": "receipt", "status": "rejected"}).to_string())
            })
            .ok();
        });
        thread::sleep(Duration::from_millis(100));
        assert!(
            agent_doc_ipc_io::is_listener_active(&root),
            "test socket listener should be active before model ensure"
        );

        let signal_file = root
            .join(".agent-doc")
            .join("patches")
            .join("publish-live-buffer.signal");
        let watcher_doc = canonical.clone();
        let watcher = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            loop {
                if let Ok(raw) = std::fs::read_to_string(&signal_file) {
                    let msg: serde_json::Value = serde_json::from_str(&raw).unwrap();
                    if msg.get("type").and_then(|value| value.as_str())
                        == Some("publish_live_buffer")
                    {
                        register_replica_for_file(&watcher_doc, "file-signal:ensure-model")
                            .expect("file-signal publish should register the model")
                            .expect("editor-attached register should allocate model");
                        let _ = std::fs::remove_file(&signal_file);
                        return true;
                    }
                }
                if std::time::Instant::now() >= deadline {
                    return false;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });

        let current = ensure_document_model(&canonical, "test_socket_reject_file_signal_fallback")
            .expect("socket rejection should fall back to file-signal model recovery");
        assert!(
            watcher.join().unwrap(),
            "file-signal watcher should observe publish-live-buffer fallback"
        );
        match current {
            CurrentText::Current {
                text, live_editors, ..
            } => {
                assert!(text.contains("ensure-model-socket-reject.md"));
                assert_eq!(live_editors, 1);
            }
            other => panic!("expected current model after file-signal fallback, got {other:?}"),
        }

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("document_model_ensure_publish_socket_error")
                && log.contains("action=file_signal_fallback")
                && log.contains("document_model_ensure_publish_requested")
                && log.contains("transport=file_signal_after_socket_error"),
            "socket rejection should be audited and then retried through file signal:\n{log}"
        );

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(&root));
        drop(server);
    }

    #[test]
    fn ensure_document_model_retries_transient_observer_timeout_until_ready() {
        let (_dir, doc) = temp_doc("ensure-model-observer-timeout.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let mut attempts = 0usize;

        let current = ensure_document_model_with_current_text_observer(
            &doc,
            "test_observer_timeout_retry",
            CurrentText::EditorAttachedMissingReplica,
            || {
                attempts += 1;
                if attempts == 1 {
                    return Err(anyhow::anyhow!(
                        "timed out after 1.0s waiting for project controller response"
                    ));
                }
                if attempts == 2 {
                    register_replica_for_file(&doc, "intellij:observer-timeout-retry")
                        .expect("retry should be able to register the model")
                        .expect("editor-attached register should allocate model");
                }
                current_text_for_file(&doc)
            },
        )
        .expect("transient observer timeout should retry until the model is ready");

        assert!(
            attempts >= 2,
            "ensure should poll again after the first observer timeout"
        );
        match current {
            CurrentText::Current {
                text, live_editors, ..
            } => {
                assert!(text.contains("ensure-model-observer-timeout.md"));
                assert_eq!(live_editors, 1);
            }
            other => panic!("expected current model after observer retry, got {other:?}"),
        }
        let log = std::fs::read_to_string(_dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("document_model_ensure_observer_error")
                && log.contains("recovery=retry_until_deadline")
                && log.contains("document_model_ensure_ready"),
            "transient observer errors should be retried inside model ensure:\n{log}"
        );
    }

    #[test]
    fn ensure_document_model_failure_is_bounded_and_names_reconciliation() {
        let (_dir, doc) = temp_doc("ensure-model-missing.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);

        let err = ensure_document_model(&doc, "test_ensure_model_missing")
            .expect_err("no editor consumer registered a model")
            .to_string();
        assert!(
            err.contains("document model startup/reconciliation failed"),
            "error should name the recovery contract: {err}"
        );
        assert!(
            err.contains("disk remained non-authoritative and was not read as a fallback"),
            "error should preserve disk authority safety: {err}"
        );
        assert!(
            !err.contains("CRDT relay has no registered replica yet"),
            "raw missing-replica text should not be the final contract: {err}"
        );
        let repeat_err = ensure_document_model(&doc, "test_ensure_model_missing_repeat")
            .expect_err("a later retry should make a fresh publish/poll attempt")
            .to_string();
        assert!(
            repeat_err.contains("recovery=retry_without_disk_write"),
            "retry should preserve retry-class error: {repeat_err}"
        );
        let log = std::fs::read_to_string(_dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("reason=recent_failure"),
            "failed ensures must not leave a retry-blocking cooldown:\n{log}"
        );
        assert_eq!(
            log.matches("document_model_ensure_start").count(),
            2,
            "a fresh retry should start another bounded ensure loop:\n{log}"
        );
    }

    #[test]
    fn ensure_document_model_active_attempt_suppresses_duplicate_probe() {
        let (_dir, doc) = temp_doc("ensure-model-in-progress.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let paths = document_model_ensure_paths(&doc).unwrap();
        std::fs::write(&paths.lock_path, "editor_attached_model_missing").unwrap();

        let err = ensure_document_model(&doc, "test_ensure_model_in_progress")
            .expect_err("active ensure should suppress duplicate publish/poll attempts")
            .to_string();
        assert!(
            err.contains("recovery=retry_without_disk_write"),
            "suppressed active ensure should preserve retry-class error: {err}"
        );
        let log = std::fs::read_to_string(_dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("document_model_ensure_suppressed") && log.contains("reason=in_progress"),
            "duplicate attempt should be in-progress-suppressed:\n{log}"
        );
        assert!(
            !log.contains("document_model_ensure_start"),
            "suppressed duplicate must not start an ensure loop:\n{log}"
        );
    }

    #[test]
    fn durable_checkpoint_defers_missing_model_to_background_repair() {
        let (_dir, doc) = temp_doc("durable-checkpoint-deferred.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let repair_paths = durable_projection_repair_paths(&doc).unwrap();
        std::fs::write(&repair_paths.lock_path, "test-held").unwrap();

        let outcome = checkpoint_durable_projection_for_file(&doc, "test_recycle").unwrap();
        match outcome {
            DurableProjectionCheckpoint::Deferred { reason } => {
                assert_eq!(reason, "editor_attached_model_missing");
            }
            other => panic!("expected deferred checkpoint, got {other:?}"),
        }
        assert!(
            repair_paths.pending_path.exists(),
            "foreground checkpoint should record a background repair marker"
        );
        let log = std::fs::read_to_string(_dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("crdt_durable_checkpoint_deferred")
                && log.contains("recovery=background_yrs_repair"),
            "foreground checkpoint should defer .yrs repair:\n{log}"
        );
        assert!(
            !log.contains("document_model_ensure_start"),
            "foreground checkpoint must not run the publish/poll ensure loop:\n{log}"
        );
    }

    #[test]
    fn editor_attached_commit_barrier_defers_when_relay_model_missing() {
        let (_dir, doc) = temp_doc("epoch-defers.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);

        assert!(!commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::MultiReplica
        ));
        let log = std::fs::read_to_string(_dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("crdt_commit_barrier_deferred")
                && log.contains("reason=missing_relay_model"),
            "multi-replica commit barrier must fail closed on missing CPC relay model:\n{log}"
        );
    }

    #[test]
    fn editor_sync_barrier_timeout_requests_live_buffer_publish_not_save_document() {
        let (dir, doc) = temp_doc("publish-buffer.md");
        let canonical = doc.canonicalize().unwrap();
        let file_str = canonical.to_string_lossy().to_string();
        let disk = std::fs::read_to_string(&canonical).unwrap();
        let visible = format!("{disk}\nvisible editor buffer\n");

        seed_live_reliable_sync_open(&file_str);
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &file_str,
            &visible,
            "jetbrains:publish-test",
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();
        assert!(
            agent_doc_debounce::editor_sync_statuses(&file_str)[0].in_flight,
            "unsynced editor-visible content should trip the barrier before publish"
        );

        let captured = Arc::new(Mutex::new(None::<serde_json::Value>));
        let captured_for_listener = captured.clone();
        let root = dir.path().to_path_buf();
        let root_for_listener = root.clone();
        let file_for_listener = file_str.clone();
        let visible_for_listener = visible.clone();
        let server = thread::spawn(move || {
            agent_doc_ipc_io::start_listener(&root_for_listener, move |msg| {
                let parsed: serde_json::Value = serde_json::from_str(msg).ok()?;
                *captured_for_listener.lock().unwrap() = Some(parsed.clone());
                if parsed.get("type").and_then(|value| value.as_str())
                    == Some("publish_live_buffer")
                {
                    agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                        &file_for_listener,
                        &visible_for_listener,
                        "jetbrains:publish-test",
                        "jetbrains",
                        "test",
                        &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                    )
                    .ok()?;
                    Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
                } else {
                    Some(
                        serde_json::json!({"type": "receipt", "status": "rejected"}).to_string(),
                    )
                }
            })
            .ok();
        });

        thread::sleep(Duration::from_millis(100));

        assert!(
            !settle_or_flush_editor_sync_barrier(&canonical, "test_publish_live_buffer"),
            "read-only publish refreshes the live-buffer projection but must not make that projection authoritative"
        );

        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("listener saw a recovery request");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], file_str);
        assert!(
            msg.get("patch_id").is_none(),
            "live-buffer publish is read-only and must not use save_document patch ids: {msg}"
        );
        assert!(
            matches!(
                current_text_for_file(&canonical).unwrap(),
                CurrentText::EditorAttachedMissingReplica
            ),
            "read-only live-buffer publish must not create the authoritative CRDT model"
        );

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(&root));
        drop(server);
    }

    #[test]
    fn editor_sync_barrier_timeout_falls_back_to_publish_live_buffer_file_signal_without_listener()
    {
        // `#vscodepublishparity` — a VS Code (or pluginless / file-IPC) session runs
        // NO socket listener. The editor-sync-barrier timeout flush must still reach
        // that editor by writing `.agent-doc/patches/publish-live-buffer.signal`
        // instead of skipping the flush with `cause=no_ipc_listener`, which silently
        // dropped the live-buffer publish for VS Code while the sibling converge path
        // already fell back to the file signal.
        let (_dir, doc) = temp_doc("publish-buffer-file-signal.md");
        let canonical = doc.canonicalize().unwrap();
        let file_str = canonical.to_string_lossy().to_string();
        let disk = std::fs::read_to_string(&canonical).unwrap();
        let visible = format!("{disk}\nvisible editor buffer\n");

        seed_live_reliable_sync_open(&file_str);
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &file_str,
            &visible,
            "vscode:publish-file-signal-test",
            "vscode",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();
        assert!(
            agent_doc_debounce::editor_sync_statuses(&file_str)[0].in_flight,
            "unsynced editor-visible content should trip the barrier before publish"
        );

        // Compute the project root exactly as settle_or_flush_editor_sync_barrier does,
        // and assert no socket listener is active so the file-signal branch is taken.
        let root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
        assert!(
            !agent_doc_ipc_io::is_listener_active(&root),
            "test must run with no socket listener so the file-signal fallback is exercised"
        );

        assert!(
            !settle_or_flush_editor_sync_barrier(
                &canonical,
                "test_publish_live_buffer_file_signal"
            ),
            "file-signal publish refreshes the projection but must not mark it synced"
        );

        let signal_file = root
            .join(".agent-doc")
            .join("patches")
            .join("publish-live-buffer.signal");
        let raw = std::fs::read_to_string(&signal_file)
            .expect("editor-sync-barrier flush must write publish-live-buffer.signal for VS Code");
        let msg: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], file_str);
        assert!(
            msg.get("patch_id").is_none(),
            "live-buffer publish is read-only and must not use save_document patch ids: {msg}"
        );
    }

    #[test]
    fn editor_attached_commit_barrier_flushes_live_replicas_on_a_consistent_cut() {
        // EditorAttached / MultiReplica: a live editor replica with an un-flushed
        // local op is flushed into the canonical replica at the barrier, and the
        // committed cut holds the editor's keystrokes.
        let (_dir, doc) = temp_doc("attached.md");
        let editor = mint_client_id("intellij:attached-test");
        with_hub(&doc, |hub| {
            hub.register(editor).unwrap();
            // Editor types locally; the op is NOT yet relayed to canonical.
            hub.local_edit(editor, 0, 0, "typed-before-commit").unwrap();
            assert!(
                !hub.canonical_text().contains("typed-before-commit"),
                "the un-relayed op is not in canonical before the barrier"
            );
        })
        .unwrap();

        // The barrier flushes the live editor into canonical (consistent cut).
        assert!(commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::MultiReplica
        ));
        with_hub(&doc, |hub| {
            assert!(
                hub.canonical_text().contains("typed-before-commit"),
                "the barrier flushed the live editor's op into the committed cut"
            );
        })
        .unwrap();
    }

    #[test]
    fn editor_attached_durable_checkpoint_writes_recovery_projection() {
        let (_dir, doc) = temp_doc("attached-checkpoint.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let editor = mint_client_id("intellij:durable-checkpoint");
        with_hub(&doc, |hub| {
            hub.register(editor).unwrap();
            hub.local_edit(editor, 0, 0, "checkpointed").unwrap();
        })
        .unwrap();

        let outcome = checkpoint_durable_projection_for_file(&doc, "test_recycle").unwrap();

        match outcome {
            DurableProjectionCheckpoint::Checkpointed {
                changed: true,
                live_editors: 1,
                ..
            } => {}
            other => panic!("expected changed checkpoint, got {other:?}"),
        }
        let projection = agent_doc_snapshot_io::load_crdt(&doc)
            .unwrap()
            .expect("checkpoint writes durable recovery projection");
        let recovered = RelayHub::recover_from_projection(1, &projection).unwrap();
        assert!(
            recovered.canonical_text().contains("checkpointed"),
            "checkpoint projection must recover the live editor text"
        );
    }

    #[test]
    fn editor_attached_commit_barrier_does_not_block_on_a_disconnected_editor() {
        // A slow / disconnected editor must NOT deadlock the commit barrier — its
        // op is excluded from the live cut and contributes on reconnect.
        let (_dir, doc) = temp_doc("disconnected.md");
        let live = mint_client_id("vscode:live");
        let slow = mint_client_id("intellij:slow");
        with_hub(&doc, |hub| {
            hub.register(live).unwrap();
            hub.register(slow).unwrap();
            hub.local_edit(live, 0, 0, "LIVE").unwrap();
            hub.local_edit(slow, 0, 0, "SLOW").unwrap();
            // The slow editor disconnects with an un-flushed op.
            hub.disconnect(slow);
        })
        .unwrap();

        // The barrier returns ready WITHOUT blocking; the live op is in the cut,
        // the disconnected op is not.
        assert!(commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::MultiReplica
        ));
        with_hub(&doc, |hub| {
            let cut = hub.canonical_text();
            assert!(cut.contains("LIVE"), "the live editor's op is in the cut");
            assert!(
                !cut.contains("SLOW"),
                "the disconnected editor's op is excluded (no deadlock)"
            );
            // No data loss: the slow editor contributes on reconnect.
            hub.reconnect(slow).unwrap();
            assert!(hub.canonical_text().contains("SLOW"));
        })
        .unwrap();
    }

    #[test]
    fn disk_demotion_in_memory_wins_at_the_live_load_seam() {
        // EditorAttached: a STALE disk projection reconciled at the live load seam
        // must not regress the live in-memory text (in-memory wins).
        let (_dir, doc) = temp_doc("demotion.md");
        let editor = mint_client_id("intellij:demotion");
        let stale_projection = with_hub(&doc, |hub| {
            hub.register(editor).unwrap();
            hub.apply_local(editor, 0, 0, "v1").unwrap();
            // Flush a durable recovery projection (what hits .yrs) at "v1".
            let proj = hub.projection_bytes();
            // The live session advances past the projection to "v1 v2".
            let len = hub.canonical_text().chars().count() as u32;
            hub.apply_local(editor, len, 0, " v2").unwrap();
            assert_eq!(hub.canonical_text(), "v1 v2");
            proj
        })
        .unwrap();

        // Reconciling the STALE disk projection holds no new ops and never regresses.
        let changed = reconcile_disk_projection_for_file_with_authority(
            &doc,
            &stale_projection,
            CrdtAuthority::MultiReplica,
        )
        .unwrap();
        assert_eq!(changed, Some(false), "a stale disk projection adds no ops");
        with_hub(&doc, |hub| {
            assert_eq!(hub.canonical_text(), "v1 v2", "in-memory replica wins");
        })
        .unwrap();
    }

    #[test]
    fn commit_barrier_rebuilds_canonical_after_out_of_band_disk_correction() {
        // `#staleinmem`: after a corrupt commit, an out-of-band disk correction
        // (e.g. `git checkout HEAD` / `reset --from-current`) must rebuild the stale
        // canonical at the NEXT commit barrier so the discarded content cannot
        // re-commit. This is the process-global-hub bug ("git checkout HEAD won't
        // hold; only a supervisor restart clears the in-memory CRDT") fixed in-place
        // without a restart.
        let (_dir, doc) = temp_doc("oob-correction.md");
        let editor = mint_client_id("intellij:oob");
        let corrupt = "GOOD\nCORRUPT-RESPONSE\n";
        with_hub(&doc, |hub| {
            hub.register(editor).unwrap();
            hub.apply_local(editor, 0, 0, corrupt).unwrap();
            // Mark this as the state we last committed to disk.
            hub.record_committed_baseline(corrupt);
        })
        .unwrap();

        // Operator corrects the document out of band (drops the corrupt block).
        let good = "GOOD\n";
        std::fs::write(&doc, good).unwrap();

        // The next commit barrier reconciles against the corrected disk first.
        assert!(commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::MultiReplica
        ));
        with_hub(&doc, |hub| {
            assert_eq!(
                hub.canonical_text(),
                good,
                "the barrier rebuilt the canonical from the corrected disk"
            );
            assert!(
                !hub.canonical_text().contains("CORRUPT-RESPONSE"),
                "the discarded out-of-band content is gone from the canonical"
            );
            assert_eq!(
                hub.member_text(editor).as_deref(),
                Some(good),
                "the editor mirror was reseeded so a flush cannot reintroduce the corruption"
            );
        })
        .unwrap();
    }

    #[test]
    fn commit_barrier_keeps_in_memory_when_disk_matches_last_commit() {
        // The normal path: disk unchanged since the last commit → no rebuild, and a
        // live editor's un-flushed op is still flushed into the cut (in-memory wins).
        let (_dir, doc) = temp_doc("no-oob.md");
        let editor = mint_client_id("intellij:no-oob");
        let committed = "# no-oob.md\n\nbody\n";
        std::fs::write(&doc, committed).unwrap();
        with_hub(&doc, |hub| {
            hub.register(editor).unwrap();
            hub.apply_local(editor, 0, 0, committed).unwrap();
            hub.record_committed_baseline(committed);
            // Editor types more locally AFTER the commit (canonical ahead of disk).
            hub.local_edit(editor, 0, 0, "NEW ").unwrap();
        })
        .unwrap();

        assert!(commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::MultiReplica
        ));
        with_hub(&doc, |hub| {
            assert!(
                hub.canonical_text().starts_with("NEW "),
                "disk == last commit → no rebuild; the new live op flushes into the cut"
            );
        })
        .unwrap();
    }

    #[test]
    fn disk_demotion_is_skipped_on_the_headless_path() {
        // GitAuthoritative: no live in-memory authority — the live reconcile is
        // skipped (the baseline-wins snapshot load path runs unchanged) and no hub
        // is allocated.
        let (_dir, doc) = temp_doc("headless-demotion.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let result = reconcile_disk_projection_for_file_with_authority(
            &doc,
            b"any-bytes-are-ignored",
            CrdtAuthority::GitAuthoritative,
        )
        .unwrap();
        assert_eq!(result, None, "the headless path performs no live reconcile");
        assert!(
            !hub_registry().lock().unwrap().contains_key(&hash),
            "the headless path must not allocate a relay hub"
        );
    }

    #[test]
    fn recover_hub_from_disk_rebuilds_canonical_on_restart() {
        // Supervisor restart: rebuild the canonical replica from the last disk
        // recovery projection; members re-register / re-sync afterward.
        let (_dir, doc) = temp_doc("recover.md");
        // Build a projection from a throwaway hub (simulating a prior session).
        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let ed = mint_client_id("intellij:prior");
        prior.register(ed).unwrap();
        prior.apply_local(ed, 0, 0, "durable").unwrap();
        let projection = prior.projection_bytes();

        recover_hub_from_disk(&doc, &projection).unwrap();
        with_hub(&doc, |hub| {
            assert_eq!(hub.canonical_text(), "durable");
            assert_eq!(hub.live_count(), 0, "members re-register after restart");
        })
        .unwrap();
    }

    /// Test-only authority-explicit variant of [`reconcile_disk_projection_for_file`]
    /// so the demotion seam is deterministically exercisable without a live lease.
    fn reconcile_disk_projection_for_file_with_authority(
        file: &Path,
        projection: &[u8],
        authority: CrdtAuthority,
    ) -> Result<Option<bool>> {
        if !authority.editor_attached() {
            return Ok(None);
        }
        let changed = with_hub(file, |hub| hub.reconcile_disk_projection(projection))??;
        Ok(Some(changed))
    }

    /// Test-only authority-explicit variant of [`apply_disk_change_for_file`]
    /// (skips the live sync barrier + lease resolution), so the C1 host seam is
    /// deterministically exercisable.
    fn apply_disk_change_for_file_with_authority(
        file: &Path,
        on_disk: &str,
        authority: CrdtAuthority,
    ) -> Result<Option<DiskChangeOutcome>> {
        if !authority.editor_attached() {
            return Ok(None);
        }
        let outcome = with_hub_seeded_from_file(file, |hub| hub.apply_disk_change(on_disk))??;
        Ok(Some(outcome))
    }

    #[test]
    fn pull_rebootstrap_is_none_when_headless() {
        // No live editor → no hub → nothing to re-bootstrap.
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("headless-rebootstrap.md");
        std::fs::write(&file, "# doc\n").unwrap();
        assert_eq!(pull_rebootstrap_for_file(&file, "editor:x").unwrap(), None);
    }

    /// Test-only authority-explicit variant of [`adopt_authoritative_text_for_file`]
    /// so the compaction convergence seam is deterministically exercisable without a
    /// live lease.
    fn adopt_authoritative_text_for_file_with_authority(
        file: &Path,
        text: &str,
        authority: CrdtAuthority,
    ) -> Result<Option<bool>> {
        if !authority.editor_attached() {
            return Ok(None);
        }
        let changed = with_hub_seeded_from_file(file, |hub| hub.adopt_authoritative_text(text))??;
        Ok(Some(changed))
    }

    #[test]
    fn adopt_authoritative_text_is_none_when_headless() {
        // GitAuthoritative (no live editor) → no live canonical; the caller's
        // disk+snapshot write is already authoritative.
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("headless-adopt.md");
        std::fs::write(&file, "# doc\n\nbody\n").unwrap();
        assert_eq!(
            adopt_authoritative_text_for_file_with_authority(
                &file,
                "# doc\n\ncompacted\n",
                CrdtAuthority::GitAuthoritative,
            )
            .unwrap(),
            None,
        );
    }

    #[test]
    fn adopt_authoritative_text_converges_a_stale_canonical_for_the_commit_read() {
        // `#jb-compact-commit-stale-relay-canonical`: seed the hub from the
        // PRE-COMPACT text (the frozen phantom-lease canonical), then adopt the
        // COMPACTED content the compaction already wrote to disk+snapshot. The hub
        // canonical must converge so the authoritative-compaction commit's
        // `try_resolve_current_document_content` reads the compacted document.
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("attached-adopt.md");
        let pre_compact = "# doc\n\nturn 1\nturn 2\nturn 3\nturn 4 (kept)\n";
        std::fs::write(&file, pre_compact).unwrap();
        let compacted = "# doc\n\n*Compacted. 3 turns archived.*\nturn 4 (kept)\n";
        let changed = adopt_authoritative_text_for_file_with_authority(
            &file,
            compacted,
            CrdtAuthority::MultiReplica,
        )
        .unwrap();
        assert_eq!(changed, Some(true), "the stale canonical is converged");
    }

    #[test]
    fn apply_disk_change_host_is_none_when_headless() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("headless.md");
        std::fs::write(&file, "# doc\n\nbody\n").unwrap();
        // GitAuthoritative (no live editor) → no live canonical to reconcile.
        assert_eq!(
            apply_disk_change_for_file_with_authority(
                &file,
                "# doc\n\nchanged\n",
                CrdtAuthority::GitAuthoritative,
            )
            .unwrap(),
            None,
        );
    }

    #[test]
    fn apply_disk_change_host_reconciles_noop_when_editor_already_has_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("attached.md");
        // Seed the hub from this exact text; an identical disk change is a no-op.
        std::fs::write(&file, "# doc\n\nbody\n").unwrap();
        let outcome = apply_disk_change_for_file_with_authority(
            &file,
            "# doc\n\nbody\n",
            CrdtAuthority::MultiReplica,
        )
        .unwrap();
        assert_eq!(outcome, Some(DiskChangeOutcome::AlreadyReconciled));
    }

    // ---- disk-change-reconcile marker (C1b cross-process signal) ----

    #[test]
    fn request_disk_change_writes_a_pending_marker() {
        let (_dir, file) = temp_doc("marker.md");
        assert!(!disk_change_request_pending(&file));
        request_disk_change_reconcile(&file).unwrap();
        assert!(disk_change_request_pending(&file));
        // The marker path resolves under the project's .agent-doc dir.
        let marker = agent_doc_fs::disk_change_request_path_for(&file).unwrap();
        assert!(marker.exists());
    }

    #[test]
    fn route_signal_drops_no_marker_for_headless_change() {
        // No live editor → decide_watch_action yields ApplyAsDiskAuthority, which
        // the disk-authority load path owns — no marker for a supervisor to consume.
        let (_dir, file) = temp_doc("route-headless.md");
        let action =
            route_disk_change_signal(&file, &WatchDelivery::Change { generation: 1 }).unwrap();
        assert_eq!(action, WatchAction::ApplyAsDiskAuthority);
        assert!(!disk_change_request_pending(&file));
    }

    #[test]
    fn route_signal_drops_no_marker_for_non_change_delivery() {
        let (_dir, file) = temp_doc("route-echo.md");
        let action = route_disk_change_signal(&file, &WatchDelivery::SelfWriteEcho).unwrap();
        assert_eq!(action, WatchAction::None);
        assert!(!disk_change_request_pending(&file));
    }

    #[test]
    fn consume_without_a_marker_is_a_noop() {
        let (_dir, file) = temp_doc("nomarker.md");
        assert_eq!(consume_disk_change_reconcile(&file).unwrap(), None);
    }

    #[test]
    fn consume_clears_the_marker_even_on_a_headless_no_op() {
        // No live editor → apply_disk_change_for_file is a headless no-op (None),
        // but the marker signal must still be consumed exactly once.
        let (_dir, file) = temp_doc("headless-consume.md");
        request_disk_change_reconcile(&file).unwrap();
        assert!(disk_change_request_pending(&file));

        let outcome = consume_disk_change_reconcile(&file).unwrap();
        // Headless: no hub allocated, so the reconcile itself is None...
        assert_eq!(outcome, None);
        // ...but the marker is cleared so the idle loop does not spin on it.
        assert!(!disk_change_request_pending(&file));
    }
}
