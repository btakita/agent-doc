//! Document-op reliable-sync channel (`#docop-plane`): replicate `TextCrdt` deltas
//! through the *same* lazily reliable-sync stack the liveness plane proved, so the
//! controller's canonical is continuously, durably fed and **never freezes**.
//!
//! ## Why this exists
//!
//! The `live_editors == 0` phantom lease had two halves. Detection (does a connected
//! plugin hold the doc open) is fixed by the liveness plane. The *content* half is
//! this module: even when authority is correctly kept for the editor, the resolve
//! path served the relay **canonical**, which was frozen-stale *because no replica fed
//! it* — the operator's edits lived only in a passive live-buffer sidecar the relay
//! reconciled opportunistically. That is how a deleted queue head (`#sy71`)
//! resurrected: a stale canonical was served over a live edit.
//!
//! The fix is structural: the connected plugin replicates its document ops into the
//! controller's canonical over a durable, backpressured channel, so the canonical is
//! always current and "editor authority" always serves live operator content.
//!
//! ## Wire model
//!
//! The unit is a [`lazily::TextOp`] list — a [`TextCrdt::delta_since`] result — carried
//! as a [`CrdtSync`] frame on the reserved [`DOCUMENT_OP_NODE`], exactly like liveness
//! carries its ops (see [`super::liveness`]). Frames flow through a `lazily::SyncDriver`
//! backed by a [`lazily::DurableOutbox`], so a delta lost while the controller is down
//! is **retained** (backpressure) and **replayed** on reconnect (retry); a recycled
//! controller rebuilds its canonical by re-folding the replayed outbox suffix. The ops
//! are commutative, associative and idempotent (each carries its own `OpId`), so
//! redelivery and reorder converge — the reliable-sync at-least-once contract is safe
//! by construction, no de-dup ledger required.
//!
//! This is Phase 1 (`plan-document-op-replication-never-frozen-canonical.md`): the pure
//! channel + canonical fold, exercised end-to-end through the real `SyncDriver` in the
//! SimWorld tests below. It does not yet flip the relay's authority — the controller
//! ingest wiring and the shadow/authority-flip land in later phases behind a parity
//! gate.

use anyhow::{Result, anyhow};
use lazily::{CrdtOp, CrdtSync, IpcMessage, IpcValue, NodeId, TextCrdt, TextOp, WireStamp};

/// Reserved graph node id carrying document-op deltas. Mirrors
/// [`super::liveness`]'s `LIVENESS_NODE = NodeId(u64::MAX)`; one below it so the two
/// planes never collide on the shared `CrdtSync` op stream.
const DOCUMENT_OP_NODE: NodeId = NodeId(u64::MAX - 1);

/// Reserved node id for a **full-state adopt** frame (`#reattach-adopt`): the editor's
/// whole `Vec<TextOp>` (`ReplicaState::encode_state` = `delta_since(∅)`), sent on
/// reattach so the controller can REPLACE a drifted canonical (drop `#sy71`-class
/// drift) rather than union-merge it. Distinct from [`DOCUMENT_OP_NODE`] so the
/// controller routes it to `adopt_editor_full_state_for_file`, not the fold path.
const DOCUMENT_OP_ADOPT_NODE: NodeId = NodeId(u64::MAX - 2);

/// Encode a `TextCrdt` delta (a [`TextCrdt::delta_since`] op list) as a reliable-sync
/// frame. A whole-state snapshot is `delta_since(&TextVersionVector::new())`.
pub fn encode_document_op_frame(ops: &[TextOp]) -> Result<IpcMessage> {
    let bytes = serde_json::to_vec(ops)?;
    let op = CrdtOp {
        node: DOCUMENT_OP_NODE,
        key: None,
        // The carriage stamp is unused by the fold (each `TextOp` carries its own
        // `OpId`); zero is a stable, deterministic placeholder — same as liveness.
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

/// Parse an editor-side `agent_doc_replica_diff` JSON buffer and encode it as a
/// document-op frame. `None` is the canonical empty-delta no-op.
pub fn encode_document_op_json_frame(delta_json: &str) -> Result<Option<IpcMessage>> {
    let ops: Vec<TextOp> =
        serde_json::from_str(delta_json).map_err(|error| anyhow!("parse delta_json: {error}"))?;
    if ops.is_empty() {
        Ok(None)
    } else {
        encode_document_op_frame(&ops).map(Some)
    }
}

/// Decode a document-op frame produced by [`encode_document_op_frame`].
///
/// Returns `None` for a non-`CrdtSync` message or a `CrdtSync` carrying no
/// document-op-tagged op (a foreign/liveness/graph frame — let the host route it
/// elsewhere); `Some(Err)` for a document-op-tagged op whose inline bytes are
/// malformed (never a silent drop). The `SharedBlob` transport is not used here, so a
/// `SharedBlob` op on the document-op node is treated as malformed.
pub fn decode_document_op_frame(message: &IpcMessage) -> Option<Result<Vec<TextOp>>> {
    let IpcMessage::CrdtSync(sync) = message else {
        return None;
    };
    let ops: Vec<&CrdtOp> = sync
        .ops
        .iter()
        .filter(|op| op.node == DOCUMENT_OP_NODE)
        .collect();
    if ops.is_empty() {
        return None;
    }
    Some(decode_document_ops(&ops))
}

fn decode_document_ops(ops: &[&CrdtOp]) -> Result<Vec<TextOp>> {
    let mut out = Vec::new();
    for op in ops {
        let IpcValue::Inline(bytes) = &op.state else {
            return Err(anyhow!(
                "document-op frame carried a non-inline (SharedBlob) op state"
            ));
        };
        let batch: Vec<TextOp> = serde_json::from_slice(bytes)
            .map_err(|e| anyhow!("document-op frame inline decode failed: {e}"))?;
        out.extend(batch);
    }
    Ok(out)
}

/// Encode an editor's **full-state adopt** frame (`#reattach-adopt`): `ops` is the
/// editor's whole `Vec<TextOp>` (`ReplicaState::encode_state`). Same envelope as
/// [`encode_document_op_frame`] but on [`DOCUMENT_OP_ADOPT_NODE`], so the receiver
/// adopts (replaces the canonical) instead of folding (union-merge).
pub fn encode_full_state_adopt_frame(ops: &[TextOp]) -> Result<IpcMessage> {
    let bytes = serde_json::to_vec(ops)?;
    let op = CrdtOp {
        node: DOCUMENT_OP_ADOPT_NODE,
        key: None,
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

/// Build a ready-to-push reliable-sync **envelope** for a full-state adopt directly
/// from the editor's `agent_doc_replica_encode_state` JSON (a `serde_json`
/// `Vec<TextOp>`) — one call so the FFI push path needs no `lazily::TextOp` knowledge.
pub fn encode_full_state_adopt_envelope(
    document_hash: &str,
    full_state_json: &str,
) -> Result<serde_json::Value> {
    let ops: Vec<TextOp> =
        serde_json::from_str(full_state_json).map_err(|e| anyhow!("parse full_state_json: {e}"))?;
    let frame = encode_full_state_adopt_frame(&ops)?;
    super::encode_envelope(document_hash, &frame)
}

/// Decode a full-state adopt frame produced by [`encode_full_state_adopt_frame`].
/// `None` for any non-adopt frame (so the caller falls through to the fold path).
pub fn decode_full_state_adopt_frame(message: &IpcMessage) -> Option<Result<Vec<TextOp>>> {
    let IpcMessage::CrdtSync(sync) = message else {
        return None;
    };
    let ops: Vec<&CrdtOp> = sync
        .ops
        .iter()
        .filter(|op| op.node == DOCUMENT_OP_ADOPT_NODE)
        .collect();
    if ops.is_empty() {
        return None;
    }
    Some(decode_adopt_ops(&ops))
}

fn decode_adopt_ops(ops: &[&CrdtOp]) -> Result<Vec<TextOp>> {
    let mut out = Vec::new();
    for op in ops {
        let IpcValue::Inline(bytes) = &op.state else {
            return Err(anyhow!(
                "full-state adopt frame carried a non-inline (SharedBlob) op state"
            ));
        };
        let batch: Vec<TextOp> = serde_json::from_slice(bytes)
            .map_err(|e| anyhow!("full-state adopt frame inline decode failed: {e}"))?;
        out.extend(batch);
    }
    Ok(out)
}

/// Reserved node id for a **bounded text adopt** frame (`#reattach-adopt`, runaway-safe):
/// carries the editor's authoritative TEXT (`O(text)`), NOT the tombstone op-log. The
/// controller rebuilds the canonical from text (`RelayHub::adopt_editor_text`).
const TEXT_ADOPT_NODE: NodeId = NodeId(u64::MAX - 3);

/// Encode a bounded text-adopt frame carrying the editor's document `text`.
pub fn encode_text_adopt_frame(text: &str) -> Result<IpcMessage> {
    let op = CrdtOp {
        node: TEXT_ADOPT_NODE,
        key: None,
        stamp: WireStamp {
            wall_time: 0,
            logical: 0,
            peer: 0,
        },
        state: IpcValue::from(text.as_bytes().to_vec()),
    };
    Ok(IpcMessage::CrdtSync(CrdtSync {
        frontier: Vec::new(),
        ops: vec![op],
    }))
}

/// Decode a bounded text-adopt frame; `None` for any other frame.
pub fn decode_text_adopt_frame(message: &IpcMessage) -> Option<Result<String>> {
    let IpcMessage::CrdtSync(sync) = message else {
        return None;
    };
    let op = sync.ops.iter().find(|op| op.node == TEXT_ADOPT_NODE)?;
    let IpcValue::Inline(bytes) = &op.state else {
        return Some(Err(anyhow!(
            "text-adopt frame carried a non-inline (SharedBlob) op state"
        )));
    };
    Some(
        String::from_utf8(bytes.clone())
            .map_err(|e| anyhow!("text-adopt frame is not UTF-8: {e}")),
    )
}

/// One-call envelope for a bounded text adopt from the editor's document `text` — the FFI
/// push path uses this (no `TextOp` knowledge, bounded payload).
pub fn encode_text_adopt_envelope(
    document_hash: &str,
    text: &str,
) -> Result<serde_json::Value> {
    let frame = encode_text_adopt_frame(text)?;
    super::encode_envelope(document_hash, &frame)
}

/// Controller-side canonical fed by replicated document-op frames.
///
/// Folding a delivered frame applies its ops into the canonical `TextCrdt`
/// (commutative/idempotent via `OpId` identity), so the canonical converges to the
/// editor's text regardless of frame loss/reorder/redelivery — it is **never frozen**
/// while the editor is connected. After a controller recycle it is rebuilt by
/// re-folding the replayed outbox suffix onto a fresh fold seeded from the last durable
/// checkpoint.
pub struct CanonicalFold {
    doc: TextCrdt,
}

impl CanonicalFold {
    /// A fresh canonical seeded from `text` (the last durable checkpoint / committed
    /// document). `peer` is the controller's stable canonical peer id; editors bootstrap
    /// from [`snapshot`](Self::snapshot) so the shared base carries the canonical's
    /// `OpId`s (no double-insert on merge).
    pub fn from_text(peer: u64, text: &str) -> Self {
        Self {
            doc: TextCrdt::from_str(peer, text),
        }
    }

    /// The whole-state op list an editor `apply_delta`s to bootstrap a converged
    /// replica of this canonical's base (shared `OpId`s).
    pub fn snapshot(&self) -> Vec<TextOp> {
        self.doc.delta_since(&lazily::TextVersionVector::new())
    }

    /// The canonical's version-vector frontier — the compact cursor an editor sends so
    /// [`TextCrdt::delta_since`] computes exactly the ops the canonical still lacks.
    pub fn version_vector(&self) -> lazily::TextVersionVector {
        self.doc.version_vector()
    }

    /// Fold one delivered reliable-sync frame into the canonical. A non-document-op
    /// frame is ignored (`Ok(false)`); a malformed document-op frame is surfaced
    /// (never a silent drop). Returns whether the visible canonical text changed.
    pub fn ingest(&mut self, message: &IpcMessage) -> Result<bool> {
        match decode_document_op_frame(message) {
            Some(ops) => Ok(self.doc.apply_delta(&ops?)),
            None => Ok(false),
        }
    }

    /// Directly fold a decoded op list (the in-process controller path, no wire hop).
    pub fn apply_ops(&mut self, ops: &[TextOp]) -> bool {
        self.doc.apply_delta(ops)
    }

    /// The current converged canonical text.
    pub fn text(&self) -> String {
        self.doc.text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stable peer ids: the controller owns the canonical base; the editor edits.
    const CANON_PEER: u64 = 1;
    const EDITOR_PEER: u64 = 2;

    /// Bootstrap an editor replica converged with `fold`'s base (shared `OpId`s), then
    /// return it ready for local edits.
    fn bootstrap_editor(fold: &CanonicalFold) -> TextCrdt {
        let mut editor = TextCrdt::new(EDITOR_PEER);
        editor.apply_delta(&fold.snapshot());
        editor
    }

    #[test]
    fn document_op_frame_round_trips() {
        let src = TextCrdt::from_str(CANON_PEER, "hello");
        let ops = src.delta_since(&lazily::TextVersionVector::new());
        let frame = encode_document_op_frame(&ops).expect("encode");
        let decoded = decode_document_op_frame(&frame)
            .expect("is a document-op frame")
            .expect("decodes");
        assert_eq!(decoded, ops, "round-trip preserves the op list verbatim");
    }

    #[test]
    fn adopt_and_fold_frames_round_trip_and_route_distinctly() {
        let src = TextCrdt::from_str(CANON_PEER, "hello\n");
        let ops = src.delta_since(&lazily::TextVersionVector::new());

        let adopt = encode_full_state_adopt_frame(&ops).expect("encode adopt");
        let fold = encode_document_op_frame(&ops).expect("encode fold");

        // Each decodes as its own kind and is invisible to the other's decoder, so the
        // controller routes adopt→replace vs fold→union-merge without ambiguity.
        assert_eq!(
            decode_full_state_adopt_frame(&adopt).expect("is adopt").expect("decodes"),
            ops
        );
        assert!(decode_document_op_frame(&adopt).is_none(), "adopt is not a fold frame");
        assert_eq!(
            decode_document_op_frame(&fold).expect("is fold").expect("decodes"),
            ops
        );
        assert!(decode_full_state_adopt_frame(&fold).is_none(), "fold is not an adopt frame");
    }

    #[test]
    fn foreign_frames_are_ignored_not_misdecoded() {
        // A liveness frame must not be mistaken for a document-op frame.
        let live = super::super::liveness::encode_liveness_frame(&[
            super::super::liveness::LivenessOp::Open {
                document_hash: "doc".into(),
                pid: 1,
                tag: "t".into(),
            },
        ])
        .expect("encode liveness");
        assert!(
            decode_document_op_frame(&live).is_none(),
            "a liveness frame is not a document-op frame"
        );
        let mut fold = CanonicalFold::from_text(CANON_PEER, "base");
        assert!(
            !fold.ingest(&live).expect("ingest foreign"),
            "folding a foreign frame is a no-op, not an error"
        );
        assert_eq!(fold.text(), "base");
    }

    #[test]
    fn redelivered_document_op_frames_are_idempotent() {
        // At-least-once redelivery must not duplicate text.
        let mut fold = CanonicalFold::from_text(CANON_PEER, "hi\n");
        let mut editor = bootstrap_editor(&fold);
        editor.insert_str(2, "there ");
        let delta = editor.delta_since(&fold.version_vector());
        let frame = encode_document_op_frame(&delta).expect("encode");

        assert!(fold.ingest(&frame).expect("first fold changes text"));
        let once = fold.text();
        // Redeliver the exact same frame twice more.
        assert!(!fold.ingest(&frame).expect("redeliver is a no-op"));
        assert!(!fold.ingest(&frame).expect("redeliver is a no-op"));
        assert_eq!(fold.text(), once, "redelivery is idempotent — no duplication");
        assert_eq!(fold.text(), editor.text(), "canonical matches the editor");
    }

    #[test]
    fn canonical_converges_to_editor_across_disconnect_reconnect_zero_loss() {
        use lazily::{Clock, InMemoryOutbox, Snapshot, SnapshotProvider, SyncDriver};
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        // --- SimWorld doubles (mirror the liveness push-loop harness) ---------------
        #[derive(Clone)]
        struct WireTransport {
            delivered: Rc<RefCell<Vec<serde_json::Value>>>,
            connected: Rc<Cell<bool>>,
        }
        impl super::super::EnvelopeTransport for WireTransport {
            fn send_envelope(&self, env: &serde_json::Value) -> anyhow::Result<()> {
                if !self.connected.get() {
                    return Err(anyhow!("SimWorld: controller socket down"));
                }
                self.delivered.borrow_mut().push(env.clone());
                Ok(())
            }
        }
        struct FixedClock;
        impl Clock for FixedClock {
            fn now_millis(&self) -> u64 {
                0
            }
        }
        // The outbox replays on reconnect, so the receiver never gaps → provider unused.
        struct TrivialProvider;
        impl SnapshotProvider for TrivialProvider {
            fn snapshot(&self, from_epoch: u64) -> IpcMessage {
                IpcMessage::Snapshot(Snapshot::new(from_epoch, vec![], vec![], vec![]))
            }
        }

        // --- Editor bootstraps from the canonical, then does the operator's edits ----
        let fold_vv0 = {
            let seed = CanonicalFold::from_text(CANON_PEER, "hello\n");
            seed.version_vector()
        };
        let mut fold = CanonicalFold::from_text(CANON_PEER, "hello\n");
        let mut editor = bootstrap_editor(&fold);
        assert_eq!(editor.text(), "hello\n");

        // Two operator edits, each producing a delta frame against the last-pushed vv.
        // "hello\n" -> "hello world\n"
        editor.insert_str(5, " world");
        let delta1 = editor.delta_since(&fold_vv0);
        let last_pushed = editor.version_vector();
        // "hello world\n" -> "world\n" (delete "hello ")
        for _ in 0..6 {
            editor.delete(0);
        }
        let delta2 = editor.delta_since(&last_pushed);
        assert_eq!(editor.text(), "world\n");

        // --- Push loop through the REAL SyncDriver with a mid-stream disconnect ------
        let delivered = Rc::new(RefCell::new(Vec::new()));
        let connected = Rc::new(Cell::new(true));
        let sink = super::super::ReliableSyncSink::new(
            WireTransport {
                delivered: delivered.clone(),
                connected: connected.clone(),
            },
            "docwire",
        );
        let (_inbox, source) = super::super::reliable_sync_channel("docwire");
        let mut driver = SyncDriver::new(
            sink,
            source,
            InMemoryOutbox::default(),
            FixedClock,
            TrivialProvider,
        );

        driver.enqueue(1, encode_document_op_frame(&delta1).expect("encode d1"));
        // Controller goes down before the second delta is delivered.
        connected.set(false);
        driver.enqueue(2, encode_document_op_frame(&delta2).expect("encode d2"));
        driver.tick().expect("tick while down");
        assert!(driver.is_stalled(), "a failed send stalls the driver");

        // Controller back: replay the retained outbox suffix, then drain.
        connected.set(true);
        driver.on_reconnect();
        driver.tick().expect("tick after reconnect");

        // --- Controller folds every delivered frame into the canonical --------------
        for env in delivered.borrow().iter() {
            let (_hash, msg) = super::super::decode_envelope(env)
                .expect("a reliable-sync envelope")
                .expect("envelope decodes");
            fold.ingest(&msg).expect("fold document-op frame");
        }
        assert_eq!(
            fold.text(),
            editor.text(),
            "canonical converged to the editor's live text — the disconnect-lost delta \
             was replayed, so the canonical was never left frozen"
        );
        assert_eq!(fold.text(), "world\n");

        // --- Recycle: a fresh canonical rebuilt from the replayed outbox reconverges -
        let mut recycled = CanonicalFold::from_text(CANON_PEER, "hello\n");
        for env in delivered.borrow().iter() {
            let (_hash, msg) = super::super::decode_envelope(env)
                .expect("a reliable-sync envelope")
                .expect("envelope decodes");
            recycled.ingest(&msg).expect("fold document-op frame");
        }
        assert_eq!(
            recycled.text(),
            editor.text(),
            "a recycled controller rebuilds the canonical from the durable outbox — \
             no operator edit is lost across a recycle"
        );
    }
}
