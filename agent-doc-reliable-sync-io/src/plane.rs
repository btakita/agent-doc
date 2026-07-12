//! Controller-side reliable-sync liveness plane + the dual-run parity oracle
//! (sidecar-retirement Phase 3C — the controller *receive* half).
//!
//! [`ControllerLivenessPlane`] is what the controller RPC handler folds inbound
//! reliable-sync liveness frames into: it owns the [`LivenessProjection`] the
//! cutover will read *instead of* scanning `.agent-doc/live-buffer/*` +
//! `plugin-owner/*.json`, plus a per-`document_hash` ack cursor returned to the
//! pushing plugin (over the request receipt) so the plugin's [`crate::ReliableSyncSink`]
//! + `DurableOutbox` can prune / resume from the frontier.
//!
//! [`SidecarOpenSetModel`] is the **parity oracle**: a straightforward model of
//! what today's sidecar scan derives for the same open / close / crash sequence.
//! The migration invariant (plan § Migration & cutover) is that the synced plane
//! and the sidecar model agree across every open/close/crash/recycle sequence
//! *before* the hot path is switched — the `parity` tests below are that SimWorld.

use anyhow::Result;
use lazily::IpcMessage;
use std::collections::{BTreeMap, BTreeSet};

use crate::liveness::{LivenessOp, LivenessProjection, Pid, decode_liveness_frame};

/// The controller's receive-side reliable-sync plane for editor liveness.
///
/// Fold inbound frames with [`ingest`](Self::ingest); read the derived authority
/// off [`projection`](Self::projection). Idempotent: a re-delivered frame (outbox
/// replay after a reconnect) folds to the same state and re-advances the same
/// cursor, so a mid-push controller recycle recovers exactly by replaying the
/// plugin's retained outbox suffix.
#[derive(Debug, Default)]
pub struct ControllerLivenessPlane {
    projection: LivenessProjection,
    /// Highest epoch applied per `document_hash` channel — the ack cursor the
    /// receipt returns so the plugin outbox prunes / resumes.
    cursors: BTreeMap<String, u64>,
}

impl ControllerLivenessPlane {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one inbound reliable-sync frame received at `epoch` on the
    /// `document_hash` channel, returning the channel's ack cursor for the receipt.
    ///
    /// A non-liveness frame (a `CrdtSync` without a liveness op, or a
    /// `Snapshot`/`Delta`) is ignored for the projection but still advances the
    /// cursor so the sender can prune it (it was received). A malformed liveness
    /// frame is surfaced as an error (never a silent drop) and does **not** advance
    /// the cursor, so the sender replays it.
    pub fn ingest(&mut self, document_hash: &str, epoch: u64, message: &IpcMessage) -> Result<u64> {
        if let Some(decoded) = decode_liveness_frame(message) {
            let ops = decoded?; // malformed → propagate, cursor unchanged
            self.projection.apply_batch(&ops);
        }
        let cursor = self.cursors.entry(document_hash.to_string()).or_insert(0);
        if epoch > *cursor {
            *cursor = epoch;
        }
        Ok(*cursor)
    }

    /// Fold a **locally-originated** liveness op (not received over the wire) —
    /// e.g. the controller's own OS exit watcher writing `Alive{value:false}` for a
    /// dead editor pid. No epoch/ack cursor is touched (there is no remote sender to
    /// resume); the LWW/OR-set join keeps it convergent with the pushed frames.
    pub fn apply_local(&mut self, op: &LivenessOp) {
        self.projection.apply(op);
    }

    /// The derived-authority projection (open-set / live-docs / per-pid alive).
    pub fn projection(&self) -> &LivenessProjection {
        &self.projection
    }

    /// The ack cursor for `document_hash` (0 if nothing seen).
    pub fn ack_cursor(&self, document_hash: &str) -> u64 {
        self.cursors.get(document_hash).copied().unwrap_or(0)
    }

    /// Rebuild after a controller recycle by re-folding the plugin's replayed
    /// outbox suffix. Equivalent to constructing a fresh plane and `ingest`ing the
    /// replayed frames — exposed so a recycle path reads intentionally.
    pub fn recycle() -> Self {
        Self::new()
    }
}

/// Reference model of the open-set the **sidecars** derive today — the dual-run
/// parity oracle. `open` mirrors a present `.agent-doc/live-buffer/<doc>` for an
/// editor `pid`; `dead` mirrors a pid the OS-exit watcher marked gone.
///
/// This is a test/verification oracle, not a hot-path type; it deliberately uses
/// the *same* derivation rules as [`LivenessProjection`] so any divergence is a
/// real semantic gap, not an apples-to-oranges artifact.
#[derive(Debug, Default)]
pub struct SidecarOpenSetModel {
    open: BTreeSet<(String, Pid)>,
    dead: BTreeSet<Pid>,
}

impl SidecarOpenSetModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Editor `pid` wrote a live-buffer sidecar for `doc` (opened it).
    pub fn open(&mut self, doc: &str, pid: Pid) {
        self.open.insert((doc.to_string(), pid));
    }

    /// Editor `pid`'s live-buffer sidecar for `doc` was reaped (closed).
    pub fn close(&mut self, doc: &str, pid: Pid) {
        self.open.remove(&(doc.to_string(), pid));
    }

    /// The OS-exit watcher marked `pid` gone (whole-editor death).
    pub fn crash(&mut self, pid: Pid) {
        self.dead.insert(pid);
    }

    /// Docs with a present sidecar (open-set ground truth), independent of alive.
    pub fn open_docs(&self) -> BTreeSet<String> {
        self.open.iter().map(|(doc, _)| doc.clone()).collect()
    }

    /// Docs with a present sidecar held by a still-alive pid (live aggregate).
    pub fn live_docs(&self) -> BTreeSet<String> {
        self.open
            .iter()
            .filter(|(_, pid)| !self.dead.contains(pid))
            .map(|(doc, _)| doc.clone())
            .collect()
    }
}

#[cfg(test)]
mod parity {
    use super::*;
    use crate::liveness::{LivenessOp, encode_liveness_frame};
    use lazily::WireStamp;

    /// One migration event applied to *both* the synced plane and the sidecar
    /// oracle, so a divergence is caught immediately.
    #[derive(Clone)]
    enum Event {
        Open {
            doc: &'static str,
            pid: Pid,
            tag: &'static str,
        },
        Close {
            doc: &'static str,
            pid: Pid,
            observed: Vec<&'static str>,
        },
        Crash {
            pid: Pid,
            wall: u64,
        },
    }

    fn liveness_ops(ev: &Event) -> Vec<LivenessOp> {
        match ev {
            Event::Open { doc, pid, tag } => vec![LivenessOp::Open {
                document_hash: (*doc).into(),
                pid: *pid,
                tag: (*tag).into(),
            }],
            Event::Close { doc, pid, observed } => vec![LivenessOp::Close {
                document_hash: (*doc).into(),
                pid: *pid,
                observed_tags: observed.iter().map(|t| (*t).to_string()).collect(),
            }],
            Event::Crash { pid, wall } => vec![LivenessOp::Alive {
                pid: *pid,
                value: false,
                stamp: WireStamp {
                    wall_time: *wall,
                    logical: 0,
                    peer: 1,
                },
            }],
        }
    }

    fn apply_to_sidecar(model: &mut SidecarOpenSetModel, ev: &Event) {
        match ev {
            Event::Open { doc, pid, .. } => model.open(doc, *pid),
            Event::Close { doc, pid, .. } => model.close(doc, *pid),
            Event::Crash { pid, .. } => model.crash(*pid),
        }
    }

    /// Push every event through the plane as a reliable-sync liveness frame at an
    /// increasing epoch, retaining each frame so a recycle can replay it.
    fn drive(
        plane: &mut ControllerLivenessPlane,
        model: &mut SidecarOpenSetModel,
        events: &[Event],
        retained: &mut Vec<(u64, IpcMessage)>,
    ) {
        for (i, ev) in events.iter().enumerate() {
            let epoch = (i + 1) as u64;
            let frame = encode_liveness_frame(&liveness_ops(ev)).expect("encode");
            plane.ingest("docwire", epoch, &frame).expect("ingest");
            retained.push((epoch, frame));
            apply_to_sidecar(model, ev);
            // Parity holds after every single event, not just at the end.
            assert_eq!(
                plane.projection().open_docs(),
                model.open_docs(),
                "open-set diverged at event {i}"
            );
            assert_eq!(
                plane.projection().live_docs(),
                model.live_docs(),
                "live-docs diverged at event {i}"
            );
        }
    }

    #[test]
    fn synced_plane_matches_sidecar_across_open_close_crash() {
        let events = vec![
            Event::Open {
                doc: "docA",
                pid: 100,
                tag: "a1",
            },
            Event::Open {
                doc: "docB",
                pid: 100,
                tag: "b1",
            },
            Event::Open {
                doc: "docC",
                pid: 200,
                tag: "c1",
            },
            Event::Close {
                doc: "docB",
                pid: 100,
                observed: vec!["b1"],
            },
            Event::Crash { pid: 100, wall: 50 },
        ];
        let mut plane = ControllerLivenessPlane::new();
        let mut model = SidecarOpenSetModel::new();
        let mut retained = Vec::new();
        drive(&mut plane, &mut model, &events, &mut retained);
        // Final: docA held by dead pid100 → not live; docB closed; docC live.
        assert_eq!(
            plane.projection().live_docs(),
            ["docC"].map(String::from).into_iter().collect()
        );
    }

    #[test]
    fn recycle_rebuilds_the_plane_from_replayed_outbox() {
        let events = vec![
            Event::Open {
                doc: "docA",
                pid: 100,
                tag: "a1",
            },
            Event::Open {
                doc: "docC",
                pid: 200,
                tag: "c1",
            },
            Event::Crash { pid: 100, wall: 50 },
        ];
        let mut plane = ControllerLivenessPlane::new();
        let mut model = SidecarOpenSetModel::new();
        let mut retained = Vec::new();
        drive(&mut plane, &mut model, &events, &mut retained);
        let before = plane.projection().live_docs();

        // Controller recycles mid-stream: projection is lost; the plugin replays
        // its retained (un-acked) outbox suffix from the frontier.
        let mut recovered = ControllerLivenessPlane::recycle();
        for (epoch, frame) in &retained {
            recovered.ingest("docwire", *epoch, frame).expect("replay");
        }
        assert_eq!(
            recovered.projection().live_docs(),
            before,
            "recycle + outbox replay rebuilds the exact derived authority"
        );
        assert_eq!(recovered.projection().open_docs(), model.open_docs());
    }

    #[test]
    fn redelivered_frames_keep_parity_and_advance_cursor_monotonically() {
        let frame = encode_liveness_frame(&[LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "a1".into(),
        }])
        .unwrap();
        let mut plane = ControllerLivenessPlane::new();
        assert_eq!(plane.ingest("docwire", 1, &frame).unwrap(), 1);
        // Re-deliver epoch 1 (an outbox replay): idempotent, cursor stays 1.
        assert_eq!(plane.ingest("docwire", 1, &frame).unwrap(), 1);
        assert!(plane.projection().is_open("docA", 100));
        assert_eq!(plane.projection().open_docs().len(), 1);
    }
}
