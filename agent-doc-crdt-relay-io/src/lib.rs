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
//!   canonical replica from the ledger projection ([`recover_hub_from_projection`]), and
//!   the per-document hub registry ([`with_hub`]).
//! - **Wired:** editor-replica lifecycle and delta transport through the
//!   supervisor IPC family (`replica_register`, `replica_update`, `replica_pull`,
//!   `replica_projection`, `replica_deregister`). Fan-out is target-owned: peer updates
//!   remain queued until the target editor applies them to its FFI replica/buffer
//!   and publishes its complete visible projection. The commit barrier refuses a
//!   MultiReplica closeout while any live target has unprojected delivery.

use agent_doc_turn::op_log::OpsLogEvent;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use agent_doc_document_realtime::crdt_authority::CrdtAuthority;
use agent_doc_document_realtime::crdt_relay::{
    AwarenessState, DiskChangeOutcome, DocumentOpDeltaOutcome, PendingReplicaUpdate, RelayHub,
    ReplicaDeliverySnapshot, RetainedCanonicalProjection, mint_client_id,
};
use agent_doc_document_realtime::watch_authority::{
    WatchAction, WatchDelivery, decide_watch_action,
};
use lazily::{DurableOutbox, ThreadSafeContext, ThreadSafeSourceMap};

/// Stable event kinds delivered to Lazily editor replicas.
/// Strings are a wire encoding only; producers select a closed enum variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrdtReplicaEventReason {
    Fanout,
    ResponseCellAdd,
    SalientResponseUpsert,
    CpWrite,
    Rebootstrap,
    CanonicalProjection,
}

/// Decision at the controller cold-start boundary for an incoming editor
/// update. A retained editor can send an incremental frame before its
/// registration event is projected by the replacement controller. The wire
/// ingress is an RPC, but authority is the relay's reactive membership/full-state
/// projection. The new hub's disk seed and retained editor state are independent
/// CRDT lineages, so they must never be union-applied merely because both contain
/// the same visible document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColdStartReplicaUpdateDecision {
    Relay,
    ReprojectCanonical,
}

/// Observable result of fencing a stable Compact Exchange snapshot into a fresh
/// CRDT lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactEpochOutcome {
    pub prior_lineage: String,
    pub lineage: String,
    pub state_bytes_before: usize,
    pub state_bytes_after: usize,
    pub rebootstrap_members: usize,
}

/// Observable result of requesting a Compact Exchange lineage fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactEpochRequestOutcome {
    /// Delivery was already stable, so the epoch was rebuilt synchronously.
    Fenced(CompactEpochOutcome),
    /// A newer canonical delivery is still in flight. The relay retained the
    /// request and will rebuild when the final visible projection arrives.
    Retained { lineage: String, state_bytes: usize },
}

pub const fn decide_cold_start_replica_update(
    registered: bool,
    controller_projection_established: bool,
    canonical_projection_pending: bool,
) -> ColdStartReplicaUpdateDecision {
    if registered && controller_projection_established && !canonical_projection_pending {
        ColdStartReplicaUpdateDecision::Relay
    } else {
        ColdStartReplicaUpdateDecision::ReprojectCanonical
    }
}

impl CrdtReplicaEventReason {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Fanout => "fanout",
            Self::ResponseCellAdd => "response_cell_add",
            Self::SalientResponseUpsert => "salient_response_upsert",
            Self::CpWrite => "cp_write",
            Self::Rebootstrap => "rebootstrap",
            Self::CanonicalProjection => "canonical_projection",
        }
    }
}

impl std::fmt::Display for CrdtReplicaEventReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.token())
    }
}

fn restore_durable_liveness(file: &Path, document_hash: &str) -> Result<()> {
    let Some(project_root) = agent_doc_fs::find_project_root(file) else {
        return Ok(());
    };
    let database_path = agent_doc_sqlite::state_store::state_db_path(&project_root);
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
        let outbox = lazily::SqliteOutbox::open(&database_path, document_hash.to_string())?;
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
    let mut plane = agent_doc_reliable_sync_io::global_liveness_plane().lock();
    for batch in &batches {
        plane.restore_liveness(batch);
    }
    for cursor in &snapshot.cursors {
        plane.restore_cursor(&cursor.document_hash, cursor.ack_through);
    }
    Ok(())
}

/// Editor liveness authority shared by relay, controller, closeout, repair, and
/// write convergence. Cold readers rebuild the receiver journal and retained
/// sender suffix. Lazily is the only authority model.
pub fn reliable_sync_editor_live_for_file(file: &Path) -> bool {
    let file_string = file.to_string_lossy();
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

/// Return every live editor registration for a document after hydrating the
/// durable Lazily liveness journal. Callers use the PID/editor-id pair as the
/// sole delivery target; no filesystem broadcast channel is implied.
pub fn reliable_sync_editor_registrations_for_file(
    file: &Path,
) -> Vec<agent_doc_reliable_sync_io::liveness::EditorRegistration> {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    let _ = reliable_sync_editor_live_for_file(file);
    agent_doc_reliable_sync_io::global_liveness_plane()
        .lock()
        .projection()
        .live_registrations(&document_hash)
}

/// Resolve CRDT authority from the shared durable reliable-sync liveness plane.
pub fn crdt_authority_for_file(file: &Path) -> CrdtAuthority {
    let routed_model_is_allocated = embedded_relay_route_is_registered_for_file(file)
        && agent_doc_fs::document_state_hash(file)
            .ok()
            .is_some_and(|hash| hub_is_allocated(&hash));
    if routed_model_is_allocated || reliable_sync_editor_live_for_file(file) {
        CrdtAuthority::MultiReplica
    } else {
        CrdtAuthority::GitAuthoritative
    }
}

fn authority_for_file(file: &str) -> CrdtAuthority {
    crdt_authority_for_file(Path::new(file))
}

/// The canonical replica's reserved yrs client-id for every per-document hub. The
/// CP/controller-owned canonical replica is the hub authority; editor replicas
/// mint their own ids via [`mint_client_id`] and can never collide with this
/// reserved id (`RelayHub::register` rejects it).
const CANONICAL_CLIENT_ID: u64 = 1;
const DOCUMENT_MODEL_ENSURE_POLL_MS: u64 = 25;
#[cfg(test)]
const DOCUMENT_MODEL_ENSURE_TIMEOUT_MS: u64 = 150;
#[cfg(not(test))]
const DOCUMENT_MODEL_ENSURE_TIMEOUT_MS: u64 = 5_000;
// `#missingreplicarecycle`: an editor that holds the document but registered no
// CRDT replica gets only this brief window to answer the observation request.
// Kept well under the 750ms
// controller read timeout so a stale/half-synced editor cannot wedge the
// single-threaded controller (a full-timeout wait per read starves every other
// reader). `EditorSyncPending` — a hub that already exists mid-flush — still uses
// the full [`DOCUMENT_MODEL_ENSURE_TIMEOUT_MS`].
#[cfg(test)]
const DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS: u64 = 60;
#[cfg(not(test))]
const DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS: u64 = 400;

/// `#ensurereplicagensup` — whether THIS process serves the relay hub.
///
/// `hub_registry()` is process-local and populated only by the process running
/// the controller, so "the registry has no hub for this document" means two very
/// different things depending on who is asking:
///
/// - the hub owner: the editor is attached but its replica has not registered
///   yet — a real, transient condition worth waiting on and repairing.
/// - anyone else (supervisor, short-lived CLI): the hub simply lives in another
///   process. Waiting cannot help; no editor behaviour can ever populate this
///   registry.
///
/// Document-scoped checks like [`embedded_relay_is_available_for_file`] cannot
/// tell these apart — both see an empty map — which is why guarding on them
/// breaks the legitimate transient case. This is process-scoped and answers the
/// question actually being asked.
static PROCESS_SERVES_RELAY_HUB: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(cfg!(test));

/// Mark this process as the relay-hub owner. Called by the controller when it
/// begins serving; every other process leaves this false.
pub fn mark_process_as_relay_hub_owner() {
    PROCESS_SERVES_RELAY_HUB.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Whether this process serves the relay hub. See [`PROCESS_SERVES_RELAY_HUB`].
pub fn process_serves_relay_hub() -> bool {
    PROCESS_SERVES_RELAY_HUB.load(std::sync::atomic::Ordering::SeqCst)
}

/// Per-document count of editor replica registrations observed in this process.
///
/// `#ensurewindowsize`: `ensure_document_model` gives a missing-replica editor
/// only [`DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS`] to answer, on the
/// assumption that a non-answering editor is stale and must not wedge the
/// single-threaded controller. That assumption breaks for a large document: a
/// live editor answering with a multi-megabyte CRDT bootstrap cannot complete
/// inside that window, so it is judged stale while it is demonstrably alive.
/// A completed registration is positive liveness proof, so the ensure loop
/// watches this counter and extends to the full timeout once it moves.
///
/// LIMITATION — this counter is process-global, so it only closes the gap when
/// the registration and the ensure share a process (the embedded-relay path).
/// On the split CLI/controller path the register lands in the controller while
/// `ensure_document_model` runs in the CLI, so the CLI observes no bump and the
/// window is NOT extended (`window_extended=false` in
/// `document_model_ensure_failed`). Closing that case needs the liveness proof
/// carried across the IPC boundary — or the ensure performed controller-side,
/// where the registration already is. Tracked in `agent:backlog`; do not read a
/// `window_extended=false` line as "no editor answered".
fn replica_registration_counts() -> &'static Mutex<HashMap<String, u64>> {
    static COUNTS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn note_replica_registration(file: &Path) {
    let Ok(key) = agent_doc_fs::document_state_hash(file) else {
        return;
    };
    *replica_registration_counts().lock().entry(key).or_insert(0) += 1;
}

fn replica_registration_count(file: &Path) -> u64 {
    let Ok(key) = agent_doc_fs::document_state_hash(file) else {
        return 0;
    };
    replica_registration_counts()
        .lock()
        .get(&key)
        .copied()
        .unwrap_or(0)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveDocumentPathRekeyReport {
    pub hub_moved: bool,
    pub replica_identities_moved: usize,
    pub embedded_route_moved: bool,
}

fn transition_document_hash(path: &Path) -> Result<String> {
    if path.exists() {
        return agent_doc_fs::document_state_hash(path);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(agent_doc_fs::document_state_hash_from_str(
        &absolute.to_string_lossy(),
    ))
}

/// Move the controller's live CRDT identity to a new path without detaching its
/// existing replicas.
///
/// The controller installs an old-path alias immediately after this operation,
/// so an in-flight request carrying the old path resolves to the moved hub. A
/// destination hub may be discarded only when it has no members and its
/// canonical projection is already durable; two genuinely live heads fail
/// closed instead of being guessed together.
pub fn rekey_live_document_path(
    old_path: &Path,
    new_path: &Path,
) -> Result<LiveDocumentPathRekeyReport> {
    let old_hash = transition_document_hash(old_path)?;
    let new_hash = transition_document_hash(new_path)?;
    if old_hash == new_hash {
        return Ok(LiveDocumentPathRekeyReport::default());
    }

    let (first_hash, second_hash) = if old_hash < new_hash {
        (&old_hash, &new_hash)
    } else {
        (&new_hash, &old_hash)
    };
    let first_lock = replica_registration_lock(first_hash)?;
    let second_lock = replica_registration_lock(second_hash)?;
    let _first = first_lock.lock();
    let _second = second_lock.lock();

    let hub_moved = {
        let mut hubs = hub_registry().lock();
        let old_hub = hubs.get(&old_hash).cloned();
        let destination_hub = hubs.get(&new_hash).cloned();
        if let (Some(old_hub), Some(destination_hub)) = (old_hub.as_ref(), destination_hub.as_ref())
            && !Arc::ptr_eq(old_hub, destination_hub)
        {
            anyhow::ensure!(
                destination_hub.lock().is_safe_to_evict(),
                "cannot move live document relay: destination path already has a live CRDT head"
            );
            hubs.remove(&new_hash);
        }
        match hubs.remove(&old_hash) {
            Some(handle) => {
                hubs.insert(new_hash.clone(), handle);
                true
            }
            None => false,
        }
    };
    retained_canonical_projections().rekey(&old_hash, &new_hash);

    let replica_identities_moved = {
        let mut identities = replica_identity_registry().lock();
        let moved = identities.remove(&old_hash).unwrap_or_default();
        let count = moved.len();
        if !moved.is_empty() {
            identities.insert(new_hash.clone(), moved);
        }
        count
    };

    let embedded_route_moved = {
        let mut routes = embedded_relay_route_registry().lock();
        let moved = routes.remove(&old_hash);
        if moved {
            routes.insert(new_hash.clone());
        }
        moved
    };
    {
        let mut counts = replica_registration_counts().lock();
        if let Some(count) = counts.remove(&old_hash) {
            counts.insert(new_hash.clone(), count);
        }
    }
    Ok(LiveDocumentPathRekeyReport {
        hub_moved,
        replica_identities_moved,
        embedded_route_moved,
    })
}

/// Process-global per-document relay-hub registry, keyed by document hash.
///
/// Per-document isolation (`#xdocsuper1/3`): each document's replicas live in
/// their own hub; there is no shared canonical replica across documents.
///
/// # Why the hub is behind its own lock (`#relayhubperdoclock`)
///
/// The map used to hold `RelayHub` by value, so every caller took **this**
/// process-global lock and held it for the whole operation — a 3.3 MB replica
/// bootstrap, a commit barrier, materializing 128 KB of canonical text, hashing
/// it, and the `log_op` file write. Data was isolated per document; *concurrency*
/// was not isolated at all, so one busy document serialized every other document
/// in the project.
///
/// That is not a theoretical cost. Observed 2026-07-26: a second session looping
/// `crdt_replica_register` with 3.3 MB bootstraps on `tasks/software/lazily.md`
/// held this lock enough of the time that an unrelated document's
/// `crdt_current_text` lost every 5s authority-resolve — nine consecutive
/// closeout attempts failed with `timed out after 5.0s waiting for project
/// controller response`, while `admin inspect` (which never takes this lock)
/// answered instantly. A misbehaving session took the whole controller down for
/// everyone else.
///
/// Holding `Arc<Mutex<RelayHub>>` makes the isolation real: the global lock is
/// held only long enough to look up or insert a handle, and all hub work happens
/// under that document's own lock. Same idiom as
/// [`replica_registration_lock`] below, which already did it this way.
///
/// **Lock ordering: registry → hub, never the reverse.** Nothing may acquire the
/// registry lock while holding a hub lock. Callers should use [`hub_handle`] /
/// [`hub_handle_or_insert_with`], which release the registry lock before
/// returning, rather than locking a hub inline under the registry guard.
type HubHandle = Arc<Mutex<RelayHub>>;

/// Controller-local canonical targets keyed independently of disposable relay
/// membership. Recreating a hub consumes this Lazily projection instead of
/// promoting an editor whole buffer or consulting the legacy CRDT sidecar.
struct RetainedCanonicalProjections {
    ctx: ThreadSafeContext,
    values: ThreadSafeSourceMap<String, RetainedCanonicalProjection>,
}

impl RetainedCanonicalProjections {
    fn new() -> Self {
        let ctx = agent_doc_document_realtime::editor_process_scope()
            .ctx()
            .clone();
        let values = ThreadSafeSourceMap::new(&ctx);
        Self { ctx, values }
    }

    fn observe(&self, document_hash: &str) -> Option<RetainedCanonicalProjection> {
        self.values.observe(&self.ctx, &document_hash.to_string())
    }

    fn retain(&self, document_hash: &str, projection: RetainedCanonicalProjection) {
        self.values
            .set(&self.ctx, document_hash.to_string(), projection);
    }

    fn rekey(&self, old_hash: &str, new_hash: &str) {
        if let Some(projection) = self.observe(old_hash) {
            self.retain(new_hash, projection);
            self.values.remove(&self.ctx, &old_hash.to_string());
        }
    }
}

fn retained_canonical_projections() -> &'static RetainedCanonicalProjections {
    static PROJECTIONS: OnceLock<RetainedCanonicalProjections> = OnceLock::new();
    PROJECTIONS.get_or_init(RetainedCanonicalProjections::new)
}

fn hub_registry() -> &'static Mutex<HashMap<String, HubHandle>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, HubHandle>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The hub handle for `document_hash`, if one is allocated. Releases the registry
/// lock before returning, so the caller's hub work never blocks another document.
fn hub_handle(document_hash: &str) -> Option<HubHandle> {
    hub_registry().lock().get(document_hash).cloned()
}

/// [`hub_handle`], allocating via `make` on first contact.
///
/// `make` runs under the registry lock, so it must stay cheap — allocating an
/// empty or already-constructed hub. Anything expensive (reading the document to
/// seed it, decoding a durable projection) belongs *outside*, with the result
/// handed in; see [`with_hub_seeded_from_file`].
fn hub_handle_or_insert_with(document_hash: &str, make: impl FnOnce() -> RelayHub) -> HubHandle {
    hub_registry()
        .lock()
        .entry(document_hash.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(make())))
        .clone()
}

fn hub_is_allocated(document_hash: &str) -> bool {
    hub_registry().lock().contains_key(document_hash)
}

/// Explicit in-process relay routes used when the controller and model share a
/// process but a replica has not allocated its hub yet (notably missing-replica
/// recovery simulations). This is process state, not a durable authority.
fn embedded_relay_route_registry() -> &'static Mutex<HashSet<String>> {
    static REGISTRY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Process-global identity metadata for editor replicas, keyed by document hash.
///
/// [`RelayHub`] intentionally knows only opaque CRDT client ids. The IO boundary
/// retains the editor identity alongside those ids so a replacement editor can
/// retire memberships left behind by an editor process that crashed or restarted
/// without sending `deregister`. Unrecognized identities remain opaque and are
/// never reaped automatically.
fn replica_identity_registry() -> &'static Mutex<HashMap<String, HashMap<u64, String>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, HashMap<u64, String>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Serialize logical-generation replacement for one document. Identity
/// metadata is the generation fence read by update handlers, while the hub is
/// the membership/canonical store; without this small per-document critical
/// section two concurrent refresh registrations can both observe the old head
/// and install themselves as independent successors.
fn replica_registration_lock(document_hash: &str) -> Result<Arc<Mutex<()>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS.get_or_init(|| Mutex::new(HashMap::new())).lock();
    Ok(locks
        .entry(document_hash.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone())
}

/// Extract the owning process id from identities minted by editor integrations.
/// Both integrations append document/refresh suffixes after their stable
/// `editor-pid-uuid` prefix, so only the decimal field immediately after the
/// integration name is significant here.
fn editor_process_id(identity: &str) -> Option<u32> {
    let rest = ["jetbrains-", "vscode-", "zed-"]
        .into_iter()
        .find_map(|prefix| identity.strip_prefix(prefix))?;
    let pid = rest.split('-').next()?;
    if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    pid.parse().ok()
}

fn editor_route_from_replica_identity(identity: &str) -> Option<ReplicaSignalRoute> {
    let pid = u64::from(editor_process_id(identity)?);
    let editor_id = identity.split_once(':')?.0.trim();
    if editor_id.is_empty() {
        return None;
    }
    Some(ReplicaSignalRoute {
        editor_id: editor_id.to_string(),
        editor_pid: pid,
    })
}

fn live_replica_signal_routes(document_hash: &str) -> Vec<ReplicaSignalRoute> {
    let registry = replica_identity_registry().lock();
    registry
        .get(document_hash)
        .into_iter()
        .flat_map(|members| members.values())
        .filter_map(|identity| editor_route_from_replica_identity(identity))
        .filter(|route| {
            u32::try_from(route.editor_pid)
                .ok()
                .is_some_and(agent_doc_reliable_sync_io::process_pid_is_live)
        })
        .collect()
}

/// Stable logical identity shared by successive native-replica incarnations of
/// one editor document. JetBrains appends `:refresh-N` while swapping a fresh
/// native replica into the same visible editor; that suffix is a generation,
/// not an independent collaborative head.
fn logical_replica_identity(identity: &str) -> &str {
    let Some((base, suffix)) = identity.rsplit_once(":refresh-") else {
        return identity;
    };
    if !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        base
    } else {
        identity
    }
}

fn superseded_logical_replica_ids(
    document_hash: &str,
    identity: &str,
    client_id: u64,
) -> Result<Vec<u64>> {
    let registry = replica_identity_registry().lock();
    let logical = logical_replica_identity(identity);
    Ok(registry
        .get(document_hash)
        .into_iter()
        .flat_map(|members| members.iter())
        .filter_map(|(registered_id, registered_identity)| {
            (*registered_id != client_id
                && logical_replica_identity(registered_identity) == logical)
                .then_some(*registered_id)
        })
        .collect())
}

/// A missing raw client id may auto-heal only when no newer generation of the
/// same logical editor is registered. Otherwise a late update from the retired
/// forwarder would resurrect it as a second head.
fn logical_replica_generation_is_current(
    document_hash: &str,
    identity: &str,
    client_id: u64,
) -> Result<bool> {
    let registry = replica_identity_registry().lock();
    let Some(members) = registry.get(document_hash) else {
        return Ok(true);
    };
    if members
        .get(&client_id)
        .is_some_and(|registered| registered == identity)
    {
        return Ok(true);
    }
    let logical = logical_replica_identity(identity);
    Ok(!members.iter().any(|(registered_id, registered_identity)| {
        *registered_id != client_id && logical_replica_identity(registered_identity) == logical
    }))
}

fn dead_editor_replica_ids(
    document_hash: &str,
    is_pid_live: impl Fn(u32) -> bool,
) -> Result<Vec<u64>> {
    let registry = replica_identity_registry().lock();
    Ok(registry
        .get(document_hash)
        .into_iter()
        .flat_map(|members| members.iter())
        .filter_map(|(client_id, identity)| {
            editor_process_id(identity)
                .filter(|pid| !is_pid_live(*pid))
                .map(|_| *client_id)
        })
        .collect())
}

fn record_replica_identity(
    document_hash: &str,
    client_id: u64,
    identity: &str,
    retired: &[u64],
) -> Result<()> {
    let mut registry = replica_identity_registry().lock();
    let members = registry.entry(document_hash.to_string()).or_default();
    for retired_id in retired {
        members.remove(retired_id);
    }
    members.insert(client_id, identity.to_string());
    Ok(())
}

fn forget_replica_identity(document_hash: &str, client_id: u64) -> Result<()> {
    let mut registry = replica_identity_registry().lock();
    if let Some(members) = registry.get_mut(document_hash) {
        members.remove(&client_id);
        if members.is_empty() {
            registry.remove(document_hash);
        }
    }
    Ok(())
}

fn replica_identity_registry_has_editor_pid(document_hash: &str, editor_pid: u32) -> bool {
    replica_identity_registry()
        .lock()
        .get(document_hash)
        .into_iter()
        .flat_map(|members| members.values())
        .any(|identity| editor_process_id(identity) == Some(editor_pid))
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
    let retained = retained_canonical_projections().observe(&hash);
    let retained_hub = retained
        .as_ref()
        .map(|projection| {
            RelayHub::from_retained_canonical_projection(CANONICAL_CLIENT_ID, projection)
        })
        .transpose()?;
    let handle = hub_handle_or_insert_with(&hash, || {
        retained_hub.unwrap_or_else(|| RelayHub::new(CANONICAL_CLIENT_ID))
    });
    let mut hub = handle.lock();
    let result = f(&mut hub);
    retained_canonical_projections().retain(&hash, hub.retained_canonical_projection());
    Ok(result)
}

/// Run `f` against an already-allocated per-document hub. Unlike
/// [`with_hub_seeded_from_file`], this never creates a hub from disk: callers use
/// it when disk is a recovery projection and an absent hub means the live model is
/// not available.
fn with_existing_hub<T>(file: &Path, f: impl FnOnce(&mut RelayHub) -> T) -> Result<Option<T>> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    let Some(handle) = hub_handle(&hash) else {
        return Ok(None);
    };
    let mut hub = handle.lock();
    let result = f(&mut hub);
    retained_canonical_projections().retain(&hash, hub.retained_canonical_projection());
    Ok(Some(result))
}

/// Drop an inactive hub only after its canonical text is known to be durable.
///
/// Callers hold the per-document replica-registration lock so a concurrent
/// registration cannot add a member between the predicate and removal.
fn evict_hub_if_safe(file: &Path, document_hash: &str, source: &str) -> bool {
    let evicted = {
        // Registry → hub ordering (`#relayhubperdoclock`). Taking the hub lock
        // under the registry guard is the one place that is allowed, because
        // `is_safe_to_evict` is a cheap predicate and the check must stay atomic
        // with the removal. Nothing here materializes text or drives a barrier.
        let mut registry = hub_registry().lock();
        let should_evict = registry
            .get(document_hash)
            .is_some_and(|handle| handle.lock().is_safe_to_evict());
        if should_evict {
            registry.remove(document_hash);
        }
        should_evict
    };
    if evicted {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_relay_hub_evicted file={} doc_hash={} source={} reason=no_members_and_canonical_committed",
                file.display(),
                document_hash,
                source,
            ),
        );
    }
    evicted
}

/// Whether this process already owns an embedded Lazily relay for `file`.
///
/// This is the in-process authority seam used by controller-hosted operation and
/// simulation tests. It deliberately does not consult a filesystem marker: an
/// allocated relay is the fact, so no second hot-path source of truth can drift.
pub fn embedded_relay_is_available_for_file(file: &Path) -> bool {
    let Ok(hash) = agent_doc_fs::document_state_hash(file) else {
        return false;
    };
    hub_is_allocated(&hash) || embedded_relay_route_registry().lock().contains(&hash)
}

/// Whether `file` is explicitly routed to the relay in this process.
pub fn embedded_relay_route_is_registered_for_file(file: &Path) -> bool {
    let Ok(hash) = agent_doc_fs::document_state_hash(file) else {
        return false;
    };
    embedded_relay_route_registry().lock().contains(&hash)
}

/// Route controller/model reads for `file` through this process without
/// manufacturing a relay hub. Used by deterministic missing-replica tests.
pub fn register_embedded_relay_route_for_file(file: &Path) -> Result<()> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    embedded_relay_route_registry().lock().insert(hash);
    Ok(())
}

/// [`with_hub`] for live file-backed authority paths. A newly allocated hub must
/// start from the current document text, not an empty CRDT, or the first editor
/// delta can be applied at a clamped offset and later overwrite the buffer.
fn with_hub_seeded_from_file<T>(file: &Path, f: impl FnOnce(&mut RelayHub) -> T) -> Result<T> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    if let Some(handle) = hub_handle(&hash) {
        let mut hub = handle.lock();
        let result = f(&mut hub);
        retained_canonical_projections().retain(&hash, hub.retained_canonical_projection());
        return Ok(result);
    }
    if let Some(projection) = retained_canonical_projections().observe(&hash) {
        let recovered =
            RelayHub::from_retained_canonical_projection(CANONICAL_CLIENT_ID, &projection)?;
        let handle = hub_handle_or_insert_with(&hash, || recovered);
        let mut hub = handle.lock();
        let result = f(&mut hub);
        retained_canonical_projections().retain(&hash, hub.retained_canonical_projection());
        return Ok(result);
    }
    // Seeding reads the whole document, so it happens with no registry lock
    // held; `hub_handle_or_insert_with` only installs the finished hub, and a
    // racing allocator's hub wins (`or_insert_with` keeps the first).
    let seed_text = std::fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("failed to seed relay hub from {}: {e}", file.display()))?;
    let handle = hub_handle_or_insert_with(&hash, || {
        RelayHub::from_text(CANONICAL_CLIENT_ID, &seed_text)
    });
    let mut hub = handle.lock();
    let result = f(&mut hub);
    retained_canonical_projections().retain(&hash, hub.retained_canonical_projection());
    Ok(result)
}

/// Allocate the embedded Lazily relay from the current document projection.
///
/// Production callers normally allocate through replica registration. Tests and
/// in-process simulations may use this explicit seam instead of creating a
/// filesystem flag that can outlive or disagree with the relay itself.
pub fn seed_embedded_relay_for_file(file: &Path) -> Result<()> {
    register_embedded_relay_route_for_file(file)?;
    with_hub_seeded_from_file(file, |_| ())
}

/// Whether a relay hub has been allocated for `doc_hash` (test-only assertion
/// helper, e.g. proving the Detached path allocates no hub).
pub fn hub_is_allocated_for_test(doc_hash: &str) -> bool {
    hub_is_allocated(doc_hash)
}

/// Optional semantic facts derived from the live per-node projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentDocumentSemantics {
    /// Unresolved prompt lines across the whole live document.
    pub unresolved_prompts: usize,
    /// Unresolved prompt lines in the first queue occurrence.
    pub queue_unresolved_prompts: usize,
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
        /// Monotonic cursor for the member/liveness/pending-delivery inputs
        /// represented by `delivery_converged`.
        delivery_version: u64,
        /// Opt-in memoized semantics from the live per-node projection. `None`
        /// preserves the default-off/fallback behavior.
        semantics: Option<CurrentDocumentSemantics>,
    },
}

/// Compact live-document revision resolved from the CRDT relay authority.
///
/// The idle supervisor compares this value before asking the relay to
/// materialize the canonical markdown. It therefore keeps full-text queue
/// parsing lazy while still observing editor attachment, replica liveness, and
/// convergence changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CurrentRevision {
    /// No live editor owns the document; callers may observe disk metadata.
    Detached,
    /// A live editor owns the document, but no relay replica has registered.
    EditorAttachedMissingReplica,
    /// The compact authoritative canonical frontier and related readiness state.
    Current {
        state_vector: Vec<u8>,
        live_editors: usize,
        delivery_converged: bool,
    },
}

/// Return a compact revision for the current live CRDT authority without
/// materializing canonical text or driving a commit barrier.
pub fn current_revision_for_file_with_authority(
    file: &Path,
    authority: CrdtAuthority,
) -> Result<CurrentRevision> {
    if !authority.editor_attached() {
        return Ok(CurrentRevision::Detached);
    }

    let hash = agent_doc_fs::document_state_hash(file)?;
    let Some(handle) = hub_handle(&hash) else {
        return Ok(CurrentRevision::EditorAttachedMissingReplica);
    };
    let hub = handle.lock();

    Ok(CurrentRevision::Current {
        state_vector: hub.canonical_state_vector(),
        live_editors: hub.live_count(),
        delivery_converged: hub.delivery_converged(),
    })
}

/// [`current_revision_for_file_with_authority`] resolving authority itself.
///
/// The revision counterpart to [`current_text_for_file_nonblocking`], for status
/// and observation callers that need `live_editors` / `delivery_converged` and
/// nothing else. Reaching for the text entry point and discarding the body with
/// `..` is the recurring shape behind the idle-watch read storm
/// (`#idlewatchtransitionrevision`): it materializes the whole document,
/// SHA-256s it, and writes an `ops.log` line to answer a question the compact
/// revision already carries.
pub fn current_revision_for_file(file: &Path) -> Result<CurrentRevision> {
    let authority = authority_for_file(&file.display().to_string());
    current_revision_for_file_with_authority(file, authority)
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
/// This is for latency-sensitive observation paths that need a cheap CP state
/// proof. If a hub exists but is not already a consistent cut, it reports
/// [`CurrentText::EditorSyncPending`] instead of driving the commit barrier.
pub fn current_text_for_file_with_authority_nonblocking(
    file: &Path,
    authority: CrdtAuthority,
) -> Result<CurrentText> {
    current_text_for_file_with_authority_inner(file, authority, false, false)
}

/// Resolve current text after a Lazily-current observation request has had a
/// bounded chance to restore the live relay model.
///
/// While an editor owns the document, durable `.yrs` state is never promoted
/// into a missing live hub: it can predate unsaved operator edits. The editor
/// must observe its exact current value or this remains unavailable. Durable
/// projection recovery is reserved for detached authority.
pub fn current_text_for_file_with_authority_recovering_projection(
    file: &Path,
    authority: CrdtAuthority,
) -> Result<CurrentText> {
    current_text_for_file_with_authority_inner(file, authority, false, true)
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
    let mut handle = hub_handle(&hash);
    if handle.is_none() && recover_missing_from_projection {
        recover_missing_hub_from_retained_projection(file, &hash)?;
        handle = hub_handle(&hash);
    }
    // `#relayhubperdoclock`: everything below — the commit barrier, materializing
    // canonical text, hashing it, the `log_op` write — runs under THIS document's
    // lock, never the registry's. Holding the process-global registry lock across
    // this block is what let one busy document time out every other document's
    // 5s authority resolve.
    let Some(handle) = handle else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{} file={} authority=multi_replica reason=missing_replica doc_hash={} process_pid={}",
                OpsLogEvent::CrdtCurrentTextUnavailable,
                file.display(),
                hash,
                std::process::id(),
            ),
        );
        return Ok(CurrentText::EditorAttachedMissingReplica);
    };
    let mut hub = handle.lock();
    let hub = &mut *hub;

    let ready = if flush_barrier {
        hub.commit_barrier_under_authority(authority)?
    } else {
        hub.commit_barrier_ready()?
    };
    // `#lazily-hot-path` Theme A — carry the convergence *version* alongside the
    // boolean. `delivery_converged=false` repeated across a wedge cannot distinguish
    // "deliveries are churning and never settling" from "nothing has moved at all";
    // a frozen `delivery_version` says the latter outright, which is the difference
    // between suspecting the editor and suspecting the relay.
    let delivery = hub.delivery_convergence_witness();
    let delivery_converged = delivery.converged;
    if !ready {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{} file={} authority=multi_replica reason=sync_pending live_editors={} delivery_converged={} delivery_version={}",
                OpsLogEvent::CrdtCurrentTextUnavailable,
                file.display(),
                hub.live_count(),
                delivery_converged,
                delivery.version,
            ),
        );
        return Ok(CurrentText::EditorSyncPending);
    }

    let text = hub.canonical_text();
    let live_editors = hub.live_count();
    let semantics =
        hub.unresolved_prompt_counts()
            .map(
                |(unresolved_prompts, queue_unresolved_prompts)| CurrentDocumentSemantics {
                    unresolved_prompts,
                    queue_unresolved_prompts,
                },
            );
    // `process_pid` mirrors the `crdt_current_text_unavailable` sibling above.
    // Without it this line says a full-document read happened but not *who*
    // asked: the relay-side log carries no `source=` (only the controller RPC
    // handler adds one), so an in-process caller is anonymous. Chasing a 10s
    // read triple on 2026-07-26 cost two wrong hypotheses for exactly this
    // reason — the pid alone separates controller from supervisor from the
    // editor's cdylib and would have answered it immediately.
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_current_text file={} authority=multi_replica len={} hash={} live_editors={} delivery_converged={} delivery_version={} process_pid={}",
            file.display(),
            text.len(),
            agent_doc_hash::content_hash(&text),
            live_editors,
            delivery_converged,
            delivery.version,
            std::process::id(),
        ),
    );
    Ok(CurrentText::Current {
        text,
        live_editors,
        delivery_converged,
        delivery_version: delivery.version,
        semantics,
    })
}

fn recover_missing_hub_from_retained_projection(file: &Path, hash: &str) -> Result<bool> {
    let Some(projection) = retained_canonical_projections().observe(hash) else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_current_text_projection_unavailable file={} authority=multi_replica doc_hash={} recovery=await_controller_canonical_projection",
                file.display(),
                hash,
            ),
        );
        return Ok(false);
    };
    let hub = RelayHub::from_retained_canonical_projection(CANONICAL_CLIENT_ID, &projection)?;
    hub_handle_or_insert_with(hash, || hub);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_current_text_projection_restored file={} authority=multi_replica doc_hash={} recovery=retained_lazily_projection",
            file.display(),
            hash,
        ),
    );
    Ok(true)
}

/// Ensure the live document model is usable before a hot-path read gives up on
/// editor authority.
///
/// This is intentionally narrower than the commit barrier: it does not treat
/// markdown or filesystem sidecars as authoritative. When the editor owns the
/// document but Lazily current is missing or not converged, it observes the
/// retained projection and live relay until either reactive source reaches a
/// usable fixed point. No editor request or registration command is emitted.
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
/// This keeps the single bounded observe/retry transition in the relay crate while
/// allowing controller clients to request an editor observation outside the
/// controller RPC handler and then poll CP-owned relay state through the
/// controller. The observer must read relay state only; it must not treat disk or
/// filesystem sidecars as fallback authority while an editor is attached.
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
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{} file={} source={} initial_state={}",
            OpsLogEvent::DocumentModelEnsureStart,
            file.display(),
            source,
            first_label,
        ),
    );
    if let Some(observer) = observe_recovery_current_text.as_mut() {
        match observer() {
            Ok(current @ (CurrentText::Detached | CurrentText::Current { .. })) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "document_model_ensure_ready file={} source={} initial_state={} final_state={} recovery=retained_lazily_projection",
                        file.display(),
                        source,
                        first_label,
                        current_text_label(&current),
                    ),
                );
                return Ok(current);
            }
            Ok(CurrentText::EditorAttachedMissingReplica | CurrentText::EditorSyncPending) => {}
            Err(error) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "document_model_ensure_projection_observer_deferred file={} source={} initial_state={} reason={error:#}",
                        file.display(),
                        source,
                        first_label,
                    ),
                );
            }
        }
    }
    let ensure_timeout_ms = if matches!(first, CurrentText::EditorAttachedMissingReplica) {
        DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS
    } else {
        DOCUMENT_MODEL_ENSURE_TIMEOUT_MS
    };

    // Bound how long we wait for the observed projection to advance. A persistent missing
    // replica must fail closed: neither disk nor a durable restart projection can
    // prove the current unsaved editor cut. `EditorSyncPending` keeps the full
    // window because a hub already exists with un-flushed ops worth waiting for.
    let started_at = std::time::Instant::now();
    let mut deadline = started_at + std::time::Duration::from_millis(ensure_timeout_ms);
    // `#ensurewindowsize`: a registration completing during this window is proof
    // the editor is alive and answering, which is exactly what the short
    // missing-replica timeout was guessing about. A large document's bootstrap
    // (observed at 1.3MB on a real session) cannot register inside 400ms, so
    // without this the live editor is judged stale on every single attempt and
    // queue maintenance can never persist. Extend once, to the full timeout; an
    // editor that never registers still fails closed on the short window, so the
    // single-threaded controller keeps its anti-starvation guarantee.
    let registrations_at_start = replica_registration_count(file);
    let mut extended_for_registration = false;
    let mut last_label = first_label;
    let mut last_observer_error: Option<String> = None;
    loop {
        if !extended_for_registration
            && ensure_timeout_ms < DOCUMENT_MODEL_ENSURE_TIMEOUT_MS
            && replica_registration_count(file) > registrations_at_start
        {
            extended_for_registration = true;
            deadline =
                started_at + std::time::Duration::from_millis(DOCUMENT_MODEL_ENSURE_TIMEOUT_MS);
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_model_ensure_window_extended file={} source={} initial_state={} reason=replica_registered_during_window from_ms={} to_ms={}",
                    file.display(),
                    source,
                    first_label,
                    ensure_timeout_ms,
                    DOCUMENT_MODEL_ENSURE_TIMEOUT_MS,
                ),
            );
        }
        if std::time::Instant::now() >= deadline {
            if let Some(observer) = observe_recovery_current_text.as_mut() {
                match observer() {
                    Ok(current @ (CurrentText::Detached | CurrentText::Current { .. })) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "document_model_ensure_ready file={} source={} initial_state={} final_state={} recovery=live_editor_republished_after_timeout",
                                file.display(),
                                source,
                                first_label,
                                current_text_label(&current),
                            ),
                        );
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
                                "document_model_ensure_republish_observer_not_ready file={} source={} initial_state={} final_state={} recovery=retry_without_disk_write",
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
                                "document_model_ensure_republish_observer_error file={} source={} initial_state={} last_state={} error={} recovery=retry_without_disk_write",
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
                    // `#ensurewindowsize`: log the deadline actually granted, not
                    // the long-form constant. This previously always printed 5000
                    // while a missing-replica ensure really got 400ms, which sent
                    // diagnosis after a phantom 5s stall for a long time.
                    "{} file={} source={} initial_state={} final_state={} timeout_ms={} window_extended={} last_observer_error={} recovery=retry_without_disk_write",
                    OpsLogEvent::DocumentModelEnsureFailed,
                    file.display(),
                    source,
                    first_label,
                    last_label,
                    if extended_for_registration {
                        DOCUMENT_MODEL_ENSURE_TIMEOUT_MS
                    } else {
                        ensure_timeout_ms
                    },
                    extended_for_registration,
                    last_observer_error.as_deref().unwrap_or("none"),
                ),
            );
            anyhow::bail!(
                "document model startup/reconciliation failed for {}: editor authority stayed in {last_label} after a bounded Lazily-current observation request; disk remained non-authoritative and was not read as a fallback; last_observer_error={}; recovery=retry_without_disk_write; binary-owned replica re-registration and retained-intent replay continue asynchronously; operator_action=none",
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
                return Ok(current);
            }
            CurrentText::EditorAttachedMissingReplica | CurrentText::EditorSyncPending => {
                last_label = current_text_label(&current);
            }
        }
    }
}

fn current_text_label(current: &CurrentText) -> &'static str {
    match current {
        CurrentText::Detached => "detached",
        CurrentText::EditorAttachedMissingReplica => "editor_attached_model_missing",
        CurrentText::EditorSyncPending => "editor_sync_pending",
        CurrentText::Current { .. } => "current",
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

/// Result of a CP-authored CRDT write into the controller-owned canonical
/// replica. Disk materialization may use this result as proof that the document
/// file is a projection of the relay, not a separate editor-authoritative path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpRelayWrite {
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

/// Registration payload returned to an editor replica.
///
/// `incremental` means `bootstrap` is a CRDT delta relative to the state vector
/// supplied by the editor. Otherwise it is the complete canonical encoded state.
/// The canonical frontier accompanies both forms so a resumed editor can publish
/// any local suffix the controller did not yet observe.
#[derive(Debug, Clone)]
pub struct ReplicaRegistration {
    pub client_id: u64,
    pub bootstrap: Vec<u8>,
    pub canonical_state_vector: Vec<u8>,
    pub incremental: bool,
    /// The controller still requires an exact editor-visible receipt for its
    /// canonical projection. A replacement editor must not publish its stale
    /// whole buffer over this bootstrap.
    pub canonical_projection_retained: bool,
    pub canonical_content_hash: String,
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

/// Seed the process-exit watcher projection from reliable-sync open pids.
/// Durable hydration runs first, so a controller recycle needs no filesystem lease.
fn mark_editor_attach_open(file: &Path) {
    let doc = file.display().to_string();
    let _ = reliable_sync_editor_live_for_file(file);
    let document_hash = agent_doc_hash::document_id_for_path(file);
    let pids = {
        let plane = agent_doc_reliable_sync_io::global_liveness_plane().lock();
        let projection = plane.projection();
        projection
            .open_pids(&document_hash)
            .into_iter()
            .filter(|pid| projection.pid_alive(*pid))
            .filter_map(|pid| u32::try_from(pid).ok())
            .collect::<Vec<_>>()
    };
    for pid in pids {
        agent_doc_document_realtime::editor_attach::editor_attach().attach(&doc, pid);
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

pub fn register_replica_for_file(file: &Path, identity: &str) -> Result<Option<(u64, Vec<u8>)>> {
    register_replica_for_file_incremental(file, identity, None).map(|registration| {
        registration.map(|registration| (registration.client_id, registration.bootstrap))
    })
}

/// Register an editor, returning a state-vector delta when the editor retained
/// its prior encoded replica state across a controller/native-generation handoff.
///
/// An absent or invalid frontier falls back to the full canonical bootstrap.
/// That compatibility fallback is deliberate: an older/corrupt retained hint
/// must not make registration unavailable or weaken the canonical authority.
pub fn register_replica_for_file_incremental(
    file: &Path,
    identity: &str,
    retained_state_vector: Option<&[u8]>,
) -> Result<Option<ReplicaRegistration>> {
    register_replica_for_file_incremental_with_liveness(
        file,
        identity,
        retained_state_vector,
        None,
        agent_doc_reliable_sync_io::process_pid_is_live,
    )
}

/// Register an explicit editor-sidecar replica and seed its crash-safe reactive
/// authority before entering the normal authority-gated registration path.
///
/// Existing native plugins publish liveness before `replica_register`. An LSP
/// sidecar has no in-process FFI host, so its `didOpen` carries the sidecar PID
/// and the controller installs the same process-exit watch directly.
pub fn register_editor_replica_for_file_incremental(
    file: &Path,
    identity: &str,
    retained_state_vector: Option<&[u8]>,
    editor_pid: u32,
) -> Result<Option<ReplicaRegistration>> {
    let doc = file.display().to_string();
    agent_doc_document_realtime::editor_open_docs::editor_open_docs().mark_open(&doc, true);
    agent_doc_document_realtime::editor_attach::editor_attach().attach(&doc, editor_pid);
    match register_replica_for_file_incremental_with_liveness(
        file,
        identity,
        retained_state_vector,
        Some(editor_pid),
        agent_doc_reliable_sync_io::process_pid_is_live,
    ) {
        Ok(Some(registration)) => Ok(Some(registration)),
        Ok(None) => {
            agent_doc_document_realtime::editor_attach::editor_attach()
                .detach_pid(&doc, editor_pid);
            Ok(None)
        }
        Err(error) => {
            agent_doc_document_realtime::editor_attach::editor_attach()
                .detach_pid(&doc, editor_pid);
            Err(error)
        }
    }
}

#[cfg(test)]
fn register_replica_for_file_with_liveness(
    file: &Path,
    identity: &str,
    is_pid_live: impl Fn(u32) -> bool,
) -> Result<Option<(u64, Vec<u8>)>> {
    register_replica_for_file_incremental_with_liveness(file, identity, None, None, is_pid_live)
        .map(|registration| {
            registration.map(|registration| (registration.client_id, registration.bootstrap))
        })
}

fn register_replica_for_file_incremental_with_liveness(
    file: &Path,
    identity: &str,
    retained_state_vector: Option<&[u8]>,
    registering_editor_pid: Option<u32>,
    is_pid_live: impl Fn(u32) -> bool,
) -> Result<Option<ReplicaRegistration>> {
    let authority = authority_for_file(&file.display().to_string());
    // `replica_register` is itself a process-scoped proof that an editor has
    // this document open. Do not require the separately-pushed reliable-sync
    // `Open` fact to win a scheduling race first: JetBrains/VS Code can publish
    // that fact and the CRDT registration on different pooled workers, and a
    // nested project used to route the two messages to different controllers.
    //
    // Generic/headless replicas retain the old fail-closed authority gate.
    // An editor registration may allocate the initial hub only while its
    // claimed PID is live; after allocation, the routed hub itself keeps
    // authority MultiReplica until normal deregistration.
    let live_registering_editor = registering_editor_pid.is_some_and(&is_pid_live);
    if !authority.editor_attached() && !live_registering_editor {
        return Ok(None);
    }
    // Registration is the routed controller command that makes this process the
    // document's relay owner. Publish that route before allocating the hub so
    // authority readers cannot observe a live editor member in an unrouted hub
    // and fall back to disk between these two steps.
    register_embedded_relay_route_for_file(file)?;
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    let registration_lock = replica_registration_lock(&document_hash)?;
    let _registration_guard = registration_lock.lock();
    let client_id = mint_client_id(identity);
    // Gather under the metadata lock, then release it before taking the hub
    // lock. This lock order is deliberate: registration and deregistration can
    // never deadlock each other by holding both registries at once.
    let dead_client_ids = dead_editor_replica_ids(&document_hash, &is_pid_live)?;
    let superseded_client_ids =
        superseded_logical_replica_ids(&document_hash, identity, client_id)?;
    let mut retired_client_ids = dead_client_ids.clone();
    retired_client_ids.extend(superseded_client_ids.iter().copied());
    retired_client_ids.sort_unstable();
    retired_client_ids.dedup();
    // Publish the new logical generation before changing hub membership. From
    // this point a late frame from the retired forwarder is fenced. A new-
    // generation update racing the remainder of registration can safely exercise
    // the existing idempotent reattach path against the same client id.
    record_replica_identity(&document_hash, client_id, identity, &retired_client_ids)?;
    let (
        bootstrap,
        canonical_state_vector,
        incremental,
        canonical_projection_retained,
        canonical_content_hash,
    ) = with_hub_seeded_from_file(file, |hub| {
        // Registration/reconnect removes the retiring member and its queue.
        // Preserve that unsettled visible-write obligation under the new
        // identity, and force a full canonical bootstrap so a stale retained
        // native frontier cannot union-merge over it.
        let canonical_projection_retained = (!hub.controller_projection_established()
            && retained_state_vector.is_some())
            || !hub.delivery_converged();
        let effective_retained_state_vector = if canonical_projection_retained {
            None
        } else {
            retained_state_vector
        };
        for retired_client_id in &retired_client_ids {
            hub.deregister(*retired_client_id);
        }
        if !superseded_client_ids.is_empty() {
            hub.fence_replica_generation();
        }
        if hub.is_registered(client_id) {
            // Idempotent re-register (e.g. an editor reconnect that re-announces
            // the same stable identity): reconnect/sync the existing mirror, then
            // derive the response from the current canonical frontier.
            hub.reconnect(client_id)?;
        } else {
            hub.register(client_id)?;
        }
        let canonical_state_vector = hub.canonical_state_vector();
        let (bootstrap, incremental) = match effective_retained_state_vector {
            Some(state_vector) => match hub.canonical_covers_state_vector(state_vector) {
                Ok(true) => match hub.canonical_diff(state_vector) {
                    Ok(delta) => (delta, true),
                    Err(error) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "crdt_replica_register_incremental_fallback file={} client_id={} reason=invalid_state_vector error={error}",
                                file.display(),
                                client_id,
                            ),
                        );
                        (hub.canonical_encoded_state(), false)
                    }
                },
                Ok(false) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "crdt_replica_register_incremental_fallback file={} client_id={} reason=retained_frontier_ahead_of_canonical",
                            file.display(),
                            client_id,
                        ),
                    );
                    (hub.canonical_encoded_state(), false)
                }
                Err(error) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "crdt_replica_register_incremental_fallback file={} client_id={} reason=invalid_state_vector error={error}",
                            file.display(),
                            client_id,
                        ),
                    );
                    (hub.canonical_encoded_state(), false)
                }
            },
            None => (hub.canonical_encoded_state(), false),
        };
        // Registration consumes the controller canonical bootstrap. A stale
        // editor baseline is a projection-consumer fault, never authority for a
        // whole-document adopt.
        hub.establish_controller_projection();
        if canonical_projection_retained {
            hub.ensure_canonical_projection_receipt(client_id)?;
        }
        let canonical_content_hash = agent_doc_hash::content_hash(&hub.canonical_text());
        Ok::<_, anyhow::Error>((
            bootstrap,
            canonical_state_vector,
            incremental,
            canonical_projection_retained,
            canonical_content_hash,
        ))
    })??;
    // Editor attach is an explicit event → drive the reactive open-docs authority.
    mark_editor_open_docs_open(file);
    // Seed the legacy process-exit watcher from durable reliable-sync open pids.
    mark_editor_attach_open(file);
    // `#ensurewindowsize`: record that a live editor answered for this document.
    // `ensure_document_model` uses this as liveness proof to extend its otherwise
    // very short missing-replica window — see `note_replica_registration`.
    note_replica_registration(file);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_replica_register file={} authority=multi_replica client_id={} bootstrap_bytes={} bootstrap_kind={} canonical_state_vector_bytes={} dead_members_pruned={} superseded_generations_pruned={} generation_fenced={}",
            file.display(),
            client_id,
            bootstrap.len(),
            if incremental { "delta" } else { "full" },
            canonical_state_vector.len(),
            dead_client_ids.len(),
            superseded_client_ids.len(),
            !superseded_client_ids.is_empty(),
        ),
    );
    Ok(Some(ReplicaRegistration {
        client_id,
        bootstrap,
        canonical_state_vector,
        incremental,
        canonical_projection_retained,
        canonical_content_hash,
    }))
}

/// Queue an exact-hash canonical projection receipt for an already registered
/// editor identity.
///
/// The controller uses this after consulting the durable retained-write
/// projection. That durable state can outlive every relay member, so relay
/// delivery convergence alone cannot reveal the obligation during a later IDE
/// restart.
pub fn ensure_canonical_projection_receipt_for_file(
    file: &Path,
    identity: &str,
) -> Result<Option<bool>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = mint_client_id(identity);
    let Some(queued) = with_existing_hub(file, |hub| {
        hub.ensure_canonical_projection_receipt(client_id)
    })?
    else {
        return Ok(None);
    };
    let queued = queued?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_canonical_projection_receipt file={} client_id={} queued={}",
            file.display(),
            client_id,
            queued,
        ),
    );
    Ok(Some(queued))
}

/// Deregister one editor replica from the document's hub on the live IPC path.
/// Document-open authority is owned independently by reliable-sync liveness;
/// membership replacement and connection churn are not document-close events.
/// Authority-gated like
/// [`register_replica_for_file`]: `Ok(false)` (no hub touched) under Detached;
/// `Ok(true)` when a live-attached hub dropped the mirror.
fn deregister_replica_for_file_locked(
    file: &Path,
    identity: &str,
    document_hash: &str,
) -> Result<bool> {
    let client_id = mint_client_id(identity);
    // A duplicate/late deregistration after eviction must not recreate the hub.
    let removed = with_existing_hub(file, |hub| hub.deregister(client_id))?.unwrap_or(false);
    forget_replica_identity(document_hash, client_id)?;
    let hub_evicted = evict_hub_if_safe(file, document_hash, "replica_deregister");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_replica_deregister file={} authority=multi_replica client_id={} removed={} hub_evicted={}",
            file.display(),
            client_id,
            removed,
            hub_evicted,
        ),
    );
    Ok(removed)
}

pub fn deregister_replica_for_file(file: &Path, identity: &str) -> Result<bool> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(false);
    }
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    let registration_lock = replica_registration_lock(&document_hash)?;
    let _registration_guard = registration_lock.lock();
    deregister_replica_for_file_locked(file, identity, &document_hash)
}

/// Deregister one editor sidecar without clearing another editor process's
/// attachment for the same document or a newer logical generation from the
/// same process.
pub fn deregister_editor_replica_for_file(
    file: &Path,
    identity: &str,
    editor_pid: u32,
) -> Result<bool> {
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    let registration_lock = replica_registration_lock(&document_hash)?;
    let _registration_guard = registration_lock.lock();
    let removed = if authority_for_file(&file.display().to_string()).editor_attached() {
        deregister_replica_for_file_locked(file, identity, &document_hash)?
    } else {
        false
    };
    let doc = file.display().to_string();
    let attach = agent_doc_document_realtime::editor_attach::editor_attach();
    // JetBrains refreshes a document by registering `:refresh-N`, atomically
    // swapping the forwarder, then deregistering the retired identity. Both
    // generations share one editor PID. Detaching that PID merely because the
    // old identity went away clears the freshly registered Lazily Source cell
    // and starts an endless missing-replica/re-register loop. Keep the
    // registration lock across this decision and Source mutation so a later
    // generation either already protects the PID or attaches after this close.
    if !replica_identity_registry_has_editor_pid(&document_hash, editor_pid) {
        attach.detach_pid(&doc, editor_pid);
    }
    if !attach.is_attached(&doc) {
        agent_doc_document_realtime::editor_open_docs::editor_open_docs().mark_closed(&doc);
    }
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
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    if !logical_replica_generation_is_current(&document_hash, identity, client_id)? {
        let canonical_len =
            with_hub_seeded_from_file(file, |hub| hub.canonical_text().chars().count())?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_stale_generation_ignored file={} authority=multi_replica client_id={} logical_identity={} canonical_len={} recovery=fence_retired_forwarder",
                file.display(),
                client_id,
                logical_replica_identity(identity),
                canonical_len,
            ),
        );
        return Ok(Some(FanOut {
            origin: client_id,
            update: Vec::new(),
            targets: Vec::new(),
            canonical_len,
        }));
    }
    // A replacement controller may receive an incremental update before the
    // editor's registration event reaches the reactive relay projection.
    // Reattach the live member but quarantine that unproven update. The normal
    // controller-to-editor canonical projection repairs the consumer; the stale
    // editor baseline is never adopted as whole-document authority.
    let (packet, reattached, canonical_projection_pending, corruption_restored) =
        with_hub_seeded_from_file(file, |hub| -> Result<_> {
            let registered = hub.is_registered(client_id);
            let decision = decide_cold_start_replica_update(
                registered,
                hub.controller_projection_established(),
                hub.awaits_canonical_projection(client_id),
            );
            // #replica-structure-guard: capture the clean pre-update canonical so a
            // connected editor pushing a stale/truncated buffer (one whose merged
            // result would structurally corrupt the canonical — e.g. tombstoning a
            // component close marker) can be rejected and the canonical restored
            // before the corruption ever becomes authoritative.
            let before_text = hub.canonical_text();
            let (packet, reattached, canonical_projection_pending) = match decision {
                ColdStartReplicaUpdateDecision::Relay => {
                    (Some(hub.relay_update(client_id, update)?), false, false)
                }
                ColdStartReplicaUpdateDecision::ReprojectCanonical => {
                    if !registered {
                        hub.register(client_id)?;
                    }
                    hub.establish_controller_projection();
                    hub.ensure_canonical_projection_receipt(client_id)?;
                    (None, !registered, true)
                }
            };
            let mut corruption_restored = None;
            if packet.is_some() {
                let after_text = hub.canonical_text();
                // Narrow to component *parse* failures (unclosed / mismatched /
                // unmatched markers) — the structural break a stale or truncated
                // editor buffer introduces and that no later normalization can
                // repair. Duplicate boundaries and duplicate singletons are
                // deliberately excluded: those have dedicated repair paths
                // (preflight boundary dedup, response-cell singleton repair) and
                // can be a legitimate transient canonical state during closeout.
                let introduced_parse_failure = matches!(
                    agent_doc_element::element::structural_corruption_reason(&after_text),
                    Some(reason) if reason.starts_with("parse_error:")
                )
                    && agent_doc_element::element::structural_corruption_reason(&before_text)
                        .is_none();
                if introduced_parse_failure
                    && let Some(reason) =
                        agent_doc_element::element::structural_corruption_reason(&after_text)
                {
                    // The merged update structurally corrupted the canonical.
                    // Restore it to the clean pre-update text. `apply_canonical_replace`
                    // generates proper CRDT ops and fans the restoration out to every
                    // live member (including the corrupting editor), so hub-side
                    // mirrors re-converge to the clean canonical. The corrupting
                    // editor is also forced to re-project so a still-stale editor
                    // buffer cannot immediately re-push the same corruption.
                    hub.apply_canonical_replace(&after_text, &before_text)?;
                    hub.require_canonical_projection(client_id);
                    corruption_restored = Some(reason);
                }
            }
            Ok((
                packet,
                reattached,
                canonical_projection_pending,
                corruption_restored,
            ))
        })??;
    if let Some(reason) = corruption_restored {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_update_corruption_rejected file={} authority=multi_replica client_id={} reason={} recovery=canonical_restored_and_member_reprojected",
                file.display(),
                client_id,
                reason,
            ),
        );
    }
    // An editor update proves the doc is open → keep the reactive open-docs authority
    // truthful (also re-seeds it after a recycle-driven phantom-heal reattach).
    mark_editor_open_docs_open(file);
    // Re-seed the legacy process-exit watcher only if not already attached
    // (recovers after a recycle/stale-exit without a per-update durable-state read).
    reseed_editor_attach_if_needed(file);
    if reattached {
        // The normal register RPC records identity metadata. Preserve the same
        // invariant when an update is the first event after controller recycle;
        // otherwise a later IDE restart could leave this auto-healed member
        // invisible to dead-process pruning.
        let document_hash = agent_doc_fs::document_state_hash(file)?;
        let dead_client_ids = dead_editor_replica_ids(
            &document_hash,
            agent_doc_reliable_sync_io::process_pid_is_live,
        )?;
        if !dead_client_ids.is_empty() {
            with_existing_hub(file, |hub| {
                for dead_client_id in &dead_client_ids {
                    hub.deregister(*dead_client_id);
                }
            })?;
        }
        record_replica_identity(&document_hash, client_id, identity, &dead_client_ids)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_reattach_on_update file={} authority=multi_replica client_id={} recovery=lazy_canonical_projection dead_members_pruned={}",
                file.display(),
                client_id,
                dead_client_ids.len(),
            ),
        );
    }
    if canonical_projection_pending {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_update_quarantined file={} authority=multi_replica client_id={} recovery=lazy_canonical_projection",
                file.display(),
                client_id,
            ),
        );
        if let Err(err) =
            signal_crdt_replica_event(file, CrdtReplicaEventReason::CanonicalProjection, 0)
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_replica_event_signal_failed file={} reason=canonical_projection error={err}",
                    file.display(),
                ),
            );
        }
    }
    let canonical_len =
        with_hub_seeded_from_file(file, |hub| hub.canonical_text().chars().count())?;
    let (origin, update, targets) = packet
        .map(|packet| (packet.origin, packet.update, packet.targets))
        .unwrap_or_else(|| (client_id, Vec::new(), Vec::new()));
    if !targets.is_empty()
        && !update.is_empty()
        && let Err(err) =
            signal_crdt_replica_event(file, CrdtReplicaEventReason::Fanout, targets.len())
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
            origin,
            targets.len(),
            update.len(),
            canonical_len,
        ),
    );
    Ok(Some(FanOut {
        origin,
        update,
        targets,
        canonical_len,
    }))
}

/// Apply a CP-authored full-document update through the CRDT relay.
///
/// This is the controller→editor direction for recovered/finalized writes. It
/// refuses to create a relay hub from disk while an editor is attached, and it
/// only mutates the canonical replica when the caller's `expected_current`
/// byte-matches the current CP canonical text after the live-editor commit
/// barrier has flushed inbound editor ops. That baseline check is the guard that
/// keeps unsaved editor-buffer changes from being overwritten by a stale binary
/// recovery response.
/// Apply a CP full-document replace against an already-resolved relay `hub`.
///
/// Shared by the first-attempt and durable-projection-recovery paths of
/// [`apply_cp_write_for_file`] so both enforce the identical commit-barrier and
/// `expected_current` baseline guards. Fails closed (`retry_crdt_merge`) when the
/// hub canonical diverges from `expected_current`, so recovering a hub from the
/// durable projection can never overwrite unsaved editor state that the caller
/// did not compact against.
fn apply_cp_write_on_hub(
    hub: &mut RelayHub,
    file: &Path,
    authority: CrdtAuthority,
    expected_current: &str,
    content: &str,
) -> Result<CpRelayWrite> {
    let ready = hub.commit_barrier_under_authority(authority)?;
    if !ready {
        anyhow::bail!(
            "CP relay write refused for {}: editor_sync_pending; disk is a non-authoritative projection",
            file.display()
        );
    }
    let canonical = hub.canonical_text();
    if canonical != expected_current {
        anyhow::bail!(
            "CP relay write refused for {}: expected_hash={} current_hash={} recovery=retry_crdt_merge",
            file.display(),
            agent_doc_hash::content_hash(expected_current),
            agent_doc_hash::content_hash(&canonical)
        );
    }
    let before_hash = agent_doc_hash::content_hash(&canonical);
    if canonical == content {
        // Retained-transition Effects are allowed to re-evaluate after an ACK.
        // An equal Yrs replace still emits a small transaction, which used to
        // advance the delivery generation and invalidate the ACK that triggered
        // the re-evaluation. The successful expected-current CAS proves this
        // Effect is already at its fixed point, so observe it without publishing
        // a successor frontier.
        return Ok(CpRelayWrite {
            applied: false,
            content_len: content.len(),
            content_hash: before_hash,
            update_bytes: 0,
            targets: 0,
            live_editors: hub.live_count(),
            delivery_converged: hub.delivery_converged(),
        });
    }
    let packet = hub.apply_canonical_replace(expected_current, content)?;
    let targets = packet.targets.len();
    Ok(CpRelayWrite {
        applied: true,
        content_len: content.len(),
        content_hash: agent_doc_hash::content_hash(content),
        update_bytes: packet.update.len(),
        targets,
        live_editors: hub.live_count(),
        // If canonical content advanced while a durable editor owner has no
        // registered replica, nobody has observed that frontier yet.
        delivery_converged: hub.delivery_converged() && targets > 0,
    })
}

fn apply_response_cell_on_hub(
    hub: &mut RelayHub,
    file: &Path,
    authority: CrdtAuthority,
    committed_content: Option<&str>,
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
    let normalized =
        agent_doc_element::element::repair_single_unmatched_duplicate_component_close(&canonical)
            .unwrap_or_else(|| canonical.clone());
    let repaired_scaffolding = normalized != canonical;
    let outcome = if let Some(committed_content) = committed_content {
        agent_doc_merge::response_cell::supersede_uncommitted_response_tail(
            &normalized,
            committed_content,
            response,
        )?
    } else {
        agent_doc_merge::response_cell::add_response_cell(&normalized, response)?
    };
    let applied = repaired_scaffolding || outcome.applied;
    let (update_bytes, targets) = if applied {
        let packet = hub.apply_canonical_replace(&canonical, &outcome.content)?;
        (packet.update.len(), packet.targets.len())
    } else {
        (0, 0)
    };
    Ok(ResponseCellRelayWrite {
        applied,
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
    committed_content: Option<&str>,
    response: &str,
    source: &str,
) -> Result<Option<ResponseCellRelayWrite>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }

    let Some(result) = with_existing_hub(file, |hub| {
        apply_response_cell_on_hub(hub, file, authority, committed_content, response)
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

fn apply_salient_response_on_hub(
    hub: &mut RelayHub,
    file: &Path,
    authority: CrdtAuthority,
    cycle_id: &str,
    body: &str,
) -> Result<ResponseCellRelayWrite> {
    let ready = hub.commit_barrier_under_authority(authority)?;
    if !ready {
        anyhow::bail!(
            "salient response upsert refused for {}: editor_sync_pending",
            file.display()
        );
    }
    let canonical = hub.canonical_text();
    let outcome = agent_doc_merge::salient_response::upsert_salient_response_node(
        &canonical, cycle_id, body,
    )?;
    let (update_bytes, targets) = if outcome.applied {
        let packet = hub.apply_canonical_replace(&canonical, &outcome.content)?;
        (packet.update.len(), packet.targets.len())
    } else {
        (0, 0)
    };
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

/// Append or replace the one non-final salient response node for an open cycle.
/// The semantic operation is evaluated against the controller canonical under
/// the same inbound barrier as final response-cell insertion.
pub fn upsert_salient_response_for_file(
    file: &Path,
    cycle_id: &str,
    body: &str,
    source: &str,
) -> Result<Option<ResponseCellRelayWrite>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let Some(result) = with_existing_hub(file, |hub| {
        apply_salient_response_on_hub(hub, file, authority, cycle_id, body)
    })?
    else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_salient_response_upsert_deferred file={} source={} reason=missing_live_canonical_model recovery=wait_for_editor_replica",
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
            CrdtReplicaEventReason::SalientResponseUpsert,
            result.targets,
        )
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_event_signal_failed file={} reason=salient_response_upsert error={err}",
                file.display()
            ),
        );
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_salient_response_upsert file={} source={} cell_id={} applied={} content_hash={} update_bytes={} targets={} live_editors={} delivery_converged={}",
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

pub fn apply_cp_write_for_file(
    file: &Path,
    expected_current: &str,
    content: &str,
    source: &str,
) -> Result<Option<CpRelayWrite>> {
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
        apply_cp_write_on_hub(hub, file, authority, expected_current, content)
    })? {
        result
    } else {
        // Recover the hub from the durable `.yrs` projection before failing —
        // symmetric with the read path
        // ([`current_text_for_file_with_authority_recovering_projection`]). The
        // projection is the last-known relay canonical, not raw disk, so this does
        // not smuggle a non-authoritative disk image in: the `expected_current`
        // baseline check inside [`apply_cp_write_on_hub`] still fails closed with
        // `retry_crdt_merge` if the recovered canonical diverges from what the
        // caller compacted against. Without this, a compact/CP write hard-fails
        // the whole operation (observed: JB `Compact Exchange` →
        // `crdt_cp_write ... no registered replica yet`, #cpcwritemissingreplica).
        let hash = agent_doc_fs::document_state_hash(file)?;
        let recovered = recover_missing_hub_from_retained_projection(file, &hash)?;
        match if recovered {
            with_existing_hub(file, |hub| {
                apply_cp_write_on_hub(hub, file, authority, expected_current, content)
            })?
        } else {
            None
        } {
            Some(result) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "crdt_cp_write_recovered_missing_replica file={} source={} authority=multi_replica doc_hash={} recovery=retained_lazily_projection",
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
                        "crdt_cp_write_deferred file={} source={} authority=multi_replica reason=missing_relay_model recovered_projection={} recovery=observe_lazily_current_register_crdt",
                        file.display(),
                        source,
                        recovered,
                    ),
                );
                anyhow::bail!(
                    "CP relay write unavailable for {}; editor is the current authority but the CRDT relay has no registered replica yet",
                    file.display()
                );
            }
        }
    };
    let result = result?;
    if result.targets > 0
        && result.update_bytes > 0
        && let Err(err) =
            signal_crdt_replica_event(file, CrdtReplicaEventReason::CpWrite, result.targets)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_replica_event_signal_failed file={} reason=cp_write error={err}",
                file.display(),
            ),
        );
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_cp_write file={} source={} authority=multi_replica applied={} content_len={} content_hash={} update_bytes={} targets={} live_editors={} delivery_converged={}",
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
/// updates remain pending until [`observe_replica_projection_for_file`] proves
/// the editor's complete visible state covers them.
pub fn pull_replica_updates_for_file(file: &Path, identity: &str) -> Result<Option<ReplicaPull>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = mint_client_id(identity);
    let Some(pull) = with_existing_hub(file, |hub| {
        let updates = hub.pending_updates(client_id)?;
        let delivery = hub
            .delivery_snapshot()
            .into_iter()
            .find(|entry| entry.client_id == client_id)
            .ok_or_else(|| anyhow::anyhow!("replica {client_id} is not registered"))?;
        Ok::<_, anyhow::Error>((updates, delivery))
    })?
    else {
        // Passive polls carry no editor state with which to rebuild an evicted
        // canonical. Registration or an authoritative update must re-contact
        // first; otherwise a stale poll recreates the phantom zero-member hub.
        return Ok(None);
    };
    let (updates, delivery) = pull?;
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
    let text = with_existing_hub(file, |hub| {
        if hub.pending_rebootstrap_members().contains(&client_id) {
            let text = hub.rebootstrap_text();
            hub.clear_rebootstrap(client_id);
            Some(text)
        } else {
            None
        }
    })?
    .flatten();
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

/// Publish one editor's complete visible state into the relay delivery
/// projection.
///
/// This is state ingress, not a per-update command receipt: the editor reports
/// the same full-buffer observation it already emits for current-document
/// authority, and the hub derives which queued generations that state covers.
pub fn observe_replica_projection_for_file(
    file: &Path,
    identity: &str,
    visible_content_hash: &str,
) -> Result<Option<bool>> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(None);
    }
    let client_id = mint_client_id(identity);
    let Some(observation) = with_existing_hub(file, |hub| {
        let prior_lineage = hub.lineage().to_string();
        let state_bytes_before = hub.canonical_encoded_state().len();
        let projected = hub.observe_delivery_projection(client_id, visible_content_hash)?;
        let compacted = (hub.lineage() != prior_lineage).then(|| CompactEpochOutcome {
            prior_lineage,
            lineage: hub.lineage().to_string(),
            state_bytes_before,
            state_bytes_after: hub.canonical_encoded_state().len(),
            rebootstrap_members: hub.pending_rebootstrap_members().len(),
        });
        Ok::<_, anyhow::Error>((projected, compacted))
    })?
    else {
        return Ok(None);
    };
    let (projected, compacted) = observation?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_replica_projection_observed file={} authority=multi_replica client_id={} content_hash={} projected={}",
            file.display(),
            client_id,
            visible_content_hash,
            projected,
        ),
    );
    if let Some(outcome) = compacted {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_compact_epoch_settled_from_projection file={} client_id={} prior_lineage={} lineage={} state_bytes_before={} state_bytes_after={} rebootstrap_members={} content_hash={}",
                file.display(),
                client_id,
                outcome.prior_lineage,
                outcome.lineage,
                outcome.state_bytes_before,
                outcome.state_bytes_after,
                outcome.rebootstrap_members,
                visible_content_hash,
            ),
        );
    }
    Ok(Some(projected))
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
    let Some(snapshot) = with_existing_hub(file, |hub| {
        hub.set_awareness(client_id, state);
        hub.awareness_snapshot()
    })?
    else {
        return Ok(None);
    };
    Ok(Some(snapshot))
}

/// Recover the per-document canonical replica from a durable `state.db` recovery
/// projection on supervisor restart (plan phase 6). Live editors re-sync newer
/// ops when they re-register. The ledger projection is a recovery input only,
/// never authority.
///
/// Idempotent on an existing hub: if a live hub for the document already exists,
/// the stale disk projection is reconciled into it (in-memory wins) rather than
/// replacing it.
pub fn recover_hub_from_projection(
    file: &Path,
    projection: &[u8],
    lineage: Option<&str>,
) -> Result<()> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    if let Some(handle) = hub_handle(&hash) {
        // A live hub already holds the authority — disk is recovery-only, so
        // reconcile the projection into it (in-memory wins) instead of clobbering.
        handle.lock().reconcile_disk_projection(projection)?;
        return Ok(());
    }
    // Decoding the projection is the expensive part, so it runs before the
    // registry lock is taken at all.
    let hub =
        RelayHub::recover_from_projection_with_lineage(CANONICAL_CLIENT_ID, projection, lineage)?;
    let mut hub = Some(hub);
    hub_handle_or_insert_with(&hash, || hub.take().expect("hub built above"));
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
    /// A live editor relay was checkpointed into the `state.db` recovery ledger.
    Checkpointed {
        bytes: usize,
        changed: bool,
        live_editors: usize,
        text_len: usize,
        text_hash: String,
    },
}

/// Flush the live relay's canonical replica to the durable `state.db` recovery
/// projection before a recycle/reload tears down the process that owns the hub.
///
/// This is **not** the closeout hot path and persisted state is not authority. It is
/// a bounded pre-recycle checkpoint: under detached/headless authority it skips
/// without allocating a hub; under editor authority it requires a live, converged
/// document model and writes the recovery projection from the in-memory canonical
/// replica in one serialized state-ledger instruction.
pub fn checkpoint_durable_projection_for_file(
    file: &Path,
    source: &str,
) -> Result<DurableProjectionCheckpoint> {
    let authority = authority_for_file(&file.display().to_string());
    if !authority.editor_attached() {
        return Ok(DurableProjectionCheckpoint::Detached);
    }
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    let Some(projection) = retained_canonical_projections().observe(&document_hash) else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_projection_observe_deferred file={} source={} reason=controller_projection_unavailable",
                file.display(),
                source,
            ),
        );
        return Ok(DurableProjectionCheckpoint::Deferred {
            reason: "controller_projection_unavailable".to_string(),
        });
    };
    let projected = RelayHub::from_retained_canonical_projection(CANONICAL_CLIENT_ID, &projection)?;
    let canonical_text = projected.canonical_text();
    let live_editors = hub_handle(&document_hash)
        .map(|handle| handle.lock().live_count())
        .unwrap_or_default();
    let text_len = canonical_text.len();
    let text_hash = agent_doc_hash::content_hash(&canonical_text);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_projection_observed file={} source={} storage=controller_lazily_projection bytes={} live_editors={} text_len={} text_hash={}",
            file.display(),
            source,
            projection.state.len(),
            live_editors,
            text_len,
            text_hash,
        ),
    );
    Ok(DurableProjectionCheckpoint::Checkpointed {
        bytes: projection.state.len(),
        changed: false,
        live_editors,
        text_len,
        text_hash,
    })
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
/// Under editor authority this barrier establishes the inbound editor→canonical
/// cut only. Outbound delivery is a reactive receipt projection consumed by the
/// controller's commit Effect; it is not request/ACK authority for this
/// transition.
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
    commit_barrier_for_file_with_authority_and_delivery(file, authority, false)
}

/// Commit barrier for a semantic response cell whose CRDT projection and
/// `ResponseCellAdded` fact are already durable in the realtime backbone.
///
/// The durable intent is already authoritative. Editor delivery remains an
/// asynchronously folded receipt Source and is derived by the controller before
/// its native-save/commit Effect runs; the barrier does not wait for an ACK.
pub fn commit_barrier_for_durable_response_cell(file: &Path) -> bool {
    let file_str = file.display().to_string();
    let authority = authority_for_file(&file_str);
    commit_barrier_for_file_with_authority_and_delivery(file, authority, false)
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
                    "crdt_commit_barrier_deferred file={} authority=multi_replica reason=missing_relay_model recovery=observe_lazily_current_register_crdt",
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
    let registration_lock = match replica_registration_lock(&hash) {
        Ok(lock) => lock,
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_record_committed_baseline_registration_lock_error file={} error={}",
                    file.display(),
                    e
                ),
            );
            return;
        }
    };
    let _registration_guard = registration_lock.lock();
    if let Some(handle) = hub_handle(&hash) {
        let mut hub = handle.lock();
        hub.record_committed_baseline(&on_disk);
        retained_canonical_projections().retain(&hash, hub.retained_canonical_projection());
    }
    evict_hub_if_safe(file, &hash, "committed_baseline");
}

/// Authority-gated reconciliation of an explicit cold restart projection into
/// an already-live Lazily replica.
///
/// Under [`CrdtAuthority::MultiReplica`] the in-memory canonical replica is the
/// authority and the supplied projection is restart evidence only. A possibly stale
/// projection is reconciled into the live replica, which can only add ops the
/// live replica genuinely lost (a crash gap) and can never regress live text —
/// in-memory wins. Returns `Some(changed)` where `changed` is whether the disk
/// held ops the live replica was missing.
///
/// Under [`CrdtAuthority::GitAuthoritative`] there is no live in-memory authority
/// to reconcile against, so the durable document baseline remains sufficient and
/// this returns `None`.
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

/// Fence an already-converged Compact Exchange snapshot into a fresh CRDT epoch.
///
/// The caller must first prove all live editor buffers and disk hold
/// the current canonical text. This function rebuilds that canonical from one
/// snapshot insertion, rotates its lineage, and queues every live member for
/// replace-capable re-bootstrap. An attached document with no relay model fails
/// closed rather than silently
/// retaining pre-compaction history. `Ok(None)` means the document is detached
/// and therefore has no live CRDT epoch to compact.
pub fn request_authoritative_epoch_compaction_for_file(
    file: &Path,
) -> Result<Option<CompactEpochRequestOutcome>> {
    let file_str = file.display().to_string();
    let authority = authority_for_file(&file_str);
    if !authority.editor_attached() {
        return Ok(None);
    }
    let Some(outcome) = with_existing_hub(file, |hub| {
        let prior_lineage = hub.lineage().to_string();
        let state_bytes_before = hub.canonical_encoded_state().len();
        let fenced = hub.request_authoritative_epoch_compaction()?;
        Ok::<_, anyhow::Error>(if fenced {
            CompactEpochRequestOutcome::Fenced(CompactEpochOutcome {
                prior_lineage,
                lineage: hub.lineage().to_string(),
                state_bytes_before,
                state_bytes_after: hub.canonical_encoded_state().len(),
                rebootstrap_members: hub.pending_rebootstrap_members().len(),
            })
        } else {
            CompactEpochRequestOutcome::Retained {
                lineage: prior_lineage,
                state_bytes: state_bytes_before,
            }
        })
    })?
    else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_compact_epoch_request_deferred file={} authority=multi_replica reason=missing_relay_model",
                file.display(),
            ),
        );
        anyhow::bail!(
            "cannot request CRDT epoch compaction for {}: editor authority is attached but the live relay model is missing",
            file.display(),
        );
    };
    let outcome = outcome?;
    match &outcome {
        CompactEpochRequestOutcome::Fenced(outcome) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_compact_epoch file={} prior_lineage={} lineage={} state_bytes_before={} state_bytes_after={} rebootstrap_members={}",
                file.display(),
                outcome.prior_lineage,
                outcome.lineage,
                outcome.state_bytes_before,
                outcome.state_bytes_after,
                outcome.rebootstrap_members,
            ),
        ),
        CompactEpochRequestOutcome::Retained {
            lineage,
            state_bytes,
        } => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_compact_epoch_request_retained file={} lineage={} state_bytes={} settlement=final_visible_projection",
                file.display(),
                lineage,
                state_bytes,
            ),
        ),
    }
    Ok(Some(outcome))
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
    Ok(
        apply_document_op_delta_for_file_in_lineage(file, None, delta)?
            .map(|outcome| matches!(outcome, DocumentOpDeltaOutcome::Applied { changed: true })),
    )
}

/// Lineage-fenced durable document-op ingest. Stale/ambiguous frames are
/// terminally quarantined so reliable-sync can ACK them instead of retrying
/// forever or union-applying an obsolete CRDT history.
pub fn apply_document_op_delta_for_file_in_lineage(
    file: &Path,
    lineage: Option<&str>,
    delta: &[u8],
) -> Result<Option<DocumentOpDeltaOutcome>> {
    let Some(outcome) = with_existing_hub(file, |hub| {
        hub.apply_document_op_delta_in_lineage(lineage, delta)
    })?
    else {
        return Ok(None);
    };
    let outcome = outcome?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_document_op_delta_ingested file={} authority=multi_replica outcome={:?} frame_lineage={} delta_len={}",
            file.display(),
            outcome,
            lineage.unwrap_or("legacy-unscoped"),
            delta.len(),
        ),
    );
    Ok(Some(outcome))
}

/// Current canonical lineage returned to a newly registered editor.
pub fn current_lineage_for_file(file: &Path) -> Result<Option<String>> {
    with_existing_hub(file, |hub| hub.lineage().to_string())
}

pub fn reconcile_disk_projection_for_file(file: &Path, projection: &[u8]) -> Result<Option<bool>> {
    let file_str = file.display().to_string();
    let authority = authority_for_file(&file_str);
    if !authority.editor_attached() {
        // Headless: no live canonical replica is authoritative; the durable
        // document baseline is sufficient.
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

/// Wake every live Lazily editor replica through its process-scoped endpoint.
///
/// This notification is advisory: the durable Lazily outbox remains authoritative
/// and a later observation/reconnect drains it. No filesystem wake sidecar is
/// created, and one unavailable replica cannot fail the canonical transition.
/// What reconciling the hub's replica cache against process liveness found
/// (`#deliveryackcut`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplicaReconcile {
    /// Members whose editor process is gone. Fully deregistered — the cache
    /// entry was wrong, so it is removed, not tombstoned.
    pub removed_dead: Vec<u64>,
    /// Members that are not ACKing but whose editor process IS alive. These are
    /// not stale cache entries; the process exists and owes us a replica, so the
    /// correct action is to make it re-register (refill), never to drop it.
    pub live_unacked: Vec<u64>,
}

/// Reconcile the per-document replica cache against ground truth
/// (`#deliveryackcut`).
///
/// The hub's member set is a **cache** of which editor replicas exist and are
/// live. When delivery stalls, that cache is what is wrong — so invalidate it
/// against the authority (the editor pid recorded in the replica identity) and
/// refill, rather than parking a zombie entry in it.
///
/// This is the same reconciliation `register_replica_for_file_with_liveness`
/// already performs via [`dead_editor_replica_ids`]; it simply had no trigger
/// except an editor announcing itself, so a stall could never repair it.
///
/// The two outcomes are deliberately different, because the failures are:
/// a dead pid means the entry is garbage and is removed outright, while a live
/// pid with no ACK means a real process with a stale replica, which the caller
/// refills through the existing re-registration nudge.
pub fn reconcile_replicas_against_process_liveness(file: &Path) -> Result<ReplicaReconcile> {
    reconcile_replicas_against_liveness_with(file, agent_doc_reliable_sync_io::process_pid_is_live)
}

/// Errors here are never retried, and that is deliberate. Since
/// `#relaylockpoison` the registries use `parking_lot`, which does not poison,
/// so the only remaining failure is
/// [`agent_doc_fs::document_state_hash`] failing to canonicalize — the document
/// was deleted or renamed mid-write. Retrying cannot conjure it back, and the
/// enclosing write already fails closed on a vanished document.
///
/// Retry that *is* useful happens one level up: this runs at the ACK deadline of
/// a single write attempt, and the CRDT write convergence loop retries the whole
/// attempt under its own backoff, which re-runs this reconciliation.
fn reconcile_replicas_against_liveness_with(
    file: &Path,
    is_pid_live: impl Fn(u32) -> bool,
) -> Result<ReplicaReconcile> {
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    let dead = dead_editor_replica_ids(&document_hash, is_pid_live)?;
    // Lock order matches `register_replica_for_file_with_liveness`: take the
    // metadata lock, release it, then take the hub lock. Never both at once, so
    // registration and reconciliation cannot deadlock each other.
    let outcome = with_existing_hub(file, |hub| {
        let mut removed_dead = Vec::new();
        for client_id in &dead {
            if hub.deregister(*client_id) {
                removed_dead.push(*client_id);
            }
        }
        let live_unacked = hub
            .delivery_snapshot()
            .into_iter()
            .filter(|entry| entry.live && entry.pending_updates > 0)
            .map(|entry| entry.client_id)
            .collect::<Vec<_>>();
        ReplicaReconcile {
            removed_dead,
            live_unacked,
        }
    })?
    .unwrap_or_default();
    // Forget every DEAD identity, not just the ones the hub still held. Gating
    // this on `removed_dead` stranded an identity whose hub member was already
    // gone: the next pass re-detects it as dead, `deregister` returns false, so
    // `removed_dead` is empty and the forget never runs again. Draining `dead`
    // is idempotent and self-healing instead.
    for client_id in &dead {
        forget_replica_identity(&document_hash, *client_id)?;
    }
    Ok(outcome)
}

/// Whether every live replica for `file` has drained its fan-out
/// (`#deliveryackcut`), or `None` when this process hosts no hub for `file`.
///
/// The `None` is the point. This used to answer `Ok(true)` — "converged" — for a
/// document the calling process cannot observe at all, because `hub_registry` is a
/// process-local static and a CLI asking its own registry finds nothing. That is
/// absence coerced into the most dangerous available answer: a caller learns
/// "delivery finished" about a hub living in another process, which may still have
/// unacked fan-out queued.
///
/// It is *not* the same shape as `LivenessProjection::pid_alive`'s presumed-alive
/// default, which is deliberate and has the 500ms process-exit watcher reaping
/// behind it. Nothing reaps behind this one, so the honest answer is `None` and the
/// caller decides. Cross-process consumers should ask the controller — see
/// `project_controller::await_delivery_convergence_for_file`.
pub fn delivery_converged_for_file(file: &Path) -> Result<Option<bool>> {
    with_existing_hub(file, |hub| hub.delivery_converged())
}

/// `#lazily-hot-path` Theme A — [`RelayHub::delivery_convergence_witness`] for `file`,
/// or `None` when this process hosts no hub for it.
///
/// Deliberately **not** defaulted to "converged" the way
/// [`delivery_converged_for_file`] is: a witness is a suppression key, and inventing
/// a version for a hub that does not exist here would let a caller conclude "nothing
/// changed" about a document it cannot observe at all. `hub_registry` is a
/// process-local static, so an absent hub means *ask the process that owns it* — the
/// CLI-side retry loops (compact's commit-observe and CRDT-merge retries) therefore
/// need a controller-side await, exactly like the visible-write receipt push, rather
/// than a direct call to this function.
pub fn delivery_convergence_witness_for_file(
    file: &Path,
) -> Result<Option<agent_doc_document_realtime::crdt_relay::DeliveryConvergenceWitness>> {
    with_existing_hub(file, |hub| hub.delivery_convergence_witness())
}

/// Await the delivery-convergence cell without polling the per-document hub.
///
/// With `after_version`, this is a revision-cursor subscription: it returns as
/// soon as the cell differs from the caller's observation (or is converged).
/// Without a cursor, it preserves the older "await convergence until deadline"
/// contract used by compact/preflight. `None` remains the honest answer when
/// this process does not host the document hub.
pub fn await_delivery_convergence_for_file(
    file: &Path,
    after_version: Option<u64>,
    wait: std::time::Duration,
) -> Result<Option<agent_doc_document_realtime::crdt_relay::DeliveryConvergenceWitness>> {
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    let deadline = std::time::Instant::now().checked_add(wait);

    loop {
        let Some(handle) = hub_handle(&document_hash) else {
            return Ok(None);
        };
        let (witness, subscription) = {
            let hub = handle.lock();
            (
                hub.delivery_convergence_witness(),
                hub.delivery_convergence_subscription(),
            )
        };

        if witness.converged
            || after_version.is_some_and(|after| witness.version != after)
            || wait.is_zero()
        {
            return Ok(Some(witness));
        }

        let Some(deadline) = deadline else {
            subscription.wait_for_change(witness.version, wait);
            continue;
        };
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() || !subscription.wait_for_change(witness.version, remaining) {
            return delivery_convergence_witness_for_file(file);
        }
    }
}

pub fn signal_crdt_replica_event(
    file: &Path,
    reason: CrdtReplicaEventReason,
    targets: usize,
) -> Result<()> {
    signal_crdt_replica_event_counting(file, reason, targets).map(|_| ())
}

/// [`signal_crdt_replica_event`] reporting how many live editor registrations
/// were actually notified.
///
/// `#ensurereregister` — the unit-returning form cannot distinguish "asked every
/// live editor to re-register" from "there was nobody to ask": with an empty
/// registration list the send loop simply never runs and it still returns
/// `Ok(())`, which callers log as `reregister=requested`. That false-positive
/// diagnostic is actively misleading during a missing-replica wedge, because it
/// reads as "the binary nudged the editor and the editor ignored it" and points
/// diagnosis at the editor plugin when in fact no message was ever sent.
/// How a replica-event signal resolved (`#mrnh`).
///
/// `notified` alone cannot distinguish "the liveness plane holds no
/// registration" from "registrations exist but every delivery failed" — both
/// are `0`. Those demand opposite responses, and conflating them sent this
/// investigation after payload size and generation fencing when the plane
/// already reported `live_editors=1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReplicaSignalRoute {
    pub editor_id: String,
    pub editor_pid: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaSignalOutcome {
    /// Distinct live editor routes discovered from reliable-sync liveness and
    /// the CRDT replica registry.
    pub found: usize,
    /// Registrations an IPC message was successfully delivered to.
    pub notified: usize,
    /// Routes rejected specifically because sender and listener builds differ.
    /// The controller uses these to choose the correct side of the rolling
    /// upgrade boundary: recycle itself when stale, reload the editor when not.
    pub build_mismatches: Vec<ReplicaSignalRoute>,
}

impl ReplicaSignalOutcome {
    /// The diagnosis token for ops logs. `found == 0` is a stale-attachment
    /// wedge inside agent-doc; `found > 0` with `notified == 0` is a delivery
    /// failure to a registration that *does* exist.
    pub fn diagnosis(&self) -> String {
        match (self.found, self.notified) {
            (0, _) => "no_live_registration".to_string(),
            (found, 0) => format!("delivery_failed_to_all:{found}"),
            (found, notified) if notified < found => {
                format!("requested:{notified}/{found}")
            }
            (_, notified) => format!("requested:{notified}"),
        }
    }
}

/// Signal a replica event, reporting both how many registrations were found and
/// how many were reached. See [`ReplicaSignalOutcome`].
pub fn signal_crdt_replica_event_with_counts(
    file: &Path,
    reason: CrdtReplicaEventReason,
    targets: usize,
) -> Result<ReplicaSignalOutcome> {
    signal_crdt_replica_event_counting_inner(file, reason, targets)
}

pub fn signal_crdt_replica_event_counting(
    file: &Path,
    reason: CrdtReplicaEventReason,
    targets: usize,
) -> Result<usize> {
    signal_crdt_replica_event_counting_inner(file, reason, targets).map(|outcome| outcome.notified)
}

fn signal_crdt_replica_event_counting_inner(
    file: &Path,
    reason: CrdtReplicaEventReason,
    targets: usize,
) -> Result<ReplicaSignalOutcome> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let _ = reliable_sync_editor_live_for_file(&canonical);
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let registrations = agent_doc_reliable_sync_io::global_liveness_plane()
        .lock()
        .projection()
        .live_registrations(&document_hash);

    // The CRDT member itself owns the ACK frontier. Its recorded editor
    // identity therefore remains a valid notification route even if the
    // separately-journaled reliable-sync registration was missed or pruned.
    // Union both planes and deduplicate by the process-scoped editor endpoint.
    let mut routes = HashSet::new();
    for registration in registrations {
        routes.insert(ReplicaSignalRoute {
            editor_id: registration.editor_id,
            editor_pid: registration.pid,
        });
    }
    routes.extend(live_replica_signal_routes(&document_hash));

    let found = routes.len();
    let mut notified = 0usize;
    let mut build_mismatches = Vec::new();
    for route in routes {
        let payload = serde_json::json!({
            "type": agent_doc_ipc_protocol::EditorIntent::DeliverCrdtRemote.as_str(),
            "file": canonical.to_string_lossy(),
            "reason": reason.token(),
            "targets": targets,
            "editor_id": route.editor_id.clone(),
            "editor_pid": route.editor_pid,
        });
        if let Err(error) = agent_doc_ipc_io::send_message_to_pid(
            &agent_doc_project_root_io::resolve_ipc_project_root(&canonical),
            route.editor_pid,
            &payload,
        ) {
            if agent_doc_ipc_io::is_ipc_build_mismatch_error(&error) {
                build_mismatches.push(route.clone());
            }
            agent_doc_ops_log_io::log_op(
                &canonical,
                &format!(
                    "crdt_replica_notify_deferred reason={} editor_pid={} error={error:#}",
                    reason.token(),
                    route.editor_pid,
                ),
            );
        } else {
            notified += 1;
        }
    }
    Ok(ReplicaSignalOutcome {
        found,
        notified,
        build_mismatches,
    })
}

/// Controller-owned disk-change transition. The watcher already routes through
/// the single project controller, so editor-attached changes are reconciled into
/// Lazily current state in that same serialized transition. No filesystem signal
/// or supervisor poll participates in the hot path.
pub fn route_disk_change_signal(file: &Path, delivery: &WatchDelivery) -> Result<WatchAction> {
    let authority = authority_for_file(&file.display().to_string());
    // Lazily owns edit settlement; the controller serializes this transition.
    let action = decide_watch_action(delivery, authority, false);
    if matches!(
        action,
        WatchAction::ReconcileIntoCanonical | WatchAction::DeferForEditSettle
    ) {
        let on_disk = std::fs::read_to_string(file).with_context(|| {
            format!("failed to read disk text for reconcile {}", file.display())
        })?;
        let _ = apply_disk_change_for_file(file, &on_disk)?;
    }
    Ok(action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::io::Write;

    #[test]
    fn convergence_await_wakes_on_one_cell_transition() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("convergence-subscription.md");
        std::fs::write(&file, "# subscription\n").unwrap();
        with_hub_seeded_from_file(&file, |_| ()).unwrap();
        let pending = with_existing_hub(&file, |hub| {
            hub.register(42).unwrap();
            hub.apply_canonical_replace("# subscription\n", "# changed\n")
                .unwrap();
            hub.pending_updates(42).unwrap().remove(0)
        })
        .unwrap()
        .unwrap();
        let before = delivery_convergence_witness_for_file(&file)
            .unwrap()
            .expect("seeded hub");
        assert!(!before.converged);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let waiter_file = file.clone();
        let waiter = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            await_delivery_convergence_for_file(
                &waiter_file,
                Some(before.version),
                std::time::Duration::from_secs(5),
            )
            .unwrap()
            .expect("hub remains observed")
        });

        ready_rx.recv().unwrap();
        with_existing_hub(&file, |hub| {
            hub.ack_delivery(42, &pending.patch_id, pending.generation)
                .unwrap()
        })
        .unwrap();
        let after = waiter.join().unwrap();

        assert_ne!(after.version, before.version);
        assert!(
            after.converged,
            "the registration transition converges the empty queue"
        );
    }

    #[test]
    fn convergence_await_is_bounded_when_the_cell_does_not_change() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("convergence-deadline.md");
        std::fs::write(&file, "# deadline\n").unwrap();
        with_hub_seeded_from_file(&file, |_| ()).unwrap();
        with_existing_hub(&file, |hub| {
            hub.register(43).unwrap();
            hub.apply_canonical_replace("# deadline\n", "# still pending\n")
                .unwrap();
        })
        .unwrap();
        let before = delivery_convergence_witness_for_file(&file)
            .unwrap()
            .expect("seeded hub");
        assert!(!before.converged);
        let started = std::time::Instant::now();

        let after = await_delivery_convergence_for_file(
            &file,
            Some(before.version),
            std::time::Duration::from_millis(40),
        )
        .unwrap()
        .expect("hub remains observed");

        assert_eq!(after, before);
        assert!(started.elapsed() >= std::time::Duration::from_millis(35));
    }
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    /// `#mrnh`: "no registration exists" and "registrations exist but nothing
    /// could be delivered" are opposite faults that both produced
    /// `notified == 0`. The diagnosis must tell them apart, or the ops log
    /// blames a stale attachment for what is really a delivery failure —
    /// which is what sent this investigation after payload size and
    /// generation fencing while the plane reported `live_editors=1`.
    #[test]
    fn replica_signal_diagnosis_separates_missing_registration_from_failed_delivery() {
        assert_eq!(
            ReplicaSignalOutcome {
                found: 0,
                notified: 0,
                build_mismatches: Vec::new(),
            }
            .diagnosis(),
            "no_live_registration",
            "no registration is a stale-attachment wedge inside agent-doc"
        );
        assert_eq!(
            ReplicaSignalOutcome {
                found: 1,
                notified: 0,
                build_mismatches: Vec::new(),
            }
            .diagnosis(),
            "delivery_failed_to_all:1",
            "a registration that exists but cannot be reached is a DELIVERY fault"
        );
        assert_eq!(
            ReplicaSignalOutcome {
                found: 3,
                notified: 1,
                build_mismatches: Vec::new(),
            }
            .diagnosis(),
            "requested:1/3",
            "partial delivery must surface both counts"
        );
        assert_eq!(
            ReplicaSignalOutcome {
                found: 2,
                notified: 2,
                build_mismatches: Vec::new(),
            }
            .diagnosis(),
            "requested:2"
        );
    }

    #[test]
    fn replica_identity_preserves_a_routable_editor_endpoint_without_liveness_registration() {
        let route = editor_route_from_replica_identity(
            "vscode-4242-9e654d90:/tmp/project/tasks/session.md:refresh-2",
        )
        .expect("native replica identities carry their process-scoped editor route");

        assert_eq!(route.editor_pid, 4242);
        assert_eq!(route.editor_id, "vscode-4242-9e654d90");
        assert!(
            editor_route_from_replica_identity("anonymous-replica").is_none(),
            "untyped identities must not invent a notification endpoint"
        );
    }

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

    /// `#relayhubperdoclock`: one busy document must not block another.
    ///
    /// This is the saturation fix's whole point, and it is a *structural* claim,
    /// so the test is structural too — no sleeps, no timing margins. A worker
    /// takes document A's hub and holds it open until told to let go. While it
    /// is held, document B's `with_hub` must complete. Under the old
    /// by-value registry both documents shared one process-global mutex, so B
    /// would block until A released and this deadlocks; `recv_timeout` turns
    /// that into a failure instead of a hung suite.
    ///
    /// The real-world shape (2026-07-26): a second session looping 3.3 MB
    /// replica bootstraps on one document made every unrelated document's 5s
    /// authority resolve time out — nine consecutive closeout attempts — while
    /// `admin inspect`, which never takes this lock, stayed instant.
    #[test]
    fn a_busy_document_hub_does_not_block_another_document() {
        let (_dir_a, doc_a) = temp_doc("busy.md");
        let (_dir_b, doc_b) = temp_doc("bystander.md");
        // Allocate both hubs up front so the test measures hub contention, not
        // first-contact allocation.
        with_hub(&doc_a, |_| ()).unwrap();
        with_hub(&doc_b, |_| ()).unwrap();

        let (holding_tx, holding_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (bystander_tx, bystander_rx) = std::sync::mpsc::channel::<()>();

        let busy = {
            let doc_a = doc_a.clone();
            thread::spawn(move || {
                with_hub(&doc_a, |_| {
                    holding_tx.send(()).unwrap();
                    // Hold A's hub until the bystander has proven it got through.
                    release_rx.recv().unwrap();
                })
                .unwrap();
            })
        };

        holding_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("worker should acquire document A's hub");

        let bystander = {
            let doc_b = doc_b.clone();
            thread::spawn(move || {
                with_hub(&doc_b, |_| ()).unwrap();
                bystander_tx.send(()).unwrap();
            })
        };

        let served = bystander_rx.recv_timeout(Duration::from_secs(10));
        // Release A regardless, so a failure reports cleanly instead of hanging.
        release_tx.send(()).unwrap();
        busy.join().unwrap();
        bystander.join().unwrap();

        served.expect(
            "document B must be served while document A's hub is held; \
             a process-global registry lock makes one busy document starve every other",
        );
    }

    /// The registry lock itself must never be held across hub work, which is what
    /// makes the isolation above hold for *allocation* too — a first-contact
    /// `with_hub` on a new document cannot be blocked by a busy existing one.
    #[test]
    fn allocating_a_new_document_hub_does_not_block_on_a_busy_one() {
        let (_dir_a, doc_a) = temp_doc("busy-alloc.md");
        let (_dir_b, doc_b) = temp_doc("fresh-alloc.md");
        with_hub(&doc_a, |_| ()).unwrap();

        let (holding_tx, holding_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (fresh_tx, fresh_rx) = std::sync::mpsc::channel::<()>();

        let busy = {
            let doc_a = doc_a.clone();
            thread::spawn(move || {
                with_hub(&doc_a, |_| {
                    holding_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                })
                .unwrap();
            })
        };
        holding_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("worker should acquire document A's hub");

        let fresh = {
            let doc_b = doc_b.clone();
            thread::spawn(move || {
                // First contact for B: allocates a hub while A's is held.
                with_hub(&doc_b, |_| ()).unwrap();
                fresh_tx.send(()).unwrap();
            })
        };
        let served = fresh_rx.recv_timeout(Duration::from_secs(10));
        release_tx.send(()).unwrap();
        busy.join().unwrap();
        fresh.join().unwrap();

        served.expect("allocating a hub for a new document must not wait on a busy document");
    }

    #[test]
    fn durable_recovery_projection_keeps_lineage_with_exact_bytes() {
        let (_dir, doc) = temp_doc("lineage-projection.md");
        let projection =
            RelayHub::from_text(CANONICAL_CLIENT_ID, "projection one\n").projection_bytes();
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(
            &doc,
            &projection,
            "lineage-one",
        )
        .unwrap();
        let loaded = agent_doc_snapshot_io::load_crdt_recovery_projection(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.projection, projection);
        assert_eq!(loaded.lineage, "lineage-one");

        let replacement =
            RelayHub::from_text(CANONICAL_CLIENT_ID, "projection two\n").projection_bytes();
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(
            &doc,
            &replacement,
            "lineage-two",
        )
        .unwrap();
        let loaded = agent_doc_snapshot_io::load_crdt_recovery_projection(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.projection, replacement);
        assert_eq!(loaded.lineage, "lineage-two");
    }

    fn seed_live_reliable_sync_open(file: &str) {
        let pid = std::process::id();
        let document_hash = agent_doc_hash::document_id_for_path(Path::new(file));
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: pid.into(),
                tag: format!("test-editor-{pid}:{file}"),
            }]);
    }

    #[test]
    fn durable_response_cell_barrier_does_not_wait_for_outbound_projection_receipt() {
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
        let first = add_response_cell_for_file(&doc, None, response, "test")
            .unwrap()
            .expect("editor-attached response add should use the relay");
        assert!(first.applied);
        assert_eq!(first.live_editors, 1);
        assert_eq!(first.targets, 1);
        assert!(!first.delivery_converged);
        assert!(first.content.contains(response));

        assert!(
            commit_barrier_for_file_with_authority(&doc, CrdtAuthority::MultiReplica),
            "the barrier owns only the inbound editor-to-canonical cut"
        );
        assert!(
            commit_barrier_for_durable_response_cell(&doc),
            "durable intent must not make an outbound editor receipt transition authority"
        );

        let pull = pull_replica_updates_for_file(&doc, identity)
            .unwrap()
            .expect("live editor should receive the response delivery");
        assert!(!pull.updates.is_empty());
        for update in pull.updates {
            assert_eq!(
                observe_replica_projection_for_file(&doc, identity, &update.expected_content_hash,)
                    .unwrap(),
                Some(true),
            );
        }
        assert!(
            commit_barrier_for_durable_response_cell(&doc),
            "folding the receipt remains idempotent and does not change barrier authority"
        );

        assert!(
            agent_doc_snapshot_io::load_crdt_recovery_projection(&doc)
                .unwrap()
                .is_none(),
            "response delivery must not materialize a CRDT recovery sidecar"
        );
        let document_hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert!(hub_registry().lock().remove(&document_hash).is_some());
        with_hub(&doc, |hub| assert!(hub.canonical_text().contains(response))).unwrap();

        let replay = add_response_cell_for_file(&doc, None, response, "test-replay")
            .unwrap()
            .expect("replay should still use the relay");
        assert!(!replay.applied);
        assert_eq!(replay.cell_id, first.cell_id);
        assert_eq!(replay.content, first.content);
    }

    #[test]
    fn response_cell_repairs_duplicate_review_close_on_live_canonical_cut() {
        let (_dir, doc) = temp_doc("duplicate-review-close.md");
        let live_cut = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n",
            "<!-- agent:boundary:current -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- [#operator-added] keep the row typed during this turn\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:review -->\n",
            "- [x] reviewed\n",
            "<!-- /agent:review -->\n",
            "<!-- /agent:review -->\n",
        );
        std::fs::write(&doc, live_cut).unwrap();
        let mut hub = RelayHub::from_text(CANONICAL_CLIENT_ID, live_cut);
        let response = "### Re: operator prompt — gpt-5\n\nDone once.";

        let first = apply_response_cell_on_hub(
            &mut hub,
            &doc,
            CrdtAuthority::GitAuthoritative,
            None,
            response,
        )
        .expect("response transaction should repair binary scaffolding before parsing");

        assert!(first.applied);
        assert_eq!(first.content.matches("<!-- /agent:review -->").count(), 1);
        assert!(first.content.contains("[#operator-added]"));
        assert!(!first.content.contains("operator-deleted"));
        assert_eq!(first.content.matches(response).count(), 1);
        assert!(agent_doc_element::element::structural_corruption_reason(&first.content).is_none());

        let replay = apply_response_cell_on_hub(
            &mut hub,
            &doc,
            CrdtAuthority::GitAuthoritative,
            None,
            response,
        )
        .expect("exact replay should remain idempotent after repair");
        assert!(!replay.applied);
        assert_eq!(replay.content, first.content);
    }

    #[test]
    fn response_cell_relay_supersedes_restored_uncommitted_tail() {
        let (_dir, doc) = temp_doc("supersede-response-cell.md");
        let committed = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n\n",
            "### Re: committed — gpt-5 (HEAD)\n\nCommitted.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = committed.replace(
            "<!-- agent:boundary:old -->",
            concat!(
                "### Re: stale retry one — gpt-5\n\nStale one.\n\n",
                "❯ follow-up typed while delivery recovers\n\n",
                "### Re: stale retry two — gpt-5\n\nStale two.\n",
                "<!-- agent:boundary:new -->",
            ),
        );
        std::fs::write(&doc, &current).unwrap();
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        register_replica_for_file(&doc, "intellij:supersede-response-cell")
            .unwrap()
            .expect("live editor should register with the relay");

        let latest = "### Re: latest retry — gpt-5\n\nLatest.";
        let write = add_response_cell_for_file(&doc, Some(committed), latest, "test-supersede")
            .unwrap()
            .expect("editor-attached response add should use the relay");

        assert!(write.applied);
        assert!(write.content.contains("Committed."));
        assert!(!write.content.contains("Stale one."));
        assert!(!write.content.contains("Stale two."));
        assert!(
            write
                .content
                .contains("follow-up typed while delivery recovers")
        );
        assert!(write.content.contains("Latest."));
        assert_eq!(write.content.matches(latest).count(), 1);
        assert_eq!(write.content.matches("agent:boundary:").count(), 1);

        let replay =
            add_response_cell_for_file(&doc, Some(committed), latest, "test-supersede-replay")
                .unwrap()
                .expect("exact replay should still use the relay");
        assert!(!replay.applied);
        assert_eq!(replay.cell_id, write.cell_id);
        assert_eq!(replay.content, write.content);
        assert_eq!(replay.content.matches(latest).count(), 1);
        assert_eq!(replay.content.matches("agent:boundary:").count(), 1);
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
    fn live_relay_identity_follows_a_filesystem_path_transition() {
        let (dir, old_doc) = temp_doc("mary-elle-zellerbach.md");
        let old_hash = agent_doc_fs::document_state_hash(&old_doc).unwrap();
        seed_live_reliable_sync_open(&old_doc.display().to_string());
        let identity = "intellij:rename-live-replica";
        register_replica_for_file(&old_doc, identity)
            .unwrap()
            .expect("live editor should register before rename");
        let expected = std::fs::read_to_string(&old_doc).unwrap();

        let new_doc = dir.path().join("mary-ellen-zellerbach.md");
        std::fs::rename(&old_doc, &new_doc).unwrap();
        let new_hash = agent_doc_fs::document_state_hash(&new_doc).unwrap();
        seed_live_reliable_sync_open(&new_doc.display().to_string());

        let report = rekey_live_document_path(&old_doc, &new_doc).unwrap();

        assert_eq!(
            report,
            LiveDocumentPathRekeyReport {
                hub_moved: true,
                replica_identities_moved: 1,
                embedded_route_moved: true,
            },
        );
        assert!(
            !hub_is_allocated(&old_hash),
            "the removed path must not retain a second canonical head",
        );
        assert!(hub_is_allocated(&new_hash));
        assert!(embedded_relay_route_is_registered_for_file(&new_doc));
        let current =
            current_text_for_file_with_authority(&new_doc, CrdtAuthority::MultiReplica).unwrap();
        match current {
            CurrentText::Current {
                text,
                live_editors,
                delivery_converged,
                ..
            } => {
                assert_eq!(text, expected);
                assert_eq!(live_editors, 1);
                assert!(delivery_converged);
            }
            other => panic!("moved relay must remain current, got {other:?}"),
        }
        assert!(
            pull_replica_updates_for_file(&new_doc, identity)
                .unwrap()
                .is_some(),
            "the existing editor identity must remain routable through the new path",
        );
    }

    #[test]
    fn live_editor_registration_bootstraps_detached_authority_without_open_fact_race() {
        let (_dir, doc) = temp_doc("editor-register-first.md");
        let editor_pid = 424_242;

        let generic = register_replica_for_file_incremental_with_liveness(
            &doc,
            "generic:must-remain-detached",
            None,
            None,
            |_| true,
        )
        .unwrap();
        assert!(
            generic.is_none(),
            "a generic replica must not allocate a hub for detached authority"
        );
        assert!(!hub_is_allocated(
            &agent_doc_fs::document_state_hash(&doc).unwrap()
        ));

        let editor = register_replica_for_file_incremental_with_liveness(
            &doc,
            "intellij-424242:/tmp/editor-register-first.md",
            None,
            Some(editor_pid),
            |pid| pid == editor_pid,
        )
        .unwrap()
        .expect("the live process-scoped editor claim should bootstrap its document model");

        assert_ne!(editor.client_id, 0);
        assert!(
            crdt_authority_for_file(&doc).editor_attached(),
            "the allocated routed hub must keep subsequent compact/write authority editor-owned"
        );
    }

    #[test]
    fn dead_editor_registration_cannot_bootstrap_detached_authority() {
        let (_dir, doc) = temp_doc("dead-editor-register.md");
        let registration = register_replica_for_file_incremental_with_liveness(
            &doc,
            "intellij-999999:/tmp/dead-editor-register.md",
            None,
            Some(999_999),
            |_| false,
        )
        .unwrap();

        assert!(registration.is_none());
        assert!(!hub_is_allocated(
            &agent_doc_fs::document_state_hash(&doc).unwrap()
        ));
    }

    #[test]
    fn replacement_registration_returns_only_the_canonical_delta_from_retained_state() {
        let (_dir, doc) = temp_doc("incremental-register.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let original = std::fs::read_to_string(&doc).unwrap();

        let (old_client_id, original_bootstrap) =
            register_replica_for_file(&doc, "intellij:incremental-old")
                .unwrap()
                .expect("initial editor should receive the full bootstrap");
        let original_replica = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(
            old_client_id,
            &original_bootstrap,
        )
        .unwrap();
        let retained_state = original_replica.encode_state();
        let retained_state_vector = original_replica.state_vector();

        let updated = format!("{original}\ncanonical suffix after controller reload\n");
        apply_cp_write_for_file(&doc, &original, &updated, "test_incremental_register")
            .unwrap()
            .expect("canonical write should use the live relay");
        let pull = pull_replica_updates_for_file(&doc, "intellij:incremental-old")
            .unwrap()
            .expect("old editor should receive the canonical suffix");
        for update in pull.updates {
            assert_eq!(
                observe_replica_projection_for_file(
                    &doc,
                    "intellij:incremental-old",
                    &update.expected_content_hash,
                )
                .unwrap(),
                Some(true),
            );
        }

        let registration = register_replica_for_file_incremental(
            &doc,
            "intellij:incremental-new",
            Some(&retained_state_vector),
        )
        .unwrap()
        .expect("replacement editor should receive an incremental registration");
        assert!(
            registration.incremental,
            "a valid retained frontier must not receive another full bootstrap"
        );

        let resumed = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(
            registration.client_id,
            &retained_state,
        )
        .unwrap();
        resumed.apply_update(&registration.bootstrap).unwrap();
        assert_eq!(resumed.text(), updated);
        assert_eq!(
            resumed.state_vector(),
            registration.canonical_state_vector,
            "the returned delta and frontier must describe the same canonical cut"
        );
    }

    #[test]
    fn replacement_registration_preserves_unsettled_canonical_projection_receipt() {
        let (_dir, doc) = temp_doc("retained-register.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let original = std::fs::read_to_string(&doc).unwrap();
        let base_identity = "intellij:retained-register";

        let (old_client_id, original_bootstrap) = register_replica_for_file(&doc, base_identity)
            .unwrap()
            .expect("initial editor should receive the full bootstrap");
        let original_replica = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(
            old_client_id,
            &original_bootstrap,
        )
        .unwrap();
        let updated = format!("{original}\ncontroller response awaiting visibility\n");
        apply_cp_write_for_file(&doc, &original, &updated, "test_retained_register")
            .unwrap()
            .expect("canonical write should use the live relay");

        let replacement_identity = "intellij:retained-register:refresh-1";
        let registration = register_replica_for_file_incremental(
            &doc,
            replacement_identity,
            Some(&original_replica.state_vector()),
        )
        .unwrap()
        .expect("replacement editor should receive a safe canonical bootstrap");
        assert!(!registration.incremental);
        assert!(registration.canonical_projection_retained);
        let replacement = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(
            registration.client_id,
            &registration.bootstrap,
        )
        .unwrap();
        assert_eq!(replacement.text(), updated);

        let pull = pull_replica_updates_for_file(&doc, replacement_identity)
            .unwrap()
            .expect("replacement identity should own the visible receipt");
        assert_eq!(pull.updates.len(), 1);
        let receipt = &pull.updates[0];
        assert!(receipt.patch_id.starts_with("crdt-bootstrap:"));
        assert_eq!(
            receipt.expected_content_hash,
            registration.canonical_content_hash
        );
        assert_eq!(
            observe_replica_projection_for_file(
                &doc,
                replacement_identity,
                &receipt.expected_content_hash,
            )
            .unwrap(),
            Some(true),
        );
    }

    #[test]
    fn replacement_registration_rejects_a_retained_frontier_ahead_of_canonical() {
        let (_dir, doc) = temp_doc("ahead-register.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let canonical = std::fs::read_to_string(&doc).unwrap();

        let (old_client_id, original_bootstrap) =
            register_replica_for_file(&doc, "intellij:ahead-old")
                .unwrap()
                .expect("initial editor should receive the full bootstrap");
        let retained = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(
            old_client_id,
            &original_bootstrap,
        )
        .unwrap();
        retained.apply_local_edit(
            canonical.chars().count() as u32,
            0,
            "\nstale retained suffix\n",
        );

        let registration = register_replica_for_file_incremental(
            &doc,
            "intellij:ahead-new",
            Some(&retained.state_vector()),
        )
        .unwrap()
        .expect("replacement editor should receive a safe bootstrap");

        assert!(
            !registration.incremental,
            "a retained frontier ahead of canonical must receive a full bootstrap"
        );
        let replacement = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(
            registration.client_id,
            &registration.bootstrap,
        )
        .unwrap();
        assert_eq!(
            replacement.text(),
            canonical,
            "the replacement must not union-replay stale retained operations"
        );
        assert_eq!(
            replacement.state_vector(),
            registration.canonical_state_vector,
            "the full bootstrap and advertised canonical frontier must match"
        );
    }

    #[test]
    fn editor_process_id_recognizes_supported_editor_identities_only() {
        assert_eq!(
            editor_process_id("jetbrains-1234-a1b2:/tmp/doc.md:refresh-2"),
            Some(1234)
        );
        assert_eq!(
            editor_process_id("vscode-5678-c3d4:/tmp/doc.md"),
            Some(5678)
        );
        assert_eq!(editor_process_id("zed-9012-e5f6:/tmp/doc.md"), Some(9012));
        assert_eq!(editor_process_id("intellij:legacy"), None);
        assert_eq!(editor_process_id("jetbrains-not-a-pid-id"), None);
        assert_eq!(editor_process_id("zed-not-a-pid-id"), None);
    }

    #[test]
    fn logical_replica_identity_collapses_only_numeric_refresh_generations() {
        let base = "jetbrains-1234-a1b2:/tmp/doc.md";
        assert_eq!(logical_replica_identity(base), base);
        assert_eq!(
            logical_replica_identity("jetbrains-1234-a1b2:/tmp/doc.md:refresh-89"),
            base,
        );
        assert_eq!(
            logical_replica_identity("jetbrains-1234-a1b2:/tmp/doc.md:refresh-next"),
            "jetbrains-1234-a1b2:/tmp/doc.md:refresh-next",
        );
    }

    #[test]
    fn replacement_registration_prunes_dead_member_and_preserves_delivery_barrier() {
        let (_dir, doc) = temp_doc("dead-editor-member.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let stale_identity = "jetbrains-1111-stale:/tmp/dead-editor-member.md";
        let stale_id = register_replica_for_file_with_liveness(&doc, stale_identity, |_| true)
            .unwrap()
            .expect("stale editor should initially attach")
            .0;

        let current = match current_text_for_file(&doc).unwrap() {
            CurrentText::Current { text, .. } => text,
            other => panic!("expected relay current text, got {other:?}"),
        };
        let next = format!("{current}\n### Re: retained\n\nComplete response.\n");
        apply_cp_write_for_file(&doc, &current, &next, "test_dead_editor_prune")
            .unwrap()
            .expect("canonical response write should queue delivery");
        with_hub(&doc, |hub| {
            assert!(!hub.delivery_converged());
            assert!(!hub.pending_updates(stale_id).unwrap().is_empty());
        })
        .unwrap();

        let replacement_identity = format!(
            "jetbrains-{}-replacement:/tmp/dead-editor-member.md",
            std::process::id()
        );
        let replacement_id =
            register_replica_for_file_with_liveness(&doc, &replacement_identity, |pid| pid != 1111)
                .unwrap()
                .expect("replacement editor should attach")
                .0;

        with_hub(&doc, |hub| {
            assert!(!hub.is_registered(stale_id));
            assert!(hub.is_registered(replacement_id));
            assert_eq!(hub.live_count(), 1);
            assert!(!hub.delivery_converged());
            let pending = hub.pending_updates(replacement_id).unwrap();
            assert_eq!(pending.len(), 1);
            assert!(pending[0].patch_id.starts_with("crdt-bootstrap:"));
        })
        .unwrap();

        let pull = pull_replica_updates_for_file(&doc, &replacement_identity)
            .unwrap()
            .expect("replacement should receive the retained visible receipt");
        for update in pull.updates {
            assert_eq!(
                observe_replica_projection_for_file(
                    &doc,
                    &replacement_identity,
                    &update.expected_content_hash,
                )
                .unwrap(),
                Some(true),
            );
        }
        with_hub(&doc, |hub| assert!(hub.delivery_converged())).unwrap();
    }

    #[test]
    fn refresh_replaces_one_logical_replica_and_fences_both_late_frame_paths() {
        let (_dir, doc) = temp_doc("logical-refresh-fence.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let base_identity = format!(
            "jetbrains-{}-logical-refresh:/tmp/logical-refresh-fence.md",
            std::process::id(),
        );
        let (old_id, old_bootstrap) =
            register_replica_for_file_with_liveness(&doc, &base_identity, |_| true)
                .unwrap()
                .expect("initial logical replica should attach");
        let old_lineage = current_lineage_for_file(&doc)
            .unwrap()
            .expect("initial lineage");
        let old_replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(old_id, &old_bootstrap).unwrap();
        let old_frontier = old_replica.state_vector();
        let old_len = old_replica.text().chars().count() as u32;
        old_replica.apply_local_edit(old_len, 0, "late retired generation\n");
        let late_delta = old_replica.diff(&old_frontier).unwrap();

        let refresh_identity = format!("{base_identity}:refresh-1");
        let (refresh_id, refresh_bootstrap) =
            register_replica_for_file_with_liveness(&doc, &refresh_identity, |_| true)
                .unwrap()
                .expect("replacement logical replica should attach");
        let refresh_lineage = current_lineage_for_file(&doc)
            .unwrap()
            .expect("replacement lineage");
        assert_ne!(old_lineage, refresh_lineage);
        with_hub(&doc, |hub| {
            assert!(!hub.is_registered(old_id));
            assert!(hub.is_registered(refresh_id));
            assert_eq!(hub.live_count(), 1);
        })
        .unwrap();

        let clean = current_text_for_file(&doc).unwrap();
        let clean = match clean {
            CurrentText::Current { text, .. } => text,
            other => panic!("expected relay current text, got {other:?}"),
        };
        let clean_boundary_count = clean.matches("agent:boundary:").count();
        let ignored = relay_replica_update_for_file(&doc, &base_identity, &late_delta)
            .unwrap()
            .expect("stale generation is terminally acknowledged");
        assert!(ignored.update.is_empty());
        assert!(ignored.targets.is_empty());
        assert_eq!(
            apply_document_op_delta_for_file_in_lineage(&doc, Some(&old_lineage), &late_delta,)
                .unwrap(),
            Some(DocumentOpDeltaOutcome::StaleLineage),
        );
        with_hub(&doc, |hub| assert_eq!(hub.canonical_text(), clean)).unwrap();

        let refresh_replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(refresh_id, &refresh_bootstrap)
                .unwrap();
        let refresh_frontier = refresh_replica.state_vector();
        let refresh_len = refresh_replica.text().chars().count() as u32;
        refresh_replica.apply_local_edit(refresh_len, 0, "current operator edit\n");
        let current_delta = refresh_replica.diff(&refresh_frontier).unwrap();
        relay_replica_update_for_file(&doc, &refresh_identity, &current_delta)
            .unwrap()
            .expect("current generation update should relay");
        // A durable replay of the same frame is idempotent in the current
        // lineage and can never append a second document copy.
        assert_eq!(
            apply_document_op_delta_for_file_in_lineage(
                &doc,
                Some(&refresh_lineage),
                &current_delta,
            )
            .unwrap(),
            Some(DocumentOpDeltaOutcome::Applied { changed: false }),
        );
        with_hub(&doc, |hub| {
            let canonical = hub.canonical_text();
            assert_eq!(canonical.matches("current operator edit").count(), 1);
            assert_eq!(
                canonical.matches("agent:boundary:").count(),
                clean_boundary_count,
            );
            assert!(!canonical.contains("late retired generation"));
            assert_eq!(hub.live_count(), 1);
        })
        .unwrap();
    }

    #[test]
    fn retired_refresh_deregister_preserves_same_pid_attachment_until_last_generation_closes() {
        #[derive(Default)]
        struct NoopWatcher;
        impl agent_doc_document_realtime::editor_attach::ProcessExitWatcher for NoopWatcher {
            fn watch(&self, _pid: u32) {}
        }

        let (_dir, doc) = temp_doc("logical-refresh-pid-attachment.md");
        let file_str = doc.display().to_string();
        let pid = std::process::id();
        seed_live_reliable_sync_open(&file_str);
        let attach = agent_doc_document_realtime::editor_attach::editor_attach();
        attach.install_watcher(std::sync::Arc::new(NoopWatcher));
        attach.attach(&file_str, pid);

        let base_identity =
            format!("jetbrains-{pid}-pid-refresh:/tmp/logical-refresh-pid-attachment.md");
        register_replica_for_file(&doc, &base_identity)
            .unwrap()
            .expect("initial logical replica should attach");
        let refresh_identity = format!("{base_identity}:refresh-1");
        register_replica_for_file(&doc, &refresh_identity)
            .unwrap()
            .expect("replacement logical replica should attach");

        assert!(
            !deregister_editor_replica_for_file(&doc, &base_identity, pid).unwrap(),
            "registration already retired the old logical generation"
        );
        assert!(
            attach.is_attached(&file_str),
            "retiring the old forwarder must preserve the replacement PID Source"
        );
        assert_eq!(crdt_authority_for_file(&doc), CrdtAuthority::MultiReplica);

        assert!(deregister_editor_replica_for_file(&doc, &refresh_identity, pid).unwrap());
        assert!(
            !attach.is_attached(&file_str),
            "the final identity for the PID closes its attachment"
        );
    }

    #[test]
    fn concurrent_refresh_registrations_elect_one_logical_successor() {
        let (_dir, doc) = temp_doc("concurrent-logical-refresh.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let base_identity = format!(
            "jetbrains-{}-concurrent-refresh:/tmp/concurrent-logical-refresh.md",
            std::process::id(),
        );
        register_replica_for_file_with_liveness(&doc, &base_identity, |_| true)
            .unwrap()
            .expect("initial logical replica should attach");

        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();
        for generation in [1, 2] {
            let doc = doc.clone();
            let identity = format!("{base_identity}:refresh-{generation}");
            let thread_barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                thread_barrier.wait();
                let (client_id, bootstrap) =
                    register_replica_for_file_with_liveness(&doc, &identity, |_| true)
                        .unwrap()
                        .expect("concurrent replacement should attach");
                (identity, client_id, bootstrap)
            }));
        }
        barrier.wait();
        let registrations = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        let document_hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let current_id = {
            let registry = replica_identity_registry().lock();
            let members = registry.get(&document_hash).expect("identity metadata");
            let logical_members = members
                .iter()
                .filter(|(_, identity)| logical_replica_identity(identity) == base_identity)
                .map(|(client_id, _)| *client_id)
                .collect::<Vec<_>>();
            assert_eq!(logical_members.len(), 1);
            logical_members[0]
        };
        with_hub(&doc, |hub| {
            assert_eq!(hub.live_count(), 1);
            assert!(hub.is_registered(current_id));
        })
        .unwrap();

        let (retired_identity, retired_id, retired_bootstrap) = registrations
            .iter()
            .find(|(_, client_id, _)| *client_id != current_id)
            .expect("one concurrent generation must be retired");
        let retired_replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(*retired_id, retired_bootstrap)
                .unwrap();
        let frontier = retired_replica.state_vector();
        let len = retired_replica.text().chars().count() as u32;
        retired_replica.apply_local_edit(len, 0, "late concurrent generation\n");
        let late_delta = retired_replica.diff(&frontier).unwrap();
        let ignored = relay_replica_update_for_file(&doc, retired_identity, &late_delta)
            .unwrap()
            .expect("retired generation is terminally ignored");
        assert!(ignored.update.is_empty());
        with_hub(&doc, |hub| {
            assert_eq!(hub.live_count(), 1);
            assert!(!hub.canonical_text().contains("late concurrent generation"));
        })
        .unwrap();
    }

    #[test]
    fn replica_membership_replacement_does_not_close_document_authority() {
        let (_dir, doc) = temp_doc("membership-replacement.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);

        register_replica_for_file(&doc, "intellij:old")
            .unwrap()
            .expect("old member should attach");
        register_replica_for_file(&doc, "intellij:replacement")
            .unwrap()
            .expect("replacement should attach before retirement");
        with_hub(&doc, |hub| assert_eq!(hub.live_count(), 2)).unwrap();

        assert!(deregister_replica_for_file(&doc, "intellij:old").unwrap());
        assert_eq!(crdt_authority_for_file(&doc), CrdtAuthority::MultiReplica);
        with_hub(&doc, |hub| {
            assert_eq!(hub.live_count(), 1);
            assert!(hub.is_registered(mint_client_id("intellij:replacement")));
        })
        .unwrap();

        assert!(deregister_replica_for_file(&doc, "intellij:replacement").unwrap());
        assert_eq!(
            crdt_authority_for_file(&doc),
            CrdtAuthority::MultiReplica,
            "replica membership is not the durable editor-open authority"
        );
        register_replica_for_file(&doc, "intellij:retry")
            .unwrap()
            .expect("a refresh retry must not be refused as detached authority");
    }

    #[test]
    fn committed_empty_hub_is_evicted_and_recontact_restores_canonical() {
        let (_dir, doc) = temp_doc("hub-eviction.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let document_hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let identity = "intellij:hub-eviction";
        let (client_id, _) = register_replica_for_file(&doc, identity)
            .unwrap()
            .expect("the live editor should attach");
        let on_disk = std::fs::read_to_string(&doc).unwrap();

        with_hub(&doc, |hub| {
            hub.apply_local(
                client_id,
                on_disk.chars().count() as u32,
                0,
                "\nunsaved canonical",
            )
            .unwrap();
        })
        .unwrap();
        let canonical = with_hub(&doc, |hub| hub.canonical_text()).unwrap();
        assert_ne!(canonical, on_disk);

        assert!(deregister_replica_for_file(&doc, identity).unwrap());
        assert!(
            hub_is_allocated_for_test(&document_hash),
            "an uncommitted canonical must pin an empty hub"
        );

        std::fs::write(&doc, &canonical).unwrap();
        record_committed_baseline_for_file(&doc);
        assert!(
            !hub_is_allocated_for_test(&document_hash),
            "the committed empty hub should be evicted"
        );
        assert_eq!(
            current_text_for_file(&doc).unwrap(),
            CurrentText::EditorAttachedMissingReplica,
            "eviction must remain a first-class missing-hub state"
        );

        register_replica_for_file(&doc, "intellij:hub-eviction-recontact")
            .unwrap()
            .expect("re-contact should seed a fresh hub from the durable projection");
        with_hub(&doc, |hub| {
            assert_eq!(hub.canonical_text(), canonical);
            assert_eq!(hub.live_count(), 1);
        })
        .unwrap();
        assert!(deregister_replica_for_file(&doc, "intellij:hub-eviction-recontact").unwrap());
        assert!(
            !hub_is_allocated_for_test(&document_hash),
            "last-member deregistration should evict an already committed hub"
        );
        assert!(
            pull_replica_updates_for_file(&doc, "intellij:hub-eviction-recontact")
                .unwrap()
                .is_none()
        );
        assert!(
            !hub_is_allocated_for_test(&document_hash),
            "a stale passive poll must not recreate the hub"
        );
        assert!(!deregister_replica_for_file(&doc, "intellij:hub-eviction-recontact").unwrap());
        assert!(
            !hub_is_allocated_for_test(&document_hash),
            "a late duplicate deregistration must not recreate the hub"
        );
    }

    #[test]
    fn cp_relay_write_requires_current_canonical_baseline() {
        let (_dir, doc) = temp_doc("cp-baseline.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        register_replica_for_file(&doc, "intellij:cp-baseline")
            .unwrap()
            .expect("editor replica should attach");

        let err = apply_cp_write_for_file(
            &doc,
            "stale baseline\n",
            "stale baseline\n### Re: no — gpt-5\n\nNo.\n",
            "test_cp_relay",
        )
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("recovery=retry_crdt_merge"),
            "stale baseline must fail closed before relay mutation: {err:#}"
        );
        with_hub(&doc, |hub| {
            assert!(hub.canonical_text().contains("body"));
            assert_eq!(
                hub.pending_updates(mint_client_id("intellij:cp-baseline"))
                    .unwrap()
                    .len(),
                0
            );
        })
        .unwrap();
    }

    #[test]
    fn cp_relay_write_zero_live_editors_keeps_doc_op_canonical_authority() {
        // The document-op plane feeds canonical independently of relay-member
        // liveness. Zero live members must not demote an existing hub to disk.
        let (_dir, doc) = temp_doc("cp-stale-lease.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        register_replica_for_file(&doc, "intellij:cp-stale")
            .unwrap()
            .expect("editor replica should attach");
        let baseline = match current_text_for_file(&doc).unwrap() {
            CurrentText::Current { text, .. } => text,
            other => panic!("expected relay current text, got {other:?}"),
        };
        // The member goes offline while the durable Open fact remains live.
        let client_id = mint_client_id("intellij:cp-stale");
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
        let result = apply_cp_write_for_file(&doc, &baseline, &next, "test_cp_relay")
            .expect("zero-live canonical write should pass its CAS")
            .expect("an existing doc-op canonical must not demote to disk");
        assert!(result.applied);
        assert_eq!(result.live_editors, 0);
        with_hub(&doc, |hub| assert_eq!(hub.canonical_text(), next)).unwrap();
    }

    #[test]
    fn cp_relay_write_queues_editor_pull_without_file_ipc_sidecar() {
        let (_dir, doc) = temp_doc("cp-write.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        register_replica_for_file(&doc, "intellij:cp-write")
            .unwrap()
            .expect("editor replica should attach");
        let current = match current_text_for_file(&doc).unwrap() {
            CurrentText::Current { text, .. } => text,
            other => panic!("expected relay current text, got {other:?}"),
        };
        let next = format!("{current}\n### Re: relay — gpt-5\n\nRecovered via relay.\n");

        let result = apply_cp_write_for_file(&doc, &current, &next, "test_cp_relay")
            .unwrap()
            .expect("attached CP relay write should apply");
        assert!(result.applied);
        assert_eq!(result.targets, 1);
        assert!(!result.delivery_converged);
        with_hub(&doc, |hub| {
            assert_eq!(hub.canonical_text(), next);
            let pending = hub
                .pending_updates(mint_client_id("intellij:cp-write"))
                .unwrap();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].origin, CANONICAL_CLIENT_ID);
        })
        .unwrap();
    }

    #[test]
    fn idempotent_cp_relay_write_preserves_the_acked_delivery_frontier() {
        let (_dir, doc) = temp_doc("cp-write-fixed-point.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let identity = "intellij:cp-write-fixed-point";
        register_replica_for_file(&doc, identity)
            .unwrap()
            .expect("editor replica should attach");

        let pull = pull_replica_updates_for_file(&doc, identity)
            .unwrap()
            .expect("registered editor should expose its initial delivery");
        for update in pull.updates {
            assert_eq!(
                observe_replica_projection_for_file(&doc, identity, &update.expected_content_hash,)
                    .unwrap(),
                Some(true),
            );
        }
        let current = match current_text_for_file(&doc).unwrap() {
            CurrentText::Current { text, .. } => text,
            other => panic!("expected relay current text, got {other:?}"),
        };
        let before = with_hub(&doc, |hub| {
            assert!(hub.delivery_converged());
            hub.delivery_snapshot()
        })
        .unwrap();

        let result = apply_cp_write_for_file(&doc, &current, &current, "test_cp_fixed_point")
            .unwrap()
            .expect("equal canonical CP write should resolve as a relay no-op");
        assert!(!result.applied);
        assert_eq!(result.update_bytes, 0);
        assert_eq!(result.targets, 0);
        assert!(result.delivery_converged);

        with_hub(&doc, |hub| {
            assert_eq!(hub.delivery_snapshot(), before);
            assert!(
                hub.pending_updates(mint_client_id(identity))
                    .unwrap()
                    .is_empty()
            );
            assert!(hub.delivery_converged());
        })
        .unwrap();
    }

    /// `#deliveryackcut`: a dead editor process is a WRONG cache entry, so it is
    /// removed outright — no zombie member, no tombstone. The member set is a
    /// cache of which replicas exist; reconciling it against pid liveness is
    /// invalidation, and re-registration is the refill.
    #[test]
    fn reconcile_removes_replicas_whose_editor_process_is_gone() {
        let (_dir, file) = temp_doc("reconcile-dead-pid.md");
        std::fs::write(&file, "# Session\n\nseed\n").unwrap();
        let file_str = file.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        // A Zed LSP identity carries the sidecar pid, which is what
        // `dead_editor_replica_ids` reconciles against when the sidecar exits
        // while the Zed application itself remains alive.
        let identity = format!("zed-424242-abcd:{file_str}");
        let (client_id, _bootstrap) = register_replica_for_file(&file, &identity)
            .unwrap()
            .expect("replica should attach");
        assert!(with_hub(&file, |hub| hub.is_registered(client_id)).unwrap());

        // The editor process is gone: every pid reports dead.
        let outcome = reconcile_replicas_against_liveness_with(&file, |_pid| false).unwrap();
        assert_eq!(
            outcome.removed_dead,
            vec![client_id],
            "a replica whose process is gone must be removed from the cache"
        );
        assert!(
            outcome.live_unacked.is_empty(),
            "a removed replica is not awaiting refill"
        );
        assert!(
            !with_hub(&file, |hub| hub.is_registered(client_id)).unwrap(),
            "the stale entry must be gone, not parked as a zombie"
        );
        hub_registry()
            .lock()
            .remove(&agent_doc_fs::document_state_hash(&file).unwrap());
    }

    #[test]
    fn cp_relay_write_recovers_missing_replica_from_retained_projection() {
        // Editor attached (authority) but this process has NO registered relay
        // replica — the transient gap after a controller recycle / editor restart
        // that made JB `Compact Exchange` hard-fail with
        // `crdt_cp_write ... no registered replica yet` (#cpcwritemissingreplica).
        // With the keyed controller projection, the write must recreate the
        // disposable hub and apply rather than aborting.
        let (_dir, doc) = temp_doc("cp-missing-replica.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        register_replica_for_file(&doc, "intellij:cp-recover")
            .unwrap()
            .expect("editor replica should attach");
        let current = match current_text_for_file(&doc).unwrap() {
            CurrentText::Current { text, .. } => text,
            other => panic!("expected relay current text, got {other:?}"),
        };
        // Evict only the relay membership object. The controller's Lazily target
        // remains and is the sole recovery input.
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert!(
            hub_registry().lock().remove(&hash).is_some(),
            "test setup should evict the live hub"
        );
        assert!(
            agent_doc_snapshot_io::load_crdt_recovery_projection(&doc)
                .unwrap()
                .is_none(),
            "relay recovery must not depend on a CRDT sidecar"
        );

        let next = format!("{current}\n### Re: recovered — gpt-5\n\nAfter recycle.\n");
        let result = apply_cp_write_for_file(&doc, &current, &next, "test_cp_relay")
            .unwrap()
            .expect("missing-replica CP write should recover from Lazily and apply");
        assert!(result.applied);
        with_hub(&doc, |hub| assert_eq!(hub.canonical_text(), next)).unwrap();
    }

    /// `#replica-structure-guard`: a connected editor that pushes a stale/truncated
    /// buffer whose merged result would structurally corrupt the canonical (here,
    /// tombstoning the `<!-- /agent:done -->` close marker so `agent:done` is left
    /// unclosed) must not be able to make that corruption authoritative. The guard
    /// restores the canonical to its clean pre-update text and forces the corrupting
    /// editor to re-project. Regression for the 2026-08-06 agent-doc-bugs2.md wedge
    /// where a JetBrains replica_update left the canonical missing its close marker.
    #[test]
    fn relay_update_that_corrupts_canonical_structure_is_restored() {
        let (_dir, doc) = temp_doc("replica-structure-guard.md");
        let body = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- completed work archived in tasks/x.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, body).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap().join(".agent-doc/logs")).unwrap();
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let identity = "intellij:stale-truncated-buffer";
        let (client_id, bootstrap) =
            register_replica_for_file_with_liveness(&doc, identity, |_| true)
                .unwrap()
                .expect("editor replica should attach");
        // The canonical is structurally clean before the corrupting push.
        with_hub(&doc, |hub| {
            assert!(
                agent_doc_element::element::structural_corruption_reason(&hub.canonical_text())
                    .is_none()
            );
        })
        .unwrap();

        // The editor's replica mirrors the clean canonical. Simulate a stale /
        // truncated editor buffer by tombstoning the close marker, then diffing
        // out the corrupting delta exactly as a real plugin would.
        let editor =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap).unwrap();
        let frontier = editor.state_vector();
        let close = "<!-- /agent:done -->";
        let byte_off = editor.text().find(close).unwrap();
        let char_off = editor.text()[..byte_off].chars().count() as u32;
        let char_len = close.chars().count() as u32;
        editor.apply_local_edit(char_off, char_len, "");
        assert!(agent_doc_element::element::structural_corruption_reason(&editor.text()).is_some());
        let corrupting_delta = editor.diff(&frontier).unwrap();

        relay_replica_update_for_file(&doc, identity, &corrupting_delta)
            .unwrap()
            .expect("corrupting relay update is handled, not a transport error");

        // The canonical must be restored to its clean pre-update state: the close
        // marker survived and the document is not structurally corrupt.
        with_hub(&doc, |hub| {
            let canonical = hub.canonical_text();
            assert!(
                canonical.contains("<!-- /agent:done -->"),
                "canonical close marker must survive a corrupting replica update:\n{canonical}"
            );
            assert!(
                agent_doc_element::element::structural_corruption_reason(&canonical).is_none(),
                "canonical must remain structurally clean:\n{canonical}"
            );
            assert!(
                hub.awaits_canonical_projection(client_id),
                "the corrupting editor must be forced to re-project the clean canonical"
            );
        })
        .unwrap();

        // The rejection is audited.
        let ops_log =
            std::fs::read_to_string(doc.parent().unwrap().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("crdt_replica_update_corruption_rejected"),
            "ops.log must audit the rejected corruption:\n{ops_log}"
        );
    }

    #[test]
    fn relay_update_after_recycle_waits_for_controller_projection_receipt() {
        let (_dir, doc) = temp_doc("relay-lazy-projection.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let identity = "intellij:lazy-projection";
        let (client_id, bootstrap) = register_replica_for_file(&doc, identity)
            .unwrap()
            .expect("editor replica should attach");

        let stale_editor =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap).unwrap();
        let stale_offset = stale_editor.text().chars().count() as u32;
        stale_editor.apply_local_edit(stale_offset, 0, "\nSTALE WHOLE BUFFER\n");
        let stale_update = stale_editor.encode_state();

        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert!(hub_registry().lock().remove(&hash).is_some());

        let quarantined = relay_replica_update_for_file(&doc, identity, &stale_update)
            .unwrap()
            .expect("a cold relay update should return a quarantined no-op");
        assert!(quarantined.update.is_empty());
        assert!(quarantined.targets.is_empty());

        let disk_text = std::fs::read_to_string(&doc).unwrap();
        let pending = with_hub(&doc, |hub| {
            assert!(hub.controller_projection_established());
            assert!(hub.awaits_canonical_projection(client_id));
            assert_eq!(hub.canonical_text(), disk_text);
            hub.pending_updates(client_id).unwrap().pop().unwrap()
        })
        .unwrap();

        let projected_editor =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &pending.update)
                .unwrap();
        assert_eq!(projected_editor.text(), disk_text);
        with_hub(&doc, |hub| {
            assert!(
                hub.ack_delivery_with_content_hash(
                    client_id,
                    &pending.patch_id,
                    pending.generation,
                    Some(&pending.expected_content_hash),
                )
                .unwrap()
            );
            assert!(!hub.awaits_canonical_projection(client_id));
        })
        .unwrap();

        let projected_frontier = projected_editor.state_vector();
        let queue_offset = projected_editor.text().chars().count() as u32;
        projected_editor.apply_local_edit(queue_offset, 0, "\nNEW QUEUE ITEM\n");
        let queue_delta = projected_editor.diff(&projected_frontier).unwrap();
        let applied = relay_replica_update_for_file(&doc, identity, &queue_delta)
            .unwrap()
            .expect("a post-projection user delta should relay");
        assert!(!applied.update.is_empty());
        with_hub(&doc, |hub| {
            assert!(hub.canonical_text().contains("NEW QUEUE ITEM"));
            assert!(!hub.canonical_text().contains("STALE WHOLE BUFFER"));
        })
        .unwrap();

        assert!(hub_registry().lock().remove(&hash).is_some());
        let registration = register_replica_for_file_incremental(
            &doc,
            identity,
            Some(&stale_editor.state_vector()),
        )
        .unwrap()
        .expect("registration-first recycle should return controller bootstrap");
        assert!(registration.canonical_projection_retained);
        assert!(!registration.incremental);
        with_hub(&doc, |hub| {
            assert!(hub.controller_projection_established());
            assert!(hub.awaits_canonical_projection(client_id));
        })
        .unwrap();

        let registration_first_stale = relay_replica_update_for_file(&doc, identity, &stale_update)
            .unwrap()
            .expect("registration-first stale update should remain quarantined");
        assert!(registration_first_stale.update.is_empty());
        with_hub(&doc, |hub| {
            assert!(hub.canonical_text().contains("NEW QUEUE ITEM"));
            assert!(!hub.canonical_text().contains("STALE WHOLE BUFFER"));
        })
        .unwrap();
    }

    #[test]
    fn cp_relay_write_without_projection_still_fails_closed_on_missing_replica() {
        // Missing replica AND no durable projection to recover from: the write must
        // still fail closed with the actionable "no registered replica yet" error
        // rather than fabricating a hub from raw disk.
        let (_dir, doc) = temp_doc("cp-no-projection.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert!(
            !hub_registry().lock().contains_key(&hash),
            "no hub should be allocated yet"
        );
        assert!(
            agent_doc_snapshot_io::load_crdt_recovery_projection(&doc)
                .unwrap()
                .is_none(),
            "no durable projection should exist"
        );

        let err = apply_cp_write_for_file(&doc, "baseline\n", "baseline\nmore\n", "test_cp_relay")
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
        let registry = hub_registry().lock();
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
            !hub_registry().lock().contains_key(&hash),
            "detached checkpoint must not create a relay hub"
        );
        assert!(
            agent_doc_snapshot_io::load_crdt_recovery_projection(&doc)
                .unwrap()
                .is_none(),
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
    fn editor_attached_projection_recovery_read_still_requires_live_editor_publish() {
        let (_dir, doc) = temp_doc("attached-projection-recovery.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let editor = mint_client_id("intellij:projection-recovery");
        prior.register(editor).unwrap();
        prior.apply_local(editor, 0, 0, "durable recovery").unwrap();
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(
            &doc,
            &prior.projection_bytes(),
            "test:projection-recovery",
        )
        .unwrap();
        hub_registry().lock().remove(&hash);

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
        .expect("recovery read should remain a legal missing-model state");
        assert_eq!(recovered, CurrentText::EditorAttachedMissingReplica);
        assert!(!hub_is_allocated_for_test(&hash));
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
    fn ensure_document_model_does_not_promote_projection_after_publish_timeout() {
        let (_dir, doc) = temp_doc("ensure-model-projection-recovery.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let editor = mint_client_id("intellij:ensure-projection-recovery");
        prior.register(editor).unwrap();
        prior
            .apply_local(editor, 0, 0, "projection after publish")
            .unwrap();
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(
            &doc,
            &prior.projection_bytes(),
            "test:ensure-projection-recovery",
        )
        .unwrap();
        hub_registry().lock().remove(&hash);

        let poll_count = Arc::new(Mutex::new(0usize));
        let poll_count_for_observer = Arc::clone(&poll_count);
        let err = ensure_document_model_with_current_text_recovery_observer(
            &doc,
            "test_projection_recovery",
            CurrentText::EditorAttachedMissingReplica,
            || {
                *poll_count_for_observer.lock() += 1;
                Ok(CurrentText::EditorAttachedMissingReplica)
            },
            || {
                current_text_for_file_with_authority_recovering_projection(
                    &doc,
                    CrdtAuthority::MultiReplica,
                )
            },
        )
        .expect_err("attached authority must wait for an exact live editor publish");

        assert!(
            *poll_count.lock() > 0,
            "ensure should poll the strict observer before recovery"
        );
        assert!(format!("{err:#}").contains("editor authority stayed"));
        assert!(!hub_is_allocated_for_test(&hash));
    }

    #[test]
    fn ensure_document_model_extends_window_when_editor_registers_a_replica() {
        // `#ensurewindowsize`: a LIVE editor whose bootstrap is too large to land
        // inside DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS must not be
        // judged stale. A completed registration is liveness proof, so the window
        // extends and the editor gets to finish. Without the extension this
        // observer never gets far enough to return Current and queue maintenance
        // can never persist on a large document.
        let (_dir, doc) = temp_doc("ensure-model-window-extend.md");
        let polls = Arc::new(Mutex::new(0usize));
        let polls_for_observer = Arc::clone(&polls);
        let doc_for_observer = doc.clone();

        let current = ensure_document_model_with_current_text_observer(
            &doc,
            "test_window_extend",
            CurrentText::EditorAttachedMissingReplica,
            move || {
                let mut n = polls_for_observer.lock();
                *n += 1;
                if *n == 1 {
                    // The editor answers by registering, but its bootstrap is
                    // still in flight — the model is not observable yet.
                    note_replica_registration(&doc_for_observer);
                    return Ok(CurrentText::EditorAttachedMissingReplica);
                }
                if *n < 4 {
                    return Ok(CurrentText::EditorAttachedMissingReplica);
                }
                Ok(CurrentText::Current {
                    text: "live editor text".to_string(),
                    live_editors: 1,
                    delivery_converged: true,
                    delivery_version: 1,
                    semantics: None,
                })
            },
        )
        .expect("a registering editor must be given the full window, not failed as stale");

        assert!(
            matches!(current, CurrentText::Current { .. }),
            "expected the extended window to reach Current, got {current:?}"
        );
        assert!(
            *polls.lock() >= 4,
            "the observer must keep polling past the short missing-replica window"
        );
    }

    #[test]
    fn ensure_document_model_fails_closed_for_missing_replica_within_short_window() {
        let (_dir, doc) = temp_doc("ensure-model-missing-replica-recycle.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let editor = mint_client_id("intellij:missing-replica-recycle");
        prior.register(editor).unwrap();
        prior
            .apply_local(editor, 0, 0, "recycled from projection")
            .unwrap();
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(
            &doc,
            &prior.projection_bytes(),
            "test:missing-replica-recycle",
        )
        .unwrap();
        hub_registry().lock().remove(&hash);

        let poll_count = Arc::new(Mutex::new(0usize));
        let poll_count_for_observer = Arc::clone(&poll_count);
        // The editor never registers a replica (stale/half-synced): the strict
        // observer stays at EditorAttachedMissingReplica forever.
        let err = ensure_document_model_with_current_text_recovery_observer(
            &doc,
            "test_missing_replica_recycle",
            CurrentText::EditorAttachedMissingReplica,
            || {
                *poll_count_for_observer.lock() += 1;
                Ok(CurrentText::EditorAttachedMissingReplica)
            },
            || {
                current_text_for_file_with_authority_recovering_projection(
                    &doc,
                    CrdtAuthority::MultiReplica,
                )
            },
        )
        .expect_err("missing-replica ensure must not replace a live editor from projection");
        assert!(format!("{err:#}").contains("editor authority stayed"));
        assert!(!hub_is_allocated_for_test(&hash));
        // `#missingreplicarecycle`: the missing-replica case uses the short window
        // (`DOCUMENT_MODEL_ENSURE_MISSING_REPLICA_TIMEOUT_MS` = 60ms in test /
        // `DOCUMENT_MODEL_ENSURE_POLL_MS` = 25ms → ~2-3 polls), well under the full
        // `DOCUMENT_MODEL_ENSURE_TIMEOUT_MS` (150ms → ~6 polls), so a stale editor
        // cannot block the single-threaded controller for the full window.
        let polls = *poll_count.lock();
        assert!(
            (1..=4).contains(&polls),
            "missing-replica ensure should poll only within the short window, got {polls}"
        );
    }

    /// Model ensure is a projection observer. Missing and syncing replicas both
    /// fail closed without emitting a re-registration or editor-content request.
    #[test]
    fn ensure_document_model_does_not_request_editor_recovery() {
        let (_dir, doc) = temp_doc("ensure-model-reactive-projection.md");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        hub_registry().lock().remove(&hash);

        for state in [
            CurrentText::EditorAttachedMissingReplica,
            CurrentText::EditorSyncPending,
        ] {
            let _ = ensure_document_model_with_current_text_observer(
                &doc,
                "test_reactive_projection",
                state.clone(),
                || Ok(state.clone()),
            );
        }

        let log = std::fs::read_to_string(_dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("document_model_ensure_replica_reregister")
                && !log.contains("lazily_current_observation_requested"),
            "projection observation must not send recovery commands, got:\n{log}"
        );
    }

    #[test]
    fn ensure_document_model_does_not_promote_compacted_projection_over_live_editor() {
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
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(
            &doc,
            &prior.projection_bytes(),
            "test:compact-exchange-recovery",
        )
        .unwrap();
        hub_registry().lock().remove(&hash);

        let err = ensure_document_model_with_current_text_recovery_observer(
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
        .expect_err("compacted restart state must not replace an attached editor cut");
        assert!(format!("{err:#}").contains("editor authority stayed"));
        assert!(!hub_is_allocated_for_test(&hash));
        let retained = agent_doc_snapshot_io::load_crdt_recovery_projection(&doc)
            .unwrap()
            .unwrap();
        let rebuilt =
            RelayHub::recover_from_projection(CANONICAL_CLIENT_ID, &retained.projection).unwrap();
        assert_eq!(rebuilt.canonical_text(), compacted);
        assert!(rebuilt.canonical_text().contains(kept_block));
    }

    #[test]
    fn ensure_document_model_does_not_promote_projection_after_publish_transport_failure() {
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
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(
            &canonical,
            &prior.projection_bytes(),
            "test:publish-transport-failure",
        )
        .unwrap();
        hub_registry().lock().remove(&hash);

        std::fs::write(dir.path().join(".agent-doc").join("patches"), "not a dir").unwrap();

        let err = ensure_document_model(&canonical, "test_publish_transport_failure")
            .expect_err("transport failure must not authorize stale projection promotion");
        assert!(format!("{err:#}").contains("editor authority stayed"));
        assert!(!hub_is_allocated_for_test(&hash));

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("document_model_ensure_republish_observer_not_ready")
                && log.contains("recovery=retry_without_disk_write")
                && log.contains("document_model_ensure_failed")
                && log.contains("source=test_publish_transport_failure"),
            "failed publish transport should be audited and fail closed for live-editor republish:\n{log}"
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
    fn projection_observation_defers_without_creating_a_repair_request() {
        let (_dir, doc) = temp_doc("durable-checkpoint-deferred.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);

        let outcome = checkpoint_durable_projection_for_file(&doc, "test_recycle").unwrap();
        match outcome {
            DurableProjectionCheckpoint::Deferred { reason } => {
                assert_eq!(reason, "controller_projection_unavailable");
            }
            other => panic!("expected deferred projection observation, got {other:?}"),
        }
        assert!(
            !_dir.path().join(".agent-doc/crdt-repair").exists(),
            "projection observation must not create a sidecar repair request"
        );
        let log = std::fs::read_to_string(_dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("crdt_projection_observe_deferred"),
            "projection observation should report the unavailable cell:\n{log}"
        );
        assert!(
            !log.contains("background_yrs_repair"),
            "projection observation must not schedule a repair request:\n{log}"
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
            "multi-replica commit barrier must fail closed on missing CP relay model:\n{log}"
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
    fn editor_attached_projection_observation_does_not_write_recovery_sidecar() {
        let (_dir, doc) = temp_doc("attached-checkpoint.md");
        let file_str = doc.display().to_string();
        seed_live_reliable_sync_open(&file_str);
        let editor = mint_client_id("intellij:durable-checkpoint");
        with_hub(&doc, |hub| {
            hub.register(editor).unwrap();
            hub.apply_local(editor, 0, 0, "checkpointed").unwrap();
        })
        .unwrap();

        let outcome = checkpoint_durable_projection_for_file(&doc, "test_recycle").unwrap();

        match outcome {
            DurableProjectionCheckpoint::Checkpointed {
                changed: false,
                live_editors: 1,
                ..
            } => {}
            other => panic!("expected retained projection observation, got {other:?}"),
        }
        assert!(
            agent_doc_snapshot_io::load_crdt_recovery_projection(&doc)
                .unwrap()
                .is_none(),
            "observing the live projection must not materialize a recovery sidecar"
        );
        let document_hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        assert!(hub_registry().lock().remove(&document_hash).is_some());
        with_hub(&doc, |hub| {
            assert!(hub.canonical_text().contains("checkpointed"));
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
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let result = reconcile_disk_projection_for_file_with_authority(
            &doc,
            b"any-bytes-are-ignored",
            CrdtAuthority::GitAuthoritative,
        )
        .unwrap();
        assert_eq!(result, None, "the headless path performs no live reconcile");
        assert!(
            !hub_registry().lock().contains_key(&hash),
            "the headless path must not allocate a relay hub"
        );
    }

    #[test]
    fn recover_hub_from_projection_rebuilds_canonical_on_restart() {
        // Supervisor restart: rebuild the canonical replica from the last disk
        // recovery projection; members re-register / re-sync afterward.
        let (_dir, doc) = temp_doc("recover.md");
        // Build a projection from a throwaway hub (simulating a prior session).
        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let ed = mint_client_id("intellij:prior");
        prior.register(ed).unwrap();
        prior.apply_local(ed, 0, 0, "durable").unwrap();
        let projection = prior.projection_bytes();

        recover_hub_from_projection(&doc, &projection, None).unwrap();
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

    #[test]
    fn route_signal_leaves_headless_disk_as_authority() {
        // No live editor → decide_watch_action yields ApplyAsDiskAuthority, which
        // the disk-authority load path owns — no marker for a supervisor to consume.
        let (_dir, file) = temp_doc("route-headless.md");
        let action =
            route_disk_change_signal(&file, &WatchDelivery::Change { generation: 1 }).unwrap();
        assert_eq!(action, WatchAction::ApplyAsDiskAuthority);
    }

    #[test]
    fn route_signal_ignores_non_change_delivery() {
        let (_dir, file) = temp_doc("route-echo.md");
        let action = route_disk_change_signal(&file, &WatchDelivery::SelfWriteEcho).unwrap();
        assert_eq!(action, WatchAction::None);
    }
}
