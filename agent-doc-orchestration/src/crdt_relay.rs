//! Multiple-editor relay hub + ephemeral awareness (`#crdtauth4`, plan phase 5).
//!
//! A **star-topology relay hub** built on top of the state-vector sync primitive
//! ([`agent_doc_core::crdt_sync`]) and gated by the CRDT-authority state machine
//! ([`crate::crdt_authority`]). It is the fan-out / registry layer the plan calls
//! for (`tasks/agent-doc/plan-crdt-authority-model.md`, "Multiple editors"):
//!
//! - The **supervisor hosts the canonical replica**; editor replicas
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
//!   [`agent_doc_core::crdt_sync::flush_to_commit_barrier`]) and never blocks on a
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

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use agent_doc_core::crdt_sync::{ReplicaState, commit_barrier_ready, flush_to_commit_barrier};

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

/// Star-topology relay hub: one canonical replica + N registered editor replicas.
pub struct RelayHub {
    /// The supervisor's canonical replica (the hub / git-checkpoint authority).
    canonical: ReplicaState,
    canonical_id: u64,
    members: HashMap<u64, Member>,
    awareness: AwarenessChannel,
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
        }
    }

    /// Recover a hub from a durable disk **recovery projection** (plan phase 6):
    /// rebuild the in-memory canonical replica from the last `.yrs` snapshot on
    /// restart. At most one flush is lost; live editors re-sync their newer ops on
    /// reconnect. The projection is a recovery input, never authority.
    pub fn recover_from_projection(canonical_id: u64, projection: &[u8]) -> Result<Self> {
        let canonical = ReplicaState::from_encoded(canonical_id, projection)?;
        Ok(Self {
            canonical,
            canonical_id,
            members: HashMap::new(),
            awareness: AwarenessChannel::new(),
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
        self.members.insert(client_id, Member { replica, live: true });
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
    pub fn relay_capture(&self, client_id: u64) -> Result<BroadcastPacket> {
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
        Ok(BroadcastPacket {
            origin: client_id,
            update,
            targets,
        })
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
    pub fn relay(&self, client_id: u64) -> Result<BroadcastPacket> {
        let packet = self.relay_capture(client_id)?;
        for target in &packet.targets {
            self.deliver(*target, &packet.update)?;
        }
        Ok(packet)
    }

    /// Apply a local edit and immediately relay + broadcast it (the normal live
    /// path = [`Self::local_edit`] + [`Self::relay`]). Returns the delivered packet.
    pub fn apply_local(
        &self,
        client_id: u64,
        offset: u32,
        delete_len: u32,
        insert: &str,
    ) -> Result<BroadcastPacket> {
        self.local_edit(client_id, offset, delete_len, insert)?;
        self.relay(client_id)
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
        assert!(DISK_IS_RECOVERY_PROJECTION_ONLY);

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
        assert_eq!(recovered.live_count(), 0, "members re-register after restart");
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
}
