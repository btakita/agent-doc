//! Cross-process editor liveness as OR-set / LWW cells (`#lzsync-liveness`,
//! sidecar-retirement Phase 3C — the controller *receiver* core).
//!
//! This is the sole editor-liveness authority: the controller folds liveness
//! frames pushed by the editor plugins into Lazily's proven convergent cells and
//! derives the open-set / per-pid-alive / live-doc aggregate.
//!
//! The semantics are exactly the ones lazily-spec pins and lazily-formal proves
//! (`ReliableSync.crdt_liveness_convergence_under_retry`,
//! `orset_add_wins_over_stale_remove`, the `joinReg_*` semilattice), so this
//! module uses lazily's [`OrSet`] and [`WireLwwRegister`] directly rather than a
//! bespoke re-implementation:
//!
//! - **Open-set membership** — "editor pid P has doc D open" — is an observed-remove
//!   set ([`OrSet`]): a re-open (fresh add tag) wins over a concurrent lagging close
//!   that only observed the earlier tag (add-wins).
//! - **Per-pid `alive`** — the OS process-exit watcher (S4b) writes `alive = false`
//!   at a fresh HLC stamp — is a [`WireLwwRegister`] (highest stamp wins), so a stale
//!   re-assert is dominated.
//! - **Derived "doc D is live"** = any *present* `(D, P)` whose `alive[P]` is true.
//!   One `alive[P] = false` fans out to every doc P held (whole-editor-death cascade),
//!   which falls out of the per-`(doc, pid)` iteration for free.
//!
//! Keys are namespaced by `document_hash` (the per-doc isolation invariant: a stale
//! overlay for doc B cannot flip doc A). The liveness ops ride the CrdtSync plane
//! (spec § `#lzsync-liveness`): [`encode_liveness_frame`] packs a `LivenessOp` batch
//! into one `IpcMessage::CrdtSync` op as inline bytes, so the batch flows through the
//! same reliable-sync `SyncDriver` + `DurableOutbox` as every other frame (the driver
//! hands an applied `CrdtSync` straight to the host, which folds it here).

use anyhow::{Result, anyhow};
use lazily::{CrdtOp, CrdtSync, IpcMessage, IpcValue, NodeId, OrSet, WireLwwRegister, WireStamp};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// OS process id of an editor.
pub type Pid = u64;

/// Metadata for one editor replica. This travels on the same reliable Lazily
/// channel as open/close state; it is not a filesystem projection of the live
/// buffer. Registrations are scoped by document and pid, and only registrations
/// whose pid is currently live/open are exposed as authority.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EditorRegistration {
    pub document_hash: String,
    pub pid: Pid,
    pub path: String,
    pub editor_id: String,
    pub editor_kind: String,
    pub editor_version: String,
    pub capabilities: Vec<String>,
    pub timestamp_ms: u64,
}

/// One liveness event pushed editor-plugin → controller on the reliable-sync plane.
///
/// The open/close pair carries the OR-set observation `tag`s (not stamps — OR-set
/// convergence is tag-set union); `Alive` carries the LWW `WireStamp`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LivenessOp {
    /// Editor `pid` opened / attached `document_hash`; `tag` is a fresh presence id
    /// minted by the plugin (a re-open mints a new tag → add-wins over a stale close).
    Open {
        document_hash: String,
        pid: Pid,
        tag: String,
    },
    /// Editor `pid` closed `document_hash`, observing `observed_tags` (only those
    /// add-tags are shadowed; a concurrent re-open's fresh tag survives).
    Close {
        document_hash: String,
        pid: Pid,
        observed_tags: Vec<String>,
    },
    /// Per-pid liveness write (the OS exit watcher sets `value = false` at a fresh
    /// stamp; the highest stamp wins).
    Alive {
        pid: Pid,
        value: bool,
        stamp: WireStamp,
    },
    /// Per-`(document_hash, pid)` sync-progress write: the editor's current
    /// `edit_epoch` (monotonic count of local edit batches) and `synced_epoch` (the
    /// highest edit batch the editor has confirmed the controller/CRDT received).
    /// `edit_epoch > synced_epoch` ⇒ the editor holds unsynced edits in flight. This
    /// is the plane replacement for the divergence signal the live-buffer sidecar's
    /// `edit_epoch`/`last_synced_epoch` fields carry (`editor_sync_statuses`); highest
    /// LWW `stamp` wins, so a later report supersedes and a re-delivery is a no-op
    /// (#sidecar-retirement sync-in-flight foundation).
    Sync {
        document_hash: String,
        pid: Pid,
        edit_epoch: u64,
        synced_epoch: u64,
        stamp: WireStamp,
    },
    /// Editor identity/generation/capabilities for one document replica. The
    /// value is folded monotonically by `(timestamp_ms, value)` so retry and
    /// reordering converge without a separate live-buffer metadata sidecar.
    Register(EditorRegistration),
}

/// The controller's convergent liveness projection over one or more documents.
///
/// Fold [`LivenessOp`]s in via [`apply`](Self::apply) (idempotent + order-independent —
/// re-delivery is a no-op), then read the derived authority: [`is_open`](Self::is_open),
/// [`pid_alive`](Self::pid_alive), [`open_docs`](Self::open_docs), [`live_docs`](Self::live_docs).
#[derive(Debug, Default)]
pub struct LivenessProjection {
    /// `(document_hash, pid)` → observed-remove membership set.
    open_set: BTreeMap<(String, Pid), OrSet>,
    /// `pid` → LWW `alive` register.
    alive: BTreeMap<Pid, WireLwwRegister<bool>>,
    /// `(document_hash, pid)` → LWW `(edit_epoch, synced_epoch)` register. The
    /// plane's sync-in-flight signal (`edit_epoch > synced_epoch` ⇒ unsynced edits),
    /// replacing the live-buffer sidecar's epoch fields (#sidecar-retirement).
    sync_state: BTreeMap<(String, Pid), WireLwwRegister<(u64, u64)>>,
    /// `(document_hash, pid)` -> newest deterministic editor registration.
    registrations: BTreeMap<(String, Pid), EditorRegistration>,
}

impl LivenessProjection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one liveness op into the projection. Idempotent (OR-set add/remove are
    /// set unions; the LWW `set` only adopts a strictly-higher stamp), so a replay
    /// or re-delivery changes nothing.
    pub fn apply(&mut self, op: &LivenessOp) {
        match op {
            LivenessOp::Open {
                document_hash,
                pid,
                tag,
            } => {
                self.open_set
                    .entry((document_hash.clone(), *pid))
                    .or_default()
                    .add(tag.clone());
            }
            LivenessOp::Close {
                document_hash,
                pid,
                observed_tags,
            } => {
                // A close seen before its open still records the shadow so a late,
                // already-observed add cannot resurrect the entry.
                self.open_set
                    .entry((document_hash.clone(), *pid))
                    .or_default()
                    .remove_observed(observed_tags.iter().cloned());
            }
            LivenessOp::Alive { pid, value, stamp } => match self.alive.get_mut(pid) {
                Some(reg) => reg.set(*stamp, *value),
                None => {
                    self.alive
                        .insert(*pid, WireLwwRegister::new(*stamp, *value));
                }
            },
            LivenessOp::Sync {
                document_hash,
                pid,
                edit_epoch,
                synced_epoch,
                stamp,
            } => {
                let key = (document_hash.clone(), *pid);
                let value = (*edit_epoch, *synced_epoch);
                match self.sync_state.get_mut(&key) {
                    Some(reg) => reg.set(*stamp, value),
                    None => {
                        self.sync_state
                            .insert(key, WireLwwRegister::new(*stamp, value));
                    }
                }
            }
            LivenessOp::Register(registration) => {
                let key = (registration.document_hash.clone(), registration.pid);
                let replace = self.registrations.get(&key).is_none_or(|current| {
                    registration.timestamp_ms > current.timestamp_ms
                        || (registration.timestamp_ms == current.timestamp_ms
                            && registration > current)
                });
                if replace {
                    self.registrations.insert(key, registration.clone());
                }
            }
        }
    }

    /// Fold a whole batch (a decoded liveness frame).
    pub fn apply_batch(&mut self, ops: &[LivenessOp]) {
        for op in ops {
            self.apply(op);
        }
    }

    /// Is `(document_hash, pid)` currently open (some add-tag not shadowed)?
    pub fn is_open(&self, document_hash: &str, pid: Pid) -> bool {
        self.open_set
            .get(&(document_hash.to_string(), pid))
            .is_some_and(OrSet::present)
    }

    /// Is `pid` alive? Absent register ⇒ presumed alive (an open editor is live
    /// until a death signal arrives — the death is the LWW `alive = false`, not a
    /// missing entry).
    pub fn pid_alive(&self, pid: Pid) -> bool {
        self.alive.get(&pid).map(|r| *r.value()).unwrap_or(true)
    }

    /// Distinct pids that currently hold `document_hash` open (regardless of alive).
    pub fn open_pids(&self, document_hash: &str) -> BTreeSet<Pid> {
        self.open_set
            .iter()
            .filter(|((doc, _), set)| doc == document_hash && set.present())
            .map(|((_, pid), _)| *pid)
            .collect()
    }

    /// Every pid with at least one present open-set membership. Controller
    /// hydration uses this to reconcile durable open facts with OS process
    /// liveness after a crash/restart where the editor's exit watcher could not
    /// publish its terminal `Alive(false)` fact.
    pub fn all_open_pids(&self) -> BTreeSet<Pid> {
        self.open_set
            .iter()
            .filter(|(_, set)| set.present())
            .map(|((_, pid), _)| *pid)
            .collect()
    }

    /// Whether this projection has ever received an open/close fact for the
    /// document. Unlike `open_docs().is_empty()`, this distinguishes a durably
    /// known closed document from a never-hydrated cold process.
    pub fn tracks_document(&self, document_hash: &str) -> bool {
        self.open_set
            .keys()
            .any(|(known_document, _)| known_document == document_hash)
    }

    /// Every document with at least one present `(doc, pid)` (open-set ground truth,
    /// independent of alive) — the replacement for the `#lbreap` live-buffer scan.
    pub fn open_docs(&self) -> BTreeSet<String> {
        self.open_set
            .iter()
            .filter(|(_, set)| set.present())
            .map(|((doc, _), _)| doc.clone())
            .collect()
    }

    /// Derived live-doc aggregate: docs with a present `(doc, pid)` whose `pid` is
    /// alive. A dead pid's docs drop unless another live pid also holds them
    /// (whole-editor-death cascade, per-doc for free).
    pub fn live_docs(&self) -> BTreeSet<String> {
        self.open_set
            .iter()
            .filter(|((_, pid), set)| set.present() && self.pid_alive(*pid))
            .map(|((doc, _), _)| doc.clone())
            .collect()
    }

    /// Live/open editor registrations for `document_hash`, ordered
    /// deterministically by pid and registration value.
    pub fn live_registrations(&self, document_hash: &str) -> Vec<EditorRegistration> {
        self.registrations
            .iter()
            .filter(|((doc, pid), _)| {
                doc == document_hash && self.is_open(doc, *pid) && self.pid_alive(*pid)
            })
            .map(|(_, registration)| registration.clone())
            .collect()
    }

    /// Live/open registrations across the whole projection.
    pub fn all_live_registrations(&self) -> Vec<EditorRegistration> {
        self.open_docs()
            .into_iter()
            .flat_map(|document_hash| self.live_registrations(&document_hash))
            .collect()
    }

    /// The plane's sync-in-flight signal: is any **live, open** editor holding
    /// unsynced edits (`edit_epoch > synced_epoch`) for `document_hash`? This is the
    /// derived-authority replacement for `editor_sync_in_flight` / the barrier's
    /// live-buffer scan (#sidecar-retirement): a disk write must not clobber an editor
    /// buffer that is ahead of the last synced epoch. A dead pid's unsynced edits do
    /// not count (its buffer is gone), matching the whole-editor-death cascade.
    pub fn document_in_flight(&self, document_hash: &str) -> bool {
        self.sync_state
            .iter()
            .filter(|((doc, _), _)| doc == document_hash)
            .any(|((doc, pid), reg)| {
                let (edit_epoch, synced_epoch) = *reg.value();
                edit_epoch > synced_epoch && self.is_open(doc, *pid) && self.pid_alive(*pid)
            })
    }

    /// The last reported `(edit_epoch, synced_epoch)` for `(document_hash, pid)`, or
    /// `None` if the editor never reported sync progress for the document.
    pub fn sync_epochs(&self, document_hash: &str, pid: Pid) -> Option<(u64, u64)> {
        self.sync_state
            .get(&(document_hash.to_string(), pid))
            .map(|reg| *reg.value())
    }
}

/// Sentinel `type_tag`-in-`node` marker distinguishing an agent-doc liveness
/// CrdtSync frame from a graph-state CrdtSync frame on a shared channel. agent-doc
/// runs liveness on its own reliable-sync channel, but tagging makes the decode
/// fail-closed rather than mis-fold a foreign frame.
const LIVENESS_NODE: NodeId = NodeId(u64::MAX);

/// Pack a `LivenessOp` batch into one `IpcMessage::CrdtSync` (spec § `#lzsync-liveness`:
/// liveness rides the CrdtSync plane). The batch is JSON bytes carried inline in a
/// single op's converged state, so it flows through the reliable-sync `SyncDriver` /
/// `DurableOutbox` exactly like any other frame; the driver hands the applied
/// `CrdtSync` back to the host, which folds it via [`decode_liveness_frame`].
pub fn encode_liveness_frame(ops: &[LivenessOp]) -> Result<IpcMessage> {
    let bytes = serde_json::to_vec(ops)?;
    let op = CrdtOp {
        node: LIVENESS_NODE,
        key: None,
        // The carriage stamp is unused by the fold (each `Alive` op carries its own
        // LWW stamp); zero is a stable, deterministic placeholder.
        stamp: WireStamp {
            wall_time: 0,
            logical: 0,
            peer: 0,
        },
        state: IpcValue::from(bytes),
    };
    Ok(IpcMessage::CrdtSync(CrdtSync {
        frontier: Vec::new(),
        ops: vec![op],
    }))
}

/// Decode a liveness frame produced by [`encode_liveness_frame`].
///
/// Returns `None` for a non-`CrdtSync` message or a `CrdtSync` that carries no
/// liveness-tagged op (a foreign/graph frame — let the host route it elsewhere);
/// `Some(Err)` for a liveness-tagged op whose inline bytes are malformed (never a
/// silent drop). The `SharedBlob` transport is not used for liveness (small frames),
/// so a `SharedBlob` op on the liveness node is treated as malformed.
pub fn decode_liveness_frame(message: &IpcMessage) -> Option<Result<Vec<LivenessOp>>> {
    let IpcMessage::CrdtSync(sync) = message else {
        return None;
    };
    let mut liveness_ops: Vec<&CrdtOp> = sync
        .ops
        .iter()
        .filter(|op| op.node == LIVENESS_NODE)
        .collect();
    if liveness_ops.is_empty() {
        return None;
    }
    // Keep op order (later frames override earlier via LWW/OR-set anyway).
    liveness_ops.sort_by_key(|op| op.stamp);
    Some(decode_liveness_ops(&liveness_ops))
}

fn decode_liveness_ops(ops: &[&CrdtOp]) -> Result<Vec<LivenessOp>> {
    let mut out = Vec::new();
    for op in ops {
        let IpcValue::Inline(bytes) = &op.state else {
            return Err(anyhow!(
                "liveness frame carried a non-inline (SharedBlob) op state"
            ));
        };
        let batch: Vec<LivenessOp> = serde_json::from_slice(bytes)
            .map_err(|e| anyhow!("liveness frame inline decode failed: {e}"))?;
        out.extend(batch);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(wall: u64, peer: u64) -> WireStamp {
        WireStamp {
            wall_time: wall,
            logical: 0,
            peer,
        }
    }

    #[test]
    fn editor_registration_is_visible_only_while_replica_is_live_and_open() {
        let mut projection = LivenessProjection::new();
        let registration = EditorRegistration {
            document_hash: "docA".into(),
            pid: 100,
            path: "/tmp/doc.md".into(),
            editor_id: "jetbrains-100-test".into(),
            editor_kind: "jetbrains".into(),
            editor_version: "0.2.270".into(),
            capabilities: vec!["operator_text_authority_v1".into()],
            timestamp_ms: 10,
        };
        projection.apply(&LivenessOp::Register(registration.clone()));
        assert!(projection.live_registrations("docA").is_empty());

        projection.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "open-1".into(),
        });
        assert_eq!(projection.live_registrations("docA"), vec![registration]);

        projection.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(20, 1),
        });
        assert!(projection.live_registrations("docA").is_empty());
    }

    #[test]
    fn editor_registration_advances_by_timestamp_before_metadata_order() {
        let mut projection = LivenessProjection::new();
        projection.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "open-1".into(),
        });
        let older = EditorRegistration {
            document_hash: "docA".into(),
            pid: 100,
            path: "/tmp/z-old.md".into(),
            editor_id: "z-old".into(),
            editor_kind: "jetbrains".into(),
            editor_version: "9.9.9".into(),
            capabilities: vec!["z-old".into()],
            timestamp_ms: 10,
        };
        let newer = EditorRegistration {
            path: "/tmp/a-new.md".into(),
            editor_id: "a-new".into(),
            editor_version: "0.2.270".into(),
            capabilities: vec!["a-new".into()],
            timestamp_ms: 20,
            ..older.clone()
        };

        projection.apply(&LivenessOp::Register(older));
        projection.apply(&LivenessOp::Register(newer.clone()));
        assert_eq!(projection.live_registrations("docA"), vec![newer.clone()]);

        projection.apply(&LivenessOp::Register(EditorRegistration {
            timestamp_ms: 5,
            path: "/tmp/zz-stale.md".into(),
            ..newer.clone()
        }));
        assert_eq!(projection.live_registrations("docA"), vec![newer]);
    }

    // Conformance scenario `open_set_add_wins_over_stale_remove`
    // (lazily-spec/conformance/reliable-sync/liveness_orset_lww.json).
    #[test]
    fn open_set_add_wins_over_stale_remove() {
        let mut p = LivenessProjection::new();
        p.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "t1".into(),
        });
        p.apply(&LivenessOp::Close {
            document_hash: "docA".into(),
            pid: 100,
            observed_tags: vec!["t1".into()],
        });
        p.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "t3".into(),
        });
        assert!(
            p.is_open("docA", 100),
            "re-open tag t3 not observed by the close"
        );
    }

    fn open(p: &mut LivenessProjection, doc: &str, pid: Pid) {
        p.apply(&LivenessOp::Open {
            document_hash: doc.into(),
            pid,
            tag: format!("open-{pid}"),
        });
    }

    // #sidecar-retirement sync-in-flight foundation.
    #[test]
    fn sync_in_flight_true_when_edit_ahead_of_synced() {
        let mut p = LivenessProjection::new();
        open(&mut p, "docA", 100);
        p.apply(&LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 3,
            synced_epoch: 1,
            stamp: stamp(10, 1),
        });
        assert!(p.document_in_flight("docA"));
        assert_eq!(p.sync_epochs("docA", 100), Some((3, 1)));
    }

    #[test]
    fn sync_not_in_flight_when_synced_catches_up_lww() {
        let mut p = LivenessProjection::new();
        open(&mut p, "docA", 100);
        p.apply(&LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 3,
            synced_epoch: 1,
            stamp: stamp(10, 1),
        });
        // A later report at a higher stamp: the editor confirmed sync through 3.
        p.apply(&LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 3,
            synced_epoch: 3,
            stamp: stamp(20, 1),
        });
        assert!(!p.document_in_flight("docA"));
    }

    #[test]
    fn sync_lww_highest_stamp_wins_regardless_of_order_and_redelivery_is_noop() {
        let mut p = LivenessProjection::new();
        open(&mut p, "docA", 100);
        let newer = LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 5,
            synced_epoch: 5,
            stamp: stamp(30, 1),
        };
        let older = LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 4,
            synced_epoch: 2,
            stamp: stamp(20, 1),
        };
        // Apply newer first, then the older (out of order) — the higher stamp holds.
        p.apply(&newer);
        p.apply(&older);
        assert_eq!(p.sync_epochs("docA", 100), Some((5, 5)));
        assert!(!p.document_in_flight("docA"));
        // Redelivery of the older op is a no-op.
        p.apply(&older);
        assert_eq!(p.sync_epochs("docA", 100), Some((5, 5)));
    }

    #[test]
    fn sync_in_flight_ignores_dead_or_closed_editor() {
        // A closed editor's unsynced edits do not count.
        let mut closed = LivenessProjection::new();
        closed.apply(&LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 3,
            synced_epoch: 1,
            stamp: stamp(10, 1),
        });
        assert!(
            !closed.document_in_flight("docA"),
            "no open fact for the pid ⇒ not in flight"
        );
        // A dead editor's unsynced edits do not count (whole-editor-death cascade).
        let mut dead = LivenessProjection::new();
        open(&mut dead, "docA", 100);
        dead.apply(&LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 3,
            synced_epoch: 1,
            stamp: stamp(10, 1),
        });
        assert!(dead.document_in_flight("docA"));
        dead.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(15, 1),
        });
        assert!(
            !dead.document_in_flight("docA"),
            "a dead pid's in-flight edits drop"
        );
    }

    #[test]
    fn sync_op_round_trips_through_the_liveness_frame() {
        let ops = vec![LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 7,
            synced_epoch: 4,
            stamp: stamp(10, 2),
        }];
        let frame = encode_liveness_frame(&ops).expect("encode");
        let decoded = decode_liveness_frame(&frame)
            .expect("liveness frame")
            .expect("decode");
        assert_eq!(decoded, ops);
    }

    #[test]
    fn closed_document_remains_tracked_for_authoritative_false_reads() {
        let mut projection = LivenessProjection::new();
        projection.apply(&LivenessOp::Close {
            document_hash: "known-closed".into(),
            pid: 9,
            observed_tags: vec!["old-open".into()],
        });

        assert!(projection.tracks_document("known-closed"));
        assert!(!projection.open_docs().contains("known-closed"));
        assert!(!projection.tracks_document("never-seen"));
    }

    // Order independence + re-delivery no-op for the same scenario.
    #[test]
    fn open_set_order_independent_and_redelivery_noop() {
        let ops = [
            LivenessOp::Open {
                document_hash: "docA".into(),
                pid: 100,
                tag: "t3".into(),
            },
            LivenessOp::Close {
                document_hash: "docA".into(),
                pid: 100,
                observed_tags: vec!["t1".into()],
            },
            LivenessOp::Open {
                document_hash: "docA".into(),
                pid: 100,
                tag: "t1".into(),
            },
        ];
        let mut p = LivenessProjection::new();
        for op in &ops {
            p.apply(op);
        }
        // Re-deliver everything again, reversed — must not change the result.
        for op in ops.iter().rev() {
            p.apply(op);
        }
        assert!(p.is_open("docA", 100));
    }

    // Conformance scenario `lww_alive_highest_stamp_wins`.
    #[test]
    fn lww_alive_highest_stamp_wins() {
        let mut p = LivenessProjection::new();
        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: true,
            stamp: stamp(20, 1),
        });
        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(25, 1),
        });
        // Stale re-assert at a lower stamp is dominated.
        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: true,
            stamp: stamp(22, 1),
        });
        assert!(!p.pid_alive(100), "highest-stamp alive=false wins");
    }

    // Conformance scenario `whole_editor_death_cascades`.
    #[test]
    fn whole_editor_death_cascades() {
        let mut p = LivenessProjection::new();
        for (doc, pid, tag) in [("docA", 100, "a"), ("docB", 100, "b"), ("docC", 200, "c")] {
            p.apply(&LivenessOp::Open {
                document_hash: doc.into(),
                pid,
                tag: tag.into(),
            });
        }
        assert_eq!(
            p.live_docs(),
            ["docA", "docB", "docC"]
                .map(String::from)
                .into_iter()
                .collect()
        );
        // pid 100 dies → docA and docB drop; docC (pid 200) unaffected.
        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(30, 1),
        });
        assert_eq!(
            p.live_docs(),
            ["docC"].map(String::from).into_iter().collect()
        );
        // open_docs (open-set ground truth) still shows all three — death is not a close.
        assert_eq!(p.open_docs().len(), 3);
        assert_eq!(
            p.all_open_pids(),
            [100, 200].into_iter().collect(),
            "restore-time OS reconciliation must still see a dead pid whose durable open facts remain"
        );
    }

    // A doc shared by a dead pid and a live pid stays live (cascade is per-pid).
    #[test]
    fn shared_doc_stays_live_via_second_pid() {
        let mut p = LivenessProjection::new();
        p.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "a".into(),
        });
        p.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 200,
            tag: "b".into(),
        });
        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(30, 1),
        });
        assert!(
            p.live_docs().contains("docA"),
            "pid 200 still alive keeps docA live"
        );
        assert_eq!(p.open_pids("docA"), [100, 200].into_iter().collect());
    }

    // `#ghosteditorliveness` regression: a document restored by durable hydration
    // (controller restart) carries `Open` + `Register` facts but NO `Alive` fact,
    // because the crashed editor's exit watcher never published its terminal
    // `Alive{false}`. `pid_alive` presumes alive absent a death fact, so the pid is
    // a *ghost* — counted live forever, holding `live_editors >= 1` and wedging
    // every disk-authority resolve. The exit-watcher reconciliation must publish the
    // missing death fact (from OS `kill(pid,0)` == ESRCH), after which the ghost
    // drops from every live view. `all_open_pids` must still surface the pid BEFORE
    // reconciliation so the watcher can find it to reap.
    #[test]
    fn hydrated_open_without_alive_is_a_ghost_until_reaped() {
        let mut p = LivenessProjection::new();
        // Hydration replays the durable Open + Register with no Alive fact.
        p.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 930287,
            tag: "boot".into(),
        });
        p.apply(&LivenessOp::Register(EditorRegistration {
            document_hash: "docA".into(),
            pid: 930287,
            path: "/proj/tasks/plan.md".into(),
            editor_id: "jetbrains".into(),
            editor_kind: "jetbrains".into(),
            editor_version: "2024.2".into(),
            capabilities: vec![],
            timestamp_ms: 1,
        }));

        // Ghost: with no death fact the crashed editor reads as fully live.
        assert!(p.pid_alive(930287), "no death fact ⇒ presumed alive (the bug)");
        assert!(p.live_docs().contains("docA"), "ghost holds live_editors>=1");
        assert_eq!(
            p.live_registrations("docA").len(),
            1,
            "ghost registration is counted as a delivery target"
        );
        // The watcher's reap candidate source must still see the ghost pid.
        assert!(
            p.all_open_pids().contains(&930287),
            "all_open_pids must surface the ghost so the watcher can reap it"
        );

        // Reconciliation: OS liveness says the pid is gone → publish the death fact
        // the crashed editor never sent.
        p.apply(&LivenessOp::Alive {
            pid: 930287,
            value: false,
            stamp: stamp(100, 0),
        });

        assert!(!p.pid_alive(930287), "reap publishes the missing Alive{{false}}");
        assert!(
            !p.live_docs().contains("docA"),
            "reaped ghost drops from live_docs ⇒ disk authority ⇒ convergence"
        );
        assert!(
            p.live_registrations("docA").is_empty(),
            "reaped ghost is no longer a delivery target"
        );
    }

    #[test]
    fn liveness_frame_roundtrips_through_crdtsync() {
        let batch = vec![
            LivenessOp::Open {
                document_hash: "docA".into(),
                pid: 100,
                tag: "t1".into(),
            },
            LivenessOp::Alive {
                pid: 100,
                value: false,
                stamp: stamp(25, 1),
            },
        ];
        let frame = encode_liveness_frame(&batch).expect("encode");
        assert!(matches!(frame, IpcMessage::CrdtSync(_)));
        let decoded = decode_liveness_frame(&frame)
            .expect("is liveness")
            .expect("ok");
        assert_eq!(decoded, batch);
    }

    #[test]
    fn decode_ignores_foreign_crdtsync_and_non_crdtsync() {
        // A non-CrdtSync message.
        let snap = IpcMessage::Snapshot(lazily::Snapshot::new(0, vec![], vec![], vec![]));
        assert!(decode_liveness_frame(&snap).is_none());
        // A CrdtSync with no liveness-tagged op (foreign graph frame).
        let foreign = IpcMessage::CrdtSync(CrdtSync {
            frontier: Vec::new(),
            ops: vec![CrdtOp {
                node: NodeId(7),
                key: None,
                stamp: stamp(1, 1),
                state: IpcValue::from(vec![1, 2, 3]),
            }],
        });
        assert!(decode_liveness_frame(&foreign).is_none());
    }

    #[test]
    fn decode_surfaces_malformed_liveness_bytes() {
        let bad = IpcMessage::CrdtSync(CrdtSync {
            frontier: Vec::new(),
            ops: vec![CrdtOp {
                node: LIVENESS_NODE,
                key: None,
                stamp: stamp(1, 1),
                state: IpcValue::from(b"not json".to_vec()),
            }],
        });
        assert!(decode_liveness_frame(&bad).expect("is liveness").is_err());
    }

    // End-to-end fold from a decoded frame (the controller's receive path).
    #[test]
    fn projection_folds_a_decoded_frame() {
        let batch = vec![
            LivenessOp::Open {
                document_hash: "docA".into(),
                pid: 100,
                tag: "t1".into(),
            },
            LivenessOp::Open {
                document_hash: "docB".into(),
                pid: 100,
                tag: "t2".into(),
            },
        ];
        let frame = encode_liveness_frame(&batch).unwrap();
        let ops = decode_liveness_frame(&frame).unwrap().unwrap();
        let mut p = LivenessProjection::new();
        p.apply_batch(&ops);
        assert_eq!(
            p.open_docs(),
            ["docA", "docB"].map(String::from).into_iter().collect()
        );
    }
}
