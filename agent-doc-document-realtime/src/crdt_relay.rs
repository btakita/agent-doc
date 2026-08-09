//! Multiple-editor relay hub + ephemeral awareness (`#crdtauth4`, plan phase 5).
//!
//! A **star-topology relay hub** built on top of the state-vector sync primitive
//! ([`agent_doc_merge::crdt_sync`]) and gated by the CRDT-authority state machine
//! ([`crate::crdt_authority`]). It is the fan-out / registry
//! layer the plan calls for (`tasks/agent-doc/plan-crdt-authority-model.md`,
//! "Multiple editors"):
//!
//! - The **project controller/CP hosts the canonical replica**; editor replicas
//!   register/deregister with the hub. On a replica's local update the hub pulls
//!   that op into the canonical replica and **broadcasts only the missing update**
//!   to every OTHER live replica via the existing `diff(their_sv)` /
//!   `apply_update` state-vector machinery — never a whole-document snapshot after
//!   first contact (a registering replica bootstraps once from canonical's encoded
//!   state, then exchanges deltas).
//! - **Unique stable client-ids** are enforced: a collision is a hard error
//!   (collision = corruption per the plan). [`mint_client_id`] mints a
//!   deterministic id from a stable string identity.
//! - **Awareness / presence** ([`AwarenessChannel`]) is a SEPARATE in-memory
//!   structure (cursor / selection / user per client-id). It is explicitly **NOT
//!   part of the document CRDT, NOT persisted, NOT committed** — it is dropped on
//!   deregister and never reaches the durable CRDT projection / git.
//! - The **commit barrier is a consistent cut of the currently-live replicas**:
//!   [`RelayHub::commit_barrier`] flushes only the live members (reusing
//!   [`agent_doc_merge::crdt_sync::flush_to_commit_barrier`]) and never blocks on a
//!   slow / disconnected editor — a commit is a checkpoint, not a global lock. An
//!   offline editor contributes its ops at next sync ([`RelayHub::reconnect`]).
//! - **Offline → reconnect convergence**: a replica that missed updates while
//!   disconnected converges via a bidirectional state-vector catch-up on
//!   reconnect (no data loss — its offline edits flow into canonical and the
//!   missed updates flow back into it).
//!
//! Disk demotion (plan phase 6) lives alongside this: the canonical replica is
//! the live authority while a session is up; the durable CRDT projection is a
//! write-through **recovery projection only**. See [`RelayHub::projection_bytes`],
//! [`RelayHub::recover_from_projection`], [`RelayHub::reconcile_disk_projection`],
//! and [`DISK_IS_RECOVERY_PROJECTION_ONLY`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use lazily::{
    Computed, EphemeralMapCore, Source, ThreadSafeContext, ThreadSafeQueueCell, ThreadSafeSemTree,
    ThreadSafeSourceMap,
};
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use agent_doc_merge::crdt_sync::{ReplicaState, commit_barrier_ready, flush_to_commit_barrier};
use agent_doc_merge::document_cell::ThreadSafeDocumentCellTree;

use crate::crdt_authority::CrdtAuthority;

/// **Persistence-demotion contract (plan phase 6).** The CRDT bytes checkpointed
/// in `state.db` are a **durable recovery projection only** — never the coordination
/// medium and never the source of truth while a session is live. The Lazily-owned
/// canonical replica is authoritative; the ledger is recovered from on restart.
/// This constant is the
/// single in-code statement of that contract, asserted by tests and consulted by
/// callers that must not treat a persisted projection as authority.
pub const DISK_IS_RECOVERY_PROJECTION_ONLY: bool = true;

/// Explicit opt-in for the live, per-node document projection (`#cdtcutover`).
///
/// The projection is default-off while it gathers live-session evidence. An
/// explicit truthy value (`1`, `true`, `on`, or `yes`) enables it for newly
/// constructed relay hubs.
pub const CELL_DOC_TREE_CUTOVER_ENV: &str = "AGENT_DOC_CELL_DOC_TREE_CUTOVER";

fn cell_doc_tree_cutover_enabled() -> bool {
    std::env::var(CELL_DOC_TREE_CUTOVER_ENV)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on" | "yes"
            )
        })
        .unwrap_or(false)
}

struct LiveDocumentProjection {
    tree: ThreadSafeDocumentCellTree,
    unresolved_prompts: ThreadSafeSemTree<String, usize>,
}

impl LiveDocumentProjection {
    fn new(ctx: &ThreadSafeContext, document: &str) -> Self {
        let tree = ThreadSafeDocumentCellTree::from_document(ctx, document);
        let unresolved_prompts = tree.unresolved_prompt_counts(ctx);
        Self {
            tree,
            unresolved_prompts,
        }
    }

    fn update_to(&mut self, ctx: &ThreadSafeContext, old_document: &str, new_document: &str) {
        if old_document == new_document {
            return;
        }
        if self.tree.update_to(ctx, old_document, new_document) {
            self.unresolved_prompts = self.tree.unresolved_prompt_counts(ctx);
        }
    }
}

/// One registered editor replica's hub-side mirror.
struct Member {
    /// The supervisor's mirror of this editor's replica (synced via deltas).
    replica: ReplicaState,
    generation: u64,
    last_ack_generation: u64,
    pending: VecDeque<PendingReplicaUpdate>,
    /// `#pullnoackdeadlock`: how many times this member has been handed the same
    /// undelivered head without its ACK ever advancing. Reset by any ACK that
    /// moves `last_ack_generation`, and by a fresh enqueue.
    redeliveries_without_ack: u32,
}

/// `#pullnoackdeadlock`: redeliveries of one unacked head before a replica stops
/// holding the convergence barrier.
///
/// A healthy editor ACKs the delivery it just pulled, so this is never reached.
/// The wedge it bounds is a replica that pulls forever and never ACKs: observed
/// 2026-08-09 on `tasks/agent-doc/agent-doc-bugs2.md`, where client
/// `5162727547735464` re-pulled `current_generation=5 last_ack_generation=4` at
/// ~2/s indefinitely — 23372 `delivery_converged=false` observations — wedging
/// every write behind the delivery barrier and making preflight refuse
/// admission with `Lazily current authority remained delivery_pending`.
///
/// At the observed ~2 pulls/second this is roughly 25 seconds of a replica
/// asking for the same bytes over and over, which no healthy editor does.
pub const MAX_REDELIVERIES_WITHOUT_ACK: u32 = 50;

/// A fan-out packet: an `update` (delta) originating from `origin` that must be
/// delivered to each replica in `targets`. Returned by
/// [`RelayHub::submit_local`] so a caller (or a SimWorld) controls delivery
/// timing / ordering; [`RelayHub::apply_local`] delivers immediately.
#[derive(Debug, Clone)]
pub struct BroadcastPacket {
    /// The replica whose local edit produced this update.
    pub origin: u64,
    /// The incremental update (only the new op(s)) to apply on each target.
    pub update: Vec<u8>,
    /// The currently-live OTHER replicas that should receive `update`.
    pub targets: Vec<u64>,
}

/// One supervisor-to-editor delivery awaiting a matching visible-state projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReplicaUpdate {
    pub patch_id: String,
    pub origin: u64,
    pub target: u64,
    pub generation: u64,
    /// Hash of the canonical visible text the editor must actually show before
    /// this delivery may advance the projection frontier. Generation alone proves only
    /// that a frame was handled, not that the native replica and editor buffer
    /// converged (#crdt-content-ack).
    pub expected_content_hash: String,
    pub update: Vec<u8>,
}

fn content_hash(text: &str) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(text.as_bytes()) {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

/// Minimal single-span codepoint edit turning `current` into `content`.
///
/// Canonical response writes used to delete and reinsert the whole document.
/// That retained every tombstone and turned a 32 KiB response into a 7 MiB
/// delta. Shared prefix/suffix peeling preserves CRDT lineage outside the
/// changed span and bounds the update by the actual response edit.
fn minimal_char_span_edit(current: &str, content: &str) -> Result<Option<(u32, u32, String)>> {
    if current == content {
        return Ok(None);
    }
    let current_chars = current.chars().collect::<Vec<_>>();
    let content_chars = content.chars().collect::<Vec<_>>();
    let mut prefix = 0usize;
    let max_prefix = current_chars.len().min(content_chars.len());
    while prefix < max_prefix && current_chars[prefix] == content_chars[prefix] {
        prefix += 1;
    }
    let mut suffix = 0usize;
    let max_suffix = (current_chars.len() - prefix).min(content_chars.len() - prefix);
    while suffix < max_suffix
        && current_chars[current_chars.len() - 1 - suffix]
            == content_chars[content_chars.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let delete_len = current_chars.len() - prefix - suffix;
    let insert = content_chars[prefix..content_chars.len() - suffix]
        .iter()
        .collect::<String>();
    Ok(Some((
        prefix
            .try_into()
            .map_err(|_| anyhow!("canonical edit offset exceeds CRDT codepoint range"))?,
        delete_len
            .try_into()
            .map_err(|_| anyhow!("canonical edit length exceeds CRDT codepoint range"))?,
        insert,
    )))
}

/// Delivery/ACK state for one registered editor replica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaDeliverySnapshot {
    pub client_id: u64,
    pub live: bool,
    pub pending_updates: usize,
    pub current_generation: u64,
    pub last_ack_generation: u64,
    /// `#pullnoackdeadlock`: redeliveries of the same unacked head.
    pub redeliveries_without_ack: u32,
    /// Whether this replica still blocks [`RelayHub::delivery_converged`].
    pub holds_delivery_barrier: bool,
}

/// Outcome of routing an out-of-band disk change into the hub
/// ([`RelayHub::apply_disk_change`]). This is the CP-replica side of the
/// file-watch propagation path (`plan-crdt-scramble-and-disk-propagation.md`
/// Phases C/D): the watcher hands the settled disk text to the hub, and the hub
/// decides how it relates to the live canonical replica.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiskChangeOutcome {
    /// The canonical replica already reflects the disk text — a live editor that
    /// authored the change (or a peer that already pulled it) means reconcile is
    /// a **no-op**. This is the "editor buffer already has the changes" case
    /// (goal 5): nothing to propagate.
    AlreadyReconciled,
    /// The disk was corrected **out of band** (a `git checkout HEAD` /
    /// `reset --from-current` / external edit the hub did not author) in a way the
    /// additive CRDT delta cannot express — typically a content-removing
    /// correction. The canonical replica was rebuilt from disk and hub-side member
    /// mirrors reseeded, but the `live_members` live editor buffers still hold the
    /// stale text. Propagating a *deletion* to them needs a replace-capable
    /// delivery (Phase D2 — a bootstrap/replace message the editor applies by
    /// replacing its buffer, not CRDT-merging). Until D2 lands, the caller must
    /// re-bootstrap those editors; this variant makes that requirement explicit
    /// rather than silently leaving them stale.
    RebuiltFromDisk { live_members: usize },
    /// No commit baseline had been recorded yet (a hub allocated mid-session
    /// before its first finalize), so the disk text was adopted as the baseline
    /// without touching the canonical replica — a later out-of-band correction is
    /// now detectable. The canonical still differs from disk; the change is
    /// deferred to the normal editor-delta / commit-barrier path rather than being
    /// forced through here.
    BaselineDeferred,
}

/// Result of admitting a durable document-op batch into the canonical CRDT.
/// Additive updates are idempotent only inside one CRDT lineage; a batch from
/// an obsolete lineage is terminally quarantined so reliable-sync can advance
/// its ACK cursor without corrupting the replacement canonical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentOpDeltaOutcome {
    Applied { changed: bool },
    StaleLineage,
    LegacyQuarantined,
}

/// Controller-owned in-memory projection retained independently of a relay
/// member generation. It is deliberately not serialized to the CRDT recovery
/// sidecar: a relay recycle reattaches to this live value, while a cold process
/// starts from the normal controller document input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedCanonicalProjection {
    pub state: Vec<u8>,
    pub lineage: String,
    pub last_committed_text: Option<String>,
    pub compact_epoch_requested: bool,
}

/// Star-topology relay hub: one canonical replica + N registered editor replicas.
pub struct RelayHub {
    /// The CP-owned canonical replica (the hub / git-checkpoint authority).
    canonical: ReplicaState,
    canonical_id: u64,
    /// Opaque identity of the current additive CRDT history. Rotated whenever
    /// the canonical is replaced from text/full-state rather than advanced by
    /// an update from that history.
    lineage: String,
    /// Rolling-upgrade compatibility for plugins that predate lineage-tagged
    /// document-op frames. Once a replacement rotates lineage, untagged frames
    /// are ambiguous and must be quarantined.
    legacy_document_ops_allowed: bool,
    members: HashMap<u64, Member>,
    awareness: AwarenessChannel,
    /// The document text this hub last committed to disk (`#staleinmem`). `None`
    /// until the first commit is recorded via [`Self::record_committed_baseline`].
    /// Used by [`Self::reconcile_canonical_against_baseline`] to detect an
    /// out-of-band disk correction (a `git checkout HEAD` / `reset` recovery the
    /// hub did not author) so the stale canonical can be rebuilt from the
    /// correction instead of re-committing the discarded content forever.
    last_committed_text: Option<String>,
    /// Live editors that need a **replace-capable re-bootstrap** (D2): after an
    /// out-of-band deletion rebuilds the canonical, an additive CRDT delta cannot
    /// express the removal, so each live editor must replace its buffer with the
    /// corrected canonical text. Populated by [`Self::apply_disk_change`] on a
    /// `RebuiltFromDisk`; drained by the caller which delivers the replace and
    /// calls [`Self::clear_rebootstrap`].
    pending_rebootstrap: HashSet<u64>,
    /// Compact Exchange requested a fresh lineage, but a newer canonical
    /// delivery still lacks the all-live visible-state proof. The final matching
    /// projection settles this retained effect and then queues rebootstrap.
    compact_epoch_requested: bool,
    /// Members carrying retained state into a restarted relay, whether
    /// registration or update arrives first. Their retained CRDT lineage cannot
    /// be union-merged with a canonical freshly seeded from disk: both lineages
    /// may encode the complete visible document, which would concatenate it.
    /// Updates remain fenced until the controller's retained canonical target
    /// has been projected to the member. This is a reactive source-map
    /// projection joined to the hub graph, not state inferred from RPC arrival
    /// order or recovered from an editor whole-buffer request.
    canonical_projection_required: ThreadSafeSourceMap<u64, bool>,
    /// Optional live per-node document projection. It shares this hub's
    /// [`ThreadSafeContext`] and is updated at every canonical mutation boundary.
    /// Default-off; see [`CELL_DOC_TREE_CUTOVER_ENV`].
    live_document_projection: Option<Mutex<LiveDocumentProjection>>,
    /// The thread-safe reactive graph that owns member liveness (#live-editor-reactive).
    /// `RelayHub` lives in a `static Mutex<HashMap<String, RelayHub>>`, so every
    /// reactive handle stored here must be `Send`; [`ThreadSafeContext`] and the
    /// `Arc`-based [`ThreadSafeSourceMap`]/[`Source`]/[`Computed`] all qualify.
    ctx: ThreadSafeContext,
    /// False until the controller canonical has been projected into the current
    /// relay generation. This reactive fact prevents command arrival order from
    /// selecting an editor buffer as authority.
    controller_projection_established: Source<bool>,
    /// Per-member liveness as a keyed reactive family (keyed by `client_id`). This is
    /// the **single** source of truth for whether a member is connected — the former
    /// `Member.live` field is gone. The present set only grows (deferral, not
    /// de-allocation): a deregistered `client_id`'s cell stays present-but-false, so it
    /// is bounded per session and never counted as live.
    liveness: ThreadSafeSourceMap<u64, bool>,
    /// Bumped on [`Self::register`] so the derived count picks up a newly-present key
    /// (a brand-new cell is not yet a dependency of `live_editor_count`; the epoch is,
    /// so the register forces a recompute that then observes the new cell).
    membership_epoch: Source<u64>,
    /// Reactive derived count of currently-live members: recomputes as
    /// `count(present_keys whose cell is true)` whenever the epoch or any observed
    /// liveness cell changes. [`Self::live_count`] is a reactive read of this slot.
    live_editor_count: Computed<usize>,
    /// `#lazily-hot-path` Theme A — monotonic version of every input to the
    /// delivery-convergence fold: the member set, each member's `pending` queue, and
    /// liveness. Bumped by [`Self::bump_delivery_epoch`] at each of those writes.
    ///
    /// This exists so a consumer can ask *"has convergence changed since I looked?"*
    /// instead of re-running an expensive re-read on a timer — the
    /// [`EditorReplicaLivenessWitness`] idiom, where suppression tracks the fact
    /// rather than a clock. The fold itself ([`Self::delivery_converged`]) stays
    /// authoritative; the epoch only says when re-folding could produce a new answer.
    delivery_epoch: Source<u64>,
    /// Race-free blocking subscription for changes to [`Self::delivery_epoch`].
    ///
    /// Lazily's thread-safe queue supplies the reactive notification cell. The
    /// one-element queue always retains the newest published epoch, while the
    /// condition variable parks non-reactive RPC threads without polling the
    /// graph. Publication and the pre-wait observation share one gate, so a
    /// transition cannot land between "unchanged" and sleeping.
    delivery_subscription: DeliveryConvergenceSubscription,
}

/// The reactive core shared by every [`RelayHub`] constructor. A named struct rather
/// than a tuple so adding an input to the graph stays readable at the call site.
struct LivenessCore {
    ctx: ThreadSafeContext,
    liveness: ThreadSafeSourceMap<u64, bool>,
    canonical_projection_required: ThreadSafeSourceMap<u64, bool>,
    controller_projection_established: Source<bool>,
    membership_epoch: Source<u64>,
    live_editor_count: Computed<usize>,
    delivery_epoch: Source<u64>,
    delivery_subscription: DeliveryConvergenceSubscription,
}

/// `#lazily-hot-path` Theme A — a point-in-time reading of delivery convergence
/// together with the version of the inputs that produced it.
///
/// Two witnesses with the same `version` were computed from identical inputs, so a
/// consumer holding an unchanged version can skip its retry work outright. A changed
/// version means only that an input moved — `converged` still carries the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryConvergenceWitness {
    pub version: u64,
    pub converged: bool,
}

/// A cloneable, race-free subscription to delivery-convergence input changes.
///
/// `ThreadSafeQueueCell` is intentionally bounded to one element: subscribers
/// need the newest invalidation version, not a replay of every intermediate
/// member/queue write. The queue is the reactive notification source; the
/// condition variable is only the blocking adapter for controller RPC threads.
#[derive(Clone)]
pub struct DeliveryConvergenceSubscription {
    ctx: ThreadSafeContext,
    notifications: ThreadSafeQueueCell<u64>,
    wait_gate: Arc<(Mutex<()>, Condvar)>,
}

impl DeliveryConvergenceSubscription {
    fn new(ctx: &ThreadSafeContext) -> Self {
        Self {
            ctx: ctx.clone(),
            notifications: ThreadSafeQueueCell::with_capacity(ctx, 1),
            wait_gate: Arc::new((Mutex::new(()), Condvar::new())),
        }
    }

    fn publish(&self, version: u64) {
        let (gate, changed) = &*self.wait_gate;
        let _guard = gate.lock();

        // Keep one coalesced latest-version notification. All queue operations
        // run under the same gate as wait registration, while lazily itself
        // releases queue storage before invalidating the ThreadSafeContext.
        if self.notifications.try_push(&self.ctx, version).is_err() {
            let _ = self.notifications.try_pop(&self.ctx);
            self.notifications
                .try_push(&self.ctx, version)
                .expect("coalesced convergence queue must accept its replacement");
        }
        changed.notify_all();
    }

    /// Block until the published convergence version differs from `after`, or
    /// until `timeout` elapses. Returns `true` for a changed version.
    pub fn wait_for_change(&self, after: u64, timeout: Duration) -> bool {
        let deadline = Instant::now().checked_add(timeout);
        let (gate, changed) = &*self.wait_gate;
        let mut guard = gate.lock();

        loop {
            if self
                .notifications
                .head(&self.ctx)
                .is_some_and(|version| version != after)
            {
                return true;
            }
            let Some(deadline) = deadline else {
                changed.wait(&mut guard);
                continue;
            };
            if changed.wait_until(&mut guard, deadline).timed_out() {
                return self
                    .notifications
                    .head(&self.ctx)
                    .is_some_and(|version| version != after);
            }
        }
    }
}

impl RelayHub {
    fn mint_lineage() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn rotate_lineage(&mut self) {
        self.lineage = Self::mint_lineage();
        self.legacy_document_ops_allowed = false;
    }

    /// Fence durable frames produced by a superseded incarnation of one logical
    /// editor replica without changing the canonical text/state.
    ///
    /// Editor integrations reconnect by opening a fresh native replica before
    /// retiring the old one.  The fresh replica receives the current lineage;
    /// rotating it at the replacement boundary makes every late durable frame
    /// from the prior incarnation terminally stale instead of union-merging it
    /// into the replacement canonical.
    pub fn fence_replica_generation(&mut self) {
        self.rotate_lineage();
    }

    /// Build the thread-safe reactive liveness core shared by every constructor:
    /// a `ThreadSafeContext`, an on-demand `client_id -> live` cell family, a membership
    /// epoch cell, and the derived live-member count slot.
    ///
    /// **Lifetime (`#lazilyscopeadopt`).** This core deliberately does *not* use
    /// lazily's `ctx.scope() -> TeardownScope`. The context is private to one
    /// `RelayHub` and is dropped with it, so hub disposal already tears the whole
    /// graph down; a scope would only re-express that ownership. And within one
    /// hub the graph is bounded: `mint_client_id` derives a client id
    /// deterministically from a **stable** editor identity, so connection churn
    /// re-materializes the same cells rather than minting new ones. Both bounds
    /// are asserted against lazily's edge introspection in
    /// `reconnect_churn_on_stable_identity_does_not_grow_the_liveness_edge_set`
    /// and `liveness_edge_set_is_bounded_by_distinct_identities_not_churn`. The
    /// remaining process-lifetime growth is one hub per document in
    /// `agent-doc-crdt-relay-io`'s `hub_registry`, which is a registry-eviction
    /// question, not a reactive-scope one.
    fn build_liveness_core() -> LivenessCore {
        // #stategraphjoin-allow: owned by the `RelayHub` and dropped with it, so hub
        // disposal already tears the whole graph down and a scope would only re-express
        // that ownership. Growth is bounded: `mint_client_id` derives client ids from a
        // STABLE editor identity, so connection churn re-materializes the same cells.
        // Both bounds are asserted against lazily's edge introspection in this module.
        let ctx = ThreadSafeContext::new();
        let membership_epoch = ctx.source(0u64);
        let delivery_epoch = ctx.source(0u64);
        let controller_projection_established = ctx.source(false);
        // Cells materialize on `register`; the factory value (`true` = live-on-register)
        // only applies before the explicit `set` in `set_live`.
        let liveness: ThreadSafeSourceMap<u64, bool> = ThreadSafeSourceMap::new(&ctx);
        let canonical_projection_required: ThreadSafeSourceMap<u64, bool> =
            ThreadSafeSourceMap::new(&ctx);
        let delivery_subscription = DeliveryConvergenceSubscription::new(&ctx);
        let live_editor_count = {
            let liveness = liveness.clone();
            ctx.computed(move |ctx| {
                // Depend on the membership epoch so a newly-registered (not-yet-observed)
                // key forces a recompute that then picks it up in `present_keys`.
                let _ = ctx.get(&membership_epoch);
                liveness
                    .present_keys()
                    .into_iter()
                    .filter(|id| liveness.observe(ctx, id).unwrap_or(false))
                    .count()
            })
        };
        LivenessCore {
            ctx,
            liveness,
            canonical_projection_required,
            controller_projection_established,
            membership_epoch,
            live_editor_count,
            delivery_epoch,
            delivery_subscription,
        }
    }

    /// Materialize (if needed) and set member `client_id`'s liveness cell. Never holds
    /// the family lock across the `ctx` write (the family releases its lock before
    /// touching `ctx`), so there is no lock-order cycle with the registry mutex.
    fn set_live(&self, client_id: u64, live: bool) {
        self.liveness.set(&self.ctx, client_id, live);
        // Liveness selects which members the convergence fold considers, so a
        // transition changes the answer even with no queue mutation.
        self.bump_delivery_epoch();
    }

    /// Advance the delivery-convergence input version (see [`Self::delivery_epoch`]).
    ///
    /// Called at every write that can change [`Self::delivery_converged`]: a member
    /// registering or leaving, a liveness transition, and each mutation of a member's
    /// `pending` queue. Missing a call here does not corrupt the fold — it only makes
    /// a consumer suppress a re-check it should have made — so the bumps are placed at
    /// the mutation sites themselves rather than inferred by a caller.
    fn bump_delivery_epoch(&self) {
        let epoch = self.ctx.get(&self.delivery_epoch);
        let next = epoch.wrapping_add(1);
        self.ctx.set(&self.delivery_epoch, next);
        self.delivery_subscription.publish(next);
    }

    /// Bump the membership epoch so `live_editor_count` recomputes and counts a
    /// newly-present key. Called on [`Self::register`] only (value-only transitions
    /// already dirty the derived count through the cell dependency).
    fn bump_membership_epoch(&self) {
        let epoch = self.ctx.get(&self.membership_epoch);
        self.ctx.set(&self.membership_epoch, epoch.wrapping_add(1));
        // The member set is an input to the convergence fold too.
        self.bump_delivery_epoch();
    }

    /// Reactive read of member `client_id`'s liveness (the single source of truth that
    /// replaced `Member.live`). Delta-routing / barrier sites read this instead of a
    /// per-member `live` field.
    fn is_live(&self, client_id: u64) -> bool {
        self.liveness
            .observe(&self.ctx, &client_id)
            .unwrap_or(false)
    }

    fn sync_live_document_projection(&self, old_document: &str, new_document: &str) {
        if let Some(projection) = &self.live_document_projection {
            projection
                .lock()
                .update_to(&self.ctx, old_document, new_document);
        }
    }

    fn reset_live_document_projection(&mut self, document: &str) {
        if let Some(projection) = &self.live_document_projection {
            *projection.lock() = LiveDocumentProjection::new(&self.ctx, document);
        }
    }

    /// Whether this hub owns the opt-in live per-node projection.
    pub fn live_document_projection_enabled(&self) -> bool {
        self.live_document_projection.is_some()
    }

    /// Memoized unresolved-prompt count for the whole live canonical document.
    ///
    /// `None` means the default-off cutover gate was not enabled for this hub.
    pub fn unresolved_prompt_count(&self) -> Option<usize> {
        self.unresolved_prompt_counts().map(|(total, _)| total)
    }

    /// Memoized unresolved-prompt count for one component occurrence.
    pub fn unresolved_prompt_count_for_component(
        &self,
        component: &str,
        occurrence: usize,
    ) -> Option<usize> {
        let node_id = format!("{component}:{occurrence}");
        self.live_document_projection
            .as_ref()
            .and_then(|projection| {
                projection
                    .lock()
                    .unresolved_prompts
                    .node_value(&self.ctx, &node_id)
            })
    }

    /// Read the whole-document and first queue-occurrence counts under one
    /// projection lock. A missing queue occurrence contributes zero.
    pub fn unresolved_prompt_counts(&self) -> Option<(usize, usize)> {
        self.live_document_projection.as_ref().map(|projection| {
            let projection = projection.lock();
            let total = projection.unresolved_prompts.value(&self.ctx);
            let queue = projection
                .unresolved_prompts
                .node_value(&self.ctx, &"queue:0".to_string())
                .unwrap_or(0);
            (total, queue)
        })
    }

    /// Create a hub whose canonical replica uses `canonical_id` as its CRDT peer
    /// peer id. `canonical_id` is reserved — no member may register with it.
    pub fn new(canonical_id: u64) -> Self {
        let LivenessCore {
            ctx,
            liveness,
            canonical_projection_required,
            controller_projection_established,
            membership_epoch,
            live_editor_count,
            delivery_epoch,
            delivery_subscription,
        } = Self::build_liveness_core();
        let live_document_projection = cell_doc_tree_cutover_enabled()
            .then(|| Mutex::new(LiveDocumentProjection::new(&ctx, "")));
        Self {
            canonical: ReplicaState::new(canonical_id),
            canonical_id,
            lineage: Self::mint_lineage(),
            legacy_document_ops_allowed: true,
            members: HashMap::new(),
            awareness: AwarenessChannel::new(),
            last_committed_text: None,
            pending_rebootstrap: HashSet::new(),
            compact_epoch_requested: false,
            canonical_projection_required,
            live_document_projection,
            ctx,
            controller_projection_established,
            liveness,
            membership_epoch,
            live_editor_count,
            delivery_epoch,
            delivery_subscription,
        }
    }

    /// Create a hub whose canonical replica is already seeded from the current
    /// editor-visible document text. File-backed live sessions use this on first
    /// allocation so the first editor delta is never applied to an empty replica.
    pub fn from_text(canonical_id: u64, text: &str) -> Self {
        let mut hub = Self::new(canonical_id);
        hub.canonical = ReplicaState::from_text(canonical_id, text);
        hub.reset_live_document_projection(text);
        hub.last_committed_text = Some(text.to_string());
        hub
    }

    /// Recover a hub from a durable disk **recovery projection** (plan phase 6):
    /// rebuild the in-memory canonical replica from the last durable snapshot on
    /// restart. At most one flush is lost; live editors re-sync their newer ops on
    /// reconnect. The projection is a recovery input, never authority.
    pub fn recover_from_projection(canonical_id: u64, projection: &[u8]) -> Result<Self> {
        Self::recover_from_projection_with_lineage(canonical_id, projection, None)
    }

    /// Recover a hub while preserving the lineage paired with the durable
    /// projection. When no matching metadata exists, mint a fresh lineage so
    /// obsolete durable deltas fail closed.
    pub fn recover_from_projection_with_lineage(
        canonical_id: u64,
        projection: &[u8],
        lineage: Option<&str>,
    ) -> Result<Self> {
        let canonical = ReplicaState::from_encoded(canonical_id, projection)?;
        // Seed the committed baseline from the recovered text so the very first
        // commit barrier after a restart can already detect an out-of-band disk
        // correction / compaction (`#staleinmem`) instead of waiting for a finalize
        // to record one.
        let last_committed_text = Some(canonical.text());
        let mut hub = Self::new(canonical_id);
        hub.canonical = canonical;
        let recovered_text = hub.canonical.text();
        hub.reset_live_document_projection(&recovered_text);
        if let Some(lineage) = lineage.filter(|value| !value.is_empty()) {
            hub.lineage = lineage.to_string();
        }
        hub.last_committed_text = last_committed_text;
        Ok(hub)
    }

    /// Recreate only the disposable relay around a controller-retained
    /// canonical target.
    pub fn from_retained_canonical_projection(
        canonical_id: u64,
        projection: &RetainedCanonicalProjection,
    ) -> Result<Self> {
        let mut hub = Self::recover_from_projection_with_lineage(
            canonical_id,
            &projection.state,
            Some(&projection.lineage),
        )?;
        hub.last_committed_text = projection.last_committed_text.clone();
        hub.compact_epoch_requested = projection.compact_epoch_requested;
        Ok(hub)
    }

    /// Snapshot the live controller target for a keyed Lazily cell. This is an
    /// in-memory reactive value, not a persistence or recovery-sidecar API.
    pub fn retained_canonical_projection(&self) -> RetainedCanonicalProjection {
        RetainedCanonicalProjection {
            state: self.canonical.encode_state(),
            lineage: self.lineage.clone(),
            last_committed_text: self.last_committed_text.clone(),
            compact_epoch_requested: self.compact_epoch_requested,
        }
    }

    pub fn lineage(&self) -> &str {
        &self.lineage
    }

    /// The canonical (authoritative) converged text.
    pub fn canonical_text(&self) -> String {
        self.canonical.text()
    }

    /// A compact revision token for the authoritative canonical replica.
    ///
    /// Unlike [`Self::canonical_text`], this does not materialize the document.
    /// Observation paths can compare the encoded CRDT state vector and fetch the
    /// full text only after the canonical frontier changes.
    pub fn canonical_state_vector(&self) -> Vec<u8> {
        self.canonical.state_vector()
    }

    /// Whether the canonical replica already contains every operation named by a
    /// retained editor frontier.
    ///
    /// A version vector can be decoded successfully while still being *ahead of*
    /// the canonical replica (for example after the controller rebuilt its CRDT
    /// history from a committed text projection). Treating that frontier as an
    /// incremental-bootstrap base would let the replacement editor relabel and
    /// replay the obsolete op history into the new lineage.
    pub fn canonical_covers_state_vector(&self, state_vector: &[u8]) -> Result<bool> {
        self.canonical.covers_state_vector(state_vector)
    }

    /// Encode only the canonical operations missing from `state_vector`.
    ///
    /// Registration normally needs a whole canonical bootstrap because a fresh
    /// editor replica has no prior state. A replacement editor/native generation
    /// can retain its local encoded state across the handoff, though, so sending
    /// its frontier lets the controller return only the missing suffix.
    pub fn canonical_diff(&self, state_vector: &[u8]) -> Result<Vec<u8>> {
        self.canonical.diff(state_vector)
    }

    /// A registered member's current text (for inspection / tests).
    pub fn member_text(&self, client_id: u64) -> Option<String> {
        self.members.get(&client_id).map(|m| m.replica.text())
    }

    /// The number of currently-live (connected) members — a reactive read of the
    /// derived `live_editor_count` slot, not a pull-scan over `members`.
    pub fn live_count(&self) -> usize {
        self.ctx.get(&self.live_editor_count)
    }

    /// Whether the process-global owner may drop this hub without losing live
    /// or uncommitted document state.
    ///
    /// An empty member set alone is insufficient: the canonical may still be
    /// ahead of the last disk commit. Re-contact can safely rebuild from disk
    /// only after that committed baseline exactly matches the canonical text.
    pub fn is_safe_to_evict(&self) -> bool {
        self.members.is_empty()
            && self.pending_rebootstrap.is_empty()
            && self
                .last_committed_text
                .as_ref()
                .is_some_and(|committed| self.canonical.text() == *committed)
    }

    /// Whether `client_id` is registered (live or offline).
    pub fn is_registered(&self, client_id: u64) -> bool {
        self.members.contains_key(&client_id)
    }

    /// Validate that `client_id` may register: it must not collide with the
    /// canonical id or an already-registered member. Collision is a hard error
    /// (collision = corruption per the plan's unique-stable-client-id rule).
    pub fn validate_unique(&self, client_id: u64) -> Result<()> {
        if client_id == self.canonical_id {
            return Err(anyhow!(
                "client-id collision: {client_id} is the canonical replica id"
            ));
        }
        if self.members.contains_key(&client_id) {
            return Err(anyhow!(
                "client-id collision: replica {client_id} is already registered"
            ));
        }
        Ok(())
    }

    /// Register an editor replica with the hub, bootstrapping it from the
    /// canonical replica's encoded state so it starts converged (the single
    /// whole-state exchange on first contact; all later traffic is deltas).
    ///
    /// Errors on a client-id collision (canonical id or already-registered) —
    /// unique stable client-ids are required for deterministic op attribution.
    pub fn register(&mut self, client_id: u64) -> Result<()> {
        self.validate_unique(client_id)?;
        let replica = ReplicaState::from_encoded(client_id, &self.canonical.encode_state())?;
        self.members.insert(
            client_id,
            Member {
                replica,
                generation: 0,
                last_ack_generation: 0,
                pending: VecDeque::new(),
                redeliveries_without_ack: 0,
            },
        );
        // Materialize this member's liveness cell (live-on-register) and bump the
        // membership epoch so the derived count observes the newly-present key.
        self.set_live(client_id, true);
        self.bump_membership_epoch();
        Ok(())
    }

    /// Fence a member whose first contact with this hub was an incremental
    /// update from a retained editor generation.
    pub fn require_canonical_projection(&mut self, client_id: u64) {
        self.canonical_projection_required
            .set(&self.ctx, client_id, true);
    }

    /// Whether this relay generation has projected the controller canonical.
    pub fn controller_projection_established(&self) -> bool {
        self.ctx.get(&self.controller_projection_established)
    }

    /// Publish that the current relay generation consumes the controller-owned
    /// canonical projection.
    pub fn establish_controller_projection(&self) {
        self.ctx.set(&self.controller_projection_established, true);
    }

    /// Whether additive updates from `client_id` must remain quarantined until
    /// the retained controller target is visibly projected to that member.
    pub fn awaits_canonical_projection(&self, client_id: u64) -> bool {
        self.canonical_projection_required
            .observe(&self.ctx, &client_id)
            .unwrap_or(false)
    }

    /// Deregister an editor replica: drop its hub-side mirror AND expire its
    /// ephemeral awareness/presence entry. The awareness channel never outlives a
    /// connection (it is not persisted and not committed).
    pub fn deregister(&mut self, client_id: u64) -> bool {
        self.awareness.remove(client_id);
        let removed = self.members.remove(&client_id).is_some();
        if removed {
            self.pending_rebootstrap.remove(&client_id);
            self.canonical_projection_required
                .set(&self.ctx, client_id, false);
            // The cell stays present-but-false (deferral, not de-allocation) so it is
            // no longer counted; a later re-register flips the same cell back to true.
            self.set_live(client_id, false);
        }
        removed
    }

    /// Mark a member offline (disconnected) without losing its replica state. A
    /// disconnected member is skipped by broadcasts and the commit barrier and
    /// catches up via [`Self::reconnect`]. Its presence entry is expired (a
    /// disconnected cursor must not linger).
    pub fn disconnect(&mut self, client_id: u64) -> bool {
        self.awareness.remove(client_id);
        let existed = match self.members.get_mut(&client_id) {
            Some(m) => {
                m.pending.clear();
                m.redeliveries_without_ack = 0;
                true
            }
            None => false,
        };
        if existed {
            // `set_live` bumps the delivery epoch for both writes.
            self.set_live(client_id, false);
        }
        existed
    }

    /// Reconnect a member: a **bidirectional state-vector catch-up** that proves
    /// no data loss. The member's offline edits flow into the canonical replica
    /// and the updates it missed while offline flow back into it. After this the
    /// member and canonical have converged.
    pub fn reconnect(&mut self, client_id: u64) -> Result<()> {
        let before_text = self.canonical.text();
        let member = self
            .members
            .get_mut(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        member.pending.clear();
        member.redeliveries_without_ack = 0;
        // Pull the member's offline ops into canonical, then push back everything
        // the member missed. Both directions are state-vector deltas.
        let to_canonical = member.replica.diff(&self.canonical.state_vector())?;
        let to_member = self.canonical.diff(&member.replica.state_vector())?;
        self.canonical.apply_update(&to_canonical)?;
        member.replica.apply_update(&to_member)?;
        let after_text = self.canonical.text();
        self.sync_live_document_projection(&before_text, &after_text);
        // Mark live only after a successful bidirectional catch-up (the `member`
        // borrow above must end before touching the reactive `ctx`).
        self.set_live(client_id, true);
        Ok(())
    }

    /// Apply a local edit to member `client_id`'s replica ONLY (the editor typing
    /// into its own local-first replica). The op is **not** yet relayed to the
    /// canonical replica or to peers — this models the editor→supervisor direction
    /// so propagation lag (and the commit barrier's "un-propagated ops" case) is
    /// representable. Call [`Self::relay`] (or use [`Self::apply_local`]) to
    /// propagate, or let [`Self::commit_barrier`] flush it at a checkpoint.
    pub fn local_edit(
        &self,
        client_id: u64,
        offset: u32,
        delete_len: u32,
        insert: &str,
    ) -> Result<()> {
        let member = self
            .members
            .get(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        member.replica.apply_local_edit(offset, delete_len, insert);
        Ok(())
    }

    /// Relay member `client_id`'s pending local ops to the hub: pull everything
    /// the member holds that the canonical replica is missing INTO canonical, then
    /// build the fan-out packet of those new op(s) for every OTHER live member,
    /// **without delivering it** (the caller controls delivery timing / ordering —
    /// used to model fan-out lag and out-of-order delivery). Use [`Self::relay`]
    /// for the immediate-delivery live path.
    pub fn relay_capture(&mut self, client_id: u64) -> Result<BroadcastPacket> {
        let before_text = self.canonical.text();
        let member = self
            .members
            .get(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        // Canonical SV before integrating, so the packet carries exactly the new op(s).
        let before = self.canonical.state_vector();
        let into_canonical = member.replica.diff(&self.canonical.state_vector())?;
        self.canonical.apply_update(&into_canonical)?;
        let after_text = self.canonical.text();
        self.sync_live_document_projection(&before_text, &after_text);
        let update = self.canonical.diff(&before)?;
        let targets: Vec<u64> = self
            .members
            .keys()
            .copied()
            .filter(|id| *id != client_id && self.is_live(*id))
            .collect();
        let packet = BroadcastPacket {
            origin: client_id,
            update,
            targets,
        };
        self.enqueue_delivery(&packet);
        Ok(packet)
    }

    /// Apply a raw encoded lazily `TextCrdt` delta from member `client_id` to that
    /// member's hub-side mirror, integrate the new op(s) into the canonical
    /// replica, and capture the fan-out packet of those op(s) for every OTHER
    /// live member **without delivering it** (the caller controls delivery — the
    /// live IPC path delivers into the hub-side mirrors so the next peer
    /// `ReplicaUpdate`/sync carries them, and returns the per-target deltas to
    /// the requester for socket fan-out).
    ///
    /// This is the IPC-delta analog of [`Self::relay_capture`]: where
    /// `relay_capture` works from a `local_edit` (offset/len) applied to the
    /// mirror, this accepts the encoded update the editor's FFI node produced
    /// (`agent_doc_replica_diff`) so the editor — not the hub — owns the local
    /// edit. Operation identities make apply idempotent and reorder-safe, so a
    /// duplicate or out-of-order update converges rather than corrupting.
    pub fn relay_update_capture(
        &mut self,
        client_id: u64,
        update: &[u8],
    ) -> Result<BroadcastPacket> {
        let before_text = self.canonical.text();
        let member = self
            .members
            .get(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        // Apply the editor's encoded op to its hub-side mirror first.
        member.replica.apply_update(update)?;
        // Then pull whatever the mirror now holds that canonical is missing.
        let before = self.canonical.state_vector();
        let into_canonical = member.replica.diff(&self.canonical.state_vector())?;
        self.canonical.apply_update(&into_canonical)?;
        let after_text = self.canonical.text();
        self.sync_live_document_projection(&before_text, &after_text);
        let delta = self.canonical.diff(&before)?;
        let targets: Vec<u64> = self
            .members
            .keys()
            .copied()
            .filter(|id| *id != client_id && self.is_live(*id))
            .collect();
        let packet = BroadcastPacket {
            origin: client_id,
            update: delta,
            targets,
        };
        self.enqueue_delivery(&packet);
        Ok(packet)
    }

    /// Apply a raw encoded lazily `TextCrdt` delta from `client_id` and **immediately
    /// broadcast** the resulting delta to every other live member's hub-side
    /// mirror (the normal live IPC path). Returns the delivered packet so the
    /// caller can also relay the per-target delta out over the socket to the
    /// peers' FFI nodes.
    pub fn relay_update(&mut self, client_id: u64, update: &[u8]) -> Result<BroadcastPacket> {
        let packet = self.relay_update_capture(client_id, update)?;
        for target in &packet.targets {
            self.deliver(*target, &packet.update)?;
        }
        Ok(packet)
    }

    /// Fold a document-op **delta frame** straight into the canonical replica
    /// **without requiring a registered member** — the durable document-op
    /// replication path (`#docop-plane`, P2). `delta` is the same wire unit as
    /// [`Self::relay_update`]: a `serde_json` `Vec<lazily::TextOp>` (a
    /// `ReplicaState::diff` / `agent-doc-reliable-sync-io::document_op` frame body).
    ///
    /// This is the fix for the `live_editors == 0` freeze: [`Self::relay_update`]
    /// only reaches the canonical through a live registered member, so a connected
    /// plugin whose CRDT member registration lapsed (the phantom lease) could not
    /// feed the canonical and it went stale. A durably-replicated document-op frame
    /// lands here regardless of member state, so the canonical is never frozen while
    /// an editor is connected. Applying is idempotent + commutative (each `TextOp`
    /// carries its `OpId`), so a duplicate or out-of-order frame converges rather
    /// than corrupting. The resulting canonical delta is broadcast to every live
    /// member so connected editors also converge. Returns the broadcast packet;
    /// `packet.update` is the empty-delta encoding when the frame added nothing new.
    pub fn apply_document_op_delta(&mut self, delta: &[u8]) -> Result<BroadcastPacket> {
        let before_text = self.canonical.text();
        let before = self.canonical.state_vector();
        self.canonical.apply_update(delta)?;
        let after_text = self.canonical.text();
        self.sync_live_document_projection(&before_text, &after_text);
        let out = self.canonical.diff(&before)?;
        let targets: Vec<u64> = self
            .members
            .keys()
            .copied()
            .filter(|id| self.is_live(*id))
            .collect();
        let packet = BroadcastPacket {
            origin: self.canonical_id,
            update: out,
            targets,
        };
        self.enqueue_delivery(&packet);
        Ok(packet)
    }

    /// Apply a durable editor delta only when its lineage identifies the
    /// canonical history it was produced from. Mismatches are terminal
    /// quarantine outcomes rather than errors: retrying the same stale frame
    /// cannot make it safe and would wedge the reliable-sync ACK frontier.
    pub fn apply_document_op_delta_in_lineage(
        &mut self,
        lineage: Option<&str>,
        delta: &[u8],
    ) -> Result<DocumentOpDeltaOutcome> {
        match lineage {
            Some(lineage) if lineage != self.lineage => {
                return Ok(DocumentOpDeltaOutcome::StaleLineage);
            }
            None if !self.legacy_document_ops_allowed => {
                return Ok(DocumentOpDeltaOutcome::LegacyQuarantined);
            }
            _ => {}
        }
        let before = self.canonical.text();
        self.apply_document_op_delta(delta)?;
        Ok(DocumentOpDeltaOutcome::Applied {
            changed: self.canonical.text() != before,
        })
    }

    /// The canonical replica's encoded state — the bootstrap snapshot a freshly
    /// registering editor needs on first contact (all later traffic is deltas).
    pub fn canonical_encoded_state(&self) -> Vec<u8> {
        self.canonical.encode_state()
    }

    /// Deliver an update to one target replica (idempotent + causal-buffered by
    /// the CRDT, so out-of-order delivery self-heals once missing ops arrive). A no-op
    /// if the target is gone.
    pub fn deliver(&self, target: u64, update: &[u8]) -> Result<()> {
        if let Some(member) = self.members.get(&target) {
            member.replica.apply_update(update)?;
        }
        Ok(())
    }

    /// Relay member `client_id`'s pending local ops and **immediately broadcast**
    /// them to every other live member (the normal live path). Returns the packet
    /// that was delivered.
    pub fn relay(&mut self, client_id: u64) -> Result<BroadcastPacket> {
        let packet = self.relay_capture(client_id)?;
        for target in &packet.targets {
            self.deliver(*target, &packet.update)?;
        }
        Ok(packet)
    }

    /// Apply a local edit and immediately relay + broadcast it (the normal live
    /// path = [`Self::local_edit`] + [`Self::relay`]). Returns the delivered packet.
    pub fn apply_local(
        &mut self,
        client_id: u64,
        offset: u32,
        delete_len: u32,
        insert: &str,
    ) -> Result<BroadcastPacket> {
        self.local_edit(client_id, offset, delete_len, insert)?;
        self.relay(client_id)
    }

    /// Apply a CP-authored document target to the canonical replica using the
    /// minimal changed span, then queue the resulting CRDT delta for every live
    /// editor replica.
    ///
    /// This is the controller→editor direction of the relay. The caller supplies
    /// the `expected_current` text it merged against; if the canonical text has
    /// moved since then, the write is refused so newer editor-buffer changes are
    /// not overwritten by a stale response.
    pub fn apply_canonical_replace(
        &mut self,
        expected_current: &str,
        content: &str,
    ) -> Result<BroadcastPacket> {
        let current = self.canonical.text();
        if current != expected_current {
            return Err(anyhow!(
                "canonical text changed before CP relay write: expected_len={} current_len={}",
                expected_current.len(),
                current.len()
            ));
        }
        let before = self.canonical.state_vector();
        if let Some((offset, delete_len, insert)) = minimal_char_span_edit(&current, content)? {
            self.canonical.apply_local_edit(offset, delete_len, &insert);
        }
        self.sync_live_document_projection(&current, content);
        let update = self.canonical.diff(&before)?;
        let mut targets: Vec<u64> = self
            .members
            .keys()
            .copied()
            .filter(|id| self.is_live(*id))
            .collect();
        targets.sort_unstable();
        let packet = BroadcastPacket {
            origin: self.canonical_id,
            update,
            targets,
        };
        self.enqueue_delivery(&packet);
        for target in &packet.targets {
            self.deliver(*target, &packet.update)?;
        }
        Ok(packet)
    }

    fn enqueue_delivery(&mut self, packet: &BroadcastPacket) {
        if packet.update.is_empty() {
            return;
        }
        let expected_content_hash = content_hash(&self.canonical.text());
        for target in &packet.targets {
            let Some(member) = self.members.get_mut(target) else {
                continue;
            };
            member.generation += 1;
            let generation = member.generation;
            member.redeliveries_without_ack = 0;
            member.redeliveries_without_ack = 0;
        member.pending.push_back(PendingReplicaUpdate {
                patch_id: format!("crdt:{}:{}:{}", packet.origin, target, generation),
                origin: packet.origin,
                target: *target,
                generation,
                expected_content_hash: expected_content_hash.clone(),
                update: packet.update.clone(),
            });
        }
        // Queueing work for any live member un-converges delivery.
        self.bump_delivery_epoch();
    }

    /// Keep an exact canonical projection visibly unsettled across an editor
    /// replica replacement.
    ///
    /// A newly registered member is already bootstrapped from canonical, so it
    /// does not need another semantic edit. It still needs a hash-qualified
    /// receipt before a controller-authored write may be treated as visible in
    /// the restarted editor. Queueing the canonical encoded state is an
    /// idempotent CRDT update and gives the replacement identity a normal
    /// delivery token that it can ACK after projecting the bootstrap.
    pub fn ensure_canonical_projection_receipt(&mut self, client_id: u64) -> Result<bool> {
        if !self.members.contains_key(&client_id) {
            return Err(anyhow!("replica {client_id} is not registered"));
        }
        self.require_canonical_projection(client_id);
        let expected_content_hash = content_hash(&self.canonical.text());
        let canonical_state = self.canonical.encode_state();
        let canonical_id = self.canonical_id;
        let member = self
            .members
            .get_mut(&client_id)
            .expect("membership checked before canonical projection receipt");
        if member
            .pending
            .back()
            .is_some_and(|update| update.expected_content_hash == expected_content_hash)
        {
            return Ok(false);
        }
        member.generation += 1;
        let generation = member.generation;
        member.pending.push_back(PendingReplicaUpdate {
            patch_id: format!("crdt-bootstrap:{canonical_id}:{client_id}:{generation}"),
            origin: canonical_id,
            target: client_id,
            generation,
            expected_content_hash,
            update: canonical_state,
        });
        self.bump_delivery_epoch();
        Ok(true)
    }

    /// Pull pending supervisor-to-editor updates for `client_id`. Updates remain in
    /// the queue until [`Self::ack_delivery`] confirms the editor applied them.
    pub fn pending_updates(&mut self, client_id: u64) -> Result<Vec<PendingReplicaUpdate>> {
        let member = self
            .members
            .get_mut(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        // `#pullnoackdeadlock`: a pull that hands out the same unacked head again
        // is the observable that bounds the barrier. Counting redeliveries — not
        // stamping a clock — keeps this a pure function of the delivery stream,
        // the same shape `#idlerevisionreactive` settled on.
        if !member.pending.is_empty() {
            member.redeliveries_without_ack = member.redeliveries_without_ack.saturating_add(1);
        }
        Ok(member.pending.iter().cloned().collect())
    }

    /// `#pullnoackdeadlock`: whether this member still holds the delivery barrier.
    ///
    /// A member with nothing pending has converged. A member whose pending head
    /// has been redelivered past [`MAX_REDELIVERIES_WITHOUT_ACK`] without its ACK
    /// ever advancing is **not converging** and must stop blocking everyone else.
    ///
    /// The update stays queued either way — this only removes the replica from
    /// the barrier, exactly as an offline member already is, so a recovered
    /// editor still receives it.
    fn member_holds_delivery_barrier(member: &Member) -> bool {
        !member.pending.is_empty()
            && member.redeliveries_without_ack <= MAX_REDELIVERIES_WITHOUT_ACK
    }

    /// Replicas that are live but have stopped converging (`#pullnoackdeadlock`).
    pub fn nonconverging_replicas(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .members
            .iter()
            .filter(|(id, member)| {
                self.is_live(**id)
                    && !member.pending.is_empty()
                    && member.redeliveries_without_ack > MAX_REDELIVERIES_WITHOUT_ACK
            })
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// ACK one delivered update. Returns `Ok(false)` when the ACK is stale or
    /// unknown; this is non-fatal because editors may retry idempotent deliveries.
    pub fn ack_delivery(
        &mut self,
        client_id: u64,
        patch_id: &str,
        generation: u64,
    ) -> Result<bool> {
        self.ack_delivery_with_content_hash(client_id, patch_id, generation, None)
    }

    /// ACK one delivery only after the editor proves its applied visible text.
    ///
    /// `None` retains wire compatibility with older editor plugins during an
    /// install handoff. Current plugins send a hash; a mismatch keeps the update
    /// pending and schedules a replace-capable rebootstrap instead of allowing a
    /// divergent unsaved editor buffer to race a disk materialization.
    pub fn ack_delivery_with_content_hash(
        &mut self,
        client_id: u64,
        patch_id: &str,
        generation: u64,
        applied_content_hash: Option<&str>,
    ) -> Result<bool> {
        let canonical_content_hash = content_hash(&self.canonical.text());
        let member = self
            .members
            .get_mut(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        let Some(pos) = member
            .pending
            .iter()
            .position(|update| update.patch_id == patch_id && update.generation == generation)
        else {
            // A cumulative ACK may already have drained this exact or an older
            // generation. Plugins still ACK every item returned by one pull,
            // so those remaining receipts must be idempotent.
            return Ok(generation <= member.last_ack_generation);
        };
        if let Some(applied_content_hash) = applied_content_hash {
            // A coalescing editor may apply several queued generations as one
            // visible target. Its final hash is a cumulative receipt: matching a
            // later pending target proves every older delivery through that
            // target is represented, so advance the whole prefix atomically.
            let matched_pos = member
                .pending
                .iter()
                .rposition(|update| update.expected_content_hash == applied_content_hash)
                .or_else(|| {
                    // A peer may apply this remote delivery after making a
                    // concurrent local edit. Its visible text is then causally
                    // ahead of the delivery's historical target, so no pending
                    // generation has that exact hash. Matching the relay's
                    // current canonical hash is still an exact convergence
                    // proof and cumulatively ACKs every queued remote delivery.
                    (applied_content_hash == canonical_content_hash)
                        .then(|| member.pending.len().saturating_sub(1))
                });
            let Some(matched_pos) = matched_pos else {
                self.pending_rebootstrap.insert(client_id);
                return Ok(false);
            };
            if matched_pos < pos {
                self.pending_rebootstrap.insert(client_id);
                return Ok(false);
            }
            let acknowledged_projection = member
                .pending
                .range(..=matched_pos)
                .any(|update| update.patch_id.starts_with("crdt-bootstrap:"));
            let acknowledged_generation = member.pending[matched_pos].generation;
            member.pending.drain(..=matched_pos);
            member.last_ack_generation = member.last_ack_generation.max(acknowledged_generation);
            // `#pullnoackdeadlock`: forward progress clears the redelivery streak.
            member.redeliveries_without_ack = 0;
            self.pending_rebootstrap.remove(&client_id);
            if acknowledged_projection {
                self.canonical_projection_required
                    .set(&self.ctx, client_id, false);
            }
            // Draining an ACKed run can be the write that converges delivery.
            self.bump_delivery_epoch();
            return Ok(true);
        }
        let acknowledged_projection = member
            .pending
            .remove(pos)
            .is_some_and(|update| update.patch_id.starts_with("crdt-bootstrap:"));
        member.last_ack_generation = member.last_ack_generation.max(generation);
        member.redeliveries_without_ack = 0;
        if acknowledged_projection {
            self.canonical_projection_required
                .set(&self.ctx, client_id, false);
        }
        self.bump_delivery_epoch();
        Ok(true)
    }

    /// Project one editor's complete visible document observation into its
    /// delivery frontier.
    ///
    /// Unlike the legacy per-update ACK protocol, the editor does not retain
    /// transport tokens or replay receipts. Its ordinary full-buffer state
    /// observation is a Source. Matching a queued target (or the causally newer
    /// controller canonical) proves the whole represented prefix and advances
    /// the delivery projection cumulatively.
    pub fn observe_delivery_projection(
        &mut self,
        client_id: u64,
        visible_content_hash: &str,
    ) -> Result<bool> {
        let canonical_content_hash = content_hash(&self.canonical.text());
        let member = self
            .members
            .get_mut(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        if member.pending.is_empty() {
            let projected = visible_content_hash == canonical_content_hash;
            if projected {
                self.settle_requested_epoch_compaction()?;
            }
            return Ok(projected);
        }
        let matched_pos = member
            .pending
            .iter()
            .rposition(|update| update.expected_content_hash == visible_content_hash)
            .or_else(|| {
                (visible_content_hash == canonical_content_hash)
                    .then(|| member.pending.len().saturating_sub(1))
            });
        let Some(matched_pos) = matched_pos else {
            self.pending_rebootstrap.insert(client_id);
            return Ok(false);
        };
        let acknowledged_projection = member
            .pending
            .range(..=matched_pos)
            .any(|update| update.patch_id.starts_with("crdt-bootstrap:"));
        let projected_generation = member.pending[matched_pos].generation;
        member.pending.drain(..=matched_pos);
        member.last_ack_generation = member.last_ack_generation.max(projected_generation);
        member.redeliveries_without_ack = 0;
        self.pending_rebootstrap.remove(&client_id);
        if acknowledged_projection {
            self.canonical_projection_required
                .set(&self.ctx, client_id, false);
        }
        self.bump_delivery_epoch();
        self.settle_requested_epoch_compaction()?;
        Ok(true)
    }

    /// `#lazily-hot-path` Theme A — convergence together with the version of the
    /// inputs it was folded from.
    ///
    /// Consumers that today re-run an expensive check on a timer (compact's
    /// commit-observe and CRDT-merge retries) can instead hold the previous witness
    /// and skip the work while `version` is unchanged: equal versions mean no member,
    /// queue, or liveness write has happened, so re-folding cannot yield a new answer.
    pub fn delivery_convergence_witness(&self) -> DeliveryConvergenceWitness {
        DeliveryConvergenceWitness {
            version: self.ctx.get(&self.delivery_epoch),
            converged: self.delivery_converged(),
        }
    }

    /// Clone the blocking adapter for the delivery-convergence cell.
    ///
    /// The clone is independent of the hub mutex, so a waiter never holds the
    /// hub while the editor/controller path publishes the transition it needs.
    pub fn delivery_convergence_subscription(&self) -> DeliveryConvergenceSubscription {
        self.delivery_subscription.clone()
    }

    /// True when every currently-live editor has ACKed all queued fan-out updates.
    /// Disconnected editors are excluded from this live convergence cut.
    pub fn delivery_converged(&self) -> bool {
        self.members
            .iter()
            .filter(|(id, _)| self.is_live(**id))
            .all(|(_, member)| !Self::member_holds_delivery_barrier(member))
    }

    pub fn delivery_snapshot(&self) -> Vec<ReplicaDeliverySnapshot> {
        let mut snapshot = self
            .members
            .iter()
            .map(|(client_id, member)| ReplicaDeliverySnapshot {
                client_id: *client_id,
                live: self.is_live(*client_id),
                pending_updates: member.pending.len(),
                current_generation: member.generation,
                last_ack_generation: member.last_ack_generation,
                redeliveries_without_ack: member.redeliveries_without_ack,
                holds_delivery_barrier: Self::member_holds_delivery_barrier(member),
            })
            .collect::<Vec<_>>();
        snapshot.sort_by_key(|entry| entry.client_id);
        snapshot
    }

    /// The currently-live member replicas (the consistent-cut set for the commit
    /// barrier — offline members are excluded so a slow editor cannot deadlock).
    fn live_editors(&self) -> Vec<&ReplicaState> {
        self.members
            .iter()
            .filter(|(id, _)| self.is_live(**id))
            .map(|(_, m)| &m.replica)
            .collect()
    }

    /// Drive the **commit barrier**: flush every CURRENTLY-LIVE editor's ops into
    /// the canonical replica and confirm a consistent cut. Offline / disconnected
    /// members are excluded — the barrier is a checkpoint of the live replicas,
    /// never a global lock that blocks on a slow editor. After `Ok(true)` a
    /// snapshot of the canonical replica ([`Self::projection_bytes`]) is safe to
    /// write to git.
    pub fn commit_barrier(&self) -> Result<bool> {
        let before_text = self.canonical.text();
        let settled = flush_to_commit_barrier(&self.canonical, &self.live_editors())?;
        let after_text = self.canonical.text();
        self.sync_live_document_projection(&before_text, &after_text);
        Ok(settled)
    }

    /// Whether the canonical replica is already a consistent cut of the live
    /// editors (no flush) — the non-mutating barrier probe.
    pub fn commit_barrier_ready(&self) -> Result<bool> {
        commit_barrier_ready(&self.canonical, &self.live_editors())
    }

    /// The commit barrier gated by CRDT authority. Under
    /// [`CrdtAuthority::MultiReplica`] it runs the live-replica barrier; under
    /// [`CrdtAuthority::GitAuthoritative`] there are no live editor replicas to
    /// flush (git is the source of truth) so it is trivially satisfied and the
    /// canonical replica is left untouched.
    pub fn commit_barrier_under_authority(&self, authority: CrdtAuthority) -> Result<bool> {
        if authority.editor_attached() {
            self.commit_barrier()
        } else {
            Ok(true)
        }
    }

    // --- Awareness / presence (ephemeral; not document CRDT) -----------------

    /// Set this hub's view of a client's local awareness (cursor / selection).
    /// Ephemeral: not part of the document CRDT, never persisted, never committed.
    pub fn set_awareness(&mut self, client_id: u64, state: AwarenessState) {
        self.awareness.set_local(client_id, state);
    }

    /// A snapshot of all live presence states (the awareness broadcast payload).
    pub fn awareness_snapshot(&self) -> Vec<(u64, AwarenessState)> {
        self.awareness.broadcast()
    }

    /// Read-only access to the awareness channel.
    pub fn awareness(&self) -> &AwarenessChannel {
        &self.awareness
    }

    // --- Disk demotion (plan phase 6) ----------------------------------------

    /// The write-through durable **recovery projection** bytes for the canonical
    /// replica — what the supervisor flushes to the durable CRDT projection.
    /// This is a projection of the live authority, NOT the coordination medium:
    /// it exists only so a restart can recover ([`Self::recover_from_projection`]).
    pub fn projection_bytes(&self) -> Vec<u8> {
        self.canonical.encode_state()
    }

    /// Reconcile a (possibly stale) disk projection against the live canonical
    /// replica, enforcing **in-memory-wins** (plan phase 6). Applying a stale disk
    /// projection to the live replica is idempotent — the disk holds a subset of
    /// the ops the live replica already has — so the live text is never regressed.
    /// Returns whether the canonical text changed (true only if the disk held ops
    /// the live replica had genuinely lost, e.g. a crash gap).
    pub fn reconcile_disk_projection(&self, projection: &[u8]) -> Result<bool> {
        let before = self.canonical.text();
        self.canonical.apply_update(projection)?;
        let after = self.canonical.text();
        self.sync_live_document_projection(&before, &after);
        Ok(after != before)
    }

    // --- Out-of-band baseline reconcile (`#staleinmem`) -----------------------

    /// Record the document text this hub just committed to disk, so a later
    /// out-of-band disk correction is detectable at the next commit barrier
    /// ([`Self::reconcile_canonical_against_baseline`]). Called after a successful
    /// git commit. This is the in-memory-wins path's only notion of "what we last
    /// authored on disk".
    pub fn record_committed_baseline(&mut self, committed: &str) {
        self.last_committed_text = Some(committed.to_string());
    }

    /// Reconcile the canonical replica against the current on-disk baseline,
    /// rebuilding it when the document was corrected **out of band** since this
    /// hub last committed (`#staleinmem`).
    ///
    /// The disk-demotion contract ([`Self::reconcile_disk_projection`]) is
    /// *additive*: it can only fold in ops the live replica lost (a crash gap), so
    /// a correction that *removes* content — a `git checkout HEAD` /
    /// `reset --from-current` recovery that drops a corrupt response block — can
    /// never displace the stale canonical ops. The stale canonical then re-commits
    /// the discarded content on every cycle ("`git checkout HEAD` won't hold"), and
    /// only a supervisor restart (which clears the process-global hub registry)
    /// recovers it. This is the live-session analogue of orchestration's
    /// headless `snapshot::crdt_merge_base_state` projection-mismatch rebuild.
    ///
    /// Rebuild fires only when ALL hold:
    /// - a commit baseline has been recorded (we have something to compare), AND
    /// - `on_disk` differs from that recorded baseline (the document changed since
    ///   our last commit and we did not author it — a hub-authored change advances
    ///   `last_committed_text`), AND
    /// - `on_disk` differs from the canonical's current text (the canonical does
    ///   not already reflect the correction).
    ///
    /// On rebuild the canonical replica is reseeded from `on_disk` and every member
    /// mirror is reseeded from it, so a stale editor mirror cannot re-introduce the
    /// discarded ops at the next flush. Returns whether a rebuild happened.
    pub fn reconcile_canonical_against_baseline(&mut self, on_disk: &str) -> Result<bool> {
        let last = match self.last_committed_text.as_deref() {
            Some(t) => t.to_string(),
            // No baseline yet (a hub allocated mid-session before any commit was
            // recorded). Adopt the current disk as the baseline WITHOUT rebuilding,
            // so a later out-of-band correction / compaction is detectable — this
            // is the seam that makes the guard engage even when a compact lands
            // before this document's first finalize.
            None => {
                self.last_committed_text = Some(on_disk.to_string());
                return Ok(false);
            }
        };
        if on_disk == last {
            // Disk is unchanged since our last commit → nothing out of band.
            return Ok(false);
        }
        if on_disk == self.canonical.text() {
            // The canonical already agrees with the corrected disk; no rebuild
            // needed, just advance the recorded baseline so we do not re-detect it.
            self.last_committed_text = Some(on_disk.to_string());
            return Ok(false);
        }
        // Out-of-band correction: rebuild the canonical from the corrected baseline.
        let before_text = self.canonical.text();
        let fresh = ReplicaState::new(self.canonical_id);
        if !on_disk.is_empty() {
            fresh.apply_local_edit(0, 0, on_disk);
        }
        let bootstrap = fresh.encode_state();
        self.canonical = fresh;
        self.sync_live_document_projection(&before_text, on_disk);
        self.rotate_lineage();
        let ids: Vec<u64> = self.members.keys().copied().collect();
        for id in ids {
            let replica = ReplicaState::from_encoded(id, &bootstrap)?;
            if let Some(member) = self.members.get_mut(&id) {
                member.replica = replica;
            }
        }
        self.last_committed_text = Some(on_disk.to_string());
        Ok(true)
    }

    /// Force the canonical (and every member replica) to `text`, unconditionally,
    /// and record it as the committed baseline.
    ///
    /// Unlike [`Self::reconcile_canonical_against_baseline`] this does NOT depend on
    /// a prior `last_committed_text` baseline and never defers: the caller asserts
    /// `text` is the authoritative document content. The single caller is the
    /// authoritative-compaction commit (`#jb-compact-commit-stale-relay-canonical`),
    /// which already archived the `### Re:` turns the compaction dropped. Adopting
    /// the compacted content into the lazily canonical is what makes a subsequent
    /// same-process read (`try_resolve_current_document_content` during the commit)
    /// resolve the compacted content instead of the frozen pre-compact canonical a
    /// phantom stale lease (`live_editors == 0` yet the reactive open-docs
    /// projection still reports the editor open) would otherwise keep serving.
    ///
    /// Returns whether the canonical changed. Live members are flagged for a
    /// replace-capable re-bootstrap because a compaction deletion cannot be
    /// expressed as an additive delta.
    pub fn adopt_authoritative_text(&mut self, text: &str) -> Result<bool> {
        let before_text = self.canonical.text();
        if before_text == text {
            self.last_committed_text = Some(text.to_string());
            return Ok(false);
        }
        self.rebuild_authoritative_epoch(&before_text, text)?;
        Ok(true)
    }

    /// Fence a stable canonical snapshot into a fresh CRDT lineage.
    ///
    /// Compact Exchange calls this only after every live replica has proved the
    /// same visible content hash. Unlike [`Self::adopt_authoritative_text`], an
    /// equal text value is the reason to rebuild: the fresh snapshot discards
    /// pre-compaction insert/delete history, rotates the lineage so older durable
    /// deltas are quarantined, and queues every live member for replace-capable
    /// re-bootstrap.
    pub fn compact_authoritative_epoch(&mut self, expected_text: &str) -> Result<()> {
        let before_text = self.canonical.text();
        if before_text != expected_text {
            return Err(anyhow!(
                "cannot compact CRDT epoch across a moving canonical (expected {} bytes, found {} bytes)",
                expected_text.len(),
                before_text.len(),
            ));
        }
        self.rebuild_authoritative_epoch(&before_text, expected_text)?;
        self.compact_epoch_requested = false;
        Ok(())
    }

    /// Retain or immediately settle a Compact Exchange epoch fence.
    ///
    /// Returns `true` when the lineage was rebuilt now. If a live member still
    /// owes a visible projection, the request stays in the retained canonical
    /// projection and the final matching observation settles it.
    pub fn request_authoritative_epoch_compaction(&mut self) -> Result<bool> {
        self.compact_epoch_requested = true;
        self.settle_requested_epoch_compaction()
    }

    pub fn compact_epoch_requested(&self) -> bool {
        self.compact_epoch_requested
    }

    fn settle_requested_epoch_compaction(&mut self) -> Result<bool> {
        if !self.compact_epoch_requested || !self.delivery_converged() {
            return Ok(false);
        }
        let text = self.canonical.text();
        self.compact_authoritative_epoch(&text)?;
        Ok(true)
    }

    fn rebuild_authoritative_epoch(&mut self, before_text: &str, text: &str) -> Result<()> {
        let fresh = ReplicaState::new(self.canonical_id);
        if !text.is_empty() {
            fresh.apply_local_edit(0, 0, text);
        }
        let bootstrap = fresh.encode_state();
        self.canonical = fresh;
        self.sync_live_document_projection(before_text, text);
        self.rotate_lineage();
        let ids: Vec<u64> = self.members.keys().copied().collect();
        for id in ids {
            let replica = ReplicaState::from_encoded(id, &bootstrap)?;
            if let Some(member) = self.members.get_mut(&id) {
                member.replica = replica;
            }
        }
        let live: Vec<u64> = self
            .members
            .keys()
            .copied()
            .filter(|id| self.is_live(*id))
            .collect();
        self.pending_rebootstrap.extend(live);
        self.last_committed_text = Some(text.to_string());
        Ok(())
    }

    /// Route a settled out-of-band disk change into the hub — the CP-replica
    /// entry point the controller watcher calls when the document file changed on
    /// disk (a `git` operation, an external editor, another process). Composes the
    /// existing in-memory-wins reconcile primitives and reports how the change
    /// relates to the live canonical replica so the caller knows what still needs
    /// to reach the editor buffers.
    ///
    /// - Canonical already reflects the disk text → [`DiskChangeOutcome::AlreadyReconciled`]
    ///   (goal 5: the editor already has it, reconcile is a no-op).
    /// - Out-of-band correction the additive delta cannot express → the canonical
    ///   is rebuilt from disk and [`DiskChangeOutcome::RebuiltFromDisk`] reports how
    ///   many live editors still need a replace-capable re-bootstrap (Phase D2).
    /// - No commit baseline yet → [`DiskChangeOutcome::BaselineDeferred`].
    ///
    /// Idempotent: applying the same disk text twice yields `AlreadyReconciled` the
    /// second time (the first rebuild made canonical agree with disk).
    pub fn apply_disk_change(&mut self, on_disk: &str) -> Result<DiskChangeOutcome> {
        if self.canonical_text() == on_disk {
            return Ok(DiskChangeOutcome::AlreadyReconciled);
        }
        let rebuilt = self.reconcile_canonical_against_baseline(on_disk)?;
        if rebuilt {
            // D2: an additive delta cannot express the out-of-band removal, so flag
            // every live editor for a replace-capable re-bootstrap of its buffer.
            let live: Vec<u64> = self
                .members
                .keys()
                .copied()
                .filter(|id| self.is_live(*id))
                .collect();
            self.pending_rebootstrap.extend(live);
            Ok(DiskChangeOutcome::RebuiltFromDisk {
                live_members: self.live_count(),
            })
        } else if self.canonical_text() == on_disk {
            Ok(DiskChangeOutcome::AlreadyReconciled)
        } else {
            Ok(DiskChangeOutcome::BaselineDeferred)
        }
    }

    /// Live editors that need a replace-capable re-bootstrap after an out-of-band
    /// deletion (D2). Sorted for deterministic delivery order.
    pub fn pending_rebootstrap_members(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.pending_rebootstrap.iter().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// The corrected canonical text an editor flagged by
    /// [`Self::pending_rebootstrap_members`] must REPLACE its buffer with (not
    /// CRDT-merge — the whole point of D2 is that the deletion is not expressible
    /// as an additive delta).
    pub fn rebootstrap_text(&self) -> String {
        self.canonical.text()
    }

    /// Clear the re-bootstrap flag for `client_id` once its editor has applied the
    /// replace. Returns whether a flag was pending.
    pub fn clear_rebootstrap(&mut self, client_id: u64) -> bool {
        self.pending_rebootstrap.remove(&client_id)
    }

    /// The text this hub last recorded as committed to disk (test introspection).
    #[cfg(test)]
    pub fn last_committed_text_for_test(&self) -> Option<&str> {
        self.last_committed_text.as_deref()
    }
}

/// One replica's ephemeral presence: cursor / selection / a display name. NONE of
/// this is part of the document CRDT — it is never persisted to the recovery state and never
/// committed to git.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AwarenessState {
    /// Caret offset (UTF-16 / char offset, convention is the binding's), if any.
    pub cursor: Option<u32>,
    /// Selection range `(anchor, head)`, if any.
    pub selection: Option<(u32, u32)>,
    /// Display name / user label, if any.
    pub user: Option<String>,
}

/// The ephemeral awareness/presence channel — a SEPARATE in-memory structure from
/// the document CRDT (the Yjs "awareness" protocol shape). Presence is keyed by
/// client-id, broadcast to peers, and **expired on deregister**. It is explicitly
/// not persisted and not committed.
#[derive(Default)]
pub struct AwarenessChannel {
    /// lazily's explicitly-ephemeral, last-writer-per-peer compute core. The
    /// channel drives eviction from editor membership and is therefore never
    /// eligible for the durable document outbox.
    presence: EphemeralMapCore<u64, AwarenessState>,
}

impl AwarenessChannel {
    /// A fresh empty channel.
    pub fn new() -> Self {
        Self {
            presence: EphemeralMapCore::new(),
        }
    }

    /// Set the local awareness for `client_id` (overwrites any prior state).
    pub fn set_local(&mut self, client_id: u64, state: AwarenessState) {
        self.presence.set(client_id, state, 0, u64::MAX);
    }

    /// The current presence for `client_id`, if any.
    pub fn get(&self, client_id: u64) -> Option<AwarenessState> {
        self.presence.get(&client_id, 0)
    }

    /// A deterministic (client-id-ordered) snapshot of all presence — the payload
    /// a hub broadcasts to peers.
    pub fn broadcast(&self) -> Vec<(u64, AwarenessState)> {
        self.presence.present(0).into_iter().collect()
    }

    /// Expire / remove a client's presence (called on deregister / disconnect).
    pub fn remove(&mut self, client_id: u64) -> bool {
        let existed = self.presence.get(&client_id, 0).is_some();
        self.presence.evict(&client_id);
        existed
    }

    /// The number of clients with live presence.
    pub fn len(&self) -> usize {
        self.presence.present(0).len()
    }

    /// Whether any presence is tracked.
    pub fn is_empty(&self) -> bool {
        self.presence.present(0).is_empty()
    }
}

/// Deterministically mint a stable, unique-by-construction CRDT peer id from a
/// stable string identity (an editor process identity, e.g. `"intellij:<pid>"`).
///
/// The same identity always yields the same id (stable across reconnects); two
/// distinct identities collide only on a hash collision in the legacy 53-bit
/// compatibility space. The mask preserves already-persisted peer identities. Callers must still
/// [`RelayHub::validate_unique`] before registering (collision = corruption per
/// the plan, surfaced as a hard error rather than silently shared state).
pub fn mint_client_id(identity: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Domain-separate from accidental raw-u64 reuse.
    "agent-doc-replica\0".hash(&mut hasher);
    identity.hash(&mut hasher);
    identity.len().hash(&mut hasher);
    // Preserve the legacy 53-bit peer-id space so persisted identities stay stable.
    const CLIENT_ID_MASK: u64 = (1u64 << 53) - 1;
    hasher.finish() & CLIENT_ID_MASK
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_merge::crdt_sync::ReplicaState;

    static CELL_DOC_TREE_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn live_document_projection_is_default_off_and_tracks_opt_in_canonical_deltas() {
        let _guard = CELL_DOC_TREE_ENV_LOCK.lock();
        let previous = std::env::var_os(CELL_DOC_TREE_CUTOVER_ENV);
        // SAFETY: the process-global mutation is serialized by
        // `CELL_DOC_TREE_ENV_LOCK` and restored before the test returns.
        unsafe { std::env::remove_var(CELL_DOC_TREE_CUTOVER_ENV) };

        let document = "\
<!-- agent:queue -->
- do [#alpha] first
- do [#beta] second
<!-- /agent:queue -->
<!-- agent:backlog -->
- [#later] later
<!-- /agent:backlog -->
";
        let off = RelayHub::from_text(1, document);
        assert!(!off.live_document_projection_enabled());
        assert_eq!(off.unresolved_prompt_count(), None);

        // SAFETY: serialized and restored as above.
        unsafe { std::env::set_var(CELL_DOC_TREE_CUTOVER_ENV, "true") };
        let mut hub = RelayHub::from_text(2, document);
        assert!(hub.live_document_projection_enabled());
        assert_eq!(hub.unresolved_prompt_count(), Some(3));
        assert_eq!(
            hub.unresolved_prompt_count_for_component("queue", 0),
            Some(2)
        );

        let resolved = document.replace("- do [#alpha] first\n", "- ~~do [#alpha] first~~\n");
        hub.apply_canonical_replace(document, &resolved).unwrap();
        assert_eq!(hub.unresolved_prompt_count(), Some(2));
        assert_eq!(
            hub.unresolved_prompt_count_for_component("backlog", 0),
            Some(1)
        );

        let grown = resolved.replace(
            "- do [#beta] second\n",
            "- do [#beta] second\n- do [#gamma] third\n",
        );
        hub.apply_canonical_replace(&resolved, &grown).unwrap();
        assert_eq!(hub.unresolved_prompt_count(), Some(3));
        assert_eq!(
            hub.unresolved_prompt_count_for_component("queue", 0),
            Some(2)
        );

        match previous {
            Some(value) => {
                // SAFETY: serialized and restored as above.
                unsafe { std::env::set_var(CELL_DOC_TREE_CUTOVER_ENV, value) };
            }
            None => {
                // SAFETY: serialized and restored as above.
                unsafe { std::env::remove_var(CELL_DOC_TREE_CUTOVER_ENV) };
            }
        }
    }

    #[test]
    fn fan_out_reaches_every_other_live_replica() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        hub.register(4).unwrap();

        // An edit from replica 2 reaches 3 and 4 (and the canonical replica).
        hub.apply_local(2, 0, 0, "hello").unwrap();
        assert_eq!(hub.canonical_text(), "hello");
        assert_eq!(hub.member_text(2).unwrap(), "hello");
        assert_eq!(hub.member_text(3).unwrap(), "hello");
        assert_eq!(hub.member_text(4).unwrap(), "hello");
    }

    #[test]
    fn canonical_state_vector_is_a_stable_lazy_revision_key() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        let before = hub.canonical_state_vector();

        hub.apply_local(2, 0, 0, "hello").unwrap();

        let after = hub.canonical_state_vector();
        assert_ne!(before, after);
        assert_eq!(after, hub.canonical_state_vector());
    }

    #[test]
    fn canonical_frontier_rejects_a_retained_replica_that_is_ahead() {
        let hub = RelayHub::from_text(1, "canonical");
        let retained = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        assert!(
            hub.canonical_covers_state_vector(&retained.state_vector())
                .unwrap()
        );

        retained.apply_local_edit("canonical".len() as u32, 0, " stale suffix");

        assert!(
            !hub.canonical_covers_state_vector(&retained.state_vector())
                .unwrap(),
            "a decodable but ahead retained frontier must not be used as an incremental base"
        );
    }

    #[test]
    fn relay_update_fans_a_raw_encoded_update_to_every_other_live_replica() {
        // The IPC-delta path: an editor's FFI node produces an encoded update; the
        // hub applies it to that member's mirror, integrates canonical, and fans
        // the delta out to the other live replicas — the editor owns its local
        // edit, the hub owns convergence + fan-out.
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        // Replica 2's FFI node makes a local edit and encodes the delta it owes a
        // peer that knows the (empty) shared base. We model that with a detached
        // ReplicaState mirroring client 2.
        let editor2 = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        editor2.apply_local_edit(0, 0, "hello-ipc");
        let update = editor2.diff(&ReplicaState::new(99).state_vector()).unwrap();

        let packet = hub.relay_update(2, &update).unwrap();
        assert_eq!(packet.origin, 2);
        assert_eq!(packet.targets, vec![3]);
        assert_eq!(hub.canonical_text(), "hello-ipc");
        assert_eq!(hub.member_text(2).unwrap(), "hello-ipc");
        assert_eq!(
            hub.member_text(3).unwrap(),
            "hello-ipc",
            "the raw-update fan-out reached the other live replica's mirror"
        );
    }

    #[test]
    fn replacement_rotates_lineage_and_quarantines_stale_durable_deltas() {
        let mut hub = RelayHub::from_text(1, "clean\n");
        let old_lineage = hub.lineage().to_string();
        let stale_editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        let stale_frontier = stale_editor.state_vector();
        stale_editor.apply_local_edit(6, 0, "resurrected\n");
        let stale_delta = stale_editor.diff(&stale_frontier).unwrap();

        hub.adopt_authoritative_text("rebuilt\n").unwrap();
        assert_ne!(hub.lineage(), old_lineage);
        assert_eq!(
            hub.apply_document_op_delta_in_lineage(Some(&old_lineage), &stale_delta)
                .unwrap(),
            DocumentOpDeltaOutcome::StaleLineage
        );
        assert_eq!(hub.canonical_text(), "rebuilt\n");
        assert_eq!(
            hub.apply_document_op_delta_in_lineage(None, &stale_delta)
                .unwrap(),
            DocumentOpDeltaOutcome::LegacyQuarantined
        );
        assert_eq!(hub.canonical_text(), "rebuilt\n");

        let current_lineage = hub.lineage().to_string();
        let current_editor = ReplicaState::from_encoded(3, &hub.canonical_encoded_state()).unwrap();
        let current_frontier = current_editor.state_vector();
        current_editor.apply_local_edit(8, 0, "current\n");
        let current_delta = current_editor.diff(&current_frontier).unwrap();
        assert_eq!(
            hub.apply_document_op_delta_in_lineage(Some(&current_lineage), &current_delta)
                .unwrap(),
            DocumentOpDeltaOutcome::Applied { changed: true }
        );
        assert_eq!(hub.canonical_text(), "rebuilt\ncurrent\n");
    }

    #[test]
    fn compact_epoch_discards_history_and_fences_same_text_stale_deltas() {
        let mut hub = RelayHub::from_text(1, "keep\n");
        hub.register(2).unwrap();

        let editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        for _ in 0..64 {
            let frontier = editor.state_vector();
            editor.apply_local_edit(5, 0, "discard");
            hub.relay_update(2, &editor.diff(&frontier).unwrap())
                .unwrap();

            let frontier = editor.state_vector();
            editor.apply_local_edit(5, 7, "");
            hub.relay_update(2, &editor.diff(&frontier).unwrap())
                .unwrap();
        }
        assert_eq!(hub.canonical_text(), "keep\n");

        let old_lineage = hub.lineage().to_string();
        let old_state_len = hub.canonical_encoded_state().len();
        let stale_frontier = editor.state_vector();
        editor.apply_local_edit(5, 0, "stale\n");
        let stale_delta = editor.diff(&stale_frontier).unwrap();

        hub.compact_authoritative_epoch("keep\n").unwrap();

        assert_ne!(hub.lineage(), old_lineage);
        assert!(
            hub.canonical_encoded_state().len() < old_state_len,
            "the fresh epoch must discard accumulated insert/delete history"
        );
        assert_eq!(hub.canonical_text(), "keep\n");
        assert_eq!(hub.pending_rebootstrap_members(), vec![2]);
        assert_eq!(
            hub.apply_document_op_delta_in_lineage(Some(&old_lineage), &stale_delta)
                .unwrap(),
            DocumentOpDeltaOutcome::StaleLineage,
        );
        assert_eq!(hub.canonical_text(), "keep\n");
    }

    #[test]
    fn compact_epoch_request_settles_on_final_visible_projection() {
        let mut hub = RelayHub::from_text(1, "keep\n");
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        let editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        let frontier = editor.state_vector();
        editor.apply_local_edit(5, 0, "next\n");
        hub.relay_update_capture(2, &editor.diff(&frontier).unwrap())
            .unwrap();
        let prior_lineage = hub.lineage().to_string();

        assert!(
            !hub.request_authoritative_epoch_compaction().unwrap(),
            "the queued peer delivery must retain the fence request"
        );
        assert!(hub.compact_epoch_requested());
        assert_eq!(hub.lineage(), prior_lineage);

        assert!(
            hub.observe_delivery_projection(3, &content_hash("keep\nnext\n"))
                .unwrap()
        );

        assert!(!hub.compact_epoch_requested());
        assert_ne!(hub.lineage(), prior_lineage);
        assert_eq!(hub.canonical_text(), "keep\nnext\n");
        assert_eq!(hub.pending_rebootstrap_members(), vec![2, 3]);
    }

    #[test]
    fn apply_document_op_delta_feeds_canonical_with_no_live_editors() {
        // The `live_editors == 0` freeze fix (`#docop-plane`, P2): a durably-replicated
        // document-op delta feeds the canonical even though NO editor member is
        // registered — the phantom-lease case where the member `relay_update` path is
        // dead and the canonical used to go stale (the `#sy71`-class resurrection).
        let mut hub = RelayHub::from_text(1, "hello\n");
        assert_eq!(
            hub.live_count(),
            0,
            "no members registered — the phantom-lease case"
        );

        // A connected plugin bootstraps a replica from the canonical snapshot (shared
        // OpIds) and makes the operator's edit — what the durable document-op push carries.
        let editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        let base_vv = editor.state_vector(); // == canonical's frontier (bootstrapped from it)
        editor.apply_local_edit(5, 0, " world"); // "hello\n" -> "hello world\n"
        let delta = editor.diff(&base_vv).unwrap(); // just the operator's new ops

        // The member relay path can't help (live_editors == 0); the document-op fold does.
        let packet = hub.apply_document_op_delta(&delta).unwrap();
        assert_eq!(
            packet.targets,
            Vec::<u64>::new(),
            "no live members to broadcast to"
        );
        assert_eq!(
            hub.canonical_text(),
            "hello world\n",
            "canonical fed the operator's delta despite live_editors == 0 — never frozen"
        );

        // Idempotent: a duplicate frame (at-least-once redelivery) is a no-op.
        hub.apply_document_op_delta(&delta).unwrap();
        assert_eq!(hub.canonical_text(), "hello world\n");
    }

    /// `#lazily-hot-path` Theme A — THE property that makes the witness usable as a
    /// suppression key: with no member, queue, or liveness write, repeated reads
    /// report the same version. A witness whose version moved on every read would
    /// still be "correct" but would suppress nothing, leaving the retry loops it
    /// exists to replace exactly as expensive as before.
    #[test]
    fn delivery_convergence_witness_version_is_stable_while_nothing_changes() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();

        let first = hub.delivery_convergence_witness();
        let second = hub.delivery_convergence_witness();
        // Reads that go through the fold (and therefore through the liveness cells)
        // must not themselves count as changes.
        let _ = hub.delivery_converged();
        let _ = hub.live_count();
        let third = hub.delivery_convergence_witness();

        assert_eq!(first, second);
        assert_eq!(first, third);
        assert!(first.converged, "a registered member with no pending work");
    }

    #[test]
    fn delivery_convergence_subscription_coalesces_to_the_latest_epoch() {
        let hub = RelayHub::new(1);
        let subscription = hub.delivery_convergence_subscription();
        let before = hub.delivery_convergence_witness().version;

        hub.bump_delivery_epoch();
        hub.bump_delivery_epoch();
        let after = hub.delivery_convergence_witness().version;

        assert_ne!(after, before);
        assert!(
            subscription.wait_for_change(before, Duration::ZERO),
            "the coalesced ThreadSafeQueue notification must retain the newest epoch"
        );
        assert!(
            !subscription.wait_for_change(after, Duration::ZERO),
            "an unchanged cursor must not manufacture a notification"
        );
    }

    #[test]
    fn delivery_convergence_subscription_cannot_miss_publish_before_wait() {
        let hub = RelayHub::new(1);
        let subscription = hub.delivery_convergence_subscription();
        let before = hub.delivery_convergence_witness().version;
        hub.bump_delivery_epoch();

        assert!(
            subscription.wait_for_change(before, Duration::from_secs(1)),
            "the retained queue head closes the observe-then-park race"
        );
    }

    #[test]
    fn delivery_convergence_subscription_wakes_a_parked_waiter() {
        let hub = RelayHub::new(1);
        let subscription = hub.delivery_convergence_subscription();
        let before = hub.delivery_convergence_witness().version;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            subscription.wait_for_change(before, Duration::from_secs(1))
        });

        ready_rx.recv().unwrap();
        hub.bump_delivery_epoch();

        assert!(
            waiter.join().unwrap(),
            "the convergence-cell publish must wake the waiter"
        );
    }

    /// The version advances at every write that can change the fold's answer, so a
    /// consumer holding an old witness is never told "nothing changed" while
    /// convergence actually moved.
    #[test]
    fn delivery_convergence_witness_version_advances_on_every_fold_input() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        let after_register = hub.delivery_convergence_witness();
        assert!(after_register.converged);

        let editor2 = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        editor2.apply_local_edit(0, 0, "needs-ack");
        let update = editor2.diff(&ReplicaState::new(99).state_vector()).unwrap();
        hub.relay_update(2, &update).unwrap();

        let after_enqueue = hub.delivery_convergence_witness();
        assert_ne!(
            after_enqueue.version, after_register.version,
            "queueing an unacked update must advance the version"
        );
        assert!(!after_enqueue.converged);
        assert_eq!(after_enqueue.converged, hub.delivery_converged());

        let pending = hub.pending_updates(3).unwrap();
        hub.ack_delivery(3, &pending[0].patch_id, pending[0].generation)
            .unwrap();

        let after_ack = hub.delivery_convergence_witness();
        assert_ne!(
            after_ack.version, after_enqueue.version,
            "draining an ACKed update must advance the version"
        );
        assert!(after_ack.converged);

        // A liveness transition changes which members the fold considers, so it is a
        // fold input even though no queue moved.
        hub.disconnect(3);
        let after_disconnect = hub.delivery_convergence_witness();
        assert_ne!(
            after_disconnect.version, after_ack.version,
            "a liveness transition must advance the version"
        );
        assert_eq!(after_disconnect.converged, hub.delivery_converged());
    }

    /// An unacked delivery to a member that then disconnects converges (the fold cuts
    /// to live members) — and the witness must report that transition, not a stale
    /// "still waiting" answer.
    #[test]
    fn delivery_convergence_witness_tracks_the_live_cut_not_the_queue_alone() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        let editor2 = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        editor2.apply_local_edit(0, 0, "unacked");
        let update = editor2.diff(&ReplicaState::new(99).state_vector()).unwrap();
        hub.relay_update(2, &update).unwrap();

        let blocked = hub.delivery_convergence_witness();
        assert!(!blocked.converged);

        hub.disconnect(3);
        let after = hub.delivery_convergence_witness();
        assert_ne!(after.version, blocked.version);
        assert!(
            after.converged,
            "a disconnected member is outside the live convergence cut"
        );
        assert_eq!(after.converged, hub.delivery_converged());
    }

    #[test]
    fn relay_update_requires_target_ack_before_delivery_converges() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        let editor2 = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        editor2.apply_local_edit(0, 0, "needs-ack");
        let update = editor2.diff(&ReplicaState::new(99).state_vector()).unwrap();

        let packet = hub.relay_update(2, &update).unwrap();
        assert_eq!(packet.targets, vec![3]);
        assert!(
            !hub.delivery_converged(),
            "a live target with an unacked delivery blocks convergence"
        );

        let pending = hub.pending_updates(3).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].origin, 2);
        assert_eq!(pending[0].target, 3);
        assert_eq!(pending[0].generation, 1);
        assert!(pending[0].patch_id.starts_with("crdt:2:3:1"));

        assert!(
            hub.ack_delivery(3, &pending[0].patch_id, pending[0].generation)
                .unwrap()
        );
        assert!(hub.pending_updates(3).unwrap().is_empty());
        assert!(hub.delivery_converged());

        let snapshot = hub.delivery_snapshot();
        let target = snapshot
            .iter()
            .find(|entry| entry.client_id == 3)
            .expect("target delivery snapshot");
        assert_eq!(target.current_generation, 1);
        assert_eq!(target.last_ack_generation, 1);
    }

    #[test]
    /// `#pullnoackdeadlock`: a replica that pulls forever and never ACKs must
    /// stop holding the delivery barrier.
    ///
    /// Observed 2026-08-09 on `tasks/agent-doc/agent-doc-bugs2.md`: editor
    /// client `5162727547735464` re-pulled `current_generation=5
    /// last_ack_generation=4` at ~2/s indefinitely — 23372
    /// `delivery_converged=false` observations, zero ACKs — which wedged every
    /// write behind the delivery barrier and made preflight refuse admission
    /// with `Lazily current authority remained delivery_pending`. `is_live` only
    /// flips on explicit disconnect, so the existing "offline members are
    /// excluded so a slow editor cannot deadlock" escape never fired: a replica
    /// that keeps pulling is maximally live.
    #[test]
    fn a_replica_that_pulls_without_acking_stops_holding_the_barrier() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        let editor2 = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        editor2.apply_local_edit(0, 0, "never-acked");
        let update = editor2.diff(&ReplicaState::new(99).state_vector()).unwrap();
        hub.relay_update(2, &update).unwrap();

        assert!(
            !hub.delivery_converged(),
            "precondition: the unacked delivery blocks convergence"
        );

        // Exactly the wedge: pull the same head over and over, never ACK.
        for _ in 0..MAX_REDELIVERIES_WITHOUT_ACK {
            assert_eq!(hub.pending_updates(3).unwrap().len(), 1);
            assert!(
                !hub.delivery_converged(),
                "within the budget the barrier must still hold — a slow editor is not a broken one"
            );
        }

        assert_eq!(hub.pending_updates(3).unwrap().len(), 1);
        assert!(
            hub.delivery_converged(),
            "past the redelivery budget a non-ACKing replica must stop wedging everyone else"
        );
        assert_eq!(
            hub.nonconverging_replicas(),
            vec![3],
            "and it must be nameable, not silently dropped"
        );

        // The update is NOT discarded — a recovered editor still receives it,
        // exactly as an offline member would.
        assert_eq!(hub.pending_updates(3).unwrap().len(), 1);
    }

    /// The other half: forward progress clears the streak, so a replica that
    /// ACKs late still holds the barrier for its NEXT delivery.
    #[test]
    fn an_ack_clears_the_redelivery_streak() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        let editor2 = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        editor2.apply_local_edit(0, 0, "first");
        let update = editor2.diff(&ReplicaState::new(99).state_vector()).unwrap();
        hub.relay_update(2, &update).unwrap();

        for _ in 0..(MAX_REDELIVERIES_WITHOUT_ACK + 5) {
            hub.pending_updates(3).unwrap();
        }
        assert!(hub.delivery_converged(), "precondition: the streak tripped");

        let pending = hub.pending_updates(3).unwrap();
        assert!(
            hub.ack_delivery(3, &pending[0].patch_id, pending[0].generation)
                .unwrap()
        );
        assert!(hub.nonconverging_replicas().is_empty(), "the ACK rehabilitates it");

        // A NEW delivery to the rehabilitated replica blocks convergence again.
        editor2.apply_local_edit(0, 0, "second");
        let update = editor2.diff(&hub.canonical_state_vector()).unwrap();
        hub.relay_update(2, &update).unwrap();
        assert!(
            !hub.delivery_converged(),
            "a recovered replica must hold the barrier for its next delivery"
        );
    }

    #[test]
    fn content_mismatch_ack_keeps_frontier_pending_and_requests_rebootstrap() {
        let mut hub = RelayHub::from_text(1, "base\n");
        hub.register(2).unwrap();
        hub.apply_canonical_replace("base\n", "base\nresponse\n")
            .unwrap();

        let pending = hub.pending_updates(2).unwrap().pop().unwrap();
        assert_eq!(
            pending.expected_content_hash,
            content_hash("base\nresponse\n")
        );
        assert!(
            !hub.ack_delivery_with_content_hash(
                2,
                &pending.patch_id,
                pending.generation,
                Some(&content_hash("base\nstale editor buffer\n")),
            )
            .unwrap()
        );
        assert_eq!(hub.pending_updates(2).unwrap().len(), 1);
        assert_eq!(hub.pending_rebootstrap_members(), vec![2]);
        assert!(!hub.delivery_converged());

        assert!(
            hub.ack_delivery_with_content_hash(
                2,
                &pending.patch_id,
                pending.generation,
                Some(&pending.expected_content_hash),
            )
            .unwrap()
        );
        assert!(hub.delivery_converged());
        assert!(hub.pending_rebootstrap_members().is_empty());
    }

    #[test]
    fn replacement_identity_can_receipt_an_already_bootstrapped_canonical_projection() {
        let mut hub = RelayHub::from_text(1, "base\n");
        hub.register(2).unwrap();
        hub.apply_canonical_replace("base\n", "base\nresponse\n")
            .unwrap();
        assert!(!hub.delivery_converged());

        // Simulate an IDE restart: the old per-identity queue disappears, but
        // the durable controller write still requires visible proof.
        assert!(hub.deregister(2));
        hub.register(3).unwrap();
        assert!(hub.delivery_converged());
        assert!(hub.ensure_canonical_projection_receipt(3).unwrap());
        assert!(
            !hub.ensure_canonical_projection_receipt(3).unwrap(),
            "repeated registration recovery must not grow the receipt queue",
        );

        let pending = hub.pending_updates(3).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].patch_id.starts_with("crdt-bootstrap:1:3:"));
        assert_eq!(
            pending[0].expected_content_hash,
            content_hash("base\nresponse\n"),
        );
        assert!(!hub.delivery_converged());
        assert!(
            hub.ack_delivery_with_content_hash(
                3,
                &pending[0].patch_id,
                pending[0].generation,
                Some(&pending[0].expected_content_hash),
            )
            .unwrap(),
        );
        assert!(hub.delivery_converged());
    }

    #[test]
    fn final_coalesced_hash_cumulatively_acknowledges_older_generations() {
        let mut hub = RelayHub::from_text(1, "base\n");
        hub.register(2).unwrap();
        hub.apply_canonical_replace("base\n", "base\none\n")
            .unwrap();
        hub.apply_canonical_replace("base\none\n", "base\none\ntwo\n")
            .unwrap();
        let pending = hub.pending_updates(2).unwrap();
        assert_eq!(pending.len(), 2);

        assert!(
            hub.ack_delivery_with_content_hash(
                2,
                &pending[0].patch_id,
                pending[0].generation,
                Some(&pending[1].expected_content_hash),
            )
            .unwrap()
        );
        assert!(hub.pending_updates(2).unwrap().is_empty());
        assert_eq!(hub.delivery_snapshot()[0].last_ack_generation, 2);
        assert!(hub.delivery_converged());
        assert!(
            hub.ack_delivery_with_content_hash(
                2,
                &pending[1].patch_id,
                pending[1].generation,
                Some(&pending[1].expected_content_hash),
            )
            .unwrap(),
            "a plugin may still ACK each item after the first cumulative receipt drains the batch"
        );
    }

    #[test]
    fn visible_state_projection_cumulatively_settles_delivery_without_update_acks() {
        let mut hub = RelayHub::from_text(1, "base\n");
        hub.register(2).unwrap();
        hub.apply_canonical_replace("base\n", "base\none\n")
            .unwrap();
        hub.apply_canonical_replace("base\none\n", "base\none\ntwo\n")
            .unwrap();

        assert_eq!(hub.pending_updates(2).unwrap().len(), 2);
        assert!(
            hub.observe_delivery_projection(2, &content_hash("base\none\ntwo\n"))
                .unwrap()
        );
        assert!(hub.pending_updates(2).unwrap().is_empty());
        assert!(hub.delivery_converged());
    }

    #[test]
    fn causally_ahead_peer_can_ack_an_older_delivery_with_current_canonical_hash() {
        let mut hub = RelayHub::from_text(1, "base\n");
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        let frontier = "base\n".len() as u32;

        hub.apply_local(2, frontier, 0, "from-two\n").unwrap();
        let pending_for_three = hub.pending_updates(3).unwrap().pop().unwrap();
        hub.apply_local(3, frontier, 0, "from-three\n").unwrap();
        let canonical_hash = content_hash(&hub.canonical_text());

        assert_ne!(
            pending_for_three.expected_content_hash, canonical_hash,
            "the target's concurrent local edit must make it causally ahead of the historical delivery"
        );
        assert!(
            hub.ack_delivery_with_content_hash(
                3,
                &pending_for_three.patch_id,
                pending_for_three.generation,
                Some(&canonical_hash),
            )
            .unwrap(),
            "an exact current-canonical hash proves the older remote delivery is included"
        );
        assert!(hub.pending_updates(3).unwrap().is_empty());
    }

    #[test]
    fn canonical_response_uses_bounded_minimal_span_delta() {
        let base = format!("{}\n", "a".repeat(5_000));
        let target = format!("{base}résumé ✓\n");
        let mut hub = RelayHub::from_text(1, &base);
        hub.register(2).unwrap();

        let packet = hub.apply_canonical_replace(&base, &target).unwrap();

        assert_eq!(hub.canonical_text(), target);
        assert_eq!(hub.member_text(2).unwrap(), target);
        assert!(
            packet.update.len() < 5_000,
            "a short append must not encode a whole-document delete/reinsert; update_bytes={}",
            packet.update.len()
        );
    }

    #[test]
    fn cp_canonical_replace_queues_delta_for_live_editors() {
        let mut hub = RelayHub::from_text(1, "before\n");
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        let packet = hub
            .apply_canonical_replace("before\n", "before\nresponse\n")
            .unwrap();
        assert_eq!(packet.origin, 1);
        assert_eq!(packet.targets, vec![2, 3]);
        assert_eq!(hub.canonical_text(), "before\nresponse\n");
        assert_eq!(hub.member_text(2).unwrap(), "before\nresponse\n");
        assert_eq!(hub.member_text(3).unwrap(), "before\nresponse\n");
        assert!(
            !hub.delivery_converged(),
            "CP-origin editor delivery still requires editor ACK"
        );

        let pending = hub.pending_updates(2).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].origin, 1);
        assert_eq!(pending[0].target, 2);
        assert!(pending[0].patch_id.starts_with("crdt:1:2:1"));
    }

    #[test]
    fn cp_canonical_replace_compacted_exchange_removes_response_cells() {
        let expanded = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: old topic - gpt-5\n\n",
            "Old response.\n\n",
            "### Re: newer topic - gpt-5\n\n",
            "Newer response.\n",
            "<!-- /agent:exchange -->\n",
        );
        let compacted = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\n",
            "- Archived 2 response topic(s): old topic; newer topic\n",
            "<!-- /agent:exchange -->\n",
        );
        let mut hub = RelayHub::from_text(1, expanded);
        hub.register(2).unwrap();

        let packet = hub.apply_canonical_replace(expanded, compacted).unwrap();

        assert_eq!(packet.origin, 1);
        assert_eq!(packet.targets, vec![2]);
        assert_eq!(hub.canonical_text(), compacted);
        assert_eq!(hub.member_text(2).unwrap(), compacted);
        assert!(!hub.canonical_text().contains("### Re: old topic"));
        assert!(!hub.canonical_text().contains("### Re: newer topic"));
        assert!(
            hub.canonical_text().contains("### Session Summary"),
            "canonical replacement must carry the compacted summary cell"
        );
        let pending = hub.pending_updates(2).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].origin, 1);
    }

    #[test]
    fn cp_canonical_replace_rejects_stale_expected_text() {
        let mut hub = RelayHub::from_text(1, "operator text\n");
        hub.register(2).unwrap();

        let err = hub
            .apply_canonical_replace("stale text\n", "agent response\n")
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("canonical text changed before CP relay write"),
            "stale CP relay writes must be rejected: {err:#}"
        );
        assert_eq!(hub.canonical_text(), "operator text\n");
        assert!(hub.pending_updates(2).unwrap().is_empty());
    }

    #[test]
    fn from_text_seeds_canonical_and_registered_members() {
        let mut hub = RelayHub::from_text(1, "# plan\n\nexisting queue\n");
        assert_eq!(hub.canonical_text(), "# plan\n\nexisting queue\n");

        hub.register(2).unwrap();
        assert_eq!(
            hub.member_text(2).unwrap(),
            "# plan\n\nexisting queue\n",
            "a fresh editor replica bootstraps from the seeded canonical"
        );
        assert_eq!(
            hub.last_committed_text_for_test(),
            Some("# plan\n\nexisting queue\n")
        );
    }

    #[test]
    fn register_rejects_client_id_collision() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        // Re-registering 2 is a hard error (corruption).
        assert!(hub.register(2).is_err());
        // Colliding with the canonical id is a hard error.
        assert!(hub.register(1).is_err());
        // A fresh id is fine.
        assert!(hub.register(3).is_ok());
    }

    #[test]
    fn mint_client_id_is_stable_and_distinct() {
        let a1 = mint_client_id("intellij:1234");
        let a2 = mint_client_id("intellij:1234");
        let b = mint_client_id("vscode:1234");
        assert_eq!(a1, a2, "same identity mints the same stable id");
        assert_ne!(a1, b, "distinct identities mint distinct ids");
    }

    #[test]
    fn out_of_order_fan_out_converges() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        // Two dependent edits from replica 2, fan-out captured but NOT delivered (lag).
        hub.local_edit(2, 0, 0, "first").unwrap();
        let p1 = hub.relay_capture(2).unwrap();
        let len = hub.member_text(2).unwrap().chars().count() as u32;
        hub.local_edit(2, len, 0, " second").unwrap();
        let p2 = hub.relay_capture(2).unwrap();

        // Deliver to replica 3 OUT OF ORDER: p2 (depends on p1) before p1.
        hub.deliver(3, &p2.update).unwrap();
        assert_ne!(
            hub.member_text(3).unwrap(),
            hub.canonical_text(),
            "the later op alone is causally buffered (deps missing)"
        );
        hub.deliver(3, &p1.update).unwrap();
        assert_eq!(
            hub.member_text(3).unwrap(),
            hub.canonical_text(),
            "out-of-order fan-out self-heals once causal deps arrive"
        );
        assert!(hub.member_text(3).unwrap().contains("first second"));
    }

    #[test]
    fn commit_barrier_captures_all_live_editors_and_ignores_disconnected() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        hub.register(4).unwrap();

        // Three editors each type locally without relaying (un-propagated ops —
        // canonical does not hold them yet).
        hub.local_edit(2, 0, 0, "AA").unwrap();
        hub.local_edit(3, 0, 0, "BB").unwrap();
        // Editor 4 disconnects with an un-flushed local op.
        hub.local_edit(4, 0, 0, "CC").unwrap();
        hub.disconnect(4);

        // The barrier captures the live editors (2,3) and does NOT deadlock on the
        // disconnected editor 4 — its op is excluded from this checkpoint.
        assert!(hub.commit_barrier().unwrap());
        let cut = hub.canonical_text();
        assert!(cut.contains("AA") && cut.contains("BB"));
        assert!(
            !cut.contains("CC"),
            "the disconnected editor's op is not in the live cut"
        );

        // Editor 4 contributes its op on reconnect (next sync) — no data loss.
        hub.reconnect(4).unwrap();
        assert!(hub.canonical_text().contains("CC"));
        assert_eq!(hub.member_text(4).unwrap(), hub.canonical_text());
    }

    #[test]
    fn offline_editor_reconnect_converges_no_data_loss() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        // Editor 3 goes offline, missing editor 2's broadcasts.
        hub.disconnect(3);
        hub.apply_local(2, 0, 0, "while-offline").unwrap();
        // Editor 3 also typed locally while offline (its own replica only).
        hub.local_edit(3, 0, 0, "local-3 ").unwrap();
        assert!(
            !hub.member_text(3).unwrap().contains("while-offline"),
            "an offline editor does not receive broadcasts"
        );

        // Reconnect: bidirectional catch-up. No data loss in either direction.
        hub.reconnect(3).unwrap();
        let t3 = hub.member_text(3).unwrap();
        assert!(t3.contains("while-offline"), "missed updates caught up");
        assert!(t3.contains("local-3"), "offline local edits preserved");
        assert_eq!(t3, hub.canonical_text(), "reconnected replica converged");
    }

    #[test]
    fn awareness_is_ephemeral_and_expires_on_deregister() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        hub.set_awareness(
            2,
            AwarenessState {
                cursor: Some(5),
                selection: Some((1, 5)),
                user: Some("alice".into()),
            },
        );
        hub.set_awareness(
            3,
            AwarenessState {
                cursor: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(hub.awareness_snapshot().len(), 2);

        // Deregister expires presence (it is not persisted / committed).
        hub.deregister(2);
        let snap = hub.awareness_snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].0, 3);
        // A disconnect also expires presence (a stale cursor must not linger).
        hub.disconnect(3);
        assert!(hub.awareness().is_empty());
    }

    #[test]
    fn disk_projection_is_recovery_only_in_memory_wins() {
        const {
            assert!(DISK_IS_RECOVERY_PROJECTION_ONLY);
        }

        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.apply_local(2, 0, 0, "v1").unwrap();
        // Flush a durable recovery projection.
        let stale_projection = hub.projection_bytes();

        // The live session advances past the projection.
        let len = hub.canonical_text().chars().count() as u32;
        hub.apply_local(2, len, 0, " v2").unwrap();
        assert_eq!(hub.canonical_text(), "v1 v2");

        // Reconciling the STALE disk projection must not regress the live text —
        // the in-memory replica wins (the projection is a recovery input only).
        let changed = hub.reconcile_disk_projection(&stale_projection).unwrap();
        assert!(!changed, "a stale disk projection holds no new ops");
        assert_eq!(hub.canonical_text(), "v1 v2", "in-memory replica wins");
    }

    #[test]
    fn recover_from_projection_rebuilds_canonical_on_restart() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.apply_local(2, 0, 0, "durable").unwrap();
        let projection = hub.projection_bytes();

        // Simulate a supervisor restart: rebuild the canonical replica from the
        // last disk recovery projection (members re-register / re-sync after).
        let recovered = RelayHub::recover_from_projection(1, &projection).unwrap();
        assert_eq!(recovered.canonical_text(), "durable");
        assert_eq!(
            recovered.live_count(),
            0,
            "members re-register after restart"
        );
    }

    #[test]
    fn commit_barrier_under_authority_skips_headless() {
        // MultiReplica: the barrier flushes the live editor's un-relayed op.
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.local_edit(2, 0, 0, "live").unwrap();
        assert!(!hub.canonical_text().contains("live"), "not relayed yet");
        assert!(
            hub.commit_barrier_under_authority(CrdtAuthority::MultiReplica)
                .unwrap()
        );
        assert!(hub.canonical_text().contains("live"), "barrier flushed it");

        // GitAuthoritative: no live editor replicas to flush — trivially ready and
        // the un-relayed op stays out of the canonical replica.
        let mut headless = RelayHub::new(1);
        headless.register(2).unwrap();
        headless.local_edit(2, 0, 0, "ignored").unwrap();
        assert!(
            headless
                .commit_barrier_under_authority(CrdtAuthority::GitAuthoritative)
                .unwrap()
        );
        assert!(
            !headless.canonical_text().contains("ignored"),
            "the git-authoritative barrier does not flush live replicas"
        );
    }

    // --- Out-of-band baseline reconcile (`#staleinmem`) -----------------------

    #[test]
    fn baseline_reconcile_seeds_baseline_on_first_contact_without_rebuilding() {
        // With no recorded baseline, the first contact ADOPTS the current disk as
        // the baseline without rebuilding (the live canonical is untouched), so a
        // LATER out-of-band change is detectable.
        let mut hub = RelayHub::new(1);
        hub.canonical.apply_local_edit(0, 0, "live content");
        assert!(
            !hub.reconcile_canonical_against_baseline("disk content")
                .unwrap(),
            "no rebuild on first contact"
        );
        assert_eq!(hub.canonical_text(), "live content", "canonical untouched");
        assert_eq!(
            hub.last_committed_text_for_test(),
            Some("disk content"),
            "first contact seeds the baseline from disk"
        );
        // A subsequent divergence from that seeded baseline now rebuilds.
        assert!(
            hub.reconcile_canonical_against_baseline("corrected content")
                .unwrap(),
            "a change from the seeded baseline rebuilds"
        );
        assert_eq!(hub.canonical_text(), "corrected content");
    }

    #[test]
    fn baseline_reconcile_adopts_a_compacted_shrink() {
        // `compact exchange` archives + truncates the document on disk OUT OF BAND
        // of the supervisor's in-memory canonical (compact runs in a separate
        // process). The next commit barrier must adopt the smaller compacted text
        // so the canonical does not re-expand the archived turns.
        let mut hub = RelayHub::new(1);
        let editor = mint_client_id("intellij:compact");
        hub.register(editor).unwrap();
        let full = "# doc\n\nturn 1\nturn 2\nturn 3\nturn 4 (kept)\n";
        hub.apply_local(editor, 0, 0, full).unwrap();
        hub.record_committed_baseline(full);

        // compact rewrote disk: older turns archived, only the tail kept.
        let compacted = "# doc\n\n*Compacted. 3 turns archived.*\nturn 4 (kept)\n";
        assert!(
            hub.reconcile_canonical_against_baseline(compacted).unwrap(),
            "the compacted shrink is adopted"
        );
        assert_eq!(hub.canonical_text(), compacted);
        assert!(
            !hub.canonical_text().contains("turn 1"),
            "archived turns do not survive in the canonical"
        );
        assert_eq!(
            hub.member_text(editor).as_deref(),
            Some(compacted),
            "the editor mirror was reseeded to the compacted text"
        );
    }

    #[test]
    fn adopt_authoritative_text_converges_canonical_without_a_baseline() {
        // `#jb-compact-commit-stale-relay-canonical`: the phantom stale-lease
        // Compact Exchange defect. The compaction wrote the compacted content to
        // disk+snapshot through the disk-authority path, but the relay canonical is
        // FROZEN at the pre-compact text and — the crucial difference from
        // `reconcile_canonical_against_baseline` — this hub has NO recorded
        // `last_committed_text` baseline (allocated mid-session before any commit),
        // so the baseline reconcile would DEFER and leave the canonical stale. The
        // authoritative-compaction commit then reads that frozen canonical and lands
        // pre-compact content in HEAD. `adopt_authoritative_text` must converge
        // unconditionally.
        let mut hub = RelayHub::new(1);
        let editor = mint_client_id("intellij:phantom-lease");
        hub.register(editor).unwrap();
        let pre_compact = "# doc\n\nturn 1\nturn 2\nturn 3\nturn 4 (kept)\n";
        hub.apply_local(editor, 0, 0, pre_compact).unwrap();
        // Deliberately NO `record_committed_baseline` — this is the deferring case.
        assert_eq!(hub.last_committed_text_for_test(), None);

        let compacted = "# doc\n\n*Compacted. 3 turns archived.*\nturn 4 (kept)\n";
        assert!(
            hub.adopt_authoritative_text(compacted).unwrap(),
            "the compacted content is adopted even with no prior baseline"
        );
        assert_eq!(
            hub.canonical_text(),
            compacted,
            "the lazily canonical is the compacted content the commit will read"
        );
        assert!(!hub.canonical_text().contains("turn 1"));
        assert_eq!(
            hub.member_text(editor).as_deref(),
            Some(compacted),
            "the editor mirror was reseeded to the compacted text"
        );
        assert_eq!(
            hub.last_committed_text_for_test(),
            Some(compacted),
            "the baseline advances so a later reconcile does not re-detect it"
        );
        // Idempotent: re-adopting the same text reports no change.
        assert!(
            !hub.adopt_authoritative_text(compacted).unwrap(),
            "re-adopting the same text is a no-op"
        );
    }

    #[test]
    fn baseline_reconcile_is_noop_when_disk_matches_last_commit() {
        // Disk unchanged since our last commit → nothing out of band, no rebuild,
        // and any un-flushed live ops on the canonical are preserved.
        let mut hub = RelayHub::new(1);
        hub.canonical.apply_local_edit(0, 0, "committed body");
        hub.record_committed_baseline("committed body");
        // An editor typed more since the commit; canonical is ahead of disk.
        hub.canonical.apply_local_edit(0, 0, "PREFIX ");
        assert!(
            !hub.reconcile_canonical_against_baseline("committed body")
                .unwrap(),
            "disk == last commit → no rebuild"
        );
        assert_eq!(
            hub.canonical_text(),
            "PREFIX committed body",
            "the un-flushed live op survives the no-op reconcile"
        );
    }

    #[test]
    fn hub_eviction_requires_no_members_and_an_exact_committed_canonical() {
        let mut hub = RelayHub::new(1);
        hub.canonical.apply_local_edit(0, 0, "current body");
        assert!(
            !hub.is_safe_to_evict(),
            "an uncheckpointed canonical must remain resident"
        );

        hub.record_committed_baseline("older body");
        assert!(
            !hub.is_safe_to_evict(),
            "a stale committed baseline must not authorize eviction"
        );

        hub.record_committed_baseline("current body");
        assert!(hub.is_safe_to_evict());

        let editor = mint_client_id("intellij:eviction");
        hub.register(editor).unwrap();
        assert!(
            !hub.is_safe_to_evict(),
            "a registered member keeps the hub resident"
        );
        hub.adopt_authoritative_text("new committed body").unwrap();
        assert!(hub.deregister(editor));
        assert!(
            hub.is_safe_to_evict(),
            "deregister drops the member's stale rebootstrap flag"
        );
    }

    #[test]
    fn baseline_reconcile_rebuilds_canonical_from_out_of_band_correction() {
        // The core bug fix: after a corrupt commit, an out-of-band disk correction
        // (e.g. `git checkout HEAD`) must REBUILD the stale canonical from the
        // correction so the discarded content cannot re-commit on the next cycle.
        let mut hub = RelayHub::new(1);
        let editor = mint_client_id("intellij:rebuild-test");
        hub.register(editor).unwrap();
        // Canonical + the editor mirror hold the "corrupt" committed state.
        hub.apply_local(editor, 0, 0, "GOOD\nCORRUPT-RESPONSE\n")
            .unwrap();
        hub.record_committed_baseline("GOOD\nCORRUPT-RESPONSE\n");
        assert!(hub.canonical_text().contains("CORRUPT-RESPONSE"));

        // Operator corrects disk out of band (drops the corrupt block).
        let rebuilt = hub.reconcile_canonical_against_baseline("GOOD\n").unwrap();
        assert!(rebuilt, "an out-of-band correction rebuilds the canonical");
        assert_eq!(hub.canonical_text(), "GOOD\n", "disk wins on rebuild");
        assert!(
            !hub.canonical_text().contains("CORRUPT-RESPONSE"),
            "the discarded corrupt op is gone from the canonical"
        );
        // The editor mirror is reseeded from the corrected canonical, so a flush
        // cannot re-introduce the corruption.
        assert_eq!(hub.member_text(editor).as_deref(), Some("GOOD\n"));
        assert!(
            hub.commit_barrier_under_authority(CrdtAuthority::MultiReplica)
                .unwrap()
        );
        assert_eq!(
            hub.canonical_text(),
            "GOOD\n",
            "the post-rebuild commit barrier holds the correction, not the corruption"
        );
        assert_eq!(hub.last_committed_text_for_test(), Some("GOOD\n"));
    }

    #[test]
    fn baseline_reconcile_advances_marker_when_canonical_already_agrees() {
        // Disk diverged from the last recorded commit but the canonical already
        // reflects the new content (a normal hub-authored advance that simply was
        // not re-recorded) → no rebuild, but the marker advances so it is not
        // re-detected as out-of-band next time.
        let mut hub = RelayHub::new(1);
        hub.canonical.apply_local_edit(0, 0, "v2 body");
        hub.record_committed_baseline("v1 body");
        assert!(
            !hub.reconcile_canonical_against_baseline("v2 body").unwrap(),
            "canonical already agrees with disk → no rebuild"
        );
        assert_eq!(hub.last_committed_text_for_test(), Some("v2 body"));
    }

    // ---- apply_disk_change: the file-watch → CP-replica entry point ----

    #[test]
    fn apply_disk_change_is_a_noop_when_canonical_already_has_it() {
        // Goal 5: the editor authored the change (or a peer already pulled it), so
        // the canonical already reflects the disk text → reconcile is a no-op.
        let mut hub = RelayHub::from_text(1, "# plan\n\nbody\n");
        assert_eq!(
            hub.apply_disk_change("# plan\n\nbody\n").unwrap(),
            DiskChangeOutcome::AlreadyReconciled
        );
        assert_eq!(hub.canonical_text(), "# plan\n\nbody\n");
    }

    #[test]
    fn apply_disk_change_rebuilds_and_reports_editors_to_rebootstrap() {
        let mut hub = RelayHub::new(1);
        let editor = mint_client_id("intellij:disk-change-test");
        hub.register(editor).unwrap();
        hub.apply_local(editor, 0, 0, "GOOD\nCORRUPT-RESPONSE\n")
            .unwrap();
        hub.record_committed_baseline("GOOD\nCORRUPT-RESPONSE\n");

        // Operator corrects the file out of band (drops the corrupt block).
        let outcome = hub.apply_disk_change("GOOD\n").unwrap();
        assert_eq!(
            outcome,
            DiskChangeOutcome::RebuiltFromDisk { live_members: 1 },
            "an out-of-band deletion rebuilds canonical and flags the live editor"
        );
        assert_eq!(hub.canonical_text(), "GOOD\n", "disk wins on rebuild");
        // The hub-side mirror is corrected; the live editor buffer still needs a
        // replace-capable re-bootstrap (Phase D2) — reported, not silently dropped.
        assert_eq!(hub.member_text(editor).as_deref(), Some("GOOD\n"));
    }

    #[test]
    fn rebuilt_from_disk_flags_live_editors_for_replace_rebootstrap() {
        // D2: an out-of-band deletion rebuilds canonical; each live editor must be
        // flagged for a replace-capable re-bootstrap with the corrected text.
        let mut hub = RelayHub::new(1);
        let editor = mint_client_id("intellij:d2-test");
        hub.register(editor).unwrap();
        hub.apply_local(editor, 0, 0, "GOOD\nCORRUPT\n").unwrap();
        hub.record_committed_baseline("GOOD\nCORRUPT\n");
        assert!(hub.pending_rebootstrap_members().is_empty());

        // Operator deletes the corrupt block out of band.
        let outcome = hub.apply_disk_change("GOOD\n").unwrap();
        assert!(matches!(outcome, DiskChangeOutcome::RebuiltFromDisk { .. }));

        // The live editor is flagged, and the replace text is the corrected canonical.
        assert_eq!(hub.pending_rebootstrap_members(), vec![editor]);
        assert_eq!(hub.rebootstrap_text(), "GOOD\n");

        // Once the editor applies the replace, the flag clears (idempotent).
        assert!(hub.clear_rebootstrap(editor));
        assert!(hub.pending_rebootstrap_members().is_empty());
        assert!(!hub.clear_rebootstrap(editor));
    }

    #[test]
    fn apply_disk_change_is_idempotent_after_a_rebuild() {
        let mut hub = RelayHub::from_text(1, "GOOD\nCORRUPT\n");
        assert!(matches!(
            hub.apply_disk_change("GOOD\n").unwrap(),
            DiskChangeOutcome::RebuiltFromDisk { .. }
        ));
        // Re-delivering the same disk text is now a no-op — canonical agrees.
        assert_eq!(
            hub.apply_disk_change("GOOD\n").unwrap(),
            DiskChangeOutcome::AlreadyReconciled
        );
    }

    #[test]
    fn apply_disk_change_defers_when_no_commit_baseline_recorded() {
        // A hub allocated mid-session before its first finalize has no baseline;
        // the disk text is adopted as the baseline and the change is deferred to
        // the normal editor-delta / commit-barrier path (canonical untouched).
        let mut hub = RelayHub::new(1);
        assert_eq!(
            hub.apply_disk_change("brand new disk text\n").unwrap(),
            DiskChangeOutcome::BaselineDeferred
        );
        assert_eq!(
            hub.last_committed_text_for_test(),
            Some("brand new disk text\n"),
            "the disk text becomes the baseline so a later correction is detectable"
        );
    }

    // ---- #live-editor-reactive S1: reactive liveness core -------------------

    /// One liveness transition in the deterministic SimWorld model. Both the hub and
    /// a plain reference `BTreeMap<client_id, live>` consume the same op via a shared
    /// pure decision function (`apply_to_model`), so the reactive derived count can be
    /// compared against the model at every step.
    #[derive(Clone, Copy, Debug)]
    enum LivenessOp {
        Register(u64),
        Disconnect(u64),
        Reconnect(u64),
        Deregister(u64),
    }

    /// The shared pure decision function: fold one op into the reference model. A
    /// deregister removes the key (present-but-false in the hub, absent here — both
    /// are uncounted, so the counts stay in lockstep).
    fn apply_to_model(model: &mut std::collections::BTreeMap<u64, bool>, op: LivenessOp) {
        match op {
            LivenessOp::Register(id) => {
                model.insert(id, true);
            }
            LivenessOp::Disconnect(id) => {
                if let Some(live) = model.get_mut(&id) {
                    *live = false;
                }
            }
            LivenessOp::Reconnect(id) => {
                model.insert(id, true);
            }
            LivenessOp::Deregister(id) => {
                model.remove(&id);
            }
        }
    }

    fn apply_to_hub(hub: &mut RelayHub, op: LivenessOp) {
        match op {
            LivenessOp::Register(id) => hub.register(id).unwrap(),
            LivenessOp::Disconnect(id) => {
                hub.disconnect(id);
            }
            LivenessOp::Reconnect(id) => hub.reconnect(id).unwrap(),
            LivenessOp::Deregister(id) => {
                hub.deregister(id);
            }
        }
    }

    fn model_live_count(model: &std::collections::BTreeMap<u64, bool>) -> usize {
        model.values().filter(|live| **live).count()
    }

    #[test]
    fn reactive_live_count_matches_reference_model_across_transitions() {
        use LivenessOp::*;
        // Scripted SimWorld: register/disconnect/reconnect/deregister, including a
        // re-register of a previously deregistered id (exercises the present-but-false
        // cell being flipped back to true).
        let script = [
            Register(2),
            Register(3),
            Register(4),
            Disconnect(3),
            Reconnect(3),
            Disconnect(2),
            Disconnect(4),
            Deregister(3),
            Register(3),
            Reconnect(2),
            Reconnect(4),
        ];

        let mut hub = RelayHub::new(1);
        let mut model: std::collections::BTreeMap<u64, bool> = Default::default();
        assert_eq!(hub.live_count(), 0, "empty hub starts at 0 live editors");

        for (step, op) in script.into_iter().enumerate() {
            apply_to_hub(&mut hub, op);
            apply_to_model(&mut model, op);
            assert_eq!(
                hub.live_count(),
                model_live_count(&model),
                "reactive live_count diverged from model at step {step} after {op:?}",
            );
        }
    }

    #[test]
    fn deregistered_present_key_is_not_counted_live() {
        // The family is deferral-not-dealloc: a deregistered client_id's cell stays
        // present-but-false. It must never inflate the reactive live count.
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        assert_eq!(hub.live_count(), 2);

        hub.deregister(2);
        assert_eq!(
            hub.live_count(),
            1,
            "deregistered member drops out of the count"
        );

        // Re-registering the same id flips the retained cell back to true.
        hub.register(2).unwrap();
        assert_eq!(
            hub.live_count(),
            2,
            "re-register flips the retained cell live"
        );
    }

    #[test]
    fn reactive_live_count_recomputes_on_each_transition() {
        // The count is a live reactive read, not a one-shot snapshot: reading it
        // before and after a transition must reflect the change.
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        assert_eq!(hub.live_count(), 2);

        hub.disconnect(3);
        assert_eq!(
            hub.live_count(),
            1,
            "disconnect recomputes the derived count"
        );

        hub.reconnect(3).unwrap();
        assert_eq!(
            hub.live_count(),
            2,
            "reconnect recomputes the derived count"
        );
    }

    /// `#lazilyscopeadopt` — the edge-set-vs-observer-registry test applied to the
    /// liveness core: *anything surviving an invalidation is not a graph edge*.
    ///
    /// `live_editor_count` observes every present liveness key, so its dependency
    /// set is the thing that could grow without bound in a long-lived controller.
    /// Reconnect churn on a **stable** client identity (what `mint_client_id`
    /// produces — a deterministic id from a stable string identity) must not add
    /// an edge per cycle: the same key re-materializes the same cell.
    #[test]
    fn reconnect_churn_on_stable_identity_does_not_grow_the_liveness_edge_set() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        // Force the derived count to compute so its dependency edges exist.
        assert_eq!(hub.live_count(), 1);
        let baseline = hub.ctx.dependency_count(&hub.live_editor_count);

        for _ in 0..64 {
            hub.deregister(2);
            assert_eq!(hub.live_count(), 0);
            hub.register(2).unwrap();
            assert_eq!(hub.live_count(), 1);
        }

        assert_eq!(
            hub.ctx.dependency_count(&hub.live_editor_count),
            baseline,
            "register/deregister churn on one stable identity must not accumulate \
             dependency edges on the derived live-editor count",
        );
    }

    /// `#lazilyscopeadopt` — the liveness edge set is bounded by the number of
    /// **distinct** editor identities for the document, not by connection churn.
    /// That bound is why the hub's reactive state needs no `ctx.scope()`: the hub
    /// owns a private `ThreadSafeContext` that is dropped with the hub, and within
    /// one hub the graph stops growing once every identity has been seen.
    #[test]
    fn liveness_edge_set_is_bounded_by_distinct_identities_not_churn() {
        let mut hub = RelayHub::new(1);
        for id in 2..=9u64 {
            hub.register(id).unwrap();
        }
        assert_eq!(hub.live_count(), 8);
        let saturated = hub.ctx.dependency_count(&hub.live_editor_count);

        // Every identity has now been seen; a second full churn pass adds nothing.
        for id in 2..=9u64 {
            hub.deregister(id);
        }
        assert_eq!(hub.live_count(), 0);
        for id in 2..=9u64 {
            hub.register(id).unwrap();
        }
        assert_eq!(hub.live_count(), 8);

        assert_eq!(
            hub.ctx.dependency_count(&hub.live_editor_count),
            saturated,
            "the derived count's edge set must saturate at the distinct-identity \
             count instead of growing with each connection cycle",
        );
    }
}
