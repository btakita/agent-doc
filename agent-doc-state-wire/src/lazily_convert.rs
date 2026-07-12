//! Type-unification bridge: agent-doc `WireDelta`/`WireDeltaOp` ↔ lazily
//! `Delta`/`DeltaOp` (sidecar-retirement Phase 3B, `#lzsync`).
//!
//! The `state_subscribe` pull today folds a **bespoke** `WireDelta` (this crate's
//! `build_delta`); Phase 3B unifies that onto lazily's `Delta`/`DeltaOp` + the
//! `ResyncCoordinator` so the state stream reuses the same proven reliable-sync
//! machinery as the liveness plane. This module is the first, behavior-preserving
//! step: a bidirectional, lossless-for-representable-ops conversion between the two
//! type systems, so the controller can move to producing/consuming `lazily::Delta`
//! internally while the on-wire JSON stays exactly what the S5 `StateGraphMirror`
//! plugins already parse (the wire cutover is a later, operator-verified slice).
//!
//! The mappings are structural: `slot_id: u64` ↔ [`NodeId`]`(u64)`; a base64
//! `payload` string ↔ [`IpcValue::Inline`] bytes; and `NodeAdd`'s
//! `Option<String>` payload ↔ [`NodeState::Payload`] / [`NodeState::Opaque`]. A
//! lazily `SharedBlob` value has no string-payload wire representation, so the
//! lazily→wire direction reports it as `None` rather than inventing bytes.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use lazily::{
    Delta, DeltaOp, EdgeSnapshot, IpcMessage, IpcValue, NodeId, NodeSnapshot, NodeState, Snapshot,
};

use crate::{WireDelta, WireDeltaOp, WireSnapshot, WireSubscribe};

#[inline]
fn nid(id: u64) -> NodeId {
    NodeId(id)
}

/// base64 payload string → lazily inline value bytes.
fn payload_to_value(payload: &str) -> Result<IpcValue, base64::DecodeError> {
    Ok(IpcValue::Inline(BASE64.decode(payload.as_bytes())?))
}

/// lazily inline value bytes → base64 payload string, or `None` for a `SharedBlob`
/// (not representable in the string-payload wire).
fn value_to_payload(value: &IpcValue) -> Option<String> {
    match value {
        IpcValue::Inline(bytes) => Some(BASE64.encode(bytes)),
        _ => None,
    }
}

/// Convert one agent-doc wire op to lazily's [`DeltaOp`].
pub fn wire_op_to_lazily(op: &WireDeltaOp) -> Result<DeltaOp, base64::DecodeError> {
    Ok(match op {
        WireDeltaOp::CellSet { slot_id, payload } => DeltaOp::CellSet {
            node: nid(*slot_id),
            payload: payload_to_value(payload)?,
        },
        WireDeltaOp::SlotValue { slot_id, payload } => DeltaOp::SlotValue {
            node: nid(*slot_id),
            payload: payload_to_value(payload)?,
        },
        WireDeltaOp::Invalidate { slot_id } => DeltaOp::Invalidate {
            node: nid(*slot_id),
        },
        WireDeltaOp::NodeAdd {
            slot_id,
            type_tag,
            payload,
        } => DeltaOp::NodeAdd {
            node: nid(*slot_id),
            type_tag: type_tag.clone(),
            state: match payload {
                Some(b64) => NodeState::Payload(BASE64.decode(b64.as_bytes())?),
                None => NodeState::Opaque,
            },
            key: None,
        },
        WireDeltaOp::NodeRemove { slot_id } => DeltaOp::NodeRemove {
            node: nid(*slot_id),
        },
        WireDeltaOp::EdgeAdd {
            dependent,
            dependency,
        } => DeltaOp::EdgeAdd {
            dependent: nid(*dependent),
            dependency: nid(*dependency),
        },
        WireDeltaOp::EdgeRemove {
            dependent,
            dependency,
        } => DeltaOp::EdgeRemove {
            dependent: nid(*dependent),
            dependency: nid(*dependency),
        },
    })
}

/// Convert one lazily [`DeltaOp`] back to an agent-doc wire op. Returns `None` when
/// a payload is a `SharedBlob` (no string-payload wire representation).
pub fn lazily_op_to_wire(op: &DeltaOp) -> Option<WireDeltaOp> {
    Some(match op {
        DeltaOp::CellSet { node, payload } => WireDeltaOp::CellSet {
            slot_id: node.0,
            payload: value_to_payload(payload)?,
        },
        DeltaOp::SlotValue { node, payload } => WireDeltaOp::SlotValue {
            slot_id: node.0,
            payload: value_to_payload(payload)?,
        },
        DeltaOp::Invalidate { node } => WireDeltaOp::Invalidate { slot_id: node.0 },
        DeltaOp::NodeAdd {
            node,
            type_tag,
            state,
            ..
        } => WireDeltaOp::NodeAdd {
            slot_id: node.0,
            type_tag: type_tag.clone(),
            payload: match state {
                NodeState::Payload(bytes) => Some(BASE64.encode(bytes)),
                NodeState::Opaque => None,
                NodeState::SharedBlob(_) => return None,
            },
        },
        DeltaOp::NodeRemove { node } => WireDeltaOp::NodeRemove { slot_id: node.0 },
        DeltaOp::EdgeAdd {
            dependent,
            dependency,
        } => WireDeltaOp::EdgeAdd {
            dependent: dependent.0,
            dependency: dependency.0,
        },
        DeltaOp::EdgeRemove {
            dependent,
            dependency,
        } => WireDeltaOp::EdgeRemove {
            dependent: dependent.0,
            dependency: dependency.0,
        },
    })
}

/// agent-doc [`WireDelta`] → lazily [`Delta`]. Drops `document_hash` (the
/// reliable-sync channel carries it) and the `"type"` discriminator (an
/// `IpcMessage::Delta` tag supplies it).
pub fn wire_delta_to_lazily(delta: &WireDelta) -> Result<Delta, base64::DecodeError> {
    let ops = delta
        .ops
        .iter()
        .map(wire_op_to_lazily)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Delta {
        base_epoch: delta.base_epoch,
        epoch: delta.epoch,
        ops,
    })
}

/// agent-doc [`WireSnapshot`] → lazily [`Snapshot`]. Drops `document_hash`/`"type"`
/// (the transport envelope + `IpcMessage::Snapshot` tag supply them): `slot_id`→
/// `NodeId`, base64 `payload`→`NodeState::Payload`, absent payload→`Opaque`.
pub fn wire_snapshot_to_lazily(snapshot: &WireSnapshot) -> Result<Snapshot, base64::DecodeError> {
    let nodes = snapshot
        .nodes
        .iter()
        .map(|n| {
            Ok(NodeSnapshot {
                node: nid(n.slot_id),
                type_tag: n.type_tag.clone(),
                state: match &n.payload {
                    Some(b64) => NodeState::Payload(BASE64.decode(b64.as_bytes())?),
                    None => NodeState::Opaque,
                },
                key: None,
            })
        })
        .collect::<Result<Vec<_>, base64::DecodeError>>()?;
    let edges = snapshot
        .edges
        .iter()
        .map(|e| EdgeSnapshot {
            dependent: nid(e.dependent),
            dependency: nid(e.dependency),
        })
        .collect();
    Ok(Snapshot {
        epoch: snapshot.epoch,
        nodes,
        edges,
        roots: snapshot.roots.iter().map(|r| nid(*r)).collect(),
    })
}

/// agent-doc [`WireSubscribe`] → lazily [`IpcMessage`] — the native wire the plugins'
/// `GraphView` folds (`document_hash` rides the transport envelope). The `#lzsync` 3B
/// wire cutover: `state_subscribe` emits this instead of the bespoke `WireSubscribe` JSON.
pub fn wire_subscribe_to_ipc_message(
    subscribe: &WireSubscribe,
) -> Result<IpcMessage, base64::DecodeError> {
    Ok(match subscribe {
        WireSubscribe::Snapshot(s) => IpcMessage::Snapshot(wire_snapshot_to_lazily(s)?),
        WireSubscribe::Delta(d) => IpcMessage::Delta(wire_delta_to_lazily(d)?),
    })
}

/// lazily [`Delta`] + `document_hash` → agent-doc [`WireDelta`]. Returns `None` if
/// any op carries a non-representable `SharedBlob` payload.
pub fn lazily_delta_to_wire(delta: &Delta, document_hash: impl Into<String>) -> Option<WireDelta> {
    let ops = delta
        .ops
        .iter()
        .map(lazily_op_to_wire)
        .collect::<Option<Vec<_>>>()?;
    Some(WireDelta {
        message_type: WireDelta::TYPE,
        base_epoch: delta.base_epoch,
        epoch: delta.epoch,
        document_hash: document_hash.into(),
        ops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_variants() -> Vec<WireDeltaOp> {
        let payload = BASE64.encode(br#"{"v":1}"#);
        vec![
            WireDeltaOp::CellSet {
                slot_id: 1,
                payload: payload.clone(),
            },
            WireDeltaOp::SlotValue {
                slot_id: 2,
                payload: payload.clone(),
            },
            WireDeltaOp::Invalidate { slot_id: 3 },
            WireDeltaOp::NodeAdd {
                slot_id: 4,
                type_tag: "agent_doc.route".into(),
                payload: Some(payload.clone()),
            },
            WireDeltaOp::NodeAdd {
                slot_id: 5,
                type_tag: "agent_doc.queue".into(),
                payload: None,
            },
            WireDeltaOp::NodeRemove { slot_id: 6 },
            WireDeltaOp::EdgeAdd {
                dependent: 7,
                dependency: 8,
            },
            WireDeltaOp::EdgeRemove {
                dependent: 9,
                dependency: 10,
            },
        ]
    }

    #[test]
    fn every_wire_op_round_trips_through_lazily() {
        for op in all_variants() {
            let lazily = wire_op_to_lazily(&op).expect("to lazily");
            let back = lazily_op_to_wire(&lazily).expect("back to wire");
            assert_eq!(back, op, "round-trip mismatch for {op:?}");
        }
    }

    #[test]
    fn wire_subscribe_converts_to_native_ipc_message() {
        use crate::{WireEdgeSnapshot, WireNodeSnapshot};
        use lazily::{IpcMessage, NodeId, NodeState};
        let payload = BASE64.encode(br#"{"phase":"committed"}"#);

        // Snapshot → IpcMessage::Snapshot: slot_id→NodeId, base64 payload→Payload bytes.
        let snap = WireSubscribe::Snapshot(WireSnapshot {
            message_type: WireSnapshot::TYPE,
            epoch: 3,
            document_hash: "doc".into(),
            nodes: vec![WireNodeSnapshot {
                slot_id: 42,
                type_tag: "agent_doc.closeout.cycle".into(),
                state: "resolved",
                payload: Some(payload.clone()),
            }],
            edges: vec![WireEdgeSnapshot {
                dependent: 1,
                dependency: 2,
            }],
            roots: vec![42],
        });
        match wire_subscribe_to_ipc_message(&snap).expect("to native") {
            IpcMessage::Snapshot(s) => {
                assert_eq!(s.epoch, 3);
                assert_eq!(s.nodes[0].node, NodeId(42));
                assert_eq!(s.nodes[0].type_tag, "agent_doc.closeout.cycle");
                assert!(
                    matches!(&s.nodes[0].state, NodeState::Payload(b) if b == br#"{"phase":"committed"}"#)
                );
                assert_eq!(s.roots, vec![NodeId(42)]);
            }
            other => panic!("expected Snapshot, got {other:?}"),
        }

        // Delta → IpcMessage::Delta.
        let delta = WireSubscribe::Delta(WireDelta {
            message_type: WireDelta::TYPE,
            base_epoch: 3,
            epoch: 5,
            document_hash: "doc".into(),
            ops: vec![WireDeltaOp::CellSet {
                slot_id: 42,
                payload,
            }],
        });
        match wire_subscribe_to_ipc_message(&delta).expect("to native") {
            IpcMessage::Delta(d) => {
                assert_eq!((d.base_epoch, d.epoch, d.ops.len()), (3, 5, 1));
            }
            other => panic!("expected Delta, got {other:?}"),
        }

        // Native serde is externally tagged (`{"Snapshot":{…}}`) — the wire GraphView folds.
        let json = serde_json::to_value(wire_subscribe_to_ipc_message(&snap).unwrap()).unwrap();
        assert!(json.get("Snapshot").is_some(), "externally-tagged: {json}");
    }

    #[test]
    fn wire_delta_round_trips_preserving_epochs_and_hash() {
        let delta = WireDelta {
            message_type: WireDelta::TYPE,
            base_epoch: 40,
            epoch: 43,
            document_hash: "doc-abc".into(),
            ops: all_variants(),
        };
        let lazily = wire_delta_to_lazily(&delta).expect("to lazily");
        // Multi-epoch span is preserved (the accepted-event fold).
        assert_eq!(lazily.base_epoch, 40);
        assert_eq!(lazily.epoch, 43);
        let back = lazily_delta_to_wire(&lazily, "doc-abc").expect("back to wire");
        assert_eq!(back, delta);
    }

    #[test]
    fn shared_blob_payload_is_not_representable_on_the_wire() {
        // A lazily op with a SharedBlob node payload has no string-payload wire form.
        let op = DeltaOp::NodeAdd {
            node: nid(1),
            type_tag: "t".into(),
            state: NodeState::SharedBlob(lazily::ShmBlobRef {
                offset: 0,
                len: 0,
                generation: 0,
                epoch: 0,
                checksum: 0,
                backend: lazily::BlobBackendKind::default(),
            }),
            key: None,
        };
        assert!(lazily_op_to_wire(&op).is_none());
    }
}
