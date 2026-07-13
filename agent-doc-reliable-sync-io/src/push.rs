//! Plugin-side reliable-sync liveness push endpoint (sidecar-retirement Phase 3C
//! — the editor-plugin *send* half).
//!
//! The editor plugins are thin event reporters: they call an FFI that turns
//! open/close/attach/crash events into [`LivenessOp`]s and hands them to a
//! [`LivenessPushEndpoint`]. The endpoint owns the durability + at-least-once
//! push loop so the plugins never have to (the FFI-first Shared Foundation rule):
//!
//! 1. **enqueue** — assign the next epoch and [`append`](lazily::DurableOutbox::append)
//!    the frame to the [`DurableOutbox`] **before** any send (so a crash between
//!    append and send still delivers it later);
//! 2. **flush** — replay every un-acked frame from the outbox through the injected
//!    [`LivenessPushTransport`] (in production: a `reliable_sync` controller RPC),
//!    prune on the returned ack cursor, and **retain-and-stop** on a transport
//!    failure so a push lost while the controller is down is re-sent on the next
//!    flush / reconnect.
//!
//! This is the send-side mirror of [`crate::plane::ControllerLivenessPlane`];
//! together they are the full plugin→controller push loop, proven end to end in
//! the SimWorld below. The transport + outbox are injected so the loop is
//! deterministic-testable without a live socket or SQLite (production plugs a
//! `SqliteOutbox` + the RPC client; the FFI + JNA/JS glue + the S4b exit-watcher
//! hookup are the operator-verified remainder).

use anyhow::Result;
use lazily::DurableOutbox;

use crate::encode_envelope;
use crate::liveness::{LivenessOp, encode_liveness_frame};

/// The byte-level push half: send one reliable-sync envelope and return the
/// controller's ack cursor. Injected so the endpoint is testable without the
/// live controller RPC.
pub trait LivenessPushTransport {
    /// Push `envelope` (a [`crate::encode_envelope`] value) for `document_hash` at
    /// `epoch`; on success return the controller's ack cursor (highest epoch it has
    /// applied on this channel). An `Err` means the frame was **not** delivered —
    /// the endpoint retains it in the outbox for replay.
    fn push(&self, document_hash: &str, epoch: u64, envelope: &serde_json::Value) -> Result<u64>;
}

/// What one [`LivenessPushEndpoint::flush`] accomplished.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushProgress {
    /// Frames delivered + acked this flush.
    pub sent: usize,
    /// The highest ack cursor observed this flush.
    pub acked_through: u64,
    /// Frames still un-acked in the outbox (retained for replay).
    pub retained: usize,
    /// True if the flush stopped early because the transport failed (retain-and-stall).
    pub stalled: bool,
}

/// Plugin-side durable liveness push endpoint for one `document_hash` channel.
pub struct LivenessPushEndpoint<O: DurableOutbox> {
    document_hash: String,
    outbox: O,
    next_epoch: u64,
}

/// Durable endpoint for an already-encoded reliable-sync frame. Document-op and
/// bounded text-adopt pushes use this sibling so they share the same ordered
/// append-before-send protocol without pretending to be liveness operations.
pub struct FramePushEndpoint<O: DurableOutbox> {
    document_hash: String,
    outbox: O,
    next_epoch: u64,
}

impl<O: DurableOutbox> FramePushEndpoint<O> {
    pub fn resuming(document_hash: impl Into<String>, outbox: O, acked_through: u64) -> Self {
        let highest_retained = outbox.retained_epochs().into_iter().max().unwrap_or(0);
        Self {
            document_hash: document_hash.into(),
            outbox,
            next_epoch: acked_through.max(highest_retained).saturating_add(1).max(1),
        }
    }

    /// Persist one frame before any transport attempt.
    pub fn enqueue_frame(&mut self, frame: lazily::IpcMessage) -> u64 {
        let epoch = self.next_epoch;
        self.next_epoch = self.next_epoch.saturating_add(1);
        self.outbox.append(epoch, frame);
        epoch
    }

    /// Replay the unacked suffix in order, pruning only through a controller ack.
    pub fn flush<T: LivenessPushTransport>(&mut self, transport: &T) -> Result<PushProgress> {
        let mut progress = PushProgress::default();
        for (epoch, frame) in self.outbox.replay_from(0) {
            let envelope = encode_envelope(&self.document_hash, &frame)?;
            match transport.push(&self.document_hash, epoch, &envelope) {
                Ok(ack) => {
                    self.outbox.ack_through(ack);
                    progress.acked_through = progress.acked_through.max(ack);
                    progress.sent += 1;
                }
                Err(error) => {
                    eprintln!(
                        "[reliable-sync-push] {}: retain epoch {epoch} after transport failure: {error:#}",
                        self.document_hash
                    );
                    progress.stalled = true;
                    break;
                }
            }
        }
        progress.retained = self.outbox.retained_epochs().len();
        Ok(progress)
    }
}

impl<O: DurableOutbox> LivenessPushEndpoint<O> {
    /// Build an endpoint whose next enqueued frame takes `next_epoch`.
    ///
    /// `next_epoch` MUST be greater than every epoch ever used on this channel
    /// (including already-acked-and-pruned ones), or the controller's monotonic
    /// cursor will ignore the re-used epoch. After a recycle, compute it from the
    /// durable store as `max(acked_through, highest_retained) + 1`; a fresh channel
    /// starts at 1.
    pub fn with_next_epoch(document_hash: impl Into<String>, outbox: O, next_epoch: u64) -> Self {
        Self {
            document_hash: document_hash.into(),
            outbox,
            next_epoch: next_epoch.max(1),
        }
    }

    /// A fresh endpoint at epoch 1.
    pub fn new(document_hash: impl Into<String>, outbox: O) -> Self {
        Self::with_next_epoch(document_hash, outbox, 1)
    }

    /// Build an endpoint that resumes the epoch counter past every epoch the
    /// channel has ever used — the highest still-retained frame *and* the caller's
    /// `acked_through` cursor (acked frames are pruned, so `retained_epochs` alone
    /// would forget them). Use after a plugin/controller recycle so a new event
    /// never re-uses an epoch the controller cursor would ignore.
    pub fn resuming(document_hash: impl Into<String>, outbox: O, acked_through: u64) -> Self {
        let highest_retained = outbox.retained_epochs().into_iter().max().unwrap_or(0);
        let next_epoch = acked_through.max(highest_retained) + 1;
        Self::with_next_epoch(document_hash, outbox, next_epoch)
    }

    pub fn document_hash(&self) -> &str {
        &self.document_hash
    }

    /// The epoch the next [`enqueue`](Self::enqueue) will use.
    pub fn next_epoch(&self) -> u64 {
        self.next_epoch
    }

    /// Borrow the durable outbox (diagnostics / recycle-cursor computation).
    pub fn outbox(&self) -> &O {
        &self.outbox
    }

    /// Stage a liveness batch: assign the next epoch and durably append the frame
    /// **before** any send. Returns the assigned epoch.
    pub fn enqueue(&mut self, ops: &[LivenessOp]) -> Result<u64> {
        let epoch = self.next_epoch;
        self.next_epoch += 1;
        let frame = encode_liveness_frame(ops)?;
        self.outbox.append(epoch, frame);
        Ok(epoch)
    }

    /// Push every un-acked frame (oldest first) through `transport`, pruning on the
    /// returned ack cursor. On a transport failure the current + remaining frames
    /// stay in the outbox and the flush stops (retain-and-stall) — the caller
    /// retries on its own cadence / on reconnect. A transport error is **not** a
    /// flush error (the controller being down is expected); only an internal encode
    /// failure returns `Err`.
    pub fn flush<T: LivenessPushTransport>(&mut self, transport: &T) -> Result<PushProgress> {
        let mut progress = PushProgress::default();
        // Acked frames are pruned from the outbox, so `replay_from(0)` is exactly
        // the un-acked suffix in ascending epoch order.
        for (epoch, frame) in self.outbox.replay_from(0) {
            let envelope = encode_envelope(&self.document_hash, &frame)?;
            match transport.push(&self.document_hash, epoch, &envelope) {
                Ok(ack) => {
                    self.outbox.ack_through(ack);
                    progress.acked_through = progress.acked_through.max(ack);
                    progress.sent += 1;
                }
                Err(e) => {
                    eprintln!(
                        "[reliable-sync-push] {}: transport failed at epoch {epoch} — retaining for replay: {e}",
                        self.document_hash
                    );
                    progress.stalled = true;
                    break;
                }
            }
        }
        progress.retained = self.outbox.retained_epochs().len();
        Ok(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_envelope;
    use crate::plane::ControllerLivenessPlane;
    use lazily::InMemoryOutbox;
    use std::cell::{Cell, RefCell};

    /// A fake transport that folds pushed frames into a real
    /// [`ControllerLivenessPlane`] and returns its ack cursor — the plugin→controller
    /// loop end to end. `connected` off makes `push` fail (controller down).
    struct PlaneTransport {
        plane: RefCell<ControllerLivenessPlane>,
        connected: Cell<bool>,
    }

    impl PlaneTransport {
        fn new() -> Self {
            Self {
                plane: RefCell::new(ControllerLivenessPlane::new()),
                connected: Cell::new(true),
            }
        }
    }

    impl LivenessPushTransport for PlaneTransport {
        fn push(
            &self,
            _document_hash: &str,
            epoch: u64,
            envelope: &serde_json::Value,
        ) -> Result<u64> {
            if !self.connected.get() {
                return Err(anyhow::anyhow!("controller socket down"));
            }
            let (doc, message) = decode_envelope(envelope)
                .expect("reliable-sync envelope")
                .expect("decodes");
            self.plane.borrow_mut().ingest(&doc, epoch, &message)
        }
    }

    fn open(doc: &str, pid: u64, tag: &str) -> Vec<LivenessOp> {
        vec![LivenessOp::Open {
            document_hash: doc.into(),
            pid,
            tag: tag.into(),
        }]
    }

    #[test]
    fn push_delivers_folds_into_plane_and_prunes() {
        let transport = PlaneTransport::new();
        let mut endpoint = LivenessPushEndpoint::new("docwire", InMemoryOutbox::default());
        endpoint.enqueue(&open("docA", 100, "a1")).unwrap();
        endpoint.enqueue(&open("docB", 100, "b1")).unwrap();

        let progress = endpoint.flush(&transport).unwrap();
        assert_eq!(progress.sent, 2);
        assert!(!progress.stalled);
        assert_eq!(progress.retained, 0, "acked frames are pruned");
        // The controller plane derived the open-set from the pushed liveness.
        assert_eq!(
            transport.plane.borrow().projection().open_docs(),
            ["docA", "docB"].map(String::from).into_iter().collect()
        );
    }

    #[test]
    fn push_retains_on_failure_then_replays_on_reconnect() {
        let transport = PlaneTransport::new();
        let mut endpoint = LivenessPushEndpoint::new("docwire", InMemoryOutbox::default());
        endpoint.enqueue(&open("docA", 100, "a1")).unwrap();

        // Controller down: retain, stall, nothing folded.
        transport.connected.set(false);
        let progress = endpoint.flush(&transport).unwrap();
        assert!(progress.stalled);
        assert_eq!(progress.sent, 0);
        assert_eq!(progress.retained, 1);
        assert!(transport.plane.borrow().projection().open_docs().is_empty());

        // Controller back: the retained frame replays and folds.
        transport.connected.set(true);
        let progress = endpoint.flush(&transport).unwrap();
        assert_eq!(progress.sent, 1);
        assert_eq!(progress.retained, 0);
        assert!(transport.plane.borrow().projection().is_open("docA", 100));
    }

    #[test]
    fn recycle_reuses_outbox_and_does_not_reuse_epochs() {
        // Simulate a durable outbox surviving a recycle by moving it into a new
        // endpoint with the correct resumed next-epoch.
        let mut outbox = InMemoryOutbox::default();
        // Frame at epoch 1 is enqueued but never acked (controller was down).
        outbox.append(1, encode_liveness_frame(&open("docA", 100, "a1")).unwrap());

        // Recover: next epoch is highest-retained + 1 = 2 (never re-use epoch 1).
        let resumed_next = outbox.retained_epochs().iter().copied().max().unwrap_or(0) + 1;
        let mut endpoint = LivenessPushEndpoint::with_next_epoch("docwire", outbox, resumed_next);
        assert_eq!(endpoint.next_epoch(), 2);

        // A new event after recovery takes epoch 2, and the un-acked epoch-1 frame
        // still replays.
        endpoint.enqueue(&open("docB", 100, "b1")).unwrap();
        let transport = PlaneTransport::new();
        let progress = endpoint.flush(&transport).unwrap();
        assert_eq!(progress.sent, 2);
        assert_eq!(
            transport.plane.borrow().projection().open_docs(),
            ["docA", "docB"].map(String::from).into_iter().collect()
        );
    }

    #[test]
    fn redelivered_push_is_idempotent_end_to_end() {
        let transport = PlaneTransport::new();
        let mut endpoint = LivenessPushEndpoint::new("docwire", InMemoryOutbox::default());
        endpoint.enqueue(&open("docA", 100, "a1")).unwrap();
        endpoint.flush(&transport).unwrap();
        // A spurious second flush (nothing left) is a no-op; the plane is unchanged.
        let progress = endpoint.flush(&transport).unwrap();
        assert_eq!(progress.sent, 0);
        assert_eq!(transport.plane.borrow().projection().open_docs().len(), 1);
    }
}
