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

use similar::{Algorithm, DiffTag, capture_diff_slices};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
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

use agent_doc_element::element::{self, Component};
use agent_doc_merge::document_cell::{ThreadSafeDocumentCellTree, project_document};
use agent_doc_merge::crdt_sync::{ReplicaState, commit_barrier_ready, flush_to_commit_barrier};

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
    /// The last replica frontier the editor has either published itself or
    /// acknowledged as visibly projected. Unlike `replica`, this does not
    /// optimistically absorb outbound deliveries. It therefore preserves the
    /// semantic base of a local edit that races a controller replacement.
    observed_replica: ReplicaState,
    generation: u64,
    last_ack_generation: u64,
    pending: VecDeque<PendingReplicaUpdate>,
    /// `#pullnoackdeadlock`: how many times this member has been handed the same
    /// undelivered head without its ACK ever advancing. Reset by any ACK that
    /// moves `last_ack_generation`, and by a fresh enqueue.
    redeliveries_without_ack: u32,
    /// `#silentreplicabarrier`: bounded delivery-convergence waits that expired
    /// against this member's unacked head without the member saying anything at
    /// all — not even a pull. Reset by any pull, ACK, or projection.
    barrier_waits_without_progress: u32,
}

impl Member {
    /// Forward progress from this member clears every non-convergence streak.
    ///
    /// The two streaks bound different wedges (`#pullnoackdeadlock` a replica
    /// that pulls forever without ACKing, `#silentreplicabarrier` one that never
    /// pulls at all), but they are released by exactly the same evidence, so
    /// they are cleared together rather than at eight separate call sites each.
    fn clear_nonconvergence_streaks(&mut self) {
        self.redeliveries_without_ack = 0;
        self.barrier_waits_without_progress = 0;
    }
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

/// `#silentreplicabarrier`: expired delivery-convergence waits before a replica
/// that has said *nothing* stops holding the barrier.
///
/// [`MAX_REDELIVERIES_WITHOUT_ACK`] bounds a replica that keeps pulling the same
/// head. It cannot bound a replica that never pulls: its counter only advances
/// inside [`RelayHub::pending_updates`], so a member that registers and then goes
/// silent holds [`RelayHub::delivery_converged`] false forever. Observed
/// 2026-09-12 on `tasks/agent-doc/agent-doc-bugs.md`, where a JetBrains replica
/// restart left client `2121428668057853` registered with a queued canonical
/// projection receipt and zero subsequent traffic — no pull, no ACK, no
/// projection — so `redeliveries_without_ack` stayed at 0 while every preflight
/// refused admission with `Lazily current authority remained delivery_pending`.
///
/// The charge is one expired bounded wait on the delivery-convergence cell. That
/// wait returns early on *any* delivery-epoch change, so an expiry proves nothing
/// moved: no ACK, no enqueue, no liveness transition. Preflight parks in 500ms
/// slices for a ~3s budget, so a single preflight attempt charges ~6 and fails
/// closed on a frontier it cannot yet distinguish from a slow one; a second
/// attempt crosses this threshold and admits. A merely slow editor never accrues
/// the streak at all, because a plain pull clears it.
pub const MAX_BARRIER_WAITS_WITHOUT_PROGRESS: u32 = 12;

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
    /// The currently-live replicas that should receive `update` — the OTHER
    /// members, plus `origin` itself when `component_isolation_reconciled` is set
    /// (a repair has to reach the buffer that can see the damage).
    pub targets: Vec<u64>,
    /// `#reconcilesyntheticbase`: the raw union materialized this member's ops in
    /// a region the member never edited, and the hub restored ONLY those regions
    /// to the content the canonical already held. `update` carries the restoring
    /// ops like any other delta; no lineage is rotated and no replica is
    /// rebootstrapped. Previously this reported a whole-document rebuild from a
    /// merge taken over a synthetic base, which reset every replica.
    pub component_isolation_reconciled: bool,
    /// `#queuelineclobber`: the region-scoped repair was REFUSED because it would
    /// have dropped text this member had just inserted, so the raw union was
    /// published instead. Callers log this: the member's characters demonstrably
    /// landed outside the component it was editing, and the only repair available
    /// would delete them, which is a defect to investigate rather than a steady
    /// state.
    pub component_isolation_refused_lossy: bool,
    /// `#cpwritecomponentscoped`: whether this packet's edits were bounded by a
    /// caller-declared component scope, and if not, why. Only a CP write can
    /// declare a scope; member-originated packets are always
    /// [`ComponentScopeOutcome::NotRequested`].
    pub component_scope: ComponentScopeOutcome,
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

/// Split a whole-document CP replacement into the disjoint codepoint spans that
/// actually changed (`#exchangetypingrevert`), highest offset first.
///
/// Peeling one shared prefix and one shared suffix (what this replaced) means
/// every difference collapses into a SINGLE delete+insert span running from the
/// first differing character to the last. A `queue_maintenance` write that
/// strikes a queue line *and* rewrites the status line therefore tombstoned
/// every character between those two regions — whole untouched components and
/// both of their markers included — and reinserted them as brand-new
/// characters. An insertion a member made concurrently inside that range is
/// anchored among those tombstones, so the raw union materializes it on the far
/// side of a component marker and `relay_update_capture`'s isolation reconcile
/// rebootstraps the hub from the reconciled text. That rebootstrap is what the
/// operator sees as their own exchange typing reverting under them.
///
/// Diffing per line first keeps the tombstoned set to the lines the CP actually
/// rewrote. A component the write left byte-identical keeps its original
/// character identities, so a concurrent insertion inside it merges natively
/// with no cross-component union to reconcile at all.
///
/// Every returned span addresses the PRE-edit text, so they are ordered by
/// descending offset: applying them in order never invalidates a later offset.
fn minimal_char_span_edits(current: &str, content: &str) -> Result<Vec<(u32, u32, String)>> {
    if current == content {
        return Ok(Vec::new());
    }
    let current_lines = split_lines_inclusive(current);
    let content_lines = split_lines_inclusive(content);
    let mut line_start_chars = Vec::with_capacity(current_lines.len() + 1);
    let mut running = 0usize;
    line_start_chars.push(running);
    for line in &current_lines {
        running += line.chars().count();
        line_start_chars.push(running);
    }

    // Myers is O(N*D); on a 60KB session document a full rewrite would make D
    // the whole document. Peeling the identical head and tail first is linear
    // and leaves Myers only the region that actually differs, which is what a CP
    // write always is. Offsets stay in whole-document space via `head`.
    let mut head = 0usize;
    while head < current_lines.len().min(content_lines.len())
        && current_lines[head] == content_lines[head]
    {
        head += 1;
    }
    let mut tail = 0usize;
    while tail < current_lines.len() - head
        && tail < content_lines.len() - head
        && current_lines[current_lines.len() - 1 - tail] == content_lines[content_lines.len() - 1 - tail]
    {
        tail += 1;
    }
    let current_middle = &current_lines[head..current_lines.len() - tail];
    let content_middle = &content_lines[head..content_lines.len() - tail];

    // Merge adjacent non-equal ops (Myers emits Delete then Insert for a
    // replacement) so one changed region becomes one span rather than two
    // spans sharing an offset, whose relative order a sort could not preserve.
    let mut hunks: Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> = Vec::new();
    for op in capture_diff_slices(Algorithm::Myers, current_middle, content_middle) {
        if op.tag() == DiffTag::Equal {
            continue;
        }
        let (old, new) = (
            op.old_range().start + head..op.old_range().end + head,
            op.new_range().start + head..op.new_range().end + head,
        );
        match hunks.last_mut() {
            Some((prev_old, prev_new)) if prev_old.end == old.start && prev_new.end == new.start => {
                prev_old.end = old.end;
                prev_new.end = new.end;
            }
            _ => hunks.push((old, new)),
        }
    }

    let mut edits = Vec::with_capacity(hunks.len());
    for (old, new) in hunks {
        let deleted: Vec<char> = current_lines[old.clone()].concat().chars().collect();
        let inserted: Vec<char> = content_lines[new].concat().chars().collect();
        // Line granularity bounds the blast radius; peeling the hunk's own
        // shared prefix/suffix keeps a one-character edit a one-character edit.
        let mut lead = 0usize;
        while lead < deleted.len() && lead < inserted.len() && deleted[lead] == inserted[lead] {
            lead += 1;
        }
        let mut tail = 0usize;
        while tail < deleted.len() - lead
            && tail < inserted.len() - lead
            && deleted[deleted.len() - 1 - tail] == inserted[inserted.len() - 1 - tail]
        {
            tail += 1;
        }
        let delete_len = deleted.len() - lead - tail;
        let insert: String = inserted[lead..inserted.len() - tail].iter().collect();
        if delete_len == 0 && insert.is_empty() {
            continue;
        }
        edits.push((
            (line_start_chars[old.start] + lead)
                .try_into()
                .map_err(|_| anyhow!("canonical edit offset exceeds CRDT codepoint range"))?,
            delete_len
                .try_into()
                .map_err(|_| anyhow!("canonical edit length exceeds CRDT codepoint range"))?,
            insert,
        ));
    }
    edits.sort_unstable_by_key(|edit| std::cmp::Reverse(edit.0));
    Ok(edits)
}

/// Outcome of the component-scope check a CP write carried (`#cpwritecomponentscoped`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentScopeOutcome {
    /// The caller supplied no scope, so the changed regions were rediscovered
    /// by a whole-document text diff.
    NotRequested,
    /// The scope resolved against both documents and the edits were computed
    /// **only** inside the scoped component bodies.
    Enforced,
    /// A scoped component is absent from one of the two documents, so the write
    /// is materializing or removing a component rather than editing one. The
    /// scope cannot bound such a write, so the unscoped path was used; callers
    /// log this because a steady-state CP write should not hit it.
    Unresolvable,
}

/// Char offset of `byte_offset` in `doc`.
fn char_offset_of(doc: &str, byte_offset: usize) -> usize {
    doc[..byte_offset].chars().count()
}

/// Edits for a CP write that declared which components it mutates
/// (`#cpwritecomponentscoped`).
///
/// The whole-document diff is never taken. Each scoped component body is diffed
/// against its counterpart and the resulting spans are shifted into document
/// space, so no edit can be emitted for any other region — the property the
/// scope exists to provide. Everything outside the scoped bodies must match
/// exactly; a target that moved text outside its own scope is a CP defect and is
/// refused here rather than published to every replica.
fn component_scoped_char_span_edits(
    current: &str,
    content: &str,
    scope: &agent_doc_element::ComponentWriteScope,
) -> Result<Option<Vec<(u32, u32, String)>>> {
    let (Some(current_bodies), Some(content_bodies)) = (
        agent_doc_element::component_scope::scoped_component_bodies(current, scope),
        agent_doc_element::component_scope::scoped_component_bodies(content, scope),
    ) else {
        return Ok(None);
    };
    if current_bodies.len() != content_bodies.len() {
        return Ok(None);
    }
    if agent_doc_element::component_scope::text_outside_bodies(current, &current_bodies)
        != agent_doc_element::component_scope::text_outside_bodies(content, &content_bodies)
    {
        return Err(anyhow!(
            "CP write changed text outside its declared component scope [{}]; refusing to publish (#cpwritecomponentscoped)",
            scope
                .components()
                .iter()
                .map(|component| format!("{}:{}", component.name, component.occurrence))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut edits = Vec::new();
    for (current_body, content_body) in current_bodies.iter().zip(content_bodies.iter()) {
        let before = &current[current_body.clone()];
        let after = &content[content_body.clone()];
        if before == after {
            continue;
        }
        let base: u32 = char_offset_of(current, current_body.start)
            .try_into()
            .map_err(|_| anyhow!("canonical edit offset exceeds CRDT codepoint range"))?;
        for (offset, delete_len, insert) in minimal_char_span_edits(before, after)? {
            edits.push((
                base.checked_add(offset)
                    .ok_or_else(|| anyhow!("canonical edit offset exceeds CRDT codepoint range"))?,
                delete_len,
                insert,
            ));
        }
    }
    // Highest offset first, so applying them in order keeps every later offset
    // valid — the same contract `minimal_char_span_edits` publishes.
    edits.sort_unstable_by_key(|edit| std::cmp::Reverse(edit.0));
    Ok(Some(edits))
}

/// Split on `\n` while keeping each terminator with its line, so concatenating
/// the result reproduces the input exactly.
fn split_lines_inclusive(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            lines.push(&text[start..index + 1]);
            start = index + 1;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
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

impl DocumentOpDeltaOutcome {
    /// Whether this terminal outcome must also fence the live replica(s) that
    /// authored the rejected operations. A durable frame and a direct replica
    /// update are two transports for the same CRDT intent; quarantining only the
    /// durable copy would let the direct transport replay stale state.
    pub const fn requires_origin_projection(self) -> bool {
        matches!(self, Self::StaleLineage | Self::LegacyQuarantined)
    }
}

/// Observable result of fencing registered replica origins after a durable
/// document-op quarantine.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DocumentOpOriginFence {
    pub registered_origins: usize,
    pub projections_queued: usize,
}

/// Controller-owned in-memory projection retained independently of a relay
/// member generation. It is deliberately not serialized to the CRDT recovery
/// sidecar: a relay recycle reattaches to this live value, while a cold process
/// starts from the normal controller document input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedCanonicalProjection {
    pub state: Vec<u8>,
    /// Exact visible text captured beside `state` at the same hub frontier.
    /// This is an in-memory controller handoff value, not a second authority.
    pub current_text: String,
    pub lineage: String,
    pub last_committed_text: Option<String>,
    pub last_committed_state_vector: Option<Vec<u8>>,
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
    /// O(1)-sized CRDT frontier paired with [`Self::last_committed_text`] when
    /// that text exactly matched the canonical at the commit boundary. Hub
    /// eviction compares this token instead of materializing the entire CRDT
    /// while the global relay registry is locked.
    last_committed_state_vector: Option<Vec<u8>>,
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

/// `#queuelineclobber`: non-blank lines the member just inserted that `isolated`
/// would not carry through.
///
/// A line present in the member's post-edit buffer more often than in their own
/// pre-edit buffer is text they just typed. No other replica has observed it yet,
/// so no concurrent side can legitimately have deleted it — if the reconciled text
/// does not contain it, the reconcile is losing operator-authored bytes.
///
/// Counts are multisets so a duplicated insertion is not masked by one surviving
/// copy, and non-blank lines are compared trimmed so pure re-indentation inside
/// the repair is not reported as loss. Blank lines are counted under their own
/// key: a blank line the member typed is still operator-authored text, and
/// skipping it was a hole in the net (`#reconcilesyntheticbase`). Lines the
/// canonical already held independently only make the check more permissive,
/// which keeps it conservative against false alarms.
///
/// The surviving-count comparison is against the member's POST-edit count, not
/// against the inserted delta. A member that duplicates a line the document
/// already held inserts one copy while one copy survives, so comparing against
/// the delta alone read `1 < 1` and reported no loss even though the duplicate
/// was dropped (`#reconcilesyntheticbase`).
fn member_insertions_lost_by(
    intent_before: &str,
    intent_after: &str,
    isolated: &str,
) -> Vec<String> {
    fn line_counts(text: &str) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::new();
        for line in text.lines() {
            *counts.entry(line.trim()).or_default() += 1;
        }
        counts
    }
    let before = line_counts(intent_before);
    let after = line_counts(intent_after);
    let result = line_counts(isolated);
    let mut lost = Vec::new();
    for (line, after_count) in after {
        let inserted = after_count.saturating_sub(before.get(line).copied().unwrap_or(0));
        if inserted > 0 && result.get(line).copied().unwrap_or(0) < after_count {
            lost.push(line.to_string());
        }
    }
    lost
}

/// One comparable region of a session document: either a component occurrence's
/// body, or the framing text between two bodies (the markers themselves plus any
/// prose that sits outside every component).
///
/// Regions are the unit the component firewall reasons over. Component bodies are
/// keyed by `component:<name>:<occurrence>` so a body edit elsewhere in the
/// document cannot shift a region's identity, and each framing run is keyed by the
/// component it follows (`frame:head` for the leading run) so it is equally stable.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentRegion {
    key: String,
    /// Byte offset of the region's first character in the source document.
    start: usize,
    /// Byte offset past the region's last character in the source document.
    end: usize,
}

/// Split `doc` into stably keyed regions, or `None` when it does not parse as a
/// component document.
///
/// Only top-level occurrences contribute a body region: a nested component's body
/// is already inside its parent's body, so projecting both would count one change
/// twice.
fn document_regions(doc: &str) -> Option<Vec<DocumentRegion>> {
    let components = element::parse(doc).ok()?;
    let mut top: Vec<&Component> = components
        .iter()
        .filter(|c| {
            !components.iter().any(|other| {
                !std::ptr::eq(other, *c)
                    && other.open_start <= c.open_start
                    && c.close_end <= other.close_end
            })
        })
        .collect();
    top.sort_by_key(|c| c.open_start);

    let mut occurrences: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<DocumentRegion> = Vec::with_capacity(top.len() * 2 + 1);
    let mut cursor = 0usize;
    let mut previous = "head".to_string();
    for comp in top {
        let occurrence = occurrences.entry(comp.name.as_str()).or_insert(0);
        let key = format!("component:{}:{}", comp.name, *occurrence);
        *occurrence += 1;
        out.push(DocumentRegion {
            key: format!("frame:{previous}"),
            start: cursor,
            end: comp.open_end,
        });
        out.push(DocumentRegion {
            key: key.clone(),
            start: comp.open_end,
            end: comp.close_start,
        });
        cursor = comp.close_start;
        previous = key;
    }
    out.push(DocumentRegion {
        key: format!("frame:{previous}"),
        start: cursor,
        end: doc.len(),
    });
    Some(out)
}

/// The keyed region contents of `doc`, or `None` when it does not parse.
fn region_texts(doc: &str) -> Option<BTreeMap<String, &str>> {
    Some(
        document_regions(doc)?
            .into_iter()
            .map(|region| (region.key, &doc[region.start..region.end]))
            .collect(),
    )
}

/// Regions of the raw peer union that hold **positive evidence** of
/// cross-component damage: they changed, and the member that produced the update
/// never touched them in its own buffer.
///
/// `#reconcilesyntheticbase`. The previous trigger was a disagreement between the
/// union and a component-scoped three-way merge taken over a SYNTHETIC
/// `CrdtDoc::from_text` base — an arbiter reasoning over fabricated op identities,
/// which the code could not trust to decide anything, least of all to authorize a
/// repair that rebootstrapped every replica. Merge disagreement is not damage: the
/// component-scoped merge and a native union routinely order the same
/// non-conflicting result differently.
///
/// This asks the question the firewall actually exists to answer. `union_after` is
/// `canonical_before` plus exactly one delta — the ops pulled from this member's
/// mirror — so a region that changed while the member's own before/after buffers
/// agree on it can only have changed because the member's characters materialized
/// there. That is the cross-component materialization, observed rather than
/// inferred. A region the member DID edit is native single-component convergence
/// and is never reported.
///
/// A region key missing from either the canonical or the member's own projection
/// is not comparable (the member is editing a projection with different framing),
/// and is deliberately not reported: an unprovable suspicion must not authorize a
/// repair that can cost operator bytes. Structural breakage that reaches the
/// canonical is still caught downstream by the `agent-doc-crdt-relay-io` parse
/// guard, which restores the pre-update canonical.
fn cross_component_union_damage(
    canonical_before: &str,
    union_after: &str,
    intent_before: &str,
    intent_after: &str,
) -> Vec<String> {
    let (Some(before), Some(after), Some(intent_pre), Some(intent_post)) = (
        region_texts(canonical_before),
        region_texts(union_after),
        region_texts(intent_before),
        region_texts(intent_after),
    ) else {
        return Vec::new();
    };
    let mut damaged = Vec::new();
    for (key, after_text) in &after {
        let Some(before_text) = before.get(key) else {
            continue;
        };
        if after_text == before_text {
            continue;
        }
        let (Some(member_pre), Some(member_post)) = (intent_pre.get(key), intent_post.get(key))
        else {
            continue;
        };
        if member_pre != member_post {
            continue;
        }
        damaged.push(key.clone());
    }
    damaged
}

/// Byte-span replacements that restore the named damaged regions of `union_after`
/// to the content the canonical held before the update, ordered by DESCENDING
/// start offset so each edit addresses pre-edit text.
///
/// This is the narrow half of `#reconcilesyntheticbase`. The repair it replaces —
/// `rebuild_component_isolated_epoch` — rebuilt the canonical and every member
/// replica from a merge result, which is the only mechanism in the system that can
/// reset an operator's buffer wholesale; a single misjudged trigger cost every
/// replica its unacknowledged typing. Restoring only the regions that carry
/// evidence of damage is structurally incapable of touching the region the member
/// was actually editing, so the worst case of a wrong call is a reverted marker
/// run rather than a lost document.
fn component_scoped_region_restore(
    union_after: &str,
    canonical_before: &str,
    damaged: &[String],
) -> Option<Vec<(usize, usize, String)>> {
    let regions = document_regions(union_after)?;
    let before = region_texts(canonical_before)?;
    let mut edits = Vec::new();
    for region in regions.iter().rev() {
        if !damaged.iter().any(|key| key == &region.key) {
            continue;
        }
        edits.push((
            region.start,
            region.end,
            (*before.get(&region.key)?).to_string(),
        ));
    }
    Some(edits)
}

/// Apply [`component_scoped_region_restore`] edits to a document string.
fn apply_region_restore(union_after: &str, edits: &[(usize, usize, String)]) -> String {
    let mut out = union_after.to_string();
    for (start, end, text) in edits {
        out.replace_range(*start..*end, text);
    }
    out
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
            last_committed_state_vector: None,
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
        hub.last_committed_state_vector = Some(hub.canonical.state_vector());
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
        let recovered_text = canonical.text();
        let mut hub = Self::new(canonical_id);
        hub.canonical = canonical;
        hub.reset_live_document_projection(&recovered_text);
        if let Some(lineage) = lineage.filter(|value| !value.is_empty()) {
            hub.lineage = lineage.to_string();
        }
        hub.last_committed_text = Some(recovered_text);
        hub.last_committed_state_vector = Some(hub.canonical.state_vector());
        Ok(hub)
    }

    /// Recreate only the disposable relay around a controller-retained
    /// canonical target.
    pub fn from_retained_canonical_projection(
        canonical_id: u64,
        projection: &RetainedCanonicalProjection,
    ) -> Result<Self> {
        // A controller handoff already carries the committed text/frontier beside the encoded
        // CRDT. Re-entering through `recover_from_projection_with_lineage` would materialize the
        // entire ordered CRDT merely to overwrite that baseline below. Repeated editor
        // rebootstrap can make the retained history much larger than the visible document, so
        // that redundant materialization can pin a replacement controller at 100% CPU and block
        // the very RPC that is meant to complete the handoff.
        //
        // Decode the retained replica without rendering it on the default path. The opt-in
        // CellDocTree projection still needs the exact current canonical text, so its explicit
        // cutover pays that cost and keeps its incremental baseline correct.
        let canonical = ReplicaState::from_encoded_with_cached_text(
            canonical_id,
            &projection.state,
            &projection.current_text,
        )?;
        let mut hub = Self::new(canonical_id);
        hub.canonical = canonical;
        if let Some(lineage) = (!projection.lineage.is_empty()).then_some(&projection.lineage) {
            hub.lineage = lineage.clone();
        }
        hub.last_committed_text = projection.last_committed_text.clone();
        hub.last_committed_state_vector = projection.last_committed_state_vector.clone();
        hub.compact_epoch_requested = projection.compact_epoch_requested;
        if hub.live_document_projection_enabled() {
            let recovered_text = hub.canonical.text();
            hub.reset_live_document_projection(&recovered_text);
        }
        Ok(hub)
    }

    /// Snapshot the live controller target for a keyed Lazily cell. This is an
    /// in-memory reactive value, not a persistence or recovery-sidecar API.
    pub fn retained_canonical_projection(&self) -> RetainedCanonicalProjection {
        RetainedCanonicalProjection {
            state: self.canonical.encode_state(),
            current_text: self.canonical.text(),
            lineage: self.lineage.clone(),
            last_committed_text: self.last_committed_text.clone(),
            last_committed_state_vector: self.last_committed_state_vector.clone(),
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
    /// only after that committed baseline exactly matches the canonical CRDT
    /// frontier. The frontier comparison avoids materializing large documents
    /// while the process-global relay registry is locked.
    pub fn is_safe_to_evict(&self) -> bool {
        self.members.is_empty()
            && self.pending_rebootstrap.is_empty()
            && self
                .last_committed_state_vector
                .as_ref()
                .is_some_and(|committed| self.canonical.state_vector() == *committed)
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
        let bootstrap = self.canonical.encode_state();
        let replica = ReplicaState::from_encoded(client_id, &bootstrap)?;
        let observed_replica = ReplicaState::from_encoded(client_id, &bootstrap)?;
        self.members.insert(
            client_id,
            Member {
                replica,
                observed_replica,
                generation: 0,
                last_ack_generation: 0,
                pending: VecDeque::new(),
                redeliveries_without_ack: 0,
                barrier_waits_without_progress: 0,
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

    /// Fence each registered author of a quarantined durable document-op frame
    /// and queue the current canonical projection as its repair receipt.
    ///
    /// The CRDT operation ids are the causal evidence. Unknown or retired peers
    /// are ignored, so one stale frame cannot fence an unrelated live replica.
    pub fn fence_registered_document_op_origins(
        &mut self,
        origin_ids: impl IntoIterator<Item = u64>,
    ) -> Result<DocumentOpOriginFence> {
        let mut unique = HashSet::new();
        let mut result = DocumentOpOriginFence::default();
        for client_id in origin_ids {
            if !unique.insert(client_id) || !self.is_registered(client_id) {
                continue;
            }
            result.registered_origins += 1;
            if self.ensure_canonical_projection_receipt(client_id)? {
                result.projections_queued += 1;
            }
        }
        Ok(result)
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
                m.clear_nonconvergence_streaks();
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
        member.clear_nonconvergence_streaks();
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
            component_isolation_reconciled: false,
            component_isolation_refused_lossy: false,
            component_scope: ComponentScopeOutcome::NotRequested,
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
        let member_before_text = member.replica.text();
        let observed_before_text = member.observed_replica.text();
        // Apply the editor's encoded op to its hub-side mirror first.
        member.replica.apply_update(update)?;
        let member_after_text = member.replica.text();
        // The acknowledged mirror has intentionally not absorbed unacknowledged
        // outbound controller writes. Applying the editor's own delta here
        // reconstructs the actual before/after intent that produced this update.
        member.observed_replica.apply_update(update)?;
        let observed_after_text = member.observed_replica.text();
        // Then pull whatever the mirror now holds that canonical is missing.
        let before = self.canonical.state_vector();
        let into_canonical = member.replica.diff(&self.canonical.state_vector())?;
        self.canonical.apply_update(&into_canonical)?;
        let after_text = self.canonical.text();

        // `TextCrdt` retains character origins, not agent-doc component
        // identity. A member editing an older projection can therefore produce
        // a causally valid insertion that a raw union materializes on the far
        // side of a component marker after a concurrent broad replacement.
        // Reconstruct what that member actually intended before deciding whether
        // the union did that. Prefer the editor-acknowledged frontier; fall back
        // to the optimistic member mirror only when a causally incomplete update
        // cannot yet materialize against that frontier.
        let (intent_before_text, intent_after_text) = if observed_before_text != observed_after_text
        {
            (&observed_before_text, &observed_after_text)
        } else {
            (&member_before_text, &member_after_text)
        };
        // Plain-text documents have no component boundary to protect and must
        // retain native CRDT peer-union behavior. The semantic firewall is only
        // active once the canonical document actually contains component cells.
        let component_scoped = !project_document(&before_text).is_empty();
        let mut component_isolation_refused_lossy = false;
        let mut component_isolation_reconciled = false;
        if component_scoped
            && (intent_before_text != &before_text || intent_after_text != &after_text)
        {
            // `#reconcilesyntheticbase`: the trigger is positive evidence of
            // cross-component damage — a region that changed while the member's
            // own before/after buffers agree it was untouched, so the member's
            // characters demonstrably materialized outside the component it was
            // editing. It is no longer a disagreement with a merge taken over a
            // synthetic base; that arbiter reasoned over fabricated op identities
            // and its verdict could not authorize anything, let alone a repair
            // that rebootstrapped every replica.
            let damaged = cross_component_union_damage(
                &before_text,
                &after_text,
                intent_before_text,
                intent_after_text,
            );
            if !damaged.is_empty() {
                match component_scoped_region_restore(&after_text, &before_text, &damaged) {
                    Some(edits) if !edits.is_empty() => {
                        // `#queuelineclobber`: the repair must never drop text
                        // this member just inserted. Region-scoped restoration
                        // cannot reach the region the member edited, so this is a
                        // post-condition rather than a gamble — but it is checked
                        // on the prospective result, before any op reaches the
                        // canonical, so a refusal costs nothing.
                        let prospective = apply_region_restore(&after_text, &edits);
                        let lost = member_insertions_lost_by(
                            intent_before_text,
                            intent_after_text,
                            &prospective,
                        );
                        if lost.is_empty() {
                            for (start, end, text) in &edits {
                                let offset = after_text[..*start].chars().count() as u32;
                                let delete_len =
                                    after_text[*start..*end].chars().count() as u32;
                                self.canonical.apply_local_edit(offset, delete_len, text);
                            }
                            component_isolation_reconciled = true;
                            eprintln!(
                                "[crdt] component_isolation_regions_restored client_id={client_id} regions={damaged:?}"
                            );
                        } else {
                            component_isolation_refused_lossy = true;
                            eprintln!(
                                "[crdt] component_isolation_restore_refused reason=member_insertion_would_be_lost client_id={client_id} regions={damaged:?} lost_lines={} first={:?}",
                                lost.len(),
                                lost.first().map(|line| line.chars().take(120).collect::<String>()),
                            );
                        }
                    }
                    // The union no longer parses into regions, so no narrow
                    // repair can be expressed. Publish the raw union: the
                    // `agent-doc-crdt-relay-io` parse guard restores the
                    // pre-update canonical for exactly this case, and a
                    // whole-document rebuild here would cost every replica its
                    // unacknowledged typing to fix a structural break that guard
                    // already owns.
                    _ => {
                        component_isolation_refused_lossy = true;
                        eprintln!(
                            "[crdt] component_isolation_restore_refused reason=region_map_unavailable client_id={client_id} regions={damaged:?}"
                        );
                    }
                }
            }
        }
        let published_text = self.canonical.text();
        self.sync_live_document_projection(&before_text, &published_text);
        let delta = self.canonical.diff(&before)?;
        // A restored region has to reach the ORIGIN editor too. Convergence is
        // deterministic, so the editor that produced the update will materialize
        // the same cross-component result as soon as it applies the canonical's
        // concurrent write; excluding it would leave the one buffer that can see
        // the damage uncorrected. Deltas are idempotent, so re-sending the
        // member's own ops back to it is a no-op.
        let targets: Vec<u64> = self
            .members
            .keys()
            .copied()
            .filter(|id| {
                (component_isolation_reconciled || *id != client_id) && self.is_live(*id)
            })
            .collect();
        let packet = BroadcastPacket {
            origin: client_id,
            update: delta,
            targets,
            component_isolation_reconciled,
            component_isolation_refused_lossy,
            component_scope: ComponentScopeOutcome::NotRequested,
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
            component_isolation_reconciled: false,
            component_isolation_refused_lossy: false,
            component_scope: ComponentScopeOutcome::NotRequested,
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
        self.apply_canonical_replace_scoped(expected_current, content, None)
    }

    /// [`Self::apply_canonical_replace`] for a CP write that declared which
    /// components it mutates (`#cpwritecomponentscoped`).
    ///
    /// With a scope, the changed regions are not rediscovered from a
    /// whole-document diff: only the scoped component bodies are diffed, so the
    /// write is structurally incapable of emitting an edit for any other
    /// component. A target whose text moved outside its own scope is refused.
    /// When the scope cannot be resolved against both documents — the write is
    /// creating or removing a component rather than editing one — this falls
    /// back to the unscoped path and reports
    /// [`ComponentScopeOutcome::Unresolvable`] so the caller can log it.
    pub fn apply_canonical_replace_scoped(
        &mut self,
        expected_current: &str,
        content: &str,
        scope: Option<&agent_doc_element::ComponentWriteScope>,
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
        let (component_scope, edits) = match scope {
            None => (
                ComponentScopeOutcome::NotRequested,
                minimal_char_span_edits(&current, content)?,
            ),
            Some(scope) => match component_scoped_char_span_edits(&current, content, scope)? {
                Some(edits) => (ComponentScopeOutcome::Enforced, edits),
                None => (
                    ComponentScopeOutcome::Unresolvable,
                    minimal_char_span_edits(&current, content)?,
                ),
            },
        };
        // `#exchangetypingrevert`: one span per changed region, highest offset
        // first, so an untouched component between two changed ones is never
        // tombstoned and a concurrent member insertion inside it survives.
        for (offset, delete_len, insert) in edits {
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
            component_isolation_reconciled: false,
            component_isolation_refused_lossy: false,
            component_scope,
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
            // `#pullnoackdeadlock`: only a member that was CAUGHT UP earns a
            // fresh budget. Clearing the streak on every enqueue let a replica
            // that never ACKs anything re-earn the full budget per write, so the
            // barrier was re-imposed for ~25s on each new write instead of
            // staying released — observed 2026-08-09 immediately after 0.35.217,
            // where the same client re-armed at `redeliveries=0` on generation 7
            // while `last_ack_generation` had been stuck at 5.
            if member.pending.is_empty() {
                member.clear_nonconvergence_streaks();
            }
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
        // `#pullnoackdeadlock`: same rule as the fan-out enqueue — only a
        // caught-up member earns a fresh budget.
        if member.pending.is_empty() {
            member.clear_nonconvergence_streaks();
        }
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
            // `#silentreplicabarrier`: a pull is not ACK progress, but it does
            // prove the member is still servicing delivery. Only total silence
            // accrues the barrier-wait streak.
            member.barrier_waits_without_progress = 0;
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
    ///
    /// `#silentreplicabarrier`: a member that has not even *pulled* across
    /// [`MAX_BARRIER_WAITS_WITHOUT_PROGRESS`] expired convergence waits is not
    /// converging either, and is released on the same terms.
    fn member_holds_delivery_barrier(member: &Member) -> bool {
        !member.pending.is_empty()
            && member.redeliveries_without_ack <= MAX_REDELIVERIES_WITHOUT_ACK
            && member.barrier_waits_without_progress <= MAX_BARRIER_WAITS_WITHOUT_PROGRESS
    }

    /// Charge one expired delivery-convergence wait against every live member
    /// still holding the barrier, and return the ids this charge released.
    ///
    /// `#silentreplicabarrier`: the caller is the bounded await on the
    /// delivery-convergence cell, which returns early on any delivery-epoch
    /// change. Reaching its deadline therefore *is* the observation that the
    /// barrier did not move, which makes this a function of the delivery stream
    /// rather than a wall clock — the same shape `#pullnoackdeadlock` settled on.
    pub fn charge_barrier_wait_without_progress(&mut self) -> Vec<u64> {
        let live_holders: Vec<u64> = self
            .members
            .iter()
            .filter(|(id, member)| self.is_live(**id) && Self::member_holds_delivery_barrier(member))
            .map(|(id, _)| *id)
            .collect();
        let mut released = Vec::new();
        for id in live_holders {
            let Some(member) = self.members.get_mut(&id) else {
                continue;
            };
            member.barrier_waits_without_progress =
                member.barrier_waits_without_progress.saturating_add(1);
            if !Self::member_holds_delivery_barrier(member) {
                released.push(id);
            }
        }
        if !released.is_empty() {
            // Releasing the last holder converges delivery, so waiters must wake.
            self.bump_delivery_epoch();
            released.sort_unstable();
        }
        released
    }

    /// Replicas that are live but have stopped converging (`#pullnoackdeadlock`,
    /// `#silentreplicabarrier`).
    pub fn nonconverging_replicas(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .members
            .iter()
            .filter(|(id, member)| {
                self.is_live(**id)
                    && !member.pending.is_empty()
                    && !Self::member_holds_delivery_barrier(member)
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
            member.clear_nonconvergence_streaks();
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
        member.clear_nonconvergence_streaks();
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
                member.observed_replica =
                    ReplicaState::from_encoded(client_id, &member.replica.encode_state())?;
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
        let acknowledged_updates: Vec<Vec<u8>> = member
            .pending
            .range(..=matched_pos)
            .map(|pending| pending.update.clone())
            .collect();
        for update in acknowledged_updates {
            member.observed_replica.apply_update(&update)?;
        }
        member.pending.drain(..=matched_pos);
        member.last_ack_generation = member.last_ack_generation.max(projected_generation);
        member.clear_nonconvergence_streaks();
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
        self.last_committed_state_vector =
            (self.canonical.text() == committed).then(|| self.canonical.state_vector());
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
                self.last_committed_state_vector = None;
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
            self.last_committed_state_vector = Some(self.canonical.state_vector());
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
        self.last_committed_state_vector = Some(self.canonical.state_vector());
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
            self.last_committed_state_vector = Some(self.canonical.state_vector());
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
        self.rebuild_component_isolated_epoch(before_text, text)?;
        self.last_committed_text = Some(text.to_string());
        self.last_committed_state_vector = Some(self.canonical.state_vector());
        Ok(())
    }

    /// Replace the additive whole-text lineage with a component-isolated visible
    /// result without claiming that result has already crossed the commit
    /// boundary. Every old-lineage delivery is discarded and each live editor is
    /// queued for the existing replace-capable re-bootstrap path.
    fn rebuild_component_isolated_epoch(&mut self, before_text: &str, text: &str) -> Result<()> {
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
            let observed_replica = ReplicaState::from_encoded(id, &bootstrap)?;
            if let Some(member) = self.members.get_mut(&id) {
                member.replica = replica;
                member.observed_replica = observed_replica;
                member.pending.clear();
                member.last_ack_generation = member.generation;
                member.clear_nonconvergence_streaks();
            }
        }
        let live: Vec<u64> = self
            .members
            .keys()
            .copied()
            .filter(|id| self.is_live(*id))
            .collect();
        self.pending_rebootstrap.extend(live);
        self.last_committed_state_vector = self
            .last_committed_text
            .as_ref()
            .filter(|committed| committed.as_str() == text)
            .map(|_| self.canonical.state_vector());
        self.bump_delivery_epoch();
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

    /// Document shared by the `#reconcilesyntheticbase` region tests.
    const REGION_DOC: &str = concat!(
        "<!-- agent:exchange -->\n",
        "Prompt.\n",
        "<!-- /agent:exchange -->\n",
        "<!-- agent:queue -->\n",
        "- do [#a]\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn document_regions_key_component_bodies_and_the_framing_runs_between_them() {
        let regions = document_regions(REGION_DOC).expect("component document");
        let keys: Vec<&str> = regions.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "frame:head",
                "component:exchange:0",
                "frame:component:exchange:0",
                "component:queue:0",
                "frame:component:queue:0",
            ],
            "a body region per component, and a framing run keyed by the component it follows"
        );
        let texts = region_texts(REGION_DOC).expect("component document");
        assert_eq!(texts["component:exchange:0"], "Prompt.\n");
        assert_eq!(texts["component:queue:0"], "- do [#a]\n");
        // The markers themselves live in the framing runs, so text that lands
        // between two components is attributed to a region of its own rather
        // than silently folded into a neighbouring body.
        assert_eq!(
            texts["frame:component:exchange:0"],
            "<!-- /agent:exchange -->\n<!-- agent:queue -->\n"
        );
        // Regions tile the document exactly: every byte is attributed once.
        let mut cursor = 0usize;
        for region in &regions {
            assert_eq!(region.start, cursor, "regions must be contiguous");
            cursor = region.end;
        }
        assert_eq!(cursor, REGION_DOC.len(), "regions must cover the document");
        assert!(document_regions("plain prose, no components\n").is_some());
        assert!(
            document_regions("<!-- agent:queue -->\nunclosed\n").is_none(),
            "an unparseable document has no comparable regions"
        );
    }

    #[test]
    fn cross_component_union_damage_reports_a_member_op_that_materialized_outside_its_component() {
        // The positive evidence the firewall exists to find: the member typed
        // `typed` inside its own projection, and the union materialized those
        // characters in the framing run between two components instead.
        let canonical_before = REGION_DOC;
        let union_after = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n",
            "<!-- /agent:exchange -->\n",
            "typed\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
        );
        let intent_after = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "typed\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(
            cross_component_union_damage(
                canonical_before,
                union_after,
                canonical_before,
                intent_after
            ),
            vec!["frame:component:exchange:0".to_string()],
            "a framing run the member never edited changed, so the member's ops landed there"
        );
    }

    #[test]
    fn cross_component_union_damage_ignores_the_region_the_member_was_editing() {
        // `#reconcilesyntheticbase`: an intra-component disagreement is native
        // single-component CRDT convergence. Reporting it as damage is what let a
        // merge heuristic authorize a repair that reset every replica.
        let canonical_before = REGION_DOC;
        let union_after = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "- do [#typed]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            cross_component_union_damage(
                canonical_before,
                union_after,
                canonical_before,
                union_after
            )
            .is_empty(),
            "the member edited queue, so a queue change is not cross-component damage"
        );
    }

    #[test]
    fn cross_component_union_damage_is_silent_when_regions_are_not_comparable() {
        // An unprovable suspicion must not authorize a repair. A member editing a
        // projection whose framing no longer exists yields no comparable key, and
        // structural breakage that reaches the canonical is owned by the
        // `agent-doc-crdt-relay-io` parse guard, not by a whole-document rebuild.
        // The canonical grew a `status` component the member has never seen, and
        // it is the only region that changed.
        let canonical_before = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:status -->\n",
            "idle\n",
            "<!-- /agent:status -->\n",
        );
        let union_after = canonical_before.replace("idle\n", "busy\n");
        assert!(
            cross_component_union_damage(
                canonical_before,
                &union_after,
                REGION_DOC,
                REGION_DOC
            )
            .is_empty(),
            "`component:status:0` has no counterpart in the member projection"
        );
        assert!(
            cross_component_union_damage(
                "<!-- agent:queue -->\nunclosed\n",
                &union_after,
                REGION_DOC,
                REGION_DOC
            )
            .is_empty(),
            "an unparseable side yields no regions and therefore no verdict"
        );
    }

    #[test]
    fn component_scoped_region_restore_touches_only_the_damaged_region() {
        let union_after = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt. typed\n",
            "<!-- /agent:exchange -->\n",
            "leaked\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
        );
        let damaged = vec!["frame:component:exchange:0".to_string()];
        let edits = component_scoped_region_restore(union_after, REGION_DOC, &damaged)
            .expect("both sides parse");
        assert_eq!(edits.len(), 1);
        let repaired = apply_region_restore(union_after, &edits);
        assert_eq!(
            repaired,
            concat!(
                "<!-- agent:exchange -->\n",
                "Prompt. typed\n",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:queue -->\n",
                "- do [#a]\n",
                "<!-- /agent:queue -->\n",
            ),
            "the leaked framing run is restored and the member's own edit is untouched"
        );
        // Descending order is the caller's contract: each span addresses the
        // pre-edit text, so a lower offset must never be applied first.
        let two = component_scoped_region_restore(
            union_after,
            REGION_DOC,
            &[
                "component:exchange:0".to_string(),
                "frame:component:exchange:0".to_string(),
            ],
        )
        .expect("both sides parse");
        assert_eq!(two.len(), 2);
        assert!(
            two.windows(2).all(|pair| pair[0].0 > pair[1].0),
            "edits must be ordered by descending offset, got {two:?}"
        );
        assert!(
            component_scoped_region_restore(union_after, REGION_DOC, &["component:absent:0".into()])
                .is_some_and(|edits| edits.is_empty()),
            "an unknown region names nothing to restore"
        );
    }

    #[test]
    fn a_synthetic_base_merge_disagreement_alone_no_longer_resets_every_replica() {
        // `#reconcilesyntheticbase`, and the reproduction the previous
        // investigation could not find. The old trigger was `isolated !=
        // after_text`, where `isolated` came from `merge_by_component` over a
        // SYNTHETIC `CrdtDoc::from_text(intent_before)` base — an arbiter the code
        // itself said "cannot be trusted". Every shape below makes that arbiter
        // disagree with the native union, and in four of them the disagreement was
        // deletion-shaped, so the `#queuelineclobber` net saw no lost insertion and
        // the reconcile ran: `rebuild_component_isolated_epoch` rebuilt the
        // canonical AND rebootstrapped every member replica, discarding whatever
        // each editor had typed but not yet acknowledged. Not one of these shapes
        // carries any cross-component evidence — they are all ordinary
        // intra-component convergence.
        use agent_doc_merge::crdt::{CrdtDoc, merge_by_component};

        const BASE: &str = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:notes -->\n",
            "alpha\n",
            "<!-- /agent:notes -->\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
        );
        const REPLACE_EXCHANGE: &str = concat!(
            "<!-- agent:exchange -->\n",
            "Response.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:notes -->\n",
            "alpha\n",
            "<!-- /agent:notes -->\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
        );
        const REPLACE_ALL: &str = concat!(
            "<!-- agent:exchange -->\n",
            "Response.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:notes -->\n",
            "beta\n",
            "<!-- /agent:notes -->\n",
            "<!-- agent:queue -->\n",
            "- do [#b]\n",
            "<!-- /agent:queue -->\n",
        );
        const GENERATION_ONE: &str = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\nOne.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:notes -->\n",
            "alpha\n",
            "<!-- /agent:notes -->\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
        );
        const GENERATION_TWO: &str = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\nOne.\nTwo.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:notes -->\n",
            "gamma\n",
            "<!-- /agent:notes -->\n",
            "<!-- agent:queue -->\n",
            "- ~~do [#a]~~\n",
            "<!-- /agent:queue -->\n",
        );

        // (label, canonical writes in order, member edit, whether the old
        // `#queuelineclobber` net would have let the rebuild run)
        struct Shape {
            label: &'static str,
            writes: &'static [&'static str],
            anchor: &'static str,
            at_end_of_anchor: bool,
            delete_chars: u32,
            insert: &'static str,
            old_rebuild: bool,
        }
        let shapes = [
            Shape {
                label: "replace-exchange-body / member deletes that body",
                writes: &[REPLACE_EXCHANGE],
                anchor: "Prompt.\n",
                at_end_of_anchor: false,
                delete_chars: 8,
                insert: "",
                old_rebuild: true,
            },
            Shape {
                label: "replace-every-body / member deletes notes",
                writes: &[REPLACE_ALL],
                anchor: "alpha\n",
                at_end_of_anchor: false,
                delete_chars: 6,
                insert: "",
                old_rebuild: true,
            },
            Shape {
                label: "replace-every-body / member deletes exchange",
                writes: &[REPLACE_ALL],
                anchor: "Prompt.\n",
                at_end_of_anchor: false,
                delete_chars: 8,
                insert: "",
                old_rebuild: true,
            },
            Shape {
                label: "two generations stale / member deletes notes",
                writes: &[GENERATION_ONE, GENERATION_TWO],
                anchor: "alpha\n",
                at_end_of_anchor: false,
                delete_chars: 6,
                insert: "",
                old_rebuild: true,
            },
            Shape {
                label: "two generations stale / member types at the exchange tail",
                writes: &[GENERATION_ONE, GENERATION_TWO],
                anchor: "<!-- /agent:exchange -->",
                at_end_of_anchor: false,
                delete_chars: 0,
                insert: "typed\n",
                old_rebuild: false,
            },
            Shape {
                label: "two generations stale / member types after the prompt",
                writes: &[GENERATION_ONE, GENERATION_TWO],
                anchor: "Prompt.\n",
                at_end_of_anchor: true,
                delete_chars: 0,
                insert: "typed\n",
                old_rebuild: false,
            },
        ];

        for shape in &shapes {
            let label = shape.label;
            let mut hub = RelayHub::from_text(1, BASE);
            hub.register(2).unwrap();
            hub.register(3).unwrap();
            let editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
            let frontier = editor.state_vector();
            let anchor_at = BASE.find(shape.anchor).unwrap();
            let at = if shape.at_end_of_anchor {
                anchor_at + shape.anchor.len()
            } else {
                anchor_at
            };
            editor.apply_local_edit(
                BASE[..at].chars().count() as u32,
                shape.delete_chars,
                shape.insert,
            );
            let intent_after = editor.text();
            let update = editor.diff(&frontier).unwrap();

            let mut previous = BASE;
            for write in shape.writes {
                hub.apply_canonical_replace(previous, write).unwrap();
                previous = write;
            }
            let before_text = hub.canonical_text();
            let packet = hub.relay_update(2, &update).unwrap();
            let after_text = hub.canonical_text();

            // The old trigger fires on every one of these shapes.
            let base_state = CrdtDoc::from_text(BASE).encode_state();
            let isolated =
                merge_by_component(Some(&base_state), &before_text, &intent_after).unwrap();
            assert_ne!(
                isolated, after_text,
                "{label}: this shape must still make the synthetic-base arbiter disagree, \
                 otherwise it no longer covers the regression"
            );
            assert_eq!(
                member_insertions_lost_by(BASE, &intent_after, &isolated).is_empty(),
                shape.old_rebuild,
                "{label}: the recorded pre-fix outcome must still be the one this shape produces"
            );

            // And none of it is cross-component damage, so nothing is repaired and
            // no replica is reset.
            assert!(
                cross_component_union_damage(&before_text, &after_text, BASE, &intent_after)
                    .is_empty(),
                "{label}: an intra-component disagreement is not cross-component damage"
            );
            assert!(
                !packet.component_isolation_reconciled,
                "{label}: a merge disagreement alone must not authorize a repair"
            );
            assert!(
                !packet.component_isolation_refused_lossy,
                "{label}: nothing was refused because nothing was attempted"
            );
            assert!(
                hub.pending_rebootstrap_members().is_empty(),
                "{label}: no replica may be rebootstrapped"
            );
            if !shape.insert.is_empty() {
                assert!(
                    after_text.contains("typed"),
                    "{label}: the member's just-typed text must survive; canonical was:\n{after_text}"
                );
            }
        }
    }

    #[test]
    fn a_member_insertion_that_lands_outside_every_component_is_flagged_not_deleted() {
        // End-to-end cover for the detector's positive path. The controller removes
        // the `notes` component while the operator is typing inside it, so every
        // character around the operator's caret is tombstoned and the insertion
        // materializes in the framing run between `exchange` and `queue` — a
        // member character on the far side of a component marker, which is the
        // damage the firewall exists to see.
        //
        // The narrow repair is then correctly REFUSED: restoring that framing run
        // would delete the operator's line, and no other replica has seen it yet,
        // so nothing can legitimately have deleted it. Publishing the raw union
        // leaves the text misplaced but present and visible, which the operator can
        // fix; deleting it is unrecoverable.
        const BASE: &str = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:notes -->\n",
            "alpha\n",
            "<!-- /agent:notes -->\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
        );
        const NOTES_REMOVED: &str = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
        );

        let mut hub = RelayHub::from_text(1, BASE);
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        let editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        let frontier = editor.state_vector();
        let caret = BASE.find("alpha\n").unwrap();
        editor.apply_local_edit(BASE[..caret].chars().count() as u32, 0, "typed\n");
        let intent_after = editor.text();
        let update = editor.diff(&frontier).unwrap();

        hub.apply_canonical_replace(BASE, NOTES_REMOVED).unwrap();
        let before_text = hub.canonical_text();
        let packet = hub.relay_update(2, &update).unwrap();
        let after_text = hub.canonical_text();

        assert_eq!(
            cross_component_union_damage(&before_text, &after_text, BASE, &intent_after),
            vec!["frame:component:exchange:0".to_string()],
            "the operator's characters materialized outside every component; canonical was:\n{after_text}"
        );
        assert!(
            after_text.contains("typed"),
            "the operator's just-typed line must survive; canonical was:\n{after_text}"
        );
        assert!(
            packet.component_isolation_refused_lossy,
            "the repair must refuse rather than delete operator-authored bytes"
        );
        assert!(
            !packet.component_isolation_reconciled,
            "a refused repair is not a repair"
        );
        assert!(
            hub.pending_rebootstrap_members().is_empty(),
            "and no replica is reset to publish a raw union"
        );
    }

    /// `#cpwritecomponentscoped` fixture: three components, so a scope can name
    /// one and leave an untouched neighbour on each side of it.
    const SCOPED_BASE: &str = concat!(
        "<!-- agent:exchange -->\n",
        "Prompt.\n",
        "<!-- /agent:exchange -->\n",
        "<!-- agent:notes -->\n",
        "alpha\n",
        "<!-- /agent:notes -->\n",
        "<!-- agent:queue -->\n",
        "- do [#a]\n",
        "<!-- /agent:queue -->\n",
    );

    fn queue_scope() -> agent_doc_element::ComponentWriteScope {
        agent_doc_element::ComponentWriteScope::new([agent_doc_element::ScopedComponent::new(
            "queue", 0,
        )])
    }

    #[test]
    fn a_scoped_cp_write_applies_only_inside_its_declared_component() {
        let target = SCOPED_BASE.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        let mut hub = RelayHub::from_text(1, SCOPED_BASE);
        let scope = queue_scope();

        let packet = hub
            .apply_canonical_replace_scoped(SCOPED_BASE, &target, Some(&scope))
            .unwrap();

        assert_eq!(packet.component_scope, ComponentScopeOutcome::Enforced);
        assert_eq!(hub.canonical_text(), target);
    }

    #[test]
    fn a_scoped_cp_write_that_would_touch_another_component_is_refused() {
        // The CP declared `queue` but its target image also rewrote `notes`.
        // Without the scope this publishes, and the only thing standing between
        // the stray rewrite and every replica is a diff noticing afterwards.
        let target = SCOPED_BASE
            .replace("- do [#a]\n", "- do [#a]\n- do [#b]\n")
            .replace("alpha\n", "clobbered\n");
        let mut hub = RelayHub::from_text(1, SCOPED_BASE);
        let scope = queue_scope();

        let err = hub
            .apply_canonical_replace_scoped(SCOPED_BASE, &target, Some(&scope))
            .expect_err("a write outside its declared scope must be refused");

        assert!(
            err.to_string()
                .contains("outside its declared component scope"),
            "unexpected refusal: {err}"
        );
        assert_eq!(
            hub.canonical_text(),
            SCOPED_BASE,
            "a refused scoped write must leave canonical untouched"
        );
    }

    #[test]
    fn an_unscoped_cp_write_still_publishes_a_cross_component_target() {
        // The same target, with no scope declared: this is the pre-
        // `#cpwritecomponentscoped` behaviour, and it is what makes the scope
        // worth carrying rather than a redundant assertion.
        let target = SCOPED_BASE
            .replace("- do [#a]\n", "- do [#a]\n- do [#b]\n")
            .replace("alpha\n", "clobbered\n");
        let mut hub = RelayHub::from_text(1, SCOPED_BASE);

        let packet = hub.apply_canonical_replace(SCOPED_BASE, &target).unwrap();

        assert_eq!(packet.component_scope, ComponentScopeOutcome::NotRequested);
        assert_eq!(hub.canonical_text(), target);
    }

    #[test]
    fn a_scope_naming_an_absent_component_falls_back_instead_of_refusing() {
        // A write that materializes a component cannot be bounded by a scope
        // resolved against both documents; it must still land.
        let target = SCOPED_BASE.replace(
            "<!-- agent:queue -->\n",
            "<!-- agent:review -->\n<!-- /agent:review -->\n<!-- agent:queue -->\n",
        );
        let mut hub = RelayHub::from_text(1, SCOPED_BASE);
        let scope = agent_doc_element::ComponentWriteScope::new([
            agent_doc_element::ScopedComponent::new("review", 0),
        ]);

        let packet = hub
            .apply_canonical_replace_scoped(SCOPED_BASE, &target, Some(&scope))
            .unwrap();

        assert_eq!(packet.component_scope, ComponentScopeOutcome::Unresolvable);
        assert_eq!(hub.canonical_text(), target);
    }

    #[test]
    fn scoped_edits_carry_document_offsets_inside_the_scoped_body_only() {
        let target = SCOPED_BASE.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        let scope = queue_scope();

        let edits = component_scoped_char_span_edits(SCOPED_BASE, &target, &scope)
            .unwrap()
            .expect("scope resolves against both documents");

        assert!(!edits.is_empty(), "the queue body did change");
        let body_start = SCOPED_BASE.find("- do [#a]\n").unwrap();
        let body_end = SCOPED_BASE.find("<!-- /agent:queue -->").unwrap();
        let first = char_offset_of(SCOPED_BASE, body_start) as u32;
        let last = char_offset_of(SCOPED_BASE, body_end) as u32;
        for (offset, delete_len, _) in &edits {
            assert!(
                *offset >= first && offset + delete_len <= last,
                "edit ({offset}, {delete_len}) escaped the queue body [{first}, {last})"
            );
        }
    }

    #[test]
    fn an_empty_scope_refuses_every_change() {
        let target = SCOPED_BASE.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        let mut hub = RelayHub::from_text(1, SCOPED_BASE);
        let scope = agent_doc_element::ComponentWriteScope::default();

        let err = hub
            .apply_canonical_replace_scoped(SCOPED_BASE, &target, Some(&scope))
            .expect_err("a write that declared no component may change none");

        assert!(
            err.to_string()
                .contains("outside its declared component scope"),
            "unexpected refusal: {err}"
        );
        assert_eq!(hub.canonical_text(), SCOPED_BASE);
    }

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
    fn member_insertions_lost_by_flags_only_text_the_member_just_added() {
        // Direct cover for the `#queuelineclobber` safety post-condition. A line
        // the member added since their own pre-edit buffer has been observed by
        // nobody else, so nothing can legitimately have deleted it — if the
        // reconciled text lacks it, the reconcile is dropping operator bytes.
        let before = "- head\n- shared\n";
        let after = "- head\n- shared\n- do [#operatortyped]\n";

        assert_eq!(
            member_insertions_lost_by(before, after, "- head\n- shared\n"),
            vec!["- do [#operatortyped]".to_string()],
            "a dropped member insertion must be reported"
        );
        assert!(
            member_insertions_lost_by(before, after, after).is_empty(),
            "a preserved insertion is not a loss"
        );
        assert!(
            member_insertions_lost_by(before, after, "  - do [#operatortyped]  \n").is_empty(),
            "re-indentation inside the isolation merge is not a loss"
        );
        assert!(
            member_insertions_lost_by(before, before, "").is_empty(),
            "a member that inserted nothing can lose nothing"
        );
        // Multiset, so one surviving copy cannot mask a duplicated insertion: two
        // copies were inserted, only one survived.
        assert_eq!(
            member_insertions_lost_by("- x\n", "- x\n- x\n- x\n", "- x\n"),
            vec!["- x".to_string()],
        );
        assert!(
            member_insertions_lost_by("- x\n", "- x\n- x\n- x\n", "- x\n- x\n- x\n").is_empty(),
            "both inserted copies surviving is not a loss"
        );
        // A line the CANONICAL already held independently only makes the check
        // more permissive — it must never be reported as a member loss.
        assert!(
            member_insertions_lost_by("", "- shared\n", "- shared\n- shared\n").is_empty(),
        );
        // `#reconcilesyntheticbase`: two holes the net used to fall through.
        // Comparing survivors against the inserted DELTA rather than the
        // member's post-edit count read `1 < 1` for a duplicate of a line the
        // document already held, so duplicating a queue line and watching the
        // copy vanish was not a "loss".
        assert_eq!(
            member_insertions_lost_by("- x\n", "- x\n- x\n", "- x\n"),
            vec!["- x".to_string()],
            "a dropped duplicate of an existing line is still a dropped insertion"
        );
        // And a blank line the member typed is operator-authored text; skipping
        // blank lines entirely meant a whitespace-only insertion could never be
        // reported.
        assert_eq!(
            member_insertions_lost_by("a\n", "a\n\n", "a\n"),
            vec![String::new()],
            "a dropped blank line the member typed is a loss"
        );
        assert!(
            member_insertions_lost_by("a\n", "a\n\n", "a\n\n").is_empty(),
            "a preserved blank line is not a loss"
        );
    }

    #[test]
    fn operator_queue_insertion_converges_with_a_concurrent_agent_queue_rewrite() {
        // Characterization test for the shape reported on 2026-09-11 (queue items
        // and exchange typing vanishing while the operator typed): the operator
        // appends a queue line on a one-generation-stale projection while the agent
        // concurrently REPLACES a different line in that same component, which is
        // what puts the merge on the guarded, deletion-bearing path.
        //
        // This documents that the component-isolation merge converges CORRECTLY
        // here. Seven such shapes were probed against `merge_by_component` —
        // append-vs-replace, in-place-edit-vs-replace, exchange-typing-vs-append,
        // append-vs-delete, in-place-edit-vs-body-rewrite, and both
        // canonical-only-line-vs-stale-editor variants — and every one preserved
        // the operator text. So this test is NOT a proof of the
        // `member_insertions_lost_by` guard (that guard has no reproduction yet;
        // it is a fail-safe). It is the record that the isolation merge was ruled
        // out, so the next investigation starts at the rebootstrap / canonical
        // projection delivered to the editor instead of re-walking this merge.
        let base = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n",
            "- Fix docs.md turn not closing properly.\n",
            "<!-- /agent:queue -->\n",
        );
        let agent_write = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n\n### Re: prompt — opus-5\n\nComplete response.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n",
            "- do [#fixdocsturn]\n",
            "<!-- /agent:queue -->\n",
        );

        let mut hub = RelayHub::from_text(1, base);
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        let editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        let editor_frontier = editor.state_vector();

        hub.apply_canonical_replace(base, agent_write).unwrap();

        let operator_line = "- do [#retainedresumewakeflake]\n";
        let insert_at = base.find("<!-- /agent:queue -->").unwrap();
        editor.apply_local_edit(base[..insert_at].chars().count() as u32, 0, operator_line);
        let update = editor.diff(&editor_frontier).unwrap();

        hub.relay_update(2, &update).unwrap();

        let canonical = hub.canonical_text();
        assert!(
            canonical.contains("- do [#retainedresumewakeflake]"),
            "the operator's just-typed queue line must survive the reconcile; \
             canonical was:\n{canonical}"
        );
        assert!(
            canonical.contains("Complete response."),
            "and the concurrent agent response must survive too; canonical was:\n{canonical}"
        );
    }

    #[test]
    fn a_two_region_write_leaves_the_component_between_them_untouched() {
        // Direct cover for the `#exchangetypingrevert` mechanism, independent of
        // any CRDT: the spans a two-region CP write produces must not span the
        // component that sits between those regions. A single prefix/suffix peel
        // reported ONE span of 63 characters covering all of `exchange` and both
        // of its markers; those are the characters that get tombstoned, and an
        // operator insertion among them is what lands cross-component.
        let current = concat!(
            "<!-- agent:status -->\n",
            "old status line\n",
            "<!-- /agent:status -->\n",
            "<!-- agent:exchange -->\n",
            "Paragraph one.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
        );
        let content = current
            .replace("old status line", "new status line")
            .replace("- do [#a]", "- ~~do [#a]~~");

        let edits = minimal_char_span_edits(current, &content).unwrap();
        assert_eq!(edits.len(), 2, "one span per changed region, got {edits:?}");
        for (offset, delete_len, _) in &edits {
            let span = *offset as usize..(*offset + *delete_len) as usize;
            let deleted: String = current.chars().take(span.end).skip(span.start).collect();
            assert!(
                !deleted.contains("agent:exchange") && !deleted.contains("Paragraph one."),
                "an untouched component must never be inside a tombstoned span; \
                 span {span:?} deleted {deleted:?}"
            );
        }
        // Descending offset order is the caller's contract: each span addresses
        // the pre-edit text, so a lower offset must never be applied first.
        assert!(
            edits.windows(2).all(|pair| pair[0].0 > pair[1].0),
            "spans must be ordered by descending offset, got {edits:?}"
        );
    }

    #[test]
    fn a_whole_document_cp_write_stays_linear_on_a_realistic_session_document() {
        // The live sighting was a 60KB / ~800-line document. Myers is O(N*D), so
        // the head/tail peel is what keeps a whole-document image cheap; without
        // it a full rewrite would hand Myers the entire file.
        let mut base = String::new();
        for component in ["status", "exchange", "backlog", "queue"] {
            base.push_str(&format!("<!-- agent:{component} -->\n"));
            for line in 0..200 {
                base.push_str(&format!("{component} line {line} with some realistic prose\n"));
            }
            base.push_str(&format!("<!-- /agent:{component} -->\n"));
        }
        let two_region = base
            .replace("status line 0 ", "status line 0 REWRITTEN ")
            .replace("queue line 199 ", "queue line 199 STRUCK ");

        let started = std::time::Instant::now();
        let edits = minimal_char_span_edits(&base, &two_region).unwrap();
        let elapsed = started.elapsed();

        assert_eq!(edits.len(), 2, "got {edits:?}");
        let touched: usize = edits.iter().map(|(_, delete_len, _)| *delete_len as usize).sum();
        assert!(
            touched < 60,
            "a two-word change in a {}-char document must touch a handful of characters, not {touched}",
            base.chars().count()
        );
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "whole-document span computation took {elapsed:?}"
        );
    }

    #[test]
    fn split_spans_reproduce_the_target_text_exactly() {
        // Splitting the edit must stay a faithful replacement, including the
        // cases line-granularity makes easy to get wrong: no trailing newline,
        // pure insert, pure delete, and multi-byte characters ahead of the span.
        for (current, content) in [
            ("a\nb\nc\n", "a\nB\nc\n"),
            ("a\nb\nc", "a\nb\nc\nd"),
            ("a\nb\nc\n", "a\nc\n"),
            ("a\nb\n", "a\nb\nc\nd\n"),
            ("", "fresh\n"),
            ("gone\n", ""),
            ("— em\nkeep\n— dash\n", "— EM\nkeep\n— DASH\n"),
            ("one line no newline", "one line no newlines"),
        ] {
            let mut chars: Vec<char> = current.chars().collect();
            for (offset, delete_len, insert) in minimal_char_span_edits(current, content).unwrap() {
                let at = offset as usize;
                chars.splice(at..at + delete_len as usize, insert.chars());
            }
            assert_eq!(
                chars.into_iter().collect::<String>(),
                content,
                "replaying the spans of {current:?} -> {content:?} must reproduce the target"
            );
        }
    }

    #[test]
    fn a_two_region_cp_write_must_not_tombstone_the_untouched_component_between_them() {
        // `#exchangetypingrevert`. `queue_maintenance` rewrites the whole
        // document: it strikes a queue line AND rewrites status, two disjoint
        // regions with `exchange` sitting between them. A single-span edit
        // covers everything from the first differing char to the last — so every character of the
        // untouched exchange, and both of its component markers, is tombstoned
        // and re-inserted. An operator insertion made concurrently inside that
        // span is then anchored among tombstones, and the raw union materializes
        // it on the far side of a component marker.
        let base = concat!(
            "<!-- agent:status -->\n",
            "old status line\n",
            "<!-- /agent:status -->\n",
            "<!-- agent:exchange -->\n",
            "Paragraph one.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n",
            "- do [#a]\n",
            "<!-- /agent:queue -->\n",
        );
        let cp_write = concat!(
            "<!-- agent:status -->\n",
            "new status line\n",
            "<!-- /agent:status -->\n",
            "<!-- agent:exchange -->\n",
            "Paragraph one.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n",
            "- ~~do [#a]~~\n",
            "<!-- /agent:queue -->\n",
        );

        let mut hub = RelayHub::from_text(1, base);
        hub.register(2).unwrap();
        let editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        let editor_frontier = editor.state_vector();

        // The operator types inside `exchange` from the pre-write projection, at
        // the same moment the CP publishes its two-region write.
        let caret = base.find("Paragraph one.").unwrap() + "Paragraph one.".len();
        editor.apply_local_edit(base[..caret].chars().count() as u32, 0, " typed");
        let update = editor.diff(&editor_frontier).unwrap();

        hub.apply_canonical_replace(base, cp_write).unwrap();
        let packet = hub.relay_update(2, &update).unwrap();

        let canonical = hub.canonical_text();
        assert!(
            canonical.contains("Paragraph one. typed"),
            "the operator's just-typed exchange text must survive a CP write that \
             did not touch exchange at all; canonical was:\n{canonical}"
        );
        assert!(
            !packet.component_isolation_reconciled,
            "a CP write that left exchange byte-identical must not force a \
             cross-component isolation reconcile; canonical was:\n{canonical}"
        );
    }

    #[test]
    fn notes_delete_cannot_splice_a_concurrent_exchange_response() {
        let base = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:notes -->\n",
            "scratch\n",
            "<!-- /agent:notes -->\n",
            "<!-- agent:queue -->\n",
            "- do [#old]\n",
            "<!-- /agent:queue -->\n",
        );
        let agent_write = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n\n### Re: prompt — codex\n\nComplete response.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:notes -->\n",
            "scratch\n",
            "<!-- /agent:notes -->\n",
            "<!-- agent:queue -->\n",
            "- do [#old]\n",
            "- do [#next]\n",
            "<!-- /agent:queue -->\n",
        );
        let expected = concat!(
            "<!-- agent:exchange -->\n",
            "Prompt.\n\n### Re: prompt — codex\n\nComplete response.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:notes -->\n",
            "<!-- /agent:notes -->\n",
            "<!-- agent:queue -->\n",
            "- do [#old]\n",
            "- do [#next]\n",
            "<!-- /agent:queue -->\n",
        );

        let mut hub = RelayHub::from_text(1, base);
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        let editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        let editor_frontier = editor.state_vector();

        // The CP write spans exchange through queue, while the editor still sees
        // the prior projection and deletes only notes.
        hub.apply_canonical_replace(base, agent_write).unwrap();
        let scratch = base.find("scratch\n").unwrap();
        editor.apply_local_edit(
            base[..scratch].chars().count() as u32,
            "scratch\n".chars().count() as u32,
            "",
        );
        let update = editor.diff(&editor_frontier).unwrap();

        let packet = hub.relay_update(2, &update).unwrap();

        let canonical = hub.canonical_text();
        assert_eq!(canonical, expected);
        let notes = canonical
            .split("<!-- agent:notes -->")
            .nth(1)
            .and_then(|body| body.split("<!-- /agent:notes -->").next())
            .unwrap();
        assert!(!notes.contains("Complete response."));
        // `#exchangetypingrevert` strengthened this guarantee rather than
        // relaxing it. The CP write above changes `exchange` and `queue` but
        // leaves `notes` byte-identical, so `minimal_char_span_edits` no longer
        // tombstones `notes` on its way from one region to the other — the
        // concurrent delete merges natively and there is nothing cross-component
        // left to splice. The reconcile is a repair, and it repairs by
        // rebootstrapping the canonical AND every member replica, which is what
        // an operator sees as their own text reverting. Never needing it is the
        // stronger outcome, so this asserts it did not run.
        assert!(
            !packet.component_isolation_reconciled,
            "the splice must be prevented at the write, not repaired after it"
        );
        assert!(
            hub.pending_rebootstrap_members().is_empty(),
            "no member may be rebootstrapped when nothing was spliced"
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

    /// `#silentreplicabarrier`: a replica that says NOTHING must also stop
    /// holding the delivery barrier.
    ///
    /// Observed 2026-09-12 on `tasks/agent-doc/agent-doc-bugs.md`. A JetBrains
    /// replica restart churned register/deregister five times; the surviving
    /// client `2121428668057853` registered at 04:06:44 with a queued canonical
    /// projection receipt (`ensure_canonical_projection_receipt`, whose whole
    /// purpose is to hold the barrier across a replica replacement) and then sent
    /// nothing at all — no pull, no ACK, no projection — for the next five
    /// minutes. `#pullnoackdeadlock`'s budget could not fire, because
    /// `redeliveries_without_ack` only advances inside `pending_updates`, so it
    /// sat at 0 while `delivery_converged` stayed false and every preflight
    /// refused admission with `Lazily current authority remained
    /// delivery_pending`. The barrier needs an observable that advances when the
    /// member is silent, which is exactly an expired convergence wait.
    #[test]
    fn a_silent_replica_stops_holding_the_barrier() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        // The exact shape that wedged: a queued canonical projection receipt on a
        // replacement replica identity that then never speaks again.
        assert!(hub.ensure_canonical_projection_receipt(3).unwrap());
        assert!(
            !hub.delivery_converged(),
            "precondition: the queued receipt blocks convergence"
        );

        for _ in 0..MAX_BARRIER_WAITS_WITHOUT_PROGRESS {
            assert!(
                hub.charge_barrier_wait_without_progress().is_empty(),
                "within the budget the barrier must still hold — an editor that is \
                 merely slow to schedule is not a broken one"
            );
            assert!(!hub.delivery_converged());
        }

        assert_eq!(
            hub.charge_barrier_wait_without_progress(),
            vec![3],
            "past the budget the release must name the replica it released"
        );
        assert!(
            hub.delivery_converged(),
            "a replica that never answers must stop wedging everyone else"
        );
        assert_eq!(hub.nonconverging_replicas(), vec![3]);

        // The receipt is NOT discarded — a recovered editor still receives it.
        assert_eq!(hub.pending_updates(3).unwrap().len(), 1);
    }

    /// `#silentreplicabarrier`: the silent-replica budget must not be reachable
    /// by a replica that is actually servicing delivery.
    ///
    /// A pull is not ACK progress, so it deliberately does NOT clear
    /// `#pullnoackdeadlock`'s streak. It does prove the member is still there,
    /// which is the whole distinction this budget rests on — without the reset, a
    /// healthy-but-slow editor would be released after a dozen waits instead of
    /// the 50 redeliveries `#pullnoackdeadlock` sized for it.
    #[test]
    fn a_pulling_replica_never_accrues_the_silent_streak() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        assert!(hub.ensure_canonical_projection_receipt(3).unwrap());

        for _ in 0..(MAX_BARRIER_WAITS_WITHOUT_PROGRESS * 4) {
            assert!(hub.charge_barrier_wait_without_progress().is_empty());
            // One pull between waits is all it takes to prove liveness.
            assert_eq!(hub.pending_updates(3).unwrap().len(), 1);
        }

        assert!(
            !hub.delivery_converged(),
            "a replica that keeps pulling stays inside the redelivery budget, \
             which is the bound sized for it"
        );
    }

    /// `#silentreplicabarrier`: a projection ACK clears the silent streak, so a
    /// replica that recovers is a first-class member again rather than one wait
    /// away from being dropped from the barrier forever.
    #[test]
    fn acking_clears_the_silent_streak() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();
        assert!(hub.ensure_canonical_projection_receipt(3).unwrap());

        for _ in 0..MAX_BARRIER_WAITS_WITHOUT_PROGRESS {
            assert!(hub.charge_barrier_wait_without_progress().is_empty());
        }

        let canonical_hash = content_hash(&hub.canonical_text());
        assert!(hub.observe_delivery_projection(3, &canonical_hash).unwrap());
        assert!(hub.delivery_converged(), "the receipt was projected");

        // A fresh obligation gets the full budget again.
        assert!(hub.ensure_canonical_projection_receipt(3).unwrap());
        assert!(!hub.delivery_converged());
        for _ in 0..MAX_BARRIER_WAITS_WITHOUT_PROGRESS {
            assert!(hub.charge_barrier_wait_without_progress().is_empty());
        }
        assert_eq!(hub.charge_barrier_wait_without_progress(), vec![3]);
    }

    /// `#pullnoackdeadlock`: a replica that never ACKs must not re-earn the
    /// budget on every new write.
    ///
    /// The first cut cleared the streak on ANY enqueue, reasoning that "a NEW
    /// head is not a redelivery of the old one". That is true for a replica that
    /// was caught up, and wrong for one that is already behind: each new write
    /// re-armed the full budget, so the barrier came back for ~25s per write
    /// instead of staying released. Observed immediately after 0.35.217 shipped
    /// — the same client re-armed at `redeliveries=0` on generation 7 while
    /// `last_ack_generation` had been stuck at 5.
    #[test]
    fn a_new_write_does_not_re_arm_the_budget_for_a_replica_still_behind() {
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap();
        hub.register(3).unwrap();

        let editor2 = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        editor2.apply_local_edit(0, 0, "first");
        let update = editor2.diff(&ReplicaState::new(99).state_vector()).unwrap();
        hub.relay_update(2, &update).unwrap();

        for _ in 0..(MAX_REDELIVERIES_WITHOUT_ACK + 1) {
            hub.pending_updates(3).unwrap();
        }
        assert!(hub.delivery_converged(), "precondition: the budget tripped");

        // A second write arrives while replica 3 is STILL behind (no ACK).
        editor2.apply_local_edit(0, 0, "second");
        let update = editor2.diff(&hub.canonical_state_vector()).unwrap();
        hub.relay_update(2, &update).unwrap();

        assert!(
            hub.delivery_converged(),
            "a replica that has never ACKed must not re-earn the budget on a new write"
        );
        assert_eq!(hub.nonconverging_replicas(), vec![3]);
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
        assert!(
            hub.nonconverging_replicas().is_empty(),
            "the ACK rehabilitates it"
        );

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
    fn retained_canonical_projection_preserves_the_committed_eviction_frontier() {
        let hub = RelayHub::from_text(1, "committed body");
        assert!(hub.is_safe_to_evict());

        let retained = hub.retained_canonical_projection();
        assert_eq!(retained.current_text, "committed body");
        assert!(retained.last_committed_state_vector.is_some());
        let recovered = RelayHub::from_retained_canonical_projection(2, &retained).unwrap();
        assert_eq!(recovered.canonical_text(), "committed body");
        assert!(recovered.is_safe_to_evict());

        recovered.canonical.apply_local_edit(0, 0, "new ");
        assert!(
            !recovered.is_safe_to_evict(),
            "a changed CRDT frontier must fence eviction without a whole-text comparison"
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
