//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
#[cfg(test)]
use agent_doc_document_realtime::write_policy::response_already_in_current;
#[cfg(test)]
use agent_doc_element_exchange::extract_post_commit_normalization_targets;
#[cfg(test)]
use agent_doc_element_exchange::normalize_exchange_prefixes_for_targets;
#[cfg(test)]
use agent_doc_element_exchange::verify_sidecar_normalization;
#[cfg(test)]
use agent_doc_ipc_protocol::{IpcDiskRepairReason, IpcRepairDecision, IpcSnapshotSource};
#[cfg(test)]
use agent_doc_write_converge_io::{
    guard_ipc_snapshot_adoption_against_live_prompt_drift, ipc_repair_decision_from_visible_write,
    log_ipc_snapshot_adoption_allowed, log_ipcfullprompt_corruption_if_any,
    materialize_missing_response_for_socket_visible_write_drift,
    poll_visible_write_content_lazily_event,
    reconcile_visible_write_snapshot_to_newer_operator_buffer, visible_write_disk_proof,
};
#[cfg(test)]
use agent_doc_write_converge_io::{
    ipc_direct_disk_degraded, record_ipc_socket_ack_timeout, try_semantic_merge_convergence,
};

#[cfg(test)]
pub(crate) fn record_lazily_visible_write_candidate(file: &Path, patch_id: &str, content: &str) {
    std::fs::write(file, content).unwrap();
    let file_key = file.to_string_lossy();
    let _ = agent_doc_debounce::record_live_buffer_synced_content_for_editor_with_capabilities(
        file_key.as_ref(),
        content,
        "test-editor",
        "test",
        "test",
        &[
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
        ],
    );
    agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
        file,
        patch_id,
        content,
        "unit_test",
    )
    .unwrap();
}

#[cfg(test)]
pub(crate) fn content_ours_with_pending_from_disk(file: &Path, content_ours: &str) -> String {
    match std::fs::read_to_string(file) {
        Ok(on_disk_content) => {
            let spliced = splice_pending_component(content_ours, &on_disk_content);
            if let Some(warning) = spliced.warning.as_ref() {
                log_splice_pending_component_warning(warning);
            }
            spliced.content
        }
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to read {} while preserving pending mutations during normalization fallback: {}",
                file.display(),
                e
            );
            content_ours.to_string()
        }
    }
}

#[cfg(test)]
pub(crate) fn content_ours_merged_with_disk_edits(
    file: &Path,
    baseline: Option<&str>,
    content_ours: &str,
) -> String {
    let Some(base) = baseline else {
        return content_ours_with_pending_from_disk(file, content_ours);
    };
    let Ok(on_disk_content) = std::fs::read_to_string(file) else {
        return content_ours_with_pending_from_disk(file, content_ours);
    };
    if strip_boundary_for_dedup(&on_disk_content) == strip_boundary_for_dedup(content_ours) {
        return content_ours.to_string();
    }
    if response_already_in_current(base, content_ours, &on_disk_content) {
        eprintln!(
            "[write] normalization fallback: response delta already in current file; adopting current content"
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "sidecar_normalization_fallback_adopted_current_delta file={} delta=response_contained",
                file.display()
            ),
        );
        return on_disk_content;
    }

    let base_state = match agent_doc_snapshot_io::crdt_merge_base_state_with(
        file,
        base,
        agent_doc_op_capture_io::has_pending_editor_ops,
        agent_doc_ops_log_io::log_op,
    ) {
        Ok(base) => base.state,
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to load overlay CRDT merge base, falling back to baseline text: {}",
                e
            );
            agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state()
        }
    };
    match agent_doc_merge_io::merge_contents_crdt_with_ops(
        file,
        Some(&base_state),
        content_ours,
        &on_disk_content,
        agent_doc_ops_log_io::log_op,
    ) {
        Ok((merged, _)) => merged,
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to merge current disk edits into normalization fallback: {}",
                e
            );
            content_ours_with_pending_from_disk(file, content_ours)
        }
    }
}

#[cfg(test)]
pub(crate) fn normalized_content_ours_fallback(
    file: &Path,
    baseline: Option<&str>,
    content_ours: &str,
    normalize_prefix_lines: &[String],
) -> String {
    let fallback = content_ours_merged_with_disk_edits(file, baseline, content_ours);
    let normalized = normalize_exchange_prefixes_for_targets(&fallback, normalize_prefix_lines);
    agent_doc_element_exchange_io::repair_duplicate_prompt_artifacts_with_log(
        &normalized,
        file,
        DuplicatePromptRepairOptions::new("normalization_fallback")
            .with_before(baseline)
            .preserving(baseline)
            .without_residue_guard(),
        agent_doc_ops_log_io::log_op,
        log_duplicate_prompt_residue_guard,
    )
    .map(|(repaired, _)| repaired)
    .unwrap_or(normalized)
}

#[cfg(test)]
mod transport;
#[cfg(test)]
pub(crate) use transport::try_ipc;

#[cfg(test)]
mod visible_write_content_snapshot_tests {
    use super::*;
    use tempfile::TempDir;

    fn redeliver_normalization_fallback_for_test(
        file: &Path,
        repaired_content: &str,
        expected_bad_state: &str,
        normalize_prefix_lines: &[String],
        source_patch_id: Option<&str>,
    ) -> bool {
        agent_doc_write_converge_io::redeliver_normalization_fallback_to_editor(
            file,
            repaired_content,
            expected_bad_state,
            normalize_prefix_lines,
            source_patch_id,
            |file, repaired_content, expected_bad_state, source_patch_id| {
                agent_doc_write_converge_io::redeliver_full_content_repair_to_editor(
                    file,
                    repaired_content,
                    expected_bad_state,
                    agent_doc_ipc_protocol::FullContentRepairRedelivery::NormalizationFallback,
                    source_patch_id,
                    &mut |file, repaired_content, expected_bad_state| {
                        agent_doc_write_converge_io::try_ipc_full_content_response_fallback_from_source(
                            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
                            file,
                            repaired_content,
                            expected_bad_state,
                        )
                    },
                )
            },
        )
    }

    fn wait_until_typing_indicator_active(file: &str) {
        for _ in 0..100 {
            if agent_doc_debounce::is_typing_via_file(file, 2_000) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("typing indicator did not become active for {file}");
    }

    fn record_lazily_visible_write_candidate(file: &Path, patch_id: &str, content: &str) {
        std::fs::write(file, content).unwrap();
        let file_key = file.to_string_lossy();
        let _ = agent_doc_debounce::record_live_buffer_synced_content_for_editor_with_capabilities(
            file_key.as_ref(),
            content,
            "test-editor",
            "test",
            "test",
            &[
                agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
                agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
            ],
        );
        agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
            file,
            patch_id,
            content,
            "unit_test",
        )
        .unwrap();
    }

    #[test]
    fn test_lazily_visible_write_event_read() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "test-patch-abc123";

        std::fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc = project_root.join("session.md");
        record_lazily_visible_write_candidate(&doc, patch_id, "applied content from plugin");

        let result = poll_visible_write_content_lazily_event(
            &doc,
            patch_id,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(10),
        )
        .unwrap();
        assert_eq!(result, Some("applied content from plugin".to_string()));
        assert!(
            agent_doc_controller_io::project_controller::visible_write_commit_candidate_for_patch_file(
                &doc, patch_id
            )
            .is_some(),
            "lazily visible-write proof should remain durable"
        );
    }

    #[test]
    fn visible_write_disk_proof_waits_for_typing_indicator_before_trusting_live_buffer() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/crdt")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/typing")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let editor_id = "jetbrains:test";
        let visible_write_content = "before\n### Re: done\n";
        let newer_editor_content = "before\n### Re: done\noperator typed after ack\n";

        agent_doc_test_support::publish_editor_text_via_crdt_relay(
            &doc,
            editor_id,
            visible_write_content,
        );
        agent_doc_debounce::document_changed(&doc_str);
        wait_until_typing_indicator_active(&doc_str);

        let updater_doc = doc.clone();
        let updater = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(25));
            agent_doc_test_support::publish_editor_text_via_crdt_relay(
                &updater_doc,
                editor_id,
                newer_editor_content,
            );
        });

        let proof = visible_write_disk_proof(&doc, Some(editor_id), visible_write_content);
        updater.join().unwrap();

        assert_eq!(
            proof.authority,
            agent_doc_document_realtime::write_policy::WholeBufferAuthority::OperatorTextAuthority
        );
        assert!(
            !proof.source_buffer_matches,
            "visible-write must not remain authoritative after active typing publishes a newer editor buffer"
        );
        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_disk_proof_typing_settle"),
            "typing-settle proof should be auditable:\n{log}"
        );
    }

    #[test]
    fn reconcile_visible_write_snapshot_forward_adopts_newer_operator_buffer_presenting_response() {
        // #adoc-live-prompt-drift-operator-edit: the ack snapshot the closeout is
        // about to persist is stale (agent's exact body) relative to the operator's
        // newer live buffer (edited body, response still present). Reconcile forward
        // to the newer operator-authoritative buffer instead of wedging.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let editor_id = "jetbrains:test";
        let stale_ack = "<!-- agent:exchange -->\n❯ fold it in\n### Re: folded — opus\n\nAgent exact body.\n<!-- /agent:exchange -->\n";
        let newer = "<!-- agent:exchange -->\n❯ fold it in\n### Re: folded — opus\n\nOperator reworded body, happy 4th.\n<!-- /agent:exchange -->\n";

        agent_doc_test_support::publish_editor_text_via_crdt_relay(&doc, editor_id, newer);

        let mut decision = IpcRepairDecision::lazily_visible_write(stale_ack.to_string());
        let reconciled = reconcile_visible_write_snapshot_to_newer_operator_buffer(
            &doc,
            Some(editor_id),
            &mut decision,
        );

        assert!(
            reconciled,
            "newer operator buffer presenting the response must reconcile forward"
        );
        assert_eq!(
            decision.snapshot_content, newer,
            "snapshot must adopt the operator's newer buffer so disk/snapshot/CRDT stay consistent"
        );
        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_snapshot_reconciled_forward"),
            "forward reconcile must be auditable:\n{log}"
        );
    }

    #[test]
    fn reconcile_visible_write_snapshot_forward_declines_when_newer_buffer_dropped_the_response() {
        // Fail closed: the newer buffer no longer carries the response, so the stale
        // ack must NOT reconcile forward (the existing proof stays authoritative).
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let editor_id = "jetbrains:test";
        let stale_ack = "<!-- agent:exchange -->\n❯ fold it in\n### Re: folded — opus\n\nAgent exact body.\n<!-- /agent:exchange -->\n";
        let newer_without_response =
            "<!-- agent:exchange -->\n❯ fold it in\n<!-- /agent:exchange -->\n";

        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            newer_without_response,
            editor_id,
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let mut decision = IpcRepairDecision::lazily_visible_write(stale_ack.to_string());
        let reconciled = reconcile_visible_write_snapshot_to_newer_operator_buffer(
            &doc,
            Some(editor_id),
            &mut decision,
        );

        assert!(
            !reconciled,
            "must not reconcile forward when the response was dropped"
        );
        assert_eq!(
            decision.snapshot_content, stale_ack,
            "snapshot unchanged on decline"
        );
    }

    // --- #dupcontent: structurally-corrupt content_ours is never adopted ---

    const DC_BASELINE: &str = "<!-- agent:status -->\nA\n<!-- /agent:status -->\n<!-- agent:exchange -->\nq\n<!-- /agent:exchange -->\n";
    const DC_CANDIDATE: &str = "<!-- agent:status -->\nB\n<!-- /agent:status -->\n<!-- agent:exchange -->\nq\n<!-- /agent:exchange -->\n";

    #[test]
    fn guard_refuses_structurally_corrupt_content_ours() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        // The live #dupcontent corruption: two agent:queue blocks ingested from
        // a bad CRDT merge — must never become the snapshot base.
        let corrupt_ours = "<!-- agent:status -->\nC\n<!-- /agent:status -->\n\
<!-- agent:exchange -->\nq\n<!-- /agent:exchange -->\n\
<!-- agent:queue -->\n- a\n<!-- /agent:queue -->\n\
<!-- agent:queue -->\n- b\n<!-- /agent:queue -->\n";
        let mut decision = IpcRepairDecision::file_read(DC_CANDIDATE.to_string());
        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("p1"),
            Some(DC_BASELINE),
            Some(corrupt_ours),
            &mut decision,
        );
        assert!(!adopted, "corrupt content_ours must be refused");
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::FileRead,
            "decision must keep the clean candidate, not adopt the corrupt buffer"
        );
        assert_eq!(
            decision.snapshot_content, DC_CANDIDATE,
            "snapshot content must remain the clean candidate"
        );
    }

    #[test]
    fn guard_replaces_visible_write_candidate_when_agent_target_would_absorb_drift() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let baseline = concat!("<!-- agent:exchange -->\n", "<!-- /agent:exchange -->\n");
        let live_visible_write_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ operator typed while closeout was running\n",
            "<!-- /agent:exchange -->\n"
        );
        let agent_target = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: queued prompt — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision =
            IpcRepairDecision::lazily_visible_write(live_visible_write_content.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "socket_visible_write_content",
            Some("p-live-no-snapshot"),
            Some(baseline),
            Some(agent_target),
            &mut decision,
        );

        assert!(
            adopted,
            "live prompt drift should still be classified and logged"
        );
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::ContentOurs,
            "the visible-write candidate must not remain snapshot authority when it would absorb drift"
        );
        assert_eq!(decision.snapshot_content, agent_target);
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::LivePromptDrift)
        );
        assert!(decision.redeliver_editor);
        assert!(decision.editor_bad_state.is_some());
    }

    #[test]
    fn guard_live_prompt_drift_reconciles_independent_visible_prompt() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let baseline = concat!("<!-- agent:exchange -->\n", "<!-- /agent:exchange -->\n");
        let editor_visible_write_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ operator typed while closeout was running\n",
            "<!-- /agent:exchange -->\n"
        );
        let response_target = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: queued prompt — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision =
            IpcRepairDecision::lazily_visible_write(editor_visible_write_content.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "socket_visible_write_content",
            Some("p-live"),
            Some(baseline),
            Some(response_target),
            &mut decision,
        );

        assert!(
            adopted,
            "live editor visible-write drift should be classified and logged"
        );
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::ContentOurs,
            "the turn path may keep only the agent-owned target as snapshot authority before repair proof"
        );
        assert_eq!(decision.snapshot_content, response_target);
        assert!(
            decision.redeliver_editor,
            "the visible candidate is missing the response and must require repair"
        );
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::LivePromptDrift)
        );
        assert!(decision.editor_bad_state.is_some());
    }

    #[test]
    fn guard_live_prompt_drift_preserves_visible_write_union_with_response() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let baseline = concat!("<!-- agent:exchange -->\n", "<!-- /agent:exchange -->\n");
        let response_target = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: queued prompt — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let editor_visible_write_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ operator typed while closeout was running\n",
            "### Re: queued prompt — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision =
            IpcRepairDecision::lazily_visible_write(editor_visible_write_content.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "socket_visible_write_content",
            Some("p-union"),
            Some(baseline),
            Some(response_target),
            &mut decision,
        );

        assert!(
            adopted,
            "visible-write union still contains unowned live prompt drift and must be classified"
        );
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::LazilyVisibleWriteEvent,
            "the response-bearing visible-write candidate remains editor-buffer authority"
        );
        assert_eq!(decision.snapshot_content, editor_visible_write_content);
        assert!(
            !decision.redeliver_editor,
            "the lazily receipt already proves the editor-visible buffer contains the response"
        );
        assert_eq!(decision.disk_repair_reason, None);
        assert!(decision.editor_bad_state.is_none());
    }

    #[test]
    fn socket_visible_write_drift_missing_response_materializes_only_with_content_ours_proof() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let visible_write_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let response = "### Re: queued prompt - gpt-5\n\nAnswered.\n";
        let content_ours = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "### Re: queued prompt - gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision =
            IpcRepairDecision::lazily_visible_write(visible_write_content.to_string());

        let repaired = materialize_missing_response_for_socket_visible_write_drift(
            &file,
            Some("p-response"),
            Some(content_ours),
            response,
            true,
            &mut decision,
        );

        assert!(
            repaired,
            "socket visible-write drift should repair only when content_ours proves the exact response"
        );
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::LazilyVisibleWriteEvent
        );
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::LivePromptDrift)
        );
        assert!(decision.redeliver_editor);
        assert_eq!(
            decision
                .editor_bad_state
                .as_ref()
                .expect("bad state fingerprint")
                .content(),
            visible_write_content
        );
        assert!(decision.snapshot_content.contains("❯ queued prompt"));
        assert!(
            response_materialized_in_content(response, &decision.snapshot_content),
            "repaired snapshot must contain the exact response body:\n{}",
            decision.snapshot_content
        );
    }

    #[test]
    fn socket_visible_write_drift_missing_response_materializes_over_prefix_repair() {
        // #ackdriftprefixmaterialize regression: a stale lazily visible-write event that went
        // through a prefix-divergence repair (`disk_repair_reason =
        // Some(PrefixDivergence)`) but is still missing the agent response must
        // NOT be blocked from the missing-response materialization rescue. Before
        // the fix, the blanket `disk_repair_reason.is_some()` bail left the stale
        // snapshot in place and the cycle dead-ended into a
        // `retry_without_disk_write` spin until supervisor supersession.
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let original_bad_state = concat!(
            "<!-- agent:exchange -->\n",
            "queued prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        // Prefix-normalized snapshot (the `❯ ` marker restored) but still no
        // response body — exactly what the sidecar prefix repair produces.
        let prefix_repaired = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let response = "### Re: queued prompt - gpt-5\n\nAnswered.\n";
        let content_ours = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "### Re: queued prompt - gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let prefix_lines = vec!["❯ queued prompt".to_string()];
        let mut decision = IpcRepairDecision::lazily_visible_write_prefix_repair(
            prefix_repaired.to_string(),
            original_bad_state.to_string(),
            &prefix_lines,
        );
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergence)
        );

        let repaired = materialize_missing_response_for_socket_visible_write_drift(
            &file,
            Some("p-prefix"),
            Some(content_ours),
            response,
            true,
            &mut decision,
        );

        assert!(
            repaired,
            "a prefix-diverged visible-write receipt still missing the response must materialize it from content_ours proof"
        );
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::LazilyVisibleWriteEvent
        );
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::LivePromptDrift)
        );
        assert!(decision.redeliver_editor);
        // The originally-recorded bad state (what the editor actually holds — the
        // un-normalized buffer) must survive so redelivery verifies against it,
        // not against the undelivered prefix-repaired image.
        assert_eq!(
            decision
                .editor_bad_state
                .as_ref()
                .expect("bad state fingerprint")
                .content(),
            original_bad_state
        );
        assert!(
            response_materialized_in_content(response, &decision.snapshot_content),
            "repaired snapshot must contain the exact response body:\n{}",
            decision.snapshot_content
        );
    }

    #[test]
    fn socket_visible_write_drift_missing_response_refuses_partial_heading() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let partial_visible_write_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "### Re: queued prompt - gpt-5\n",
            "<!-- /agent:exchange -->\n"
        );
        let response = "### Re: queued prompt - gpt-5\n\nAnswered.\n";
        let content_ours = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "### Re: queued prompt - gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision =
            IpcRepairDecision::lazily_visible_write(partial_visible_write_content.to_string());

        let repaired = materialize_missing_response_for_socket_visible_write_drift(
            &file,
            Some("p-partial"),
            Some(content_ours),
            response,
            true,
            &mut decision,
        );

        assert!(
            !repaired,
            "partial response headings must fail closed instead of appending a second body"
        );
        assert_eq!(decision.snapshot_content, partial_visible_write_content);
        assert_eq!(decision.disk_repair_reason, None);
        assert!(!decision.redeliver_editor);
    }

    #[test]
    fn socket_visible_write_drift_missing_response_refuses_without_content_ours_response() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let visible_write_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let response = "### Re: queued prompt - gpt-5\n\nAnswered.\n";
        let content_ours_without_response = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision =
            IpcRepairDecision::lazily_visible_write(visible_write_content.to_string());

        let repaired = materialize_missing_response_for_socket_visible_write_drift(
            &file,
            Some("p-no-proof"),
            Some(content_ours_without_response),
            response,
            true,
            &mut decision,
        );

        assert!(
            !repaired,
            "missing content_ours response proof must fail closed"
        );
        assert_eq!(decision.snapshot_content, visible_write_content);
        assert_eq!(decision.disk_repair_reason, None);
        assert!(!decision.redeliver_editor);
    }

    // --- #smconv: node-keyed semantic-merge convergence on live drift ---
    //
    // The shipped Phase-1 `document_cell_merge` models exchange turns (and all
    // component content) as list *items* keyed by id; it reconstructs each
    // operator-skeleton component body from items and keeps only the operator's
    // non-item prose (documented assumption). These fixtures therefore use the
    // list-item exchange representation (`- re [#id] ...`) that the primitive
    // supports — the heading-prose (`### Re:`) form is exercised separately by
    // `smconv_declines_when_heading_prose_response_would_drop`, which proves the
    // conservative decline path. See the report for the representation finding.

    // base: queue:start + head [#cf-txn-email] + a prior exchange turn item.
    const SM_BASE: &str = concat!(
        "---\n",
        "session: test\n",
        "queue: start\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "- re [#cf-txn-email] prior turn\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- do [#cf-txn-email]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [#bk1] original backlog text\n",
        "<!-- /agent:backlog -->\n",
    );

    // candidate (AGENT): head struck, a NEW exchange turn appended, and the
    // backlog item edited. All node-DISJOINT from the operator's edits below.
    const SM_AGENT: &str = concat!(
        "---\n",
        "session: test\n",
        "queue: start\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "- re [#cf-txn-email] prior turn\n",
        "- re [#new-turn] implemented the cf-txn-email change and verified it\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- ~~do [#cf-txn-email]~~\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [#bk1] edited backlog text by the agent\n",
        "<!-- /agent:backlog -->\n",
    );

    // ours (OPERATOR): frontmatter flipped to queue:stop + an unrelated queue
    // line added. Disjoint from the agent's exchange/strike/backlog edits.
    const SM_OPERATOR: &str = concat!(
        "---\n",
        "session: test\n",
        "queue: stop\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "- re [#cf-txn-email] prior turn\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- do [#cf-txn-email]\n",
        "- do [#operator-unrelated]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [#bk1] original backlog text\n",
        "<!-- /agent:backlog -->\n",
    );

    #[test]
    fn smconv_disjoint_drift_merges_both_change_sets() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let mut decision = IpcRepairDecision::file_read(SM_AGENT.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("smconv"),
            Some(SM_BASE),
            Some(SM_OPERATOR),
            &mut decision,
        );

        assert!(
            adopted,
            "node-disjoint live drift must converge via semantic merge"
        );
        let merged = &decision.snapshot_content;
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::ContentOurs,
            "the merged result is installed via the content_ours snapshot slot"
        );
        assert!(
            merged.contains("[#new-turn]"),
            "merged result must preserve the agent's new exchange turn (the case that used to drop it); got:\n{merged}"
        );
        assert!(
            merged.contains("~~do [#cf-txn-email]~~"),
            "merged result must preserve the agent's queue strike; got:\n{merged}"
        );
        assert!(
            merged.contains("edited backlog text by the agent"),
            "merged result must preserve the agent's backlog edit; got:\n{merged}"
        );
        assert!(
            merged.contains("queue: stop"),
            "merged result must preserve the operator's queue: stop frontmatter flip; got:\n{merged}"
        );
        assert!(
            merged.contains("[#operator-unrelated]"),
            "merged result must preserve the operator's added queue line; got:\n{merged}"
        );
        assert!(
            element::structural_corruption_reason(merged).is_none(),
            "merged result must re-parse cleanly"
        );
    }

    #[test]
    fn smconv_preserves_freetext_fenced_queue_head_on_drift() {
        // `#qdup-freetext` (root cause of the persistent live_prompt_drift churn):
        // when the queue carries a multi-line free-text fenced head (a pasted-
        // console bug report — not a `- ` list item), the per-node reconstruction
        // used to DROP it, trip `dropped_queue_prompt_lines_after_content_ours`,
        // and decline the merge on EVERY cycle, blocking every IPC write. It must
        // now converge AND preserve the head verbatim.
        let head = concat!(
            "---\n",
            ":pushpin: JB `Run Agent Doc` did not submit.\n",
            "\n",
            "```\n",
            "claude exited cleanly.\n",
            "[agent-doc] idle-queue watch: reconciled stale busy actor to ready\n",
            "```\n",
            "---\n",
        );
        let base = format!(
            "---\nsession: test\nqueue: start\n---\n\n\
<!-- agent:exchange -->\n- re [#prior] prior turn\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n{head}- do [#a]\n<!-- /agent:queue -->\n"
        );
        // candidate (live editor buffer): head intact, agent struck the queue item
        // (an outside-exchange change, so the drift guard engages as in the real case).
        let candidate = format!(
            "---\nsession: test\nqueue: start\n---\n\n\
<!-- agent:exchange -->\n- re [#prior] prior turn\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n{head}- ~~do [#a]~~\n<!-- /agent:queue -->\n"
        );
        // content_ours: head intact, operator flipped frontmatter (disjoint node).
        let content_ours = format!(
            "---\nsession: test\nqueue: stop\n---\n\n\
<!-- agent:exchange -->\n- re [#prior] prior turn\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n{head}- do [#a]\n<!-- /agent:queue -->\n"
        );

        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let mut decision = IpcRepairDecision::file_read(candidate.clone());
        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("fthead"),
            Some(&base),
            Some(&content_ours),
            &mut decision,
        );
        assert!(
            adopted,
            "free-text-head drift must converge via semantic merge, not block forever"
        );
        let merged = &decision.snapshot_content;
        assert!(
            merged.contains(":pushpin: JB `Run Agent Doc` did not submit."),
            "free-text head line lost in merge:\n{merged}"
        );
        assert!(
            merged.contains("[agent-doc] idle-queue watch: reconciled stale busy actor to ready"),
            "fenced head body lost in merge:\n{merged}"
        );
        assert!(
            element::structural_corruption_reason(merged).is_none(),
            "merged result must re-parse cleanly"
        );
    }

    #[test]
    fn smconv_merges_heading_prose_response_preserving_both_changesets() {
        // The real-session `### Re:` heading-prose exchange turn is now modeled by
        // document_cell_merge as an append-only node (#semmerge-owner heading-prose
        // extension), so a live drift no longer drops the agent's response: the
        // node-disjoint merge applies BOTH the agent's new `### Re:` turn AND the
        // operator's concurrent frontmatter/queue edits. This is the root-cause
        // fix for the `content_ours`-drops-the-response transition.
        let base = concat!(
            "---\nsession: test\nqueue: start\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #cf-txn-email\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#cf-txn-email]\n<!-- /agent:queue -->\n",
        );
        let agent = concat!(
            "---\nsession: test\nqueue: start\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #cf-txn-email\n",
            "### Re: do #cf-txn-email — opus-4-8\n\n",
            "Implemented the cf-txn-email change and verified it end to end. This\n",
            "response body is comfortably over the stale-drift threshold so the\n",
            "live drift guard genuinely engages.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- ~~do [#cf-txn-email]~~\n<!-- /agent:queue -->\n",
        );
        let operator = concat!(
            "---\nsession: test\nqueue: stop\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #cf-txn-email\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#cf-txn-email]\n- do [#op]\n<!-- /agent:queue -->\n",
        );
        // The merge now SUCCEEDS: the agent's heading-prose turn is appended.
        let merged = try_semantic_merge_convergence(base, agent, operator)
            .expect("semantic merge must converge a heading-prose response turn now");
        let doc = &merged.merged_doc;
        assert!(
            doc.contains("### Re: do #cf-txn-email — opus-4-8")
                && doc.contains("Implemented the cf-txn-email change"),
            "merged result must preserve the agent's `### Re:` heading-prose turn; got:\n{doc}"
        );
        assert!(
            doc.contains("queue: stop"),
            "merged result must preserve the operator's frontmatter flip; got:\n{doc}"
        );
        assert!(
            doc.contains("[#op]"),
            "merged result must preserve the operator's added queue line; got:\n{doc}"
        );
        assert!(
            element::structural_corruption_reason(doc).is_none(),
            "merged result must re-parse cleanly; got:\n{doc}"
        );

        // End-to-end through the guard: it now converges via semantic merge
        // (snapshot installed from the merged doc) instead of dropping the turn.
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let mut decision = IpcRepairDecision::file_read(agent.to_string());
        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("smconv-heading"),
            Some(base),
            Some(operator),
            &mut decision,
        );
        assert!(adopted, "the guard resolves the drift");
        assert!(
            decision
                .snapshot_content
                .contains("### Re: do #cf-txn-email — opus-4-8"),
            "the installed snapshot must carry the agent's response turn; got:\n{}",
            decision.snapshot_content
        );
        assert!(
            decision.snapshot_content.contains("queue: stop"),
            "the installed snapshot must carry the operator's frontmatter flip; got:\n{}",
            decision.snapshot_content
        );
    }

    #[test]
    fn smconv_same_node_conflict_is_safe() {
        // Operator DELETED the queue item the agent struck, and operator edited
        // the backlog node the agent also edited (same-node conflict → operator
        // wins). Assert no agent exchange content (the new turn) is lost: either
        // it converges via semantic merge (preferred) or it falls through to
        // content_ours. Both are acceptable; the invariant is "no silent loss of
        // agent content" and "no panic / clean re-parse".
        let operator_conflict = concat!(
            "---\n",
            "session: test\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "- re [#cf-txn-email] prior turn\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [#bk1] operator-rewritten backlog text\n",
            "<!-- /agent:backlog -->\n",
        );
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let mut decision = IpcRepairDecision::file_read(SM_AGENT.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("smconv-conflict"),
            Some(SM_BASE),
            Some(operator_conflict),
            &mut decision,
        );

        assert!(adopted, "the conflict case must still resolve (no panic)");
        let merged = &decision.snapshot_content;
        assert!(
            element::structural_corruption_reason(merged).is_none(),
            "resolved result must re-parse cleanly"
        );
        // The agent's new exchange turn is a list item, so whichever path runs it
        // must NOT silently lose it: converged merges keep it, and the content_ours
        // fallback would only run after recording the dropped evidence.
        if decision.snap_source != IpcSnapshotSource::ContentOurs || merged != operator_conflict {
            assert!(
                merged.contains("[#new-turn]"),
                "converged merge must preserve the agent's new exchange turn; got:\n{merged}"
            );
        }
    }

    #[test]
    fn smconv_declines_on_structurally_corrupt_ours_falls_through() {
        // A structurally-corrupt operator buffer (duplicate singleton queue
        // component) must make the semantic merge decline AND the existing
        // content_ours structural-refusal guard run — the corrupt buffer never
        // becomes the snapshot, the clean candidate is kept.
        let corrupt_ours = concat!(
            "---\n",
            "session: test\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #cf-txn-email\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#cf-txn-email]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#dup]\n",
            "<!-- /agent:queue -->\n",
        );
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let mut decision = IpcRepairDecision::file_read(SM_AGENT.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("smconv-corrupt"),
            Some(SM_BASE),
            Some(corrupt_ours),
            &mut decision,
        );

        assert!(
            !adopted,
            "a structurally-corrupt operator buffer must be refused (semantic merge declines, content_ours guard refuses)"
        );
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::FileRead,
            "decision must keep the clean candidate, not adopt the corrupt buffer"
        );
        assert_eq!(decision.snapshot_content, SM_AGENT);
    }

    #[test]
    fn test_poll_lazily_event_present_immediately() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-immediate";

        std::fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc = project_root.join("session.md");
        record_lazily_visible_write_candidate(&doc, patch_id, "immediate content");

        let result = poll_visible_write_content_lazily_event(
            &doc,
            patch_id,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(10),
        )
        .unwrap();
        assert_eq!(result, Some("immediate content".to_string()));
    }

    #[test]
    fn test_poll_lazily_event_appears_after_delay() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-delayed";

        std::fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc = project_root.join("session.md");

        // Spawn a thread that records the durable lazily proof after 50ms.
        let doc_for_thread = doc.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            record_lazily_visible_write_candidate(&doc_for_thread, patch_id, "delayed content");
        });

        let result = poll_visible_write_content_lazily_event(
            &doc,
            patch_id,
            std::time::Duration::from_millis(500),
            std::time::Duration::from_millis(10),
        )
        .unwrap();
        assert_eq!(result, Some("delayed content".to_string()));
    }

    #[test]
    fn test_poll_lazily_event_timeout() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-timeout";

        // Don't record the lazily event — poll should timeout.
        std::fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc = project_root.join("session.md");
        std::fs::write(&doc, "unproven content").unwrap();

        let start = std::time::Instant::now();
        let result = poll_visible_write_content_lazily_event(
            &doc,
            patch_id,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(25),
        )
        .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result, None);
        assert!(
            elapsed >= std::time::Duration::from_millis(100),
            "should wait at least the timeout"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(300),
            "should not wait much longer than timeout"
        );
    }

    #[test]
    fn normalization_fallback_fails_closed_when_lazily_receipt_is_missing() {
        // When the plugin consumes file IPC but does not publish the lazily
        // visible-write receipt, try_ipc must not fall back to content_ours for
        // the snapshot (#jbpfx2).
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "---\nsession: test\n---\n\n<!-- agent:exchange -->\ndo #jbpfx2\n<!-- agent:boundary:test-bnd-001 -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, original).unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "agent response");

        // content_ours has the ❯ prefix, but it is only an agent-owned candidate.
        let content_ours = "---\nsession: test\n---\n\n<!-- agent:exchange -->\n❯ do #jbpfx2\nagent response\n<!-- /agent:exchange -->\n";
        let normalize_prefix_lines = vec!["do #jbpfx2".to_string()];

        // Simulate plugin consumption without publishing the durable lazily event.
        let patches_dir = agent_doc_dir.join("patches");
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap();
        assert!(
            !result.success,
            "missing lazily receipt must fail closed instead of repairing disk: {result:?}"
        );

        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            original,
            "missing lazily receipt must not trigger a direct document rewrite"
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "unproven normalization fallback must not save a snapshot"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_proof_insufficient")
                && ops_log.contains("invariant=no_lazily_visible_write_receipt")
                && ops_log.contains("recovery=retry_without_disk_write")
                && ops_log.contains("typing_status=absent")
                && ops_log.contains("operator_activity_inferred=false"),
            "ops log should record retry-only missing lazily receipt:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_divergence_repair_decision_keeps_lazily_event_authoritative() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "disk state\n").unwrap();
        let visible_write = "\
<!-- agent:exchange patch=append -->
do #sidecar
operator visible-write text
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #sidecar
agent-owned response
<!-- /agent:exchange -->
";
        let lines = vec!["do #sidecar".to_string()];

        let decision = ipc_repair_decision_from_visible_write(
            &doc,
            Some("patch-1"),
            None,
            visible_write.to_string(),
            Some(content_ours),
            Some(lines.as_slice()),
        );

        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::LazilyVisibleWriteEvent
        );
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergence)
        );
        assert!(
            decision.snapshot_content.contains("❯ do #sidecar"),
            "normalization retry may add the missing prefix to the lazily visible-write event"
        );
        assert!(
            decision
                .snapshot_content
                .contains("operator visible-write text"),
            "operator-visible lazily event text must be preserved"
        );
        assert!(
            !decision.snapshot_content.contains("agent-owned response"),
            "normalization retry must not adopt content_ours as repair authority"
        );
    }

    #[test]
    fn normalization_fallback_retries_missing_prompt_prefix_from_visible_write_event() {
        // Regression for #bppfxstrip: if visible-write verification rejects the
        // plugin snapshot, normalization retry may add normalize_prefix_lines
        // only to the lazily event candidate and must fail closed without editor
        // proof.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, original).unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let normalize_prefix_lines =
            vec!["do #bppfxstrip. spec-test-build-install-commit-push".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
                            record_lazily_visible_write_candidate(
                                &doc_for_watcher,
                                pid,
                                bad_sidecar,
                            );
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let err = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("refusing direct document write"),
            "normalization fallback must fail closed instead of repairing disk: {err}"
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("do #bppfxstrip. spec-test-build-install-commit-push")
                && !disk.contains("❯ do #bppfxstrip. spec-test-build-install-commit-push"),
            "unproven normalization fallback must leave the editor-visible lazily state untouched; got: {}",
            disk
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "unproven normalization fallback must not save a snapshot"
        );
    }

    #[test]
    fn normfallback_records_repaired_working_tree_when_lazily_event_strips_prompt_prefix() {
        // Regression for #normfallback: the observed ops-log signal should be
        // backed by deterministic coverage. A lazily visible-write event that drops a
        // required prompt prefix must be rejected, and an unproven editor repair
        // must not rewrite the live file behind the editor.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("agent-doc-bugs2.md");
        let original = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
do [#normfallback]
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, original).unwrap();

        let patch = agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: #normfallback — gpt-5\n\nCovered.",
        );
        let content_ours = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do [#normfallback]
### Re: #normfallback — gpt-5

Covered.
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let normalize_prefix_lines = vec!["do [#normfallback]".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
do [#normfallback]
### Re: #normfallback — gpt-5

Covered.
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
                            record_lazily_visible_write_candidate(
                                &doc_for_watcher,
                                pid,
                                bad_sidecar,
                            );
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let err = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("refusing direct document write"),
            "normalization fallback must fail closed instead of repairing disk: {err}"
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("do [#normfallback]") && !disk.contains("❯ do [#normfallback]"),
            "unproven normalization fallback must leave the stripped editor state untouched: {disk}"
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "unproven normalization fallback must not save a snapshot"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback")
                && ops_log.contains("reason=prefix_divergence"),
            "ops log should record why the primary sidecar snapshot was rejected:\n{ops_log}"
        );
        assert!(
            ops_log.contains("ipc_visible_repair_retry_required_no_disk_write"),
            "ops log should record retry without direct working-tree repair:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_redelivers_narrow_patch_before_full_content() {
        // A disk-only fallback can leave an editor buffer stale. If the rejected
        // editor state differs only by prompt-prefix normalization, the repair
        // should converge the editor with a narrow normalization patch.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #sidecar-diverge. spec-test-build-install-commit-push
### Re: #sidecar-diverge — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let repaired = "\
---
session: test
---

<!-- agent:exchange patch=append -->
❯ do #sidecar-diverge. spec-test-build-install-commit-push
### Re: #sidecar-diverge — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, bad_state).unwrap();
        let normalize_prefix_lines =
            vec!["do #sidecar-diverge. spec-test-build-install-commit-push".to_string()];

        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen_repair_payloads =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let listener_root = dir.path().to_path_buf();
        let listener_doc = doc.clone();
        let listener_count = call_count.clone();
        let listener_repair_payloads = seen_repair_payloads.clone();
        std::fs::create_dir_all(listener_root.join(".agent-doc")).unwrap();
        let _listener = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                listener_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let patch_id = v
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                if let Some(full_content) = v.get("fullContent").and_then(|value| value.as_str()) {
                    record_lazily_visible_write_candidate(&listener_doc, patch_id, full_content);
                    listener_repair_payloads.lock().unwrap().push(v.clone());
                    return Some(
                        serde_json::json!({"type": "receipt", "status": "applied"}).to_string(),
                    );
                }

                let patches_empty = v
                    .get("patches")
                    .and_then(|value| value.as_array())
                    .is_none_or(|patches| patches.is_empty());
                if patches_empty
                    && let Some(lines) = v.get("normalize_prefix_lines").and_then(|value| {
                        value.as_array().map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                    })
                {
                    let current = std::fs::read_to_string(&listener_doc).ok()?;
                    let repaired = normalize_exchange_prefixes_for_targets(&current, &lines);
                    record_lazily_visible_write_candidate(&listener_doc, patch_id, &repaired);
                    listener_repair_payloads.lock().unwrap().push(v.clone());
                    return Some(
                        serde_json::json!({"type": "receipt", "status": "applied"}).to_string(),
                    );
                }

                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            });
        });
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(dir.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            agent_doc_ipc_io::is_listener_active(dir.path()),
            "fake socket listener did not start"
        );

        let result = redeliver_normalization_fallback_for_test(
            &doc,
            repaired,
            bad_state,
            &normalize_prefix_lines,
            Some("source-patch"),
        );
        assert!(result, "narrow normalization repair should be delivered");

        assert!(
            call_count.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "fallback should send a narrow IPC repair"
        );
        let repair_payloads = seen_repair_payloads.lock().unwrap();
        assert_eq!(
            repair_payloads.len(),
            1,
            "expected exactly one narrow repair payload"
        );
        assert!(
            repair_payloads[0].get("fullContent").is_none(),
            "eligible prefix repair should avoid fullContent payloads: {}",
            repair_payloads[0]
        );
        assert_eq!(
            repair_payloads[0]["normalize_prefix_lines"][0],
            "do #sidecar-diverge. spec-test-build-install-commit-push"
        );
        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #sidecar-diverge. spec-test-build-install-commit-push"),
            "editor narrow repair should leave disk/editor content normalized: {disk}"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repaired_editor"),
            "ops log should record the narrow editor repair:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_file_ipc_queues_narrow_patch_before_full_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #sidecar-file. spec-test-build-install-commit-push
### Re: #sidecar-file — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let repaired = "\
---
session: test
---

<!-- agent:exchange patch=append -->
❯ do #sidecar-file. spec-test-build-install-commit-push
### Re: #sidecar-file — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, bad_state).unwrap();
        let normalize_prefix_lines =
            vec!["do #sidecar-file. spec-test-build-install-commit-push".to_string()];
        let patch_hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let patch_file = agent_doc_dir
            .join("patches")
            .join(format!("{patch_hash}.json"));

        let seen_repair_payloads =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let watcher_doc = doc.clone();
        let watcher_patch_file = patch_file.clone();
        let watcher_repair_payloads = seen_repair_payloads.clone();
        let watcher = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(3) {
                if !watcher_patch_file.exists() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                let payload_text = match std::fs::read_to_string(&watcher_patch_file) {
                    Ok(text) => text,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                };
                let payload: serde_json::Value = match serde_json::from_str(&payload_text) {
                    Ok(payload) => payload,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                };
                watcher_repair_payloads
                    .lock()
                    .unwrap()
                    .push(payload.clone());
                let patch_id = payload
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap()
                    .to_string();
                let lines = payload
                    .get("normalize_prefix_lines")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let current = std::fs::read_to_string(&watcher_doc).unwrap();
                let repaired = normalize_exchange_prefixes_for_targets(&current, &lines);
                record_lazily_visible_write_candidate(&watcher_doc, &patch_id, &repaired);
                std::fs::remove_file(&watcher_patch_file).unwrap();
                return true;
            }
            false
        });

        let result = redeliver_normalization_fallback_for_test(
            &doc,
            repaired,
            bad_state,
            &normalize_prefix_lines,
            Some("source-patch-file"),
        );
        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");
        assert!(result, "file IPC narrow normalization repair should apply");

        let repair_payloads = seen_repair_payloads.lock().unwrap();
        assert_eq!(
            repair_payloads.len(),
            1,
            "expected exactly one file IPC repair payload"
        );
        let payload = &repair_payloads[0];
        assert!(
            payload.get("fullContent").is_none(),
            "eligible file IPC prefix repair should avoid fullContent payloads: {payload}"
        );
        assert_eq!(payload["patches"].as_array().unwrap().len(), 0);
        assert_eq!(payload["unmatched"], "");
        assert_eq!(payload["reposition_boundary"], true);
        assert_eq!(payload["preserve_head"], true);
        assert_eq!(
            payload["normalize_prefix_lines"][0],
            "do #sidecar-file. spec-test-build-install-commit-push"
        );
        assert_eq!(payload["expected_content_len"], bad_state.len());
        assert_eq!(
            payload["expected_content_hash"],
            agent_doc_hash::content_hash(bad_state)
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #sidecar-file. spec-test-build-install-commit-push"),
            "file IPC narrow repair should leave disk/editor content normalized: {disk}"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repaired_editor")
                && ops_log.contains("transport=file"),
            "ops log should record the file IPC narrow editor repair:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("sidecar_normalization_fallback_redelivered_editor"),
            "file IPC normalization-only repair should not fall back to fullContent:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_redelivery_skips_when_bad_state_is_stale() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
<!-- agent:exchange patch=append -->
do #stale. spec-test-build-install-commit-push
### Re: #stale — gpt-5

Done.
<!-- /agent:exchange -->
";
        let live_state = "\
<!-- agent:exchange patch=append -->
do #stale. spec-test-build-install-commit-push
live prompt typed after sidecar fallback
<!-- /agent:exchange -->
";
        let repaired = "\
<!-- agent:exchange patch=append -->
❯ do #stale. spec-test-build-install-commit-push
### Re: #stale — gpt-5

Done.
<!-- /agent:exchange -->
";
        std::fs::write(&doc, live_state).unwrap();

        let delivered = redeliver_normalization_fallback_for_test(
            &doc,
            repaired,
            bad_state,
            &["do #stale. spec-test-build-install-commit-push".to_string()],
            Some("source-patch"),
        );

        assert!(
            !delivered,
            "normalization fallback redelivery must skip stale bad-state proof"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), live_state);
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repair_skipped")
                && ops_log.contains("skip=stale_bad_state")
                && ops_log.contains("sidecar_normalization_fallback_editor_redelivery_skipped"),
            "stale proof skip should be logged for narrow and full-content fallback:\n{ops_log}"
        );
    }

    #[test]
    fn redelivery_skips_when_cpc_editor_buffer_diverges_from_bad_state() {
        // #clearexchstale: disk still equals the bad state (so the disk-divergence
        // guard passes), but the operator has freshly cleared/edited the live editor
        // buffer (a smaller cleared exchange published through the CRDT relay).
        // Redelivering the stale snapshot would REVIVE the cleared content, so the
        // redeliver must fail closed on the CPC model divergence.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Stale response that the operator cleared.
<!-- /agent:exchange -->
";
        // Disk still holds the bad state (the redeliver's disk check will pass).
        std::fs::write(&doc, bad_state).unwrap();

        // The operator cleared the exchange in the editor; the relay-published CPC
        // current text diverges from the bad state and from disk.
        let cleared_buffer = "\
<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->
";
        agent_doc_test_support::publish_editor_text_via_crdt_relay(
            &doc,
            "jetbrains-capable-diverged",
            cleared_buffer,
        );

        let repaired = bad_state; // the stale snapshot the repair would re-apply
        let delivered = agent_doc_write_converge_io::redeliver_full_content_repair_to_editor(
            &doc,
            repaired,
            bad_state,
            agent_doc_ipc_protocol::FullContentRepairRedelivery::IpcDedupe,
            None,
            &mut |file, repaired_content, expected_bad_state| {
                agent_doc_write_converge_io::try_ipc_full_content_response_fallback_from_source(
                    &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
                    file,
                    repaired_content,
                    expected_bad_state,
                )
            },
        );

        assert!(
            !delivered,
            "redelivery must skip when the live editor buffer has unsaved edits ahead of the bad state"
        );
        // Disk must be left untouched (the guard returns before any write).
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), bad_state);
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("skip=cpc_model_diverges"),
            "CPC model divergence skip should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_redelivery_blocks_capability_unknown_live_editor_before_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
<!-- agent:exchange patch=append -->
do #old-editor. spec-test-build-install-commit-push
### Re: #old-editor — gpt-5

Done.
<!-- /agent:exchange -->
";
        let repaired = "\
<!-- agent:exchange patch=append -->
❯ do #old-editor. spec-test-build-install-commit-push
### Re: #old-editor — gpt-5

Done.
<!-- /agent:exchange -->
";
        std::fs::write(&doc, bad_state).unwrap();

        let indicator_path = doc
            .canonicalize()
            .unwrap_or_else(|_| doc.clone())
            .to_string_lossy()
            .to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &indicator_path,
            bad_state,
            Some("jetbrains-old-editor"),
        )
        .unwrap();

        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let listener_root = dir.path().to_path_buf();
        let listener_doc = doc.clone();
        let listener_count = call_count.clone();
        let captured_clone = captured.clone();
        std::fs::create_dir_all(listener_root.join(".agent-doc")).unwrap();
        let _listener = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                listener_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                *captured_clone.lock().unwrap() = Some(v.clone());
                if let Some(lines) = v.get("normalize_prefix_lines").and_then(|value| {
                    value.as_array().map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                }) {
                    let current = std::fs::read_to_string(&listener_doc).ok()?;
                    let repaired = normalize_exchange_prefixes_for_targets(&current, &lines);
                    let _ = std::fs::write(&listener_doc, repaired);
                }
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            });
        });
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(dir.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            agent_doc_ipc_io::is_listener_active(dir.path()),
            "fake socket listener did not start"
        );

        let delivered = redeliver_normalization_fallback_for_test(
            &doc,
            repaired,
            bad_state,
            &["do #old-editor. spec-test-build-install-commit-push".to_string()],
            Some("source-patch-old-editor"),
        );

        assert!(
            !delivered,
            "capability-unknown editor must not receive normalization repair"
        );
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "capability guard may only send one read-only authority refresh before blocking repair IPC"
        );
        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("listener should receive the authority refresh");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], indicator_path);
        assert!(
            msg.get("content").is_none()
                && msg.get("patches").is_none()
                && msg.get("normalize_prefix_lines").is_none(),
            "authority refresh must not carry repair or document mutation payload: {msg}"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), bad_state);
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("skip=editor_capability_missing")
                && ops_log.contains(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY),
            "missing-capability redelivery skip should be logged:\n{ops_log}"
        );
        assert!(
            ops_log
                .contains("sidecar_normalization_fallback_narrow_repair_editor_authority_refresh")
                && ops_log.contains("action=publish_live_buffer"),
            "missing-capability redelivery should log the read-only authority refresh:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("sidecar_normalization_fallback_narrow_repair_attempt"),
            "guard must run before repair IPC attempt:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_dedupes_already_applied_editor_response() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
do #duppb. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let editor_already_applied = "\
<!-- agent:exchange patch=append -->
do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        std::fs::write(&doc, editor_already_applied).unwrap();

        let fallback = normalized_content_ours_fallback(
            &doc,
            Some(baseline),
            content_ours,
            &["do #duppb. spec-test-build-install-commit-push".to_string()],
        );

        assert_eq!(
            fallback.matches("### Re: #duppb — gpt-5").count(),
            1,
            "fallback full-content repair must not redeliver duplicate responses: {fallback}"
        );
        assert!(fallback.contains("❯ do #duppb. spec-test-build-install-commit-push"));
    }

    #[test]
    fn normalization_fallback_adopts_visible_write_response_delta_before_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
do #ackdelta
<!-- agent:boundary:base -->
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #ackdelta
### Re: ack delta — gpt-5

Done.
<!-- agent:boundary:ours -->
<!-- /agent:exchange -->
";
        let disk_after_visible_write = "\
<!-- agent:exchange patch=append -->
do #ackdelta
while typing next prompt
### Re: ack delta — gpt-5

Done.
<!-- agent:boundary:current -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, disk_after_visible_write).unwrap();

        let fallback = normalized_content_ours_fallback(
            &doc,
            Some(baseline),
            content_ours,
            &["do #ackdelta".to_string()],
        );

        assert_eq!(
            fallback.matches("### Re: ack delta — gpt-5").count(),
            1,
            "visible-write normalization fallback must not replay an already-applied response: {fallback}"
        );
        assert!(
            fallback.contains("while typing next prompt"),
            "visible-write fallback should preserve concurrent disk edits: {fallback}"
        );
        assert!(
            fallback.contains("❯ do #ackdelta"),
            "visible-write fallback should still normalize the prompt prefix: {fallback}"
        );
    }

    #[test]
    fn normalization_fallback_missing_lazily_receipt_preserves_pending_mutations() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
        let on_disk_with_pending = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!-- agent:backlog -->
- [ ] [#keepme] Preserve pending add from disk
<!-- /agent:backlog -->
";
        std::fs::write(&doc, on_disk_with_pending).unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange -->
❯ do #splpend
agent response
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
        let normalize_prefix_lines = vec!["do #splpend".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap();
        assert!(
            !result.success,
            "missing lazily receipt must fail closed instead of repairing disk: {result:?}"
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            disk, on_disk_with_pending,
            "unproven normalization fallback must leave pending disk mutations untouched"
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "unproven normalization fallback must not save a snapshot"
        );
    }

    #[test]
    fn normalization_visible_write_retry_preserves_concurrent_comment_deletion() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange -->
do #commentdel
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!--
The tmux focus should be snappy.
-->
";
        std::fs::write(&doc, original).unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange -->
❯ do #commentdel
agent response
<!-- /agent:exchange -->

<!--
The tmux focus should be snappy.
-->
";
        let normalize_prefix_lines = vec!["do #commentdel".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange -->
do #commentdel
agent response
<!-- /agent:exchange -->
";
                            record_lazily_visible_write_candidate(
                                &doc_for_watcher,
                                pid,
                                bad_sidecar,
                            );
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let err = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("refusing direct document write"),
            "prefix-divergent visible write must fail closed: {err}"
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("do #commentdel"),
            "visible-write-derived retry should preserve the visible prompt text: {disk}"
        );
        assert!(
            disk.contains("agent response"),
            "agent response from the visible-write event should be preserved: {disk}"
        );
        assert!(
            !disk.contains("The tmux focus should be snappy."),
            "operator-visible deletion from the sidecar must not be resurrected from content_ours: {disk}"
        );
        let snapshot = agent_doc_snapshot_io::load(&doc).unwrap();
        assert!(
            snapshot.is_none(),
            "unproven prefix-divergent visible write must not save a snapshot"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("snap_source=lazily_visible_write_event")
                && ops_log.contains("recovery=retry_without_disk_write"),
            "normalization retry must fail closed without promoting content_ours:\n{ops_log}"
        );
    }

    #[test]
    fn verify_sidecar_normalization_requires_duplicate_occurrences() {
        let sidecar = "\
---
session: test
---

<!-- agent:exchange -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5

Done.

❯ follow-up
spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let normalize_prefix_lines = vec![
            "do [#dup]. Are repeated presets handled?".to_string(),
            "spec-test-build-install-commit-push".to_string(),
            "follow-up".to_string(),
            "spec-test-build-install-commit-push".to_string(),
        ];

        assert!(
            !verify_sidecar_normalization(sidecar, &normalize_prefix_lines),
            "one earlier prefixed preset line must not mask a later bare duplicate"
        );
    }

    #[test]
    fn extract_post_commit_normalization_targets_preserves_duplicate_missing_lines() {
        let committed = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5

Done.

❯ Why follow up?
❯ spec-test-build-install-commit-push
<!-- agent:boundary:committed -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5 (HEAD)

Done.

❯ Why follow up?
spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert_eq!(
            targets,
            vec!["spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_repairs_late_duplicate_occurrence() {
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5 (HEAD)

Done.

❯ Why follow up?
spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[String::from("spec-test-build-install-commit-push")],
        );

        assert_eq!(
            repaired
                .matches("❯ spec-test-build-install-commit-push")
                .count(),
            2,
            "repair should prefix the later bare duplicate without losing the earlier one"
        );
        assert!(
            !repaired.contains("\n❯ ❯ spec-test-build-install-commit-push"),
            "repair must not double-prefix existing matches"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_assistant_verification_lists() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Implemented.

Verification:
- Passed focused tests:
  - `cargo test normalize_prefix`
- `cargo test` is still red on a pre-existing failure.
<!-- agent:boundary:previous -->
do #verfpfx. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "- Passed focused tests:".to_string(),
                "  - `cargo test normalize_prefix`".to_string(),
                "- `cargo test` is still red on a pre-existing failure.".to_string(),
                "do #verfpfx. spec-test-build-install-commit-push".to_string(),
            ],
        );

        assert!(
            repaired.contains("Verification:\n- Passed focused tests:\n  - `cargo test normalize_prefix`\n- `cargo test` is still red on a pre-existing failure."),
            "assistant verification list must stay unprefixed:\n{repaired}"
        );
        assert!(
            repaired.contains("\n❯ do #verfpfx. spec-test-build-install-commit-push\n"),
            "real prompt after the response boundary must still be repaired:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ - Passed focused tests:")
                && !repaired.contains("\n❯   - `cargo test normalize_prefix`")
                && !repaired.contains("\n❯ - `cargo test` is still red on a pre-existing failure."),
            "assistant list items must not receive prompt prefixes:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_requires_targeted_prompt_start_after_response() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Why did this keep happening?
spec-test-build-install-commit-push
<!-- agent:boundary:previous -->
do #spfxnorm. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "spec-test-build-install-commit-push".to_string(),
                "do #spfxnorm. spec-test-build-install-commit-push".to_string(),
            ],
        );

        assert!(
            repaired
                .contains("\nWhy did this keep happening?\nspec-test-build-install-commit-push\n"),
            "assistant question and preset-looking prose must stay bare:\n{repaired}"
        );
        assert!(
            repaired.contains("\n❯ do #spfxnorm. spec-test-build-install-commit-push\n"),
            "real prompt after the boundary must still be repaired:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ spec-test-build-install-commit-push\n"),
            "a stale target inside assistant prose must not be enough to start repair:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_assistant_commit_label() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #done — gpt-5
Verification:
- `cargo test`

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let repaired =
            normalize_exchange_prefixes_for_targets(working, &[String::from("Commit / push:")]);

        assert!(
            repaired.contains("\nCommit / push:\n"),
            "assistant commit evidence label must stay unprefixed:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Commit / push:\n"),
            "assistant commit evidence label must not receive prompt prefix:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_later_assistant_commit_label_after_stale_target()
     {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

Commit / push:
- `old-sha`
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `new-sha`
<!-- agent:boundary:abc --> (HEAD)
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[String::from("Commit / push:"), String::from("- `old-sha`")],
        );

        assert!(
            repaired.contains("\nCommit / push:\n- `new-sha`\n"),
            "later assistant commit label/list must stay bare:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Commit / push:\n- `new-sha`\n"),
            "later assistant commit label must not become a prompt:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_treats_prefixed_response_heading_as_assistant_boundary()
     {
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#done]. spec-test-build-install-commit-push
❯ ### Re: #done — gpt-5

Implemented.

Verification:
- `cargo test normalize_prefix`

Commit / push:
- `abc123`
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "Implemented.".to_string(),
                "Verification:".to_string(),
                "- `cargo test normalize_prefix`".to_string(),
                "Commit / push:".to_string(),
                "- `abc123`".to_string(),
            ],
        );

        assert!(
            repaired.contains("\n❯ ### Re: #done — gpt-5\n\nImplemented.\n"),
            "prefixed response heading must still start an assistant block:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Implemented.")
                && !repaired.contains("\n❯ Verification:")
                && !repaired.contains("\n❯ - `cargo test normalize_prefix`")
                && !repaired.contains("\n❯ Commit / push:")
                && !repaired.contains("\n❯ - `abc123`"),
            "assistant response body after a prefixed heading must not be prompt-prefixed:\n{repaired}"
        );
    }

    #[test]
    fn normalize_patch_content_skips_assistant_commit_label() {
        let patch = "\
### Re: #done — gpt-5
Verification:
- `cargo test`

Commit / push:
- `git push` returned `Everything up-to-date`.
";

        let normalized = agent_doc_document_realtime::write_policy::normalize_patch_content(
            patch,
            &[String::from("Commit / push:")],
        );

        assert!(
            normalized.contains("\nCommit / push:\n"),
            "assistant commit evidence label must stay unprefixed:\n{normalized}"
        );
        assert!(
            !normalized.contains("\n❯ Commit / push:\n"),
            "assistant commit evidence label must not receive prompt prefix:\n{normalized}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_assistant_commit_label() {
        let committed = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

❯ Commit / push:
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

❯ Commit / push:
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc --> (HEAD)
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            !targets.iter().any(|target| target == "Commit / push:"),
            "assistant evidence label must not become a prefix repair target: {targets:?}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_assistant_prose_before_next_heading() {
        let committed = "\
<!-- agent:exchange patch=append -->
### Re: sync latency — gpt-5

❯ The current tree has already started making this accountable.
### Re: closeout guard — gpt-5

Done.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
### Re: sync latency — gpt-5

The current tree has already started making this accountable.
### Re: closeout guard — gpt-5 (HEAD)

Done.
<!-- agent:boundary:def -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            targets.is_empty(),
            "a stale prefixed assistant sentence must not become a repair target: {targets:?}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_markdown_lists() {
        let committed = "\
<!-- agent:exchange patch=append -->
❯ Please compare these options:
❯ - keep this bullet bare
❯   - keep this nested bullet bare
❯ 1. keep this ordered bullet bare
### Re: options — gpt-5
Done.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
❯ Please compare these options:
- keep this bullet bare
  - keep this nested bullet bare
1. keep this ordered bullet bare
### Re: options — gpt-5 (HEAD)
Done.
<!-- agent:boundary:def -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            targets.is_empty(),
            "stale prefixed markdown list items must not become repair targets: {targets:?}"
        );
    }

    #[test]
    fn verify_sidecar_normalization_rejects_assistant_list_prefix_substitute() {
        let sidecar = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Verification:
❯ - Passed focused tests:
<!-- agent:boundary:previous -->
do #verfpfx. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        assert!(
            !verify_sidecar_normalization(sidecar, &["- Passed focused tests:".to_string()]),
            "a prefixed assistant list item must not satisfy prompt-prefix sidecar verification"
        );
    }
}

#[cfg(test)]
mod core_tests {
    #![allow(unused_imports)]
    use super::*;
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn ipc_live_prompt_drift_replaces_visible_write_candidate_and_records_queue_proof() {
        let dir = tempfile::tempdir().unwrap();
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ original prompt\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do [#deleted]\n",
            "<!-- /agent:queue -->\n",
        );
        let content_ours = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ original prompt\n",
            "### Re: original prompt — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do [#deleted]\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ original prompt\n",
            "❯ live prompt after preflight\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#manual]\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = agent_doc_test_support::init_repo_with_doc(dir.path(), "session.md", baseline);
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut decision = IpcRepairDecision::lazily_visible_write(candidate.to_string());

        let blocked = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &doc,
            "test",
            Some("patch-q"),
            Some(baseline),
            Some(content_ours),
            &mut decision,
        );

        assert!(blocked);
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::ContentOurs,
            "turn closeout must not promote the visible-write candidate to snapshot authority"
        );
        assert!(
            decision
                .snapshot_content
                .contains("### Re: original prompt — gpt-5"),
            "the snapshot candidate should carry the agent-owned response:\n{}",
            decision.snapshot_content
        );
        assert!(
            !decision
                .snapshot_content
                .contains("❯ live prompt after preflight"),
            "a response-missing lazily visible-write receipt must not become snapshot authority:\n{}",
            decision.snapshot_content
        );
        assert!(
            !decision.snapshot_content.contains("do [#manual]"),
            "a response-missing lazily visible-write receipt must not carry queue additions into the snapshot:\n{}",
            decision.snapshot_content
        );
        assert!(
            decision.snapshot_content.contains("do [#deleted]"),
            "content_ours remains the snapshot when the visible write only proves delivery, not whole-buffer authority:\n{}",
            decision.snapshot_content
        );
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::LivePromptDrift)
        );
        assert!(decision.redeliver_editor);
        let bad_state = decision
            .editor_bad_state
            .as_ref()
            .expect("visible repair should retain the response-missing visible-write candidate");
        assert!(
            bad_state
                .content()
                .contains("❯ live prompt after preflight")
        );
        assert!(bad_state.content().contains("do [#manual]"));
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            log.contains("live_prompt_drift_agent_target_not_snapshot_authority")
                && log.contains("visible_repair_required=true")
                && log.contains("queue_live_deletion_ignored")
                && log.contains("reason=unproven_ipc_candidate_queue_deletion"),
            "response-missing visible-write proof must fail closed while recording queue deletion proof:\n{log}"
        );
    }
    #[test]
    fn ipc_ack_timeouts_degrade_current_session_to_file_ipc_retry() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test-session\n---\n\ncontent").unwrap();

        assert!(
            !record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap(),
            "first timeout should only record health state"
        );
        assert!(
            record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap(),
            "second consecutive timeout should mark the listener degraded"
        );
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "current session should now bypass IPC"
        );

        fs::write(&doc, "---\nsession: next-session\n---\n\ncontent").unwrap();
        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a new session id must not inherit the old session's degraded marker"
        );
    }
    #[test]
    fn degraded_latch_self_heals_when_listener_recovers() {
        // `#ipc-degrade-self-heal`: the degrade latch is a circuit breaker, not
        // a permanent session verdict. Once a recovered plugin socket is
        // accepting connections again, `ipc_direct_disk_degraded` must clear the
        // marker and resume the reliable IPC path instead of staying disk-only
        // until session restart.
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: heal-session\n---\n\ncontent").unwrap();

        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "two timeouts with no live listener should stay degraded"
        );

        // Bring a live socket listener up (the recovered plugin).
        let root_clone = dir.path().to_path_buf();
        let server = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root_clone, |_msg| {
                Some(r#"{"type":"receipt","status":"applied","id":"x"}"#.to_string())
            });
        });
        std::thread::sleep(std::time::Duration::from_millis(150));

        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a recovered live listener must self-heal the degrade latch"
        );
        let marker = dir.path().join(".agent-doc/ipc-degraded").join(format!(
            "{}.json",
            agent_doc_fs::document_state_hash(&doc).unwrap()
        ));
        assert!(
            !marker.exists(),
            "self-heal must remove the degraded marker"
        );

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(dir.path()));
        drop(server);
    }
    #[test]
    fn degraded_latch_does_not_self_heal_when_listener_connects_without_receipt() {
        // A wedged editor plugin can leave ipc.sock connectable while its accept
        // / apply path no longer returns acks. The degraded latch must not clear
        // on connect-only evidence; otherwise the next write re-enters the bad
        // socket path instead of preferring file IPC.
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: wedged-session\n---\n\ncontent").unwrap();

        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();

        let root_clone = dir.path().to_path_buf();
        let server = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root_clone, |_msg| None);
        });
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());

        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "connectable but non-receipting listener must remain degraded"
        );
        let marker = dir.path().join(".agent-doc/ipc-degraded").join(format!(
            "{}.json",
            agent_doc_fs::document_state_hash(&doc).unwrap()
        ));
        assert!(
            marker.exists(),
            "non-receipting listener must not clear the degraded marker"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_degraded_self_heal_probe_failed")
                && log.contains("IPC_receipt_timeout"),
            "failed self-heal probe must be observable:\n{log}"
        );

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(dir.path()));
        drop(server);
    }
    #[test]
    fn try_ipc_prefers_file_ipc_when_socket_degraded() {
        // `#ipc-degraded-prefers-file-ipc`: a latched-degraded socket must NOT
        // jump straight to a raw disk write. The write routes through the
        // file-IPC patch queue (plugin file watcher applies via Document API).
        // With no plugin consuming the patch, file IPC times out and returns
        // `false` so the caller can fall back to disk as the LAST resort — but
        // the degraded write still attempted file IPC first.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "new content");
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();

        assert!(
            !result.success,
            "degraded file-IPC with no plugin should report not consumed for retry"
        );
        // The file-IPC poll leaves the unconsumed patch for editor retry.
        let leftover: Vec<_> = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            !leftover.is_empty(),
            "file-IPC timeout must leave the unconsumed patch queued"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_socket_degraded_prefer_file_ipc")
                && ops_log.contains("transport=try_ipc"),
            "degraded socket should log the prefer-file-IPC routing decision:\n{ops_log}"
        );
        assert!(
            ops_log.contains("ipc_write_attempt"),
            "degraded write must still attempt the file-IPC patch queue, not bypass it:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("ipc_listener_degraded_direct_disk"),
            "degraded write must NOT take the old direct-disk bypass:\n{ops_log}"
        );
    }
    #[test]
    fn try_ipc_degraded_succeeds_via_file_ipc_when_plugin_consumes() {
        // `#ipc-degraded-prefers-file-ipc`: even with the socket latched
        // degraded, a live plugin file watcher consuming the file-IPC patch
        // makes the degraded write succeed through the plugin (Document API) —
        // no raw disk write, so no manufactured File Cache Conflict.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "two timeouts with no live listener should latch degraded"
        );

        // Simulate the plugin file watcher applying then deleting the patch.
        let watcher_dir = agent_doc_dir.join("patches");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().is_some_and(|e| e == "json") {
                            let patch_id = fs::read_to_string(&path)
                                .ok()
                                .and_then(|text| {
                                    serde_json::from_str::<serde_json::Value>(&text).ok()
                                })
                                .and_then(|json| {
                                    json.get("patch_id")
                                        .and_then(|value| value.as_str())
                                        .map(str::to_string)
                                });
                            let applied = "---\nsession: test\n---\n\n<!-- agent:exchange -->\nnew content\n<!-- /agent:exchange -->\n";
                            let _ = fs::write(&doc_for_watcher, applied);
                            if let Some(patch_id) = patch_id {
                                record_lazily_visible_write_candidate(
                                    &doc_for_watcher,
                                    &patch_id,
                                    applied,
                                );
                            }
                            let _ = fs::remove_file(path);
                            return;
                        }
                    }
                }
            }
        });

        let patch = agent_doc_template::PatchBlock::new("exchange", "new content");
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "degraded write must succeed through the file-IPC patch queue when the plugin consumes it"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_socket_degraded_prefer_file_ipc"),
            "degraded socket should log the prefer-file-IPC routing decision:\n{ops_log}"
        );
    }
    #[test]
    fn try_editor_converge_skips_wedged_socket_then_uses_detached_disk_when_editorless() {
        // `#fcc0e`: once the de-wedge latch trips degraded (repeated socket ack
        // timeouts) and no live listener can be re-probed, the converger must skip
        // the wedged socket, try the plugin-owned file-IPC queue, then use the
        // guarded detached-disk path when no live editor sidecar owns the document.
        // It must not take the old raw disk-fallback bypass.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        // Trip the degraded latch (threshold = 2 distinct ack timeouts), mirroring
        // the existing dewedge tests. No live listener exists, so the self-heal
        // re-probe in `ipc_direct_disk_degraded` cannot clear it.
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "queue_consume").unwrap();
        let degraded =
            record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "queue_consume").unwrap();
        assert!(
            degraded,
            "two distinct ack timeouts must trip the degraded latch"
        );

        let written = agent_doc_write_converge_io::try_editor_converge(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            &doc,
            &target,
            &source,
            "queue_consume",
        )
        .unwrap();
        assert!(written, "editorless degraded socket should converge");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "editorless degraded socket should write through guarded detached disk"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_degraded_prefer_file_ipc")
                && log.contains("transport=queue_consume"),
            "the degraded skip must prefer file IPC before detached disk:\n{log}"
        );
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=listener_degraded_editor_detached"),
            "the degraded editorless write must be source-labelled as detached disk:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "the degraded converger must not take the old direct-disk bypass:\n{log}"
        );
        assert!(
            !log.contains("reason=no_listener"),
            "the degraded check must short-circuit before the no_listener check:\n{log}"
        );
    }

    #[test]
    fn try_editor_converge_degraded_socket_succeeds_via_file_ipc_when_plugin_consumes() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "queue_consume").unwrap();
        let degraded =
            record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "queue_consume").unwrap();
        assert!(
            degraded,
            "two distinct ack timeouts must trip the degraded latch"
        );

        let watcher_dir = agent_doc_dir.join("patches");
        let watcher_doc = doc.clone();
        let watcher_target = target.clone();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().is_none_or(|e| e != "json") {
                            continue;
                        }
                        let payload_text = fs::read_to_string(&path).unwrap();
                        let payload: serde_json::Value =
                            serde_json::from_str(&payload_text).unwrap();
                        let patch_id = payload
                            .get("patch_id")
                            .and_then(|value| value.as_str())
                            .unwrap()
                            .to_string();
                        record_lazily_visible_write_candidate(
                            &watcher_doc,
                            &patch_id,
                            &watcher_target,
                        );
                        fs::remove_file(path).unwrap();
                        return true;
                    }
                }
            }
            false
        });

        let converged = agent_doc_write_converge_io::try_editor_converge(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            &doc,
            &target,
            &source,
            "queue_consume",
        )
        .unwrap();
        assert!(
            converged,
            "degraded convergence must succeed through file IPC when the plugin consumes it"
        );
        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_degraded_prefer_file_ipc")
                && log.contains("queue_consume_file_ipc_convergence_attempt")
                && log.contains("transport=file_ipc")
                && log.contains("degraded_cause=listener_degraded"),
            "degraded convergence should be auditable as file IPC:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "degraded convergence must not raw-write behind the plugin:\n{log}"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), target);
    }
    #[test]
    fn ipc_snapshot_adoption_allowed_logs_benign_recheck() {
        // Every adoption that the fail-closed guards did NOT block must still leave
        // a diagnostic so a corruption slipping through as "allowed" is traceable.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Q\n",
            "<!-- /agent:exchange -->\n",
        );
        let content_ours = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Q\n### Re: Q — gpt-5\n\nAnswered.\n",
            "<!-- /agent:exchange -->\n",
        );
        let decision = IpcRepairDecision::content_ours(content_ours.to_string());

        log_ipc_snapshot_adoption_allowed(
            &doc,
            "socket_visible_write_content",
            Some("pid-allowed"),
            Some(baseline),
            Some(content_ours),
            &decision,
            false,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_snapshot_adoption_allowed")
                && log.contains("source=socket_visible_write_content")
                && log.contains("patch_id=pid-allowed")
                && log.contains("drift_recheck=false")
                && log.contains("dup_growth_recheck=0"),
            "allowed adoption must log a benign re-check:\n{log}"
        );
    }
    #[test]
    fn ipc_snapshot_adoption_allowed_is_silent_when_blocked() {
        // Blocked adoptions log their own rich diagnostic; the allowed line must not
        // also fire (it would falsely report an unguarded adoption).
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let decision = IpcRepairDecision::content_ours("snapshot".to_string());
        log_ipc_snapshot_adoption_allowed(
            &doc,
            "file_ipc",
            Some("pid-blocked"),
            None,
            None,
            &decision,
            true,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("ipc_snapshot_adoption_allowed"),
            "allowed diagnostic must stay silent once a guard fired:\n{log}"
        );
    }
    #[test]
    fn ipcfullprompt_corruption_logged_on_deleted_response() {
        // #ipcfullprompt-recur2: a live editor buffer (candidate) that dropped a
        // previously-committed `### Re:` block must leave a forensic ops.log line
        // and preserve the baseline + candidate for analysis — default-on capture,
        // no manual editor debug opt-in required.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "### Re: second — opus-4-8\nA2.\n",
            "<!-- /agent:exchange -->\n",
        );
        // candidate dropped the second response block.
        let candidate = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "<!-- /agent:exchange -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "socket_visible_write_content",
            Some("pid-corrupt"),
            Some(baseline),
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipcfullprompt_corruption_suspected")
                && log.contains("source=socket_visible_write_content")
                && log.contains("patch_id=pid-corrupt")
                && log.contains("deleted=1")
                && log.contains("response_deleted(### Re: second — opus-4-8:1->0)"),
            "deleted prior response must be captured:\n{log}"
        );
        let forensic_dir = agent_doc_dir.join("logs/ipcfullprompt");
        let preserved: Vec<_> = fs::read_dir(&forensic_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            preserved.iter().any(|n| n.ends_with(".baseline.md"))
                && preserved.iter().any(|n| n.ends_with(".candidate.md")),
            "forensic baseline + candidate must be preserved: {preserved:?}"
        );
    }
    #[test]
    fn ipcfullprompt_scaffold_duplication_logged_without_baseline() {
        // The brandon-cinquegrana.md shape: a full-tail duplication leaves two
        // `<!-- /agent:exchange -->` markers around an in-progress prompt. This is
        // a self-check on the candidate, so it must fire even with no baseline.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let candidate = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: prior — opus-4-8\nAnswer.\n",
            "<!-- agent:boundary:709a41ae -->\n",
            "Is the issue still happening?\nCan it be re\n",
            "<!-- /agent:exchange -->\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
            "Can it be rep11ro\n",
            "<!-- /agent:exchange -->\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "socket_visible_write_content",
            Some("pid-x"),
            None,
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipcfullprompt_corruption_suspected")
                && log.contains("scaffold_duplicated=")
                && log.contains("scaffold_duplicated(<!-- /agent:exchange -->:1->2)"),
            "full-tail scaffold duplication must be captured without a baseline:\n{log}"
        );
    }
    #[test]
    fn ipcfullprompt_corruption_silent_on_clean_candidate() {
        // A candidate that only *adds* a new response (expected growth) must not
        // be flagged — no false positive on normal cycles.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "<!-- /agent:exchange -->\n",
        );
        let candidate = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "### Re: second — opus-4-8\nA2.\n",
            "<!-- /agent:exchange -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "file_ipc",
            Some("pid-clean"),
            Some(baseline),
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("ipcfullprompt_corruption_suspected"),
            "clean growth must not be flagged as corruption:\n{log}"
        );
    }
}

#[cfg(test)]
mod committed_blob_authority_autoheal_tests {
    //! `#rtheal-write` — the WRITE/FINALIZE capability gate must UNBLOCK (heal +
    //! return false) when the stale editor buffer byte-matches a recent committed
    //! blob (a previously-saved, recoverable state with no unsaved work), and must
    //! STILL REFUSE (return true) when the buffer matches no committed blob (could
    //! be unsaved-ahead operator work).
    use tempfile::TempDir;

    /// Build a temp git repo with `doc.md`, committing each snapshot in order, and
    /// leave the working tree at the LAST snapshot (== HEAD). Also creates the
    /// `.agent-doc/logs` dir the ops-log writer expects.
    fn temp_git_doc(commits: &[&str]) -> (TempDir, std::path::PathBuf, String) {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        let file = root.join("doc.md");
        for (i, body) in commits.iter().enumerate() {
            std::fs::write(&file, body).unwrap();
            git(&["add", "doc.md"]);
            git(&["commit", "-m", &format!("commit {i}")]);
        }
        let canonical = std::fs::canonicalize(&file)
            .unwrap()
            .to_string_lossy()
            .to_string();
        (dir, file, canonical)
    }

    const OLD: &str = concat!(
        "# Doc\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Older committed answer.\n",
        "<!-- /agent:exchange -->\n",
    );
    const NEW: &str = concat!(
        "# Doc\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Older committed answer.\n",
        "### Re: next — gpt-5\n\n",
        "Newer committed answer.\n",
        "<!-- /agent:exchange -->\n",
    );

    #[test]
    fn unblocks_when_stale_buffer_matches_a_committed_blob() {
        // disk == HEAD == NEW; the editor still holds committed ancestor OLD, and
        // its live-buffer sidecar lacks the operator-text-authority capability.
        let (_dir, file, canonical) = temp_git_doc(&[OLD, NEW]);
        // Missing-capability sidecar holding the OLD committed blob.
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &canonical,
            OLD,
            Some("vscode-committed-blob"),
        )
        .unwrap();

        // The stale buffer (expected_bad_state) IS a committed blob → proceed.
        let refuse = agent_doc_write_converge_io::redelivery_missing_operator_text_authority(
            &file,
            OLD,
            "write",
            Some("p1"),
        );
        assert!(
            !refuse,
            "a stale buffer equal to a committed blob holds no unsaved work — the write must proceed (return false)"
        );
        // The autoheal marker is emitted; the capability-missing skip is NOT.
        let ops_log =
            std::fs::read_to_string(file.parent().unwrap().join(".agent-doc/logs/ops.log"))
                .unwrap();
        assert!(
            ops_log.contains("write_editor_authority_autoheal")
                && ops_log.contains("reason=stale_behind_committed_blob"),
            "the committed-blob autoheal must be auditable:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("skip=editor_capability_missing"),
            "the capability-missing refusal must NOT fire for a committed blob:\n{ops_log}"
        );
    }

    #[test]
    fn still_refuses_when_stale_buffer_matches_no_committed_blob() {
        // SAFETY INVARIANT: disk == HEAD == NEW, but the editor holds a genuine
        // unsaved edit that matches NO commit. The gate MUST still refuse.
        let (_dir, file, canonical) = temp_git_doc(&[OLD, NEW]);
        let unsaved = format!("{NEW}<!-- operator note -->\nGENUINELY UNSAVED WORK\n");
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &canonical,
            &unsaved,
            Some("vscode-unsaved-ahead"),
        )
        .unwrap();

        let refuse = agent_doc_write_converge_io::redelivery_missing_operator_text_authority(
            &file,
            &unsaved,
            "write",
            Some("p2"),
        );
        assert!(
            refuse,
            "a buffer matching no committed blob may hold unsaved operator work — the gate MUST still refuse (return true)"
        );
        let ops_log =
            std::fs::read_to_string(file.parent().unwrap().join(".agent-doc/logs/ops.log"))
                .unwrap();
        assert!(
            ops_log.contains("skip=editor_capability_missing"),
            "the capability-missing refusal must fire for a non-committed buffer:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("write_editor_authority_autoheal"),
            "no autoheal may fire when the buffer is not a proven committed blob:\n{ops_log}"
        );
    }
}
