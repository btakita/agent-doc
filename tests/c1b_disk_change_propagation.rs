//! Integration: the C1b disk-change → CPC-replica reconcile vertical, end-to-end
//! in one process, against a genuinely **editor-attached** document.
//!
//! Editor-attachment is real, not mocked: the test folds an Open fact into the
//! reliable-sync OR-set, so `authority_for_file` resolves to `MultiReplica` and a
//! canonical `RelayHub` is allocated with a registered editor replica — exactly
//! the state a live JetBrains/VS Code plugin establishes. It then drives the
//! shipped producer/consumer:
//!
//!   route_disk_change_signal (watch-daemon side, uses decide_watch_action)
//!     -> `.agent-doc/disk-change-requests/<hash>.json` marker
//!     -> consume_disk_change_reconcile (supervisor idle-loop side)
//!     -> apply_disk_change_for_file -> RelayHub::apply_disk_change
//!
//! Demonstrates goals 4/5: an out-of-band disk change reconciles into the CPC
//! canonical replica, idempotently (already-present → no-op) and safely
//! (out-of-band deletion → rebuild + editors flagged for re-bootstrap).

use agent_doc_crdt_relay_io as relay;
use agent_doc_document_realtime::crdt_relay::DiskChangeOutcome;
use agent_doc_document_realtime::watch_authority::{WatchAction, WatchDelivery};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Build a temp project with a tracked, **editor-attached** document seeded from
/// `body`: publish a reliable-sync Open fact, register an editor replica in the
/// hub, and record the on-disk baseline.
fn attached_doc(name: &str, body: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let file = dir.path().join(name);
    fs::write(&file, body).unwrap();

    // Editor-attached = a durable-plane Open fact held by this live process.
    let document_hash = agent_doc_hash::document_id_for_path(&file);
    agent_doc_reliable_sync_io::global_liveness_plane()
        .lock()
        .unwrap()
        .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
            document_hash,
            pid: std::process::id().into(),
            tag: format!("c1b-integration:{}", file.display()),
        }]);

    // Allocate the canonical hub + a registered editor replica, then record the
    // committed baseline so an out-of-band correction is detectable.
    let registered = relay::register_replica_for_file(&file, "editor:c1b")
        .expect("register_replica_for_file ok")
        .expect("an editor-attached document allocates a hub + replica");
    let _ = registered; // (client_id, bootstrap_state)
    relay::record_committed_baseline_for_file(&file);

    (dir, file)
}

#[test]
fn additive_change_reconciles_as_idempotent_noop() {
    let (_dir, file) = attached_doc("plan.md", "# Plan\n\nbody\n");

    // Producer: a settled change on an editor-attached doc is routed to canonical
    // and drops a reconcile marker.
    let action = relay::route_disk_change_signal(&file, &WatchDelivery::Change { generation: 1 })
        .expect("route_disk_change_signal ok");
    assert_eq!(action, WatchAction::ReconcileIntoCanonical);
    assert!(
        relay::disk_change_request_pending(&file),
        "an editor-attached change drops a reconcile marker"
    );

    // Consumer: canonical was seeded from the same text, so disk == canonical →
    // the "editor already has it" reconcile is an idempotent no-op.
    let outcome = relay::consume_disk_change_reconcile(&file).expect("consume ok");
    assert_eq!(outcome, Some(DiskChangeOutcome::AlreadyReconciled));
    assert!(
        !relay::disk_change_request_pending(&file),
        "the marker is consumed exactly once"
    );

    // Consuming again with no marker is a no-op.
    assert_eq!(relay::consume_disk_change_reconcile(&file).unwrap(), None);
}

#[test]
fn out_of_band_deletion_rebuilds_canonical_and_flags_editors() {
    let (_dir, file) = attached_doc("plan.md", "# Plan\n\nGOOD\nCORRUPT-BLOCK\n");

    // Operator corrects the file on disk out of band (drops the corrupt block) —
    // a deletion the additive CRDT delta cannot express.
    fs::write(&file, "# Plan\n\nGOOD\n").unwrap();

    relay::route_disk_change_signal(&file, &WatchDelivery::Change { generation: 1 })
        .expect("route ok");
    let outcome = relay::consume_disk_change_reconcile(&file).expect("consume ok");

    match outcome {
        Some(DiskChangeOutcome::RebuiltFromDisk { live_members }) => assert!(
            live_members >= 1,
            "the attached editor is flagged for a replace-capable re-bootstrap (D2)"
        ),
        other => panic!("expected RebuiltFromDisk, got {other:?}"),
    }
    assert!(!relay::disk_change_request_pending(&file));
}

#[test]
fn headless_document_gets_no_marker() {
    // No reliable-sync Open fact → not editor-attached → the disk-authority load path
    // owns the change and no marker is dropped for a supervisor to consume.
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let file = dir.path().join("headless.md");
    fs::write(&file, "# Headless\n\nbody\n").unwrap();

    let action = relay::route_disk_change_signal(&file, &WatchDelivery::Change { generation: 1 })
        .expect("route ok");
    assert_eq!(action, WatchAction::ApplyAsDiskAuthority);
    assert!(!relay::disk_change_request_pending(&file));
}
