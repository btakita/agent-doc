//! Unix-domain-socket carrier for Lazily's reliable-sync plane (`#lzsync`).
//!
//! Lazily reliable-sync `IpcMessage` frames travel over the existing PID-scoped
//! `agent-doc-ipc-io` controller Unix socket — no second transport.
//! This crate is the byte transport the design table assigns to the app:
//!
//! | Byte transport (which socket) | **app** (UDS `IpcSink`/`IpcSource`) | deployment choice |
//!
//! The correctness-critical protocol (`ResyncCoordinator`, `DurableOutbox`,
//! `SyncDriver`, the OR-set/LWW liveness cells) lives in lazily; this crate only
//! moves the already-encoded frames across the process boundary. It stays
//! transport-agnostic in the sense that the `IpcSink` half is generic over an
//! [`EnvelopeTransport`] so the driver's send path is unit-testable without a
//! live socket.
//!
//! ## Wire shape
//!
//! Each reliable-sync frame rides inside one NDJSON control message on the
//! existing socket:
//!
//! ```json
//! {"type":"reliable_sync","document_hash":"<hash>","codec":"msgpack","frame":"<base64>"}
//! ```
//!
//! The `frame` payload is the lazily `IpcMessage` encoded with the **msgpack**
//! codec — the decided cross-language wire codec (plan § Frame codec grounding
//! fact). msgpack is portable and evolution-safe, and the socket envelope stays
//! JSON so a mixed-codec listener can still route by `type`/`document_hash`
//! without decoding the opaque frame. The `document_hash` tag gives the per-doc
//! isolation invariant (a stale overlay for doc B cannot flip doc A's authority)
//! at the envelope layer, before the frame is ever decoded.

pub mod document_op;
pub mod liveness;
pub mod plane;
pub mod push;

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use lazily::{IpcCodec, IpcMessage, IpcSink, IpcSource};
use serde::{Deserialize, Serialize};

/// NDJSON `type` tag for a reliable-sync control frame on the shared socket.
pub const RELIABLE_SYNC_MESSAGE_TYPE: &str = "reliable_sync";

/// Process-global reliable-sync liveness plane — the hot-path authority source.
/// The controller feeds it (`ingest`); authority reads
/// through `agent-doc-crdt-relay-io` derive from its projection. It is
/// in-memory **per process**: warm in the controller (fed by editor pushes over the
/// reliable-sync RPC) and hydrated by controller-io from durable reliable-sync state
/// in a short-lived CLI. It lives here so every controller/realtime consumer reads
/// the same process projection.
pub fn global_liveness_plane() -> &'static parking_lot::Mutex<plane::ControllerLivenessPlane> {
    static PLANE: std::sync::LazyLock<parking_lot::Mutex<plane::ControllerLivenessPlane>> =
        std::sync::LazyLock::new(|| parking_lot::Mutex::new(plane::ControllerLivenessPlane::new()));
    &PLANE
}

/// Plane-primary "is a live editor attached to `file`?" — the shared hot-path authority
/// read for the step-3 flip. Returns `Some(true/false)` when the plane has ever
/// received an open/close fact for this document (including a durably known closed
/// document); returns `None` only on a true cold miss (a never-hydrated
/// document). The lock cannot poison (`#relaylockpoison`), so `None` can no
/// longer mean "a panic happened somewhere else and we are now silently
/// reporting no editor". Both
/// `controller-io::crdt_authority_for_file` and the
/// `document-realtime-io` `#6b5h` disk-write guard route through this so every hot-path
/// reader agrees. Controller-io owns durable hydration when this returns `None`.
pub fn plane_editor_live_for_path(file: &str) -> Option<bool> {
    let plane = global_liveness_plane().lock();
    let projection = plane.projection();
    let document_hash = agent_doc_hash::document_id_for_path(std::path::Path::new(file));
    if !projection.tracks_document(&document_hash) {
        return None;
    }
    Some(projection.live_docs().contains(&document_hash))
}

/// Plane-primary sync-in-flight read for one document. This compares Lazily
/// editor edit/sync epochs without filesystem state.
pub fn plane_document_in_flight_for_path(file: &str) -> Option<bool> {
    let plane = global_liveness_plane().lock();
    let projection = plane.projection();
    let document_hash = agent_doc_hash::document_id_for_path(std::path::Path::new(file));
    if !projection.tracks_document(&document_hash) {
        return None;
    }
    Some(projection.document_in_flight(&document_hash))
}

/// Process liveness used by reliable-sync registration pruning. This is process
/// identity support for Lazily's liveness facts, not a second authority model.
#[cfg(unix)]
pub fn process_pid_is_live(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
    ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
pub fn process_pid_is_live(_pid: u32) -> bool {
    true
}

/// Codec token carried in the envelope (`IpcCodec::MessagePack.name()`).
const MSGPACK_CODEC: &str = "msgpack";

/// The NDJSON envelope wrapping one lazily reliable-sync frame.
///
/// Serializes with an internally-tagged `type` field so it coexists with every
/// other message on the `agent-doc-ipc-io` socket (`patch`, queue convergence,
/// compatibility publish requests, …). Non-reliable-sync messages never parse into this
/// struct, so [`decode_envelope`] can cheaply reject them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliableSyncEnvelope {
    /// Constant `"reliable_sync"`.
    #[serde(rename = "type")]
    pub message_type: String,
    /// Per-document channel tag — the isolation boundary (`#lzsync` invariant).
    pub document_hash: String,
    /// Frame codec token; currently always `"msgpack"`.
    pub codec: String,
    /// Base64 of the codec-encoded [`IpcMessage`].
    pub frame: String,
}

/// Encode `message` for `document_hash` into the shared-socket JSON envelope.
///
/// The frame body is msgpack (the decided cross-language wire codec). The
/// returned value is ready to hand to [`agent_doc_ipc_io::send_message`].
pub fn encode_envelope(document_hash: &str, message: &IpcMessage) -> Result<serde_json::Value> {
    let frame_bytes = IpcCodec::MessagePack
        .encode(message)
        .map_err(|e| anyhow!("reliable-sync msgpack encode failed: {e}"))?;
    let envelope = ReliableSyncEnvelope {
        message_type: RELIABLE_SYNC_MESSAGE_TYPE.to_string(),
        document_hash: document_hash.to_string(),
        codec: MSGPACK_CODEC.to_string(),
        frame: BASE64.encode(frame_bytes),
    };
    serde_json::to_value(&envelope).context("reliable-sync envelope to_value failed")
}

/// Decode a shared-socket message back into `(document_hash, IpcMessage)`.
///
/// Returns:
/// - `None` — the message is **not** a reliable-sync frame (wrong/absent
///   `type`); the caller routes it through the normal socket handler.
/// - `Some(Err)` — it *is* a reliable-sync frame but is malformed (bad codec,
///   base64, or msgpack); the caller must surface the error, never silently drop.
/// - `Some(Ok((hash, msg)))` — a well-formed frame ready to feed a
///   `ResyncCoordinator` / `SyncDriver`.
pub fn decode_envelope(message: &serde_json::Value) -> Option<Result<(String, IpcMessage)>> {
    // Cheap `type` gate before attempting a full deserialize.
    if message.get("type").and_then(|v| v.as_str()) != Some(RELIABLE_SYNC_MESSAGE_TYPE) {
        return None;
    }
    Some(decode_envelope_inner(message))
}

fn decode_envelope_inner(message: &serde_json::Value) -> Result<(String, IpcMessage)> {
    let envelope: ReliableSyncEnvelope = serde_json::from_value(message.clone())
        .context("reliable-sync envelope deserialize failed")?;
    if envelope.codec != MSGPACK_CODEC {
        return Err(anyhow!(
            "reliable-sync unsupported frame codec {:?} (expected {MSGPACK_CODEC})",
            envelope.codec
        ));
    }
    let frame_bytes = BASE64
        .decode(envelope.frame.as_bytes())
        .context("reliable-sync frame base64 decode failed")?;
    let message = IpcCodec::MessagePack
        .decode(&frame_bytes)
        .map_err(|e| anyhow!("reliable-sync msgpack decode failed: {e}"))?;
    Ok((envelope.document_hash, message))
}

/// The byte-level send half — abstracts *which* socket a frame goes out on so
/// the sink is unit-testable without a live controller.
pub trait EnvelopeTransport {
    /// Deliver one already-built reliable-sync envelope. A returned `Err` means
    /// the frame was **not** durably handed to the peer; the driver keeps it in
    /// the outbox for replay-on-reconnect (at-least-once).
    fn send_envelope(&self, envelope: &serde_json::Value) -> Result<()>;
}

/// lazily [`IpcSink`] over an [`EnvelopeTransport`] for one document channel.
///
/// This is the plugin→controller push side (Phase 3C wires a `SyncDriver` onto
/// it). Every `send` msgpack-encodes the frame, tags it with `document_hash`,
/// and hands it to the transport; a transport error surfaces as the sink error
/// so the driver retains the frame.
pub struct ReliableSyncSink<T: EnvelopeTransport> {
    transport: T,
    document_hash: String,
}

impl<T: EnvelopeTransport> ReliableSyncSink<T> {
    pub fn new(transport: T, document_hash: impl Into<String>) -> Self {
        Self {
            transport,
            document_hash: document_hash.into(),
        }
    }

    pub fn document_hash(&self) -> &str {
        &self.document_hash
    }
}

impl<T: EnvelopeTransport> IpcSink for ReliableSyncSink<T> {
    type Error = anyhow::Error;

    fn send(&mut self, message: &IpcMessage) -> Result<(), Self::Error> {
        let envelope = encode_envelope(&self.document_hash, message)?;
        self.transport.send_envelope(&envelope)
    }
}

/// The delivery half of a reliable-sync inbox: the controller listener pushes a
/// decoded frame here; the paired [`QueueIpcSource`] drains it for the driver.
///
/// Cloneable so one document channel can be fed from any listener handler
/// thread. Delivery after the source is dropped is a no-op (the peer went away),
/// logged rather than panicking.
#[derive(Clone)]
pub struct ReliableSyncInbox {
    document_hash: String,
    tx: Sender<IpcMessage>,
}

impl ReliableSyncInbox {
    /// Deliver a decoded frame to the paired source. Returns `false` if the
    /// source was dropped (channel closed).
    pub fn deliver(&self, message: IpcMessage) -> bool {
        match self.tx.send(message) {
            Ok(()) => true,
            Err(_) => {
                eprintln!(
                    "[reliable-sync-io] {}: inbox delivery dropped — source closed",
                    self.document_hash
                );
                false
            }
        }
    }

    pub fn document_hash(&self) -> &str {
        &self.document_hash
    }
}

/// lazily [`IpcSource`] backed by an in-memory queue fed by a [`ReliableSyncInbox`].
///
/// Non-blocking by contract: `recv` returns `Ok(None)` when the queue is
/// currently empty (or the inbox has been dropped and drained), matching the
/// `IpcSource` "currently exhausted or closed" semantics the `SyncDriver`
/// expects — the driver polls, it does not block a thread on the source.
pub struct QueueIpcSource {
    document_hash: String,
    rx: Receiver<IpcMessage>,
}

impl QueueIpcSource {
    pub fn document_hash(&self) -> &str {
        &self.document_hash
    }
}

impl IpcSource for QueueIpcSource {
    type Error = anyhow::Error;

    fn recv(&mut self) -> Result<Option<IpcMessage>, Self::Error> {
        match self.rx.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(TryRecvError::Empty) => Ok(None),
            // A disconnected-but-drained inbox is "closed", not an error: the
            // peer stopped pushing. The driver treats `None` as no-progress.
            Err(TryRecvError::Disconnected) => Ok(None),
        }
    }
}

/// Build a connected inbox/source pair for one `document_hash` channel.
///
/// The controller registers the [`ReliableSyncInbox`] in its per-document
/// routing map (keyed by the envelope `document_hash`) and hands the
/// [`QueueIpcSource`] to that document's driver.
pub fn reliable_sync_channel(
    document_hash: impl Into<String>,
) -> (ReliableSyncInbox, QueueIpcSource) {
    let document_hash = document_hash.into();
    let (tx, rx) = channel();
    (
        ReliableSyncInbox {
            document_hash: document_hash.clone(),
            tx,
        },
        QueueIpcSource { document_hash, rx },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazily::{Delta, Snapshot};
    use parking_lot::Mutex;

    fn sample_messages() -> Vec<IpcMessage> {
        vec![
            IpcMessage::Snapshot(Snapshot::new(0, Vec::new(), Vec::new(), Vec::new())),
            IpcMessage::Delta(Delta {
                base_epoch: 3,
                epoch: 5,
                ops: Vec::new(),
            }),
            IpcMessage::ResyncRequest(lazily::ResyncRequest { from_epoch: 7 }),
            IpcMessage::OutboxAck(lazily::OutboxAck { through_epoch: 9 }),
        ]
    }

    #[test]
    fn envelope_roundtrips_every_variant() {
        for msg in sample_messages() {
            let value = encode_envelope("doc-abc", &msg).expect("encode");
            assert_eq!(value["type"], RELIABLE_SYNC_MESSAGE_TYPE);
            assert_eq!(value["document_hash"], "doc-abc");
            assert_eq!(value["codec"], MSGPACK_CODEC);
            let (hash, decoded) = decode_envelope(&value)
                .expect("is reliable-sync")
                .expect("ok");
            assert_eq!(hash, "doc-abc");
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn decode_ignores_non_reliable_sync_messages() {
        let patch = serde_json::json!({ "type": "apply_canonical", "id": "x" });
        assert!(decode_envelope(&patch).is_none());
        let untyped = serde_json::json!({ "document_hash": "d" });
        assert!(decode_envelope(&untyped).is_none());
    }

    #[test]
    fn decode_rejects_malformed_frame() {
        let bad_codec = serde_json::json!({
            "type": RELIABLE_SYNC_MESSAGE_TYPE,
            "document_hash": "d",
            "codec": "json",
            "frame": "AAAA",
        });
        assert!(
            decode_envelope(&bad_codec)
                .expect("is reliable-sync")
                .is_err()
        );

        let bad_b64 = serde_json::json!({
            "type": RELIABLE_SYNC_MESSAGE_TYPE,
            "document_hash": "d",
            "codec": MSGPACK_CODEC,
            "frame": "not base64!!!",
        });
        assert!(
            decode_envelope(&bad_b64)
                .expect("is reliable-sync")
                .is_err()
        );
    }

    /// Collecting fake transport: proves the sink builds a routable envelope
    /// that decodes back to the exact frame, with no live socket.
    #[derive(Default)]
    struct CollectingTransport {
        sent: Mutex<Vec<serde_json::Value>>,
        fail: bool,
    }

    impl EnvelopeTransport for CollectingTransport {
        fn send_envelope(&self, envelope: &serde_json::Value) -> Result<()> {
            if self.fail {
                return Err(anyhow!("transport down"));
            }
            self.sent.lock().push(envelope.clone());
            Ok(())
        }
    }

    #[test]
    fn sink_encodes_and_transport_carries_every_frame() {
        let transport = CollectingTransport::default();
        let mut sink = ReliableSyncSink::new(transport, "doc-xyz");
        let msgs = sample_messages();
        for msg in &msgs {
            sink.send(msg).expect("send ok");
        }
        // Re-borrow the transport via the sink to inspect what it carried.
        let sent = {
            // SAFETY: sink owns the transport; expose it for the assertion.
            let ReliableSyncSink { transport, .. } = &sink;
            transport.sent.lock().clone()
        };
        assert_eq!(sent.len(), msgs.len());
        for (value, expected) in sent.iter().zip(&msgs) {
            let (hash, decoded) = decode_envelope(value).unwrap().unwrap();
            assert_eq!(hash, "doc-xyz");
            assert_eq!(&decoded, expected);
        }
    }

    #[test]
    fn sink_surfaces_transport_failure_for_driver_retain() {
        let transport = CollectingTransport {
            fail: true,
            ..Default::default()
        };
        let mut sink = ReliableSyncSink::new(transport, "doc-xyz");
        let err = sink.send(&IpcMessage::Snapshot(Snapshot::new(
            0,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )));
        assert!(
            err.is_err(),
            "transport failure must surface for retain-on-fail"
        );
    }

    #[test]
    fn channel_source_drains_in_order_then_empty() {
        let (inbox, mut source) = reliable_sync_channel("doc-1");
        assert_eq!(source.document_hash(), "doc-1");
        let msgs = sample_messages();
        for msg in &msgs {
            assert!(inbox.deliver(msg.clone()));
        }
        for expected in &msgs {
            let got = source.recv().expect("recv ok").expect("some");
            assert_eq!(&got, expected);
        }
        // Drained → None, not an error.
        assert!(source.recv().expect("recv ok").is_none());
    }

    #[test]
    fn source_reports_none_after_inbox_dropped() {
        let (inbox, mut source) = reliable_sync_channel("doc-2");
        inbox.deliver(IpcMessage::OutboxAck(lazily::OutboxAck {
            through_epoch: 1,
        }));
        drop(inbox);
        // Buffered frame still drains...
        assert!(source.recv().expect("recv").is_some());
        // ...then a dropped inbox reads as closed (None), never an error.
        assert!(source.recv().expect("recv").is_none());
    }

    #[test]
    fn deliver_after_source_dropped_is_false_not_panic() {
        let (inbox, source) = reliable_sync_channel("doc-3");
        drop(source);
        assert!(!inbox.deliver(IpcMessage::OutboxAck(lazily::OutboxAck {
            through_epoch: 1
        })));
    }
}

/// End-to-end push loop through the *real* lazily `SyncDriver`, this crate's
/// `ReliableSyncSink` carrier, and the liveness projection — the Phase 3C proof
/// that a liveness push lost while the controller is down is re-sent from the
/// outbox on reconnect (at-least-once), and the controller's derived open-set
/// converges. SimWorld over mocks (deterministic doubles, no real socket).
#[cfg(test)]
mod push_loop_simworld {
    use super::liveness::{
        LivenessOp, LivenessProjection, decode_liveness_frame, encode_liveness_frame,
    };
    use super::{EnvelopeTransport, ReliableSyncSink, decode_envelope, reliable_sync_channel};
    use anyhow::{Result, anyhow};
    use lazily::{Clock, InMemoryOutbox, IpcMessage, Snapshot, SnapshotProvider, SyncDriver};
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeSet;
    use std::rc::Rc;

    /// In-memory carrier with a controllable `connected` flag: flipping it off
    /// makes `send` fail, standing in for a downed controller socket.
    #[derive(Clone)]
    struct WireTransport {
        delivered: Rc<RefCell<Vec<serde_json::Value>>>,
        connected: Rc<Cell<bool>>,
    }

    impl EnvelopeTransport for WireTransport {
        fn send_envelope(&self, env: &serde_json::Value) -> Result<()> {
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

    // Pure-liveness channel never gaps a Delta, so the provider is never invoked.
    struct TrivialProvider;
    impl SnapshotProvider for TrivialProvider {
        fn snapshot(&self, from_epoch: u64) -> IpcMessage {
            IpcMessage::Snapshot(Snapshot::new(from_epoch, vec![], vec![], vec![]))
        }
    }

    /// Controller receive path: decode each delivered socket envelope → liveness
    /// frame → fold into the projection.
    fn fold_delivered(delivered: &[serde_json::Value], proj: &mut LivenessProjection) {
        for env in delivered {
            let (_hash, msg) = decode_envelope(env)
                .expect("delivered a reliable-sync envelope")
                .expect("envelope decodes");
            if let Some(ops) = decode_liveness_frame(&msg) {
                proj.apply_batch(&ops.expect("liveness frame decodes"));
            }
        }
    }

    fn open(doc: &str, pid: u64, tag: &str) -> IpcMessage {
        encode_liveness_frame(&[LivenessOp::Open {
            document_hash: doc.into(),
            pid,
            tag: tag.into(),
        }])
        .expect("encode liveness frame")
    }

    #[test]
    fn liveness_survives_disconnect_reconnect_at_least_once() {
        let delivered = Rc::new(RefCell::new(Vec::new()));
        let connected = Rc::new(Cell::new(true));
        let transport = WireTransport {
            delivered: delivered.clone(),
            connected: connected.clone(),
        };
        let sink = ReliableSyncSink::new(transport, "docwire");
        // Sender's inbound channel (acks/resync) — empty for this push-only test.
        let (_inbox, source) = reliable_sync_channel("docwire");
        let mut driver = SyncDriver::new(
            sink,
            source,
            InMemoryOutbox::default(),
            FixedClock,
            TrivialProvider,
        );

        driver.enqueue(1, open("docA", 100, "t1"));
        driver.enqueue(2, open("docB", 100, "t2"));

        // Controller down: the first send fails, the frame is retained in the
        // outbox (append-before-send), and the driver stalls.
        connected.set(false);
        driver.tick().expect("tick");
        assert!(driver.is_stalled(), "a failed send stalls the driver");
        assert!(
            delivered.borrow().is_empty(),
            "nothing reaches a downed controller"
        );

        // Controller back: replay the unacked outbox suffix, then drain the rest.
        connected.set(true);
        driver.on_reconnect();
        driver.tick().expect("tick");

        let mut proj = LivenessProjection::new();
        fold_delivered(&delivered.borrow(), &mut proj);
        let expected: BTreeSet<String> = ["docA", "docB"].map(String::from).into_iter().collect();
        assert_eq!(
            proj.open_docs(),
            expected,
            "both liveness frames converge after reconnect — the disconnect-lost frame was replayed"
        );
    }

    #[test]
    fn redelivered_frames_are_idempotent() {
        let delivered = Rc::new(RefCell::new(Vec::new()));
        let connected = Rc::new(Cell::new(true));
        let transport = WireTransport {
            delivered: delivered.clone(),
            connected,
        };
        let sink = ReliableSyncSink::new(transport, "docwire");
        let (_inbox, source) = reliable_sync_channel("docwire");
        let mut driver = SyncDriver::new(
            sink,
            source,
            InMemoryOutbox::default(),
            FixedClock,
            TrivialProvider,
        );
        driver.enqueue(1, open("docA", 100, "t1"));
        driver.tick().expect("tick");

        let mut proj = LivenessProjection::new();
        // Fold the same delivery twice (a re-delivered frame) — must be a no-op.
        fold_delivered(&delivered.borrow(), &mut proj);
        fold_delivered(&delivered.borrow(), &mut proj);
        assert!(proj.is_open("docA", 100));
        assert_eq!(proj.open_docs().len(), 1);
    }
}
