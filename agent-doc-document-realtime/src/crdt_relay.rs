//! Multiple-editor relay hub + ephemeral awareness (`#crdtauth4`, plan phase 5).
//!
//! A **star-topology relay hub** built on top of the state-vector sync primitive
//! ([`agent_doc_merge::crdt_sync`]) and gated by the CRDT-authority state machine
//! ([`crate::crdt_authority`]). It is the fan-out / registry
//! layer the plan calls for (`tasks/agent-doc/plan-crdt-authority-model.md`,
//! "Multiple editors"):
//!
//! - The **project controller/CPC hosts the canonical replica**; editor replicas
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
//!   deregister and never reaches `.yrs` / git.
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
//! the live authority while a session is up; the `.yrs` projection is a
//! write-through **recovery projection only**. See [`RelayHub::projection_bytes`],
//! [`RelayHub::recover_from_projection`], [`RelayHub::reconcile_disk_projection`],
//! and [`DISK_IS_RECOVERY_PROJECTION_ONLY`].

use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use agent_doc_merge::crdt_sync::{ReplicaState, commit_barrier_ready, flush_to_commit_barrier};

use crate::crdt_authority::CrdtAuthority;

/// **Disk-demotion contract (plan phase 6).** The persisted
/// `.agent-doc/crdt/<hash>.yrs` is a write-through **durable recovery projection
/// only** — never the coordination medium and never the source of truth while a
/// session is live. The in-memory canonical replica is authoritative; disk is
/// recovered-from on restart (losing at most one flush). This constant is the
/// single in-code statement of that contract, asserted by tests and consulted by
/// callers that must not treat a persisted projection as authority.
pub const DISK_IS_RECOVERY_PROJECTION_ONLY: bool = true;

/// One registered editor replica's hub-side mirror.
struct Member {
    /// The supervisor's mirror of this editor's replica (synced via deltas).
    replica: ReplicaState,
    /// Whether the editor is currently connected. A disconnected member is
    /// skipped by broadcasts and the commit barrier (no deadlock on a slow /
    /// offline editor) and catches up on [`RelayHub::reconnect`].
    live: bool,
    generation: u64,
    last_ack_generation: u64,
    pending: VecDeque<PendingReplicaUpdate>,
}

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

/// One supervisor-to-editor delivery awaiting an explicit editor ACK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReplicaUpdate {
    pub patch_id: String,
    pub origin: u64,
    pub target: u64,
    pub generation: u64,
    pub update: Vec<u8>,
}

/// Delivery/ACK state for one registered editor replica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaDeliverySnapshot {
    pub client_id: u64,
    pub live: bool,
    pub pending_updates: usize,
    pub current_generation: u64,
    pub last_ack_generation: u64,
}

/// Outcome of routing an out-of-band disk change into the hub
/// ([`RelayHub::apply_disk_change`]). This is the CPC-replica side of the
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

/// Star-topology relay hub: one canonical replica + N registered editor replicas.
pub struct RelayHub {
    /// The CPC-owned canonical replica (the hub / git-checkpoint authority).
    canonical: ReplicaState,
    canonical_id: u64,
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
}

impl RelayHub {
    /// Create a hub whose canonical replica uses `canonical_id` as its yrs client
    /// id. `canonical_id` is reserved — no member may register with it.
    pub fn new(canonical_id: u64) -> Self {
        Self {
            canonical: ReplicaState::new(canonical_id),
            canonical_id,
            members: HashMap::new(),
            awareness: AwarenessChannel::new(),
            last_committed_text: None,
            pending_rebootstrap: HashSet::new(),
        }
    }

    /// Create a hub whose canonical replica is already seeded from the current
    /// editor-visible document text. File-backed live sessions use this on first
    /// allocation so the first editor delta is never applied to an empty replica.
    pub fn from_text(canonical_id: u64, text: &str) -> Self {
        let mut hub = Self::new(canonical_id);
        hub.canonical = ReplicaState::from_text(canonical_id, text);
        hub.last_committed_text = Some(text.to_string());
        hub
    }

    /// Recover a hub from a durable disk **recovery projection** (plan phase 6):
    /// rebuild the in-memory canonical replica from the last `.yrs` snapshot on
    /// restart. At most one flush is lost; live editors re-sync their newer ops on
    /// reconnect. The projection is a recovery input, never authority.
    pub fn recover_from_projection(canonical_id: u64, projection: &[u8]) -> Result<Self> {
        let canonical = ReplicaState::from_encoded(canonical_id, projection)?;
        // Seed the committed baseline from the recovered text so the very first
        // commit barrier after a restart can already detect an out-of-band disk
        // correction / compaction (`#staleinmem`) instead of waiting for a finalize
        // to record one.
        let last_committed_text = Some(canonical.text());
        Ok(Self {
            canonical,
            canonical_id,
            members: HashMap::new(),
            awareness: AwarenessChannel::new(),
            last_committed_text,
            pending_rebootstrap: HashSet::new(),
        })
    }

    /// The canonical (authoritative) converged text.
    pub fn canonical_text(&self) -> String {
        self.canonical.text()
    }

    /// A registered member's current text (for inspection / tests).
    pub fn member_text(&self, client_id: u64) -> Option<String> {
        self.members.get(&client_id).map(|m| m.replica.text())
    }

    /// The number of currently-live (connected) members.
    pub fn live_count(&self) -> usize {
        self.members.values().filter(|m| m.live).count()
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
                live: true,
                generation: 0,
                last_ack_generation: 0,
                pending: VecDeque::new(),
            },
        );
        Ok(())
    }

    /// Deregister an editor replica: drop its hub-side mirror AND expire its
    /// ephemeral awareness/presence entry. The awareness channel never outlives a
    /// connection (it is not persisted and not committed).
    pub fn deregister(&mut self, client_id: u64) -> bool {
        self.awareness.remove(client_id);
        self.members.remove(&client_id).is_some()
    }

    /// Mark a member offline (disconnected) without losing its replica state. A
    /// disconnected member is skipped by broadcasts and the commit barrier and
    /// catches up via [`Self::reconnect`]. Its presence entry is expired (a
    /// disconnected cursor must not linger).
    pub fn disconnect(&mut self, client_id: u64) -> bool {
        self.awareness.remove(client_id);
        match self.members.get_mut(&client_id) {
            Some(m) => {
                m.live = false;
                m.pending.clear();
                true
            }
            None => false,
        }
    }

    /// Reconnect a member: a **bidirectional state-vector catch-up** that proves
    /// no data loss. The member's offline edits flow into the canonical replica
    /// and the updates it missed while offline flow back into it. After this the
    /// member and canonical have converged.
    pub fn reconnect(&mut self, client_id: u64) -> Result<()> {
        let member = self
            .members
            .get_mut(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        member.live = true;
        member.pending.clear();
        // Pull the member's offline ops into canonical, then push back everything
        // the member missed. Both directions are state-vector deltas.
        let to_canonical = member.replica.diff(&self.canonical.state_vector())?;
        let to_member = self.canonical.diff(&member.replica.state_vector())?;
        self.canonical.apply_update(&to_canonical)?;
        member.replica.apply_update(&to_member)?;
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
        let member = self
            .members
            .get(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        // Canonical SV before integrating, so the packet carries exactly the new op(s).
        let before = self.canonical.state_vector();
        let into_canonical = member.replica.diff(&self.canonical.state_vector())?;
        self.canonical.apply_update(&into_canonical)?;
        let update = self.canonical.diff(&before)?;
        let targets: Vec<u64> = self
            .members
            .iter()
            .filter(|(id, m)| **id != client_id && m.live)
            .map(|(id, _)| *id)
            .collect();
        let packet = BroadcastPacket {
            origin: client_id,
            update,
            targets,
        };
        self.enqueue_delivery(&packet);
        Ok(packet)
    }

    /// Apply a **raw encoded yrs update** from member `client_id` to that
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
    /// edit. yrs guarantees the apply is idempotent + causal-buffered, so a
    /// duplicate or out-of-order update converges rather than corrupting.
    pub fn relay_update_capture(
        &mut self,
        client_id: u64,
        update: &[u8],
    ) -> Result<BroadcastPacket> {
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
        let delta = self.canonical.diff(&before)?;
        let targets: Vec<u64> = self
            .members
            .iter()
            .filter(|(id, m)| **id != client_id && m.live)
            .map(|(id, _)| *id)
            .collect();
        let packet = BroadcastPacket {
            origin: client_id,
            update: delta,
            targets,
        };
        self.enqueue_delivery(&packet);
        Ok(packet)
    }

    /// Apply a raw encoded yrs update from `client_id` and **immediately
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

    /// The canonical replica's encoded state — the bootstrap snapshot a freshly
    /// registering editor needs on first contact (all later traffic is deltas).
    pub fn canonical_encoded_state(&self) -> Vec<u8> {
        self.canonical.encode_state()
    }

    /// Deliver an update to one target replica (idempotent + causal-buffered by
    /// yrs, so out-of-order delivery self-heals once causal deps arrive). A no-op
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

    /// Apply a CPC-authored whole-document replacement to the canonical replica
    /// and queue the resulting CRDT delta for every live editor replica.
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
                "canonical text changed before CPC relay write: expected_len={} current_len={}",
                expected_current.len(),
                current.len()
            ));
        }
        let before = self.canonical.state_vector();
        if current != content {
            let delete_len: u32 = current
                .len()
                .try_into()
                .map_err(|_| anyhow!("canonical text is too large for a single CRDT replace"))?;
            self.canonical.apply_local_edit(0, delete_len, content);
        }
        let update = self.canonical.diff(&before)?;
        let mut targets: Vec<u64> = self
            .members
            .iter()
            .filter(|(_, m)| m.live)
            .map(|(id, _)| *id)
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
        for target in &packet.targets {
            let Some(member) = self.members.get_mut(target) else {
                continue;
            };
            member.generation += 1;
            let generation = member.generation;
            member.pending.push_back(PendingReplicaUpdate {
                patch_id: format!("crdt:{}:{}:{}", packet.origin, target, generation),
                origin: packet.origin,
                target: *target,
                generation,
                update: packet.update.clone(),
            });
        }
    }

    /// Pull pending supervisor-to-editor updates for `client_id`. Updates remain in
    /// the queue until [`Self::ack_delivery`] confirms the editor applied them.
    pub fn pending_updates(&self, client_id: u64) -> Result<Vec<PendingReplicaUpdate>> {
        let member = self
            .members
            .get(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        Ok(member.pending.iter().cloned().collect())
    }

    /// ACK one delivered update. Returns `Ok(false)` when the ACK is stale or
    /// unknown; this is non-fatal because editors may retry idempotent deliveries.
    pub fn ack_delivery(
        &mut self,
        client_id: u64,
        patch_id: &str,
        generation: u64,
    ) -> Result<bool> {
        let member = self
            .members
            .get_mut(&client_id)
            .ok_or_else(|| anyhow!("replica {client_id} is not registered"))?;
        let Some(pos) = member
            .pending
            .iter()
            .position(|update| update.patch_id == patch_id && update.generation == generation)
        else {
            return Ok(false);
        };
        member.pending.remove(pos);
        member.last_ack_generation = member.last_ack_generation.max(generation);
        Ok(true)
    }

    /// True when every currently-live editor has ACKed all queued fan-out updates.
    /// Disconnected editors are excluded from this live convergence cut.
    pub fn delivery_converged(&self) -> bool {
        self.members
            .values()
            .filter(|member| member.live)
            .all(|member| member.pending.is_empty())
    }

    pub fn delivery_snapshot(&self) -> Vec<ReplicaDeliverySnapshot> {
        let mut snapshot = self
            .members
            .iter()
            .map(|(client_id, member)| ReplicaDeliverySnapshot {
                client_id: *client_id,
                live: member.live,
                pending_updates: member.pending.len(),
                current_generation: member.generation,
                last_ack_generation: member.last_ack_generation,
            })
            .collect::<Vec<_>>();
        snapshot.sort_by_key(|entry| entry.client_id);
        snapshot
    }

    /// The currently-live member replicas (the consistent-cut set for the commit
    /// barrier — offline members are excluded so a slow editor cannot deadlock).
    fn live_editors(&self) -> Vec<&ReplicaState> {
        self.members
            .values()
            .filter(|m| m.live)
            .map(|m| &m.replica)
            .collect()
    }

    /// Drive the **commit barrier**: flush every CURRENTLY-LIVE editor's ops into
    /// the canonical replica and confirm a consistent cut. Offline / disconnected
    /// members are excluded — the barrier is a checkpoint of the live replicas,
    /// never a global lock that blocks on a slow editor. After `Ok(true)` a
    /// snapshot of the canonical replica ([`Self::projection_bytes`]) is safe to
    /// write to git.
    pub fn commit_barrier(&self) -> Result<bool> {
        flush_to_commit_barrier(&self.canonical, &self.live_editors())
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
    /// replica — what the supervisor flushes to `.agent-doc/crdt/<hash>.yrs`.
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
        Ok(self.canonical.text() != before)
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
        let fresh = ReplicaState::new(self.canonical_id);
        if !on_disk.is_empty() {
            fresh.apply_local_edit(0, 0, on_disk);
        }
        let bootstrap = fresh.encode_state();
        self.canonical = fresh;
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

    /// Route a settled out-of-band disk change into the hub — the CPC-replica
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
                .iter()
                .filter(|(_, m)| m.live)
                .map(|(id, _)| *id)
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
/// this is part of the document CRDT — it is never persisted to `.yrs` and never
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
    presence: HashMap<u64, AwarenessState>,
}

impl AwarenessChannel {
    /// A fresh empty channel.
    pub fn new() -> Self {
        Self {
            presence: HashMap::new(),
        }
    }

    /// Set the local awareness for `client_id` (overwrites any prior state).
    pub fn set_local(&mut self, client_id: u64, state: AwarenessState) {
        self.presence.insert(client_id, state);
    }

    /// The current presence for `client_id`, if any.
    pub fn get(&self, client_id: u64) -> Option<&AwarenessState> {
        self.presence.get(&client_id)
    }

    /// A deterministic (client-id-ordered) snapshot of all presence — the payload
    /// a hub broadcasts to peers.
    pub fn broadcast(&self) -> Vec<(u64, AwarenessState)> {
        let mut out: Vec<(u64, AwarenessState)> = self
            .presence
            .iter()
            .map(|(id, s)| (*id, s.clone()))
            .collect();
        out.sort_by_key(|(id, _)| *id);
        out
    }

    /// Expire / remove a client's presence (called on deregister / disconnect).
    pub fn remove(&mut self, client_id: u64) -> bool {
        self.presence.remove(&client_id).is_some()
    }

    /// The number of clients with live presence.
    pub fn len(&self) -> usize {
        self.presence.len()
    }

    /// Whether any presence is tracked.
    pub fn is_empty(&self) -> bool {
        self.presence.is_empty()
    }
}

/// Deterministically mint a stable, unique-by-construction yrs client-id from a
/// stable string identity (an editor process identity, e.g. `"intellij:<pid>"`).
///
/// The same identity always yields the same id (stable across reconnects); two
/// distinct identities collide only on a hash collision in the 53-bit space.
/// yrs requires client-ids to fit in 53 bits (the Yjs-compatible range), so the
/// hash is masked to the low 53 bits. Callers must still
/// [`RelayHub::validate_unique`] before registering (collision = corruption per
/// the plan, surfaced as a hard error rather than silently shared state).
pub fn mint_client_id(identity: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Domain-separate from accidental raw-u64 reuse.
    "agent-doc-replica\0".hash(&mut hasher);
    identity.hash(&mut hasher);
    identity.len().hash(&mut hasher);
    // yrs client-ids must fit in 53 bits (Yjs-compatible range).
    const CLIENT_ID_MASK: u64 = (1u64 << 53) - 1;
    hasher.finish() & CLIENT_ID_MASK
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_merge::crdt_sync::ReplicaState;

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
    fn cpc_canonical_replace_queues_delta_for_live_editors() {
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
            "CPC-origin editor delivery still requires editor ACK"
        );

        let pending = hub.pending_updates(2).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].origin, 1);
        assert_eq!(pending[0].target, 2);
        assert!(pending[0].patch_id.starts_with("crdt:1:2:1"));
    }

    #[test]
    fn cpc_canonical_replace_rejects_stale_expected_text() {
        let mut hub = RelayHub::from_text(1, "operator text\n");
        hub.register(2).unwrap();

        let err = hub
            .apply_canonical_replace("stale text\n", "agent response\n")
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("canonical text changed before CPC relay write"),
            "stale CPC relay writes must be rejected: {err:#}"
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
        // Flush a durable recovery projection (what hits .yrs).
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

    // ---- apply_disk_change: the file-watch → CPC-replica entry point ----

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
}
