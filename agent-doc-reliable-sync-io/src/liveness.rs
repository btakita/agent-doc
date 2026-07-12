//! Cross-process editor liveness as OR-set / LWW cells (`#lzsync-liveness`,
//! sidecar-retirement Phase 3C — the controller *receiver* core).
//!
//! This is the derived-authority engine the cutover reads **instead of** scanning
//! `.agent-doc/live-buffer/*` + `plugin-owner/*.json`: the controller folds the
//! liveness frames pushed by the editor plugins into lazily's proven convergent
//! cells and derives the open-set / per-pid-alive / live-doc aggregate.
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
