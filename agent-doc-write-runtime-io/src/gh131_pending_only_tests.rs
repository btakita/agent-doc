//! GH #131 (`#trackedrepairterminates`): a `--pending-only` retention beside an
//! already-committed response is absorbed into its commit continuation.

use super::*;

fn seed_reliable_sync_open(file: &Path, tag: &str) {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    agent_doc_reliable_sync_io::global_liveness_plane()
        .lock()
        .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
            document_hash,
            pid: std::process::id().into(),
            tag: tag.to_string(),
        }]);
}

/// GH #131 shape 1, on real state: `session-check` named
/// `write --done <id> --pending-only --commit`, the envelope reached the
/// editor authority, and the post-write barrier refused with the
/// converging-projection deferral. On 0.35.453 that refusal reached the
/// agent (whose remedy sent it back to `session-check`) and — because only
/// retry-without-disk refusals recorded one — left no commit continuation
/// for the retained `--done`. Beside an already-committed response the
/// write must instead record the continuation and absorb the retention.
#[test]
fn pending_only_retention_beside_committed_response_is_absorbed() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let file = dir.path().join("session.md");
    let committed = "---\nagent_doc_session: gh131\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n### Re: sweep — gpt-5\n\nThe committed response.\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n- [ ] [#turnleasesweep] Sweep turn leases\n<!-- /agent:backlog -->\n";
    std::fs::write(&file, committed).unwrap();
    agent_doc_crdt_relay_io::register_embedded_relay_route_for_file(&file).unwrap();
    let file = file.canonicalize().unwrap();

    agent_doc_cycle_state_io::start_preflight(&file, Some(committed), Some(committed)).unwrap();
    agent_doc_cycle_state_io::mark_committed(
        &file,
        "commit_success",
        Some(committed),
        Some(committed),
    )
    .unwrap();
    assert!(
        !agent_doc_cycle_state_io::load_with_closeout_projection(&file)
            .unwrap()
            .expect("cycle state")
            .phase
            .is_open(),
        "test setup: the response cycle is committed"
    );

    // A live editor replica that has not acknowledged delivery yet.
    let identity = "gh131-pending-only-absorb";
    seed_reliable_sync_open(&file, identity);
    agent_doc_crdt_relay_io::register_replica_for_file(&file, identity)
        .unwrap()
        .expect("editor replica should attach");
    let prior = agent_doc_document_realtime_io::pending_document_write(&file)
        .map(|pending| pending.intent_id);

    // The `--done` envelope reaches the editor authority and is retained...
    let done_target = committed.replace("- [ ] [#turnleasesweep] Sweep turn leases\n", "");
    agent_doc_crdt_relay_io::apply_cp_write_for_file(
        &file,
        committed,
        &done_target,
        "pending_status_write",
    )
    .unwrap();
    agent_doc_document_realtime_io::retain_deferred_document_write_target(
        &file,
        committed,
        &done_target,
        "pending_status_write",
        agent_doc_state_backbone::DocumentWriteDeferredReason::EditorProjectionPending,
    )
    .unwrap();
    // ...and the real post-write barrier refuses with the deferral.
    let error = agent_doc_document_realtime_io::guard_visible_delivery_convergence(
        &file,
        "pending_status_write",
    )
    .expect_err("test setup: an unacknowledged delivery must refuse");
    let message = format!("{error:#}");
    assert!(
        agent_doc_turn::write_ownership::is_retained_delivery_projection_pending(&message),
        "test setup: the converging-projection refusal: {message}"
    );

    // A retained intent that predates this envelope is not this write's.
    let existing = agent_doc_document_realtime_io::pending_document_write(&file)
        .map(|pending| pending.intent_id)
        .expect("retained intent");
    assert_eq!(
        absorb_retained_pending_only_mutation(
            &file,
            CommitMode::BestEffort,
            &error,
            Some(existing.as_str()),
        )
        .unwrap(),
        None,
        "an intent this envelope did not create proves nothing"
    );
    assert!(
        !await_settled_pending_only_mutation(
            &file,
            &error,
            false,
            &["turnleasesweep".to_string()],
            &[],
            &[],
        )
        .unwrap(),
        "an intent that is still observable must not masquerade as the settled race"
    );

    let absorbed = absorb_retained_pending_only_mutation(
        &file,
        CommitMode::BestEffort,
        &error,
        prior.as_deref(),
    )
    .unwrap();
    let target_hash = agent_doc_hash::content_hash(&done_target);
    assert_eq!(absorbed.as_deref(), Some(target_hash.as_str()));
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(&file)
        .unwrap()
        .expect("cycle state");
    assert_eq!(
        state.pending_only_commit_target_hash.as_deref(),
        Some(target_hash.as_str()),
        "session-check must find the continuation that commits the retained --done"
    );
    let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("pending_only_mutation_absorbed_by_retained_continuation"),
        "{log}"
    );
}

/// The controller may settle and remove the retained intent in the narrow
/// interval between the delivery barrier constructing its refusal and the
/// pending-only error handler inspecting that intent. The refusal still proves
/// the envelope reached a converging delivery projection; once the ordinary
/// delivery and tracked-work witnesses agree it landed, the command must
/// continue its commit tail instead of returning that stale refusal.
#[test]
fn pending_only_retention_settled_before_inspection_continues_after_landing_proof() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let file = dir.path().join("session.md");
    let landed = "---\nagent_doc_session: gh131-settled\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n### Re: sweep — gpt-5\n\nThe committed response.\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
    std::fs::write(&file, landed).unwrap();
    let file = file.canonicalize().unwrap();

    agent_doc_cycle_state_io::start_preflight(&file, Some(landed), Some(landed)).unwrap();
    agent_doc_cycle_state_io::mark_committed(
        &file,
        "commit_success",
        Some(landed),
        Some(landed),
    )
    .unwrap();
    agent_doc_cycle_state_io::record_requested_tracked_work(
        &file,
        &["turnleasesweep".to_string()],
        &[],
    )
    .unwrap();
    agent_doc_cycle_state_io::record_requested_tracked_work_mutations(&file).unwrap();

    assert!(
        agent_doc_document_realtime_io::pending_document_write(&file).is_none(),
        "test setup: the controller already settled and removed the intent"
    );
    let error = anyhow::anyhow!(format!(
        "{}; {}",
        agent_doc_turn::write_ownership::AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN,
        agent_doc_turn::write_ownership::RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN,
    ));

    assert_eq!(
        agent_doc_turn::write_ownership::pending_only_retention(
            &format!("{error:#}"),
            true,
            false,
            false,
        ),
        agent_doc_turn::write_ownership::PendingOnlyRetention::AwaitSettledDelivery,
    );
    agent_doc_cycle_state_io::mark_tracked_work_mutations_retained(&file).unwrap();
    assert_eq!(
        recorded_tracked_work_unlanded_now(&file, false, true),
        Some(false),
        "the shared landing witness accepts the retained envelope only after delivery"
    );
    finish_settled_pending_only_mutation(
        &file,
        &["turnleasesweep".to_string()],
        &[],
        &[],
    )
    .unwrap();
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(&file)
        .unwrap()
        .expect("cycle state");
    assert!(state.tracked_work_mutations_retained);
    assert_eq!(state.pending_done_ids, ["turnleasesweep"]);
    let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("pending_only_mutation_settled_before_error_inspection"),
        "{log}"
    );
}
