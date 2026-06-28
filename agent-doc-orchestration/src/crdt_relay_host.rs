//! Live wiring of the CRDT relay/commit-barrier into the finalize + disk paths
//! (`#crdtauth4` cutover).
//!
//! The state-vector sync primitive ([`agent_doc_core::crdt_sync`]), the authority
//! state machine ([`crate::crdt_authority`]), and the relay hub
//! ([`crate::crdt_relay`]) were built and tested as standalone modules. This
//! module is the **live cutover**: it routes the real `finalize` commit point and
//! the real `.yrs` load/merge call-sites through the authority-gated barrier,
//! while keeping the headless / [`CrdtAuthority::GitAuthoritative`] path
//! byte-for-byte unchanged.
//!
//! ## Authority gate is load-bearing
//!
//! Every entry point here resolves the document's [`CrdtAuthority`] first (cheaply,
//! per-document, fail-safe to `GitAuthoritative`) via
//! [`crate::crdt_authority::authority_for_file`]:
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
//! ([`crate::snapshot::doc_hash`]). Each document gets its own independent
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
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::Result;

use crate::crdt_authority::{CrdtAuthority, authority_for_file};
use crate::crdt_relay::{PendingReplicaUpdate, RelayHub, ReplicaDeliverySnapshot};

/// The canonical replica's reserved yrs client-id for every per-document hub. The
/// supervisor's canonical replica is the hub authority; editor replicas mint
/// their own ids via [`crate::crdt_relay::mint_client_id`] and can never collide
/// with this reserved id (`RelayHub::register` rejects it).
const CANONICAL_CLIENT_ID: u64 = 1;
const EDITOR_SYNC_SETTLE_MS: u64 = 75;
const EDITOR_SYNC_TIMEOUT_MS: u64 = 150;

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
    let hash = crate::snapshot::doc_hash(file)?;
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    let hub = registry
        .entry(hash)
        .or_insert_with(|| RelayHub::new(CANONICAL_CLIENT_ID));
    Ok(f(hub))
}

/// [`with_hub`] for live file-backed authority paths. A newly allocated hub must
/// start from the current document text, not an empty CRDT, or the first editor
/// delta can be applied at a clamped offset and later overwrite the buffer.
fn with_hub_seeded_from_file<T>(file: &Path, f: impl FnOnce(&mut RelayHub) -> T) -> Result<T> {
    let hash = crate::snapshot::doc_hash(file)?;
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
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    let hub = registry
        .entry(hash)
        .or_insert_with(|| RelayHub::from_text(CANONICAL_CLIENT_ID, &seed_text));
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
pub fn register_replica_for_file(file: &Path, identity: &str) -> Result<Option<(u64, Vec<u8>)>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = crate::crdt_relay::mint_client_id(identity);
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
    crate::ops_log::log_op(
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
    let client_id = crate::crdt_relay::mint_client_id(identity);
    let removed = with_hub_seeded_from_file(file, |hub| hub.deregister(client_id))?;
    crate::ops_log::log_op(
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
/// document's hub (keyed by [`crate::snapshot::doc_hash`]) — `#xdocsuper1/3`.
pub fn relay_replica_update_for_file(
    file: &Path,
    identity: &str,
    update: &[u8],
) -> Result<Option<FanOut>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = crate::crdt_relay::mint_client_id(identity);
    let packet = with_hub_seeded_from_file(file, |hub| hub.relay_update(client_id, update))??;
    let canonical_len =
        with_hub_seeded_from_file(file, |hub| hub.canonical_text().chars().count())?;
    crate::ops_log::log_op(
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

/// Pull supervisor-to-editor updates queued for this replica. The returned
/// updates remain pending until [`ack_replica_update_for_file`] confirms the
/// editor applied them.
pub fn pull_replica_updates_for_file(file: &Path, identity: &str) -> Result<Option<ReplicaPull>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = crate::crdt_relay::mint_client_id(identity);
    let updates = with_hub_seeded_from_file(file, |hub| hub.pending_updates(client_id))??;
    let delivery = with_hub_seeded_from_file(file, |hub| {
        hub.delivery_snapshot()
            .into_iter()
            .find(|entry| entry.client_id == client_id)
    })?
    .ok_or_else(|| anyhow::anyhow!("replica {client_id} is not registered"))?;
    crate::ops_log::log_op(
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
    Ok(Some(ReplicaPull {
        client_id,
        updates,
        delivery,
    }))
}

/// ACK one pulled update after the editor applied it to the local document
/// replica/buffer.
pub fn ack_replica_update_for_file(
    file: &Path,
    identity: &str,
    patch_id: &str,
    generation: u64,
) -> Result<Option<bool>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = crate::crdt_relay::mint_client_id(identity);
    let acknowledged = with_hub_seeded_from_file(file, |hub| {
        hub.ack_delivery(client_id, patch_id, generation)
    })??;
    crate::ops_log::log_op(
        file,
        &format!(
            "crdt_replica_ack file={} authority=multi_replica client_id={} patch_id={} generation={} acknowledged={}",
            file.display(),
            client_id,
            patch_id,
            generation,
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
    state: crate::crdt_relay::AwarenessState,
) -> Result<Option<Vec<(u64, crate::crdt_relay::AwarenessState)>>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = crate::crdt_relay::mint_client_id(identity);
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
    let hash = crate::snapshot::doc_hash(file)?;
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    match registry.get(&hash) {
        // A live hub already holds the authority — disk is recovery-only, so
        // reconcile the projection into it (in-memory wins) instead of clobbering.
        Some(existing) => {
            existing.reconcile_disk_projection(projection)?;
            Ok(())
        }
        None => {
            let hub = RelayHub::recover_from_projection(CANONICAL_CLIENT_ID, projection)?;
            registry.insert(hash, hub);
            Ok(())
        }
    }
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
/// second lease read.
pub fn commit_barrier_for_file_with_authority(file: &Path, authority: CrdtAuthority) -> bool {
    if !authority.editor_attached() {
        // Detached / headless: the CRDT is ephemeral, git is the source of truth,
        // and there are no live editor replicas to flush. The barrier is trivially
        // satisfied and NO hub is touched — the headless path is byte-for-byte
        // unchanged.
        return true;
    }
    if !settle_or_flush_editor_sync_barrier(file, "commit_barrier") {
        return false;
    }
    // `#staleinmem` — out-of-band baseline reconcile, BEFORE flushing live editors
    // into the canonical for the commit cut. If the document was corrected out of
    // band on disk since this hub's last commit (a `git checkout HEAD` /
    // `reset --from-current` recovery), the additive in-memory-wins reconcile can
    // never displace the stale canonical ops, so the stale canonical otherwise
    // re-commits the discarded content every cycle until a supervisor restart
    // clears the process-global hub. Rebuilding the canonical from the corrected
    // disk baseline makes the correction stick without a restart.
    if let Ok(on_disk) = std::fs::read_to_string(file) {
        match with_hub_seeded_from_file(file, |hub| {
            hub.reconcile_canonical_against_baseline(&on_disk)
        }) {
            Ok(Ok(true)) => crate::ops_log::log_op(
                file,
                &format!(
                    "crdt_canonical_rebuilt_from_baseline file={} authority=multi_replica disk_len={}",
                    file.display(),
                    on_disk.len()
                ),
            ),
            Ok(Ok(false)) => {}
            Ok(Err(e)) => crate::ops_log::log_op(
                file,
                &format!(
                    "crdt_canonical_baseline_reconcile_error file={} error={}",
                    file.display(),
                    e
                ),
            ),
            Err(e) => crate::ops_log::log_op(
                file,
                &format!(
                    "crdt_canonical_baseline_reconcile_registry_error file={} error={}",
                    file.display(),
                    e
                ),
            ),
        }
    }
    match with_hub_seeded_from_file(file, |hub| hub.commit_barrier_under_authority(authority)) {
        Ok(Ok(ready)) => {
            let delivery_converged =
                with_hub_seeded_from_file(file, |hub| hub.delivery_converged()).unwrap_or(false);
            crate::ops_log::log_op(
                file,
                &format!(
                    "crdt_commit_barrier file={} authority=multi_replica ready={} delivery_converged={} live_editors={}",
                    file.display(),
                    ready,
                    delivery_converged,
                    with_hub_seeded_from_file(file, |hub| hub.live_count()).unwrap_or(0),
                ),
            );
            ready && delivery_converged
        }
        Ok(Err(e)) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "crdt_commit_barrier_error file={} authority=multi_replica error={}",
                    file.display(),
                    e
                ),
            );
            false
        }
        Err(e) => {
            crate::ops_log::log_op(
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
            crate::ops_log::log_op(
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
    let hash = match crate::snapshot::doc_hash(file) {
        Ok(h) => h,
        Err(e) => {
            crate::ops_log::log_op(
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
        Err(e) => crate::ops_log::log_op(
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
/// ([`crate::crdt_relay::DISK_IS_RECOVERY_PROJECTION_ONLY`]): a (possibly stale)
/// disk projection is reconciled INTO the live replica, which can only add ops the
/// live replica genuinely lost (a crash gap) and can never regress live text —
/// in-memory wins. Returns `Some(changed)` where `changed` is whether the disk
/// held ops the live replica was missing.
///
/// Under [`CrdtAuthority::GitAuthoritative`] there is no live in-memory authority
/// to reconcile against — disk demotion does not apply, and the existing
/// baseline-wins load path ([`crate::snapshot::crdt_merge_base_state`], which
/// already discards a stale `.yrs` whose markdown projection does not match the
/// cycle baseline) is left to run unchanged. Returns `None` (no live reconcile
/// performed).
pub fn reconcile_disk_projection_for_file(file: &Path, projection: &[u8]) -> Result<Option<bool>> {
    let file_str = file.display().to_string();
    let authority = authority_for_file(&file_str);
    if !authority.editor_attached() {
        // Headless: no live canonical replica is authoritative. The
        // baseline-wins load path in snapshot.rs already handles stale disk.
        return Ok(None);
    }
    let _ = settle_or_flush_editor_sync_barrier(file, "disk_projection_reconcile");
    let changed =
        with_hub_seeded_from_file(file, |hub| hub.reconcile_disk_projection(projection))??;
    crate::ops_log::log_op(
        file,
        &format!(
            "crdt_disk_demotion_reconcile file={} authority=multi_replica disk_added_ops={}",
            file.display(),
            changed
        ),
    );
    Ok(Some(changed))
}

fn settle_or_flush_editor_sync_barrier(file: &Path, reason: &str) -> bool {
    let file_str = file.display().to_string();
    let outcome = crate::debounce::await_editor_sync_barrier(
        &file_str,
        EDITOR_SYNC_SETTLE_MS,
        EDITOR_SYNC_TIMEOUT_MS,
    );
    let in_flight = outcome
        .statuses
        .iter()
        .filter(|status| status.in_flight)
        .count();
    crate::ops_log::log_op(
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
    if outcome.kind != crate::debounce::EditorSyncBarrierKind::TimedOut {
        return true;
    }

    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(e) => {
            crate::ops_log::log_op(
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
    let project_root = crate::write::resolve_ipc_project_root_pub(&canonical);
    if !crate::ipc_socket::is_listener_active(&project_root) {
        crate::ops_log::log_op(
            file,
            &format!(
                "editor_sync_barrier_flush_skipped file={} reason={} cause=no_ipc_listener",
                file.display(),
                reason
            ),
        );
        return false;
    }

    let patch_id = uuid::Uuid::new_v4().to_string();
    let path_str = canonical.to_string_lossy().to_string();
    match crate::ipc_socket::send_save_document(&project_root, &path_str, &patch_id) {
        Ok(true) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "editor_sync_barrier_flush_requested file={} reason={} patch_id={}",
                    file.display(),
                    reason,
                    patch_id
                ),
            );
        }
        Ok(false) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "editor_sync_barrier_flush_not_acked file={} reason={} patch_id={}",
                    file.display(),
                    reason,
                    patch_id
                ),
            );
            return false;
        }
        Err(e) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "editor_sync_barrier_flush_error file={} reason={} patch_id={} error={}",
                    file.display(),
                    reason,
                    patch_id,
                    e
                ),
            );
            return false;
        }
    }

    let after_flush = crate::debounce::await_editor_sync_barrier(
        &file_str,
        EDITOR_SYNC_SETTLE_MS,
        EDITOR_SYNC_TIMEOUT_MS,
    );
    let in_flight = after_flush
        .statuses
        .iter()
        .filter(|status| status.in_flight)
        .count();
    crate::ops_log::log_op(
        file,
        &format!(
            "editor_sync_barrier_after_flush file={} reason={} outcome={:?} statuses={} in_flight={} typing_recent={}",
            file.display(),
            reason,
            after_flush.kind,
            after_flush.statuses.len(),
            in_flight,
            after_flush.typing_recent
        ),
    );
    after_flush.kind != crate::debounce::EditorSyncBarrierKind::TimedOut
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt_relay::mint_client_id;
    use std::io::Write;

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

    #[test]
    fn register_replica_seeds_fresh_hub_from_current_document_text() {
        let (_dir, doc) = temp_doc("seed-register.md");
        let file_str = doc.display().to_string();
        crate::plugin_owner::write_plugin_owner_lease_for_test(&file_str, std::process::id());
        let on_disk = std::fs::read_to_string(&doc).unwrap();

        let (client_id, bootstrap) = register_replica_for_file(&doc, "intellij:seed")
            .unwrap()
            .expect("editor-attached register should return a bootstrap");
        let replica =
            agent_doc_core::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap).unwrap();
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
    fn detached_commit_barrier_is_a_trivial_noop() {
        // Detached / GitAuthoritative: the barrier is trivially ready and NO hub is
        // allocated for the document — the headless commit path is untouched.
        let (_dir, doc) = temp_doc("detached.md");
        let hash = crate::snapshot::doc_hash(&doc).unwrap();
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
    fn editor_attached_commit_barrier_defers_in_flight_editor_epoch() {
        let (_dir, doc) = temp_doc("epoch-defers.md");
        let file_str = doc.display().to_string();
        let disk = std::fs::read_to_string(&doc).unwrap();
        crate::debounce::document_changed_with_content_for_editor(
            &file_str,
            &format!("{disk}\nunsaved editor text"),
            Some("jetbrains:epoch-defers"),
        );

        let start = std::time::Instant::now();
        assert!(!commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::MultiReplica
        ));
        assert!(
            start.elapsed() >= std::time::Duration::from_millis(100),
            "multi-replica commit barrier must defer briefly before failing closed on an in-flight editor epoch"
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
        let hash = crate::snapshot::doc_hash(&doc).unwrap();
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
}
