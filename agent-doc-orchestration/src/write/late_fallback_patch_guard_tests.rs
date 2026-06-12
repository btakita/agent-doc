    use super::{
        IpcDiskRepairReason, IpcRepairDecision, IpcSnapshotSource, WriteFlags,
        cleanup_fallback_patch_files, cycle_already_committed, recover_dedupe_only_drift,
        recover_empty_response_for_strict_closeout, redeliver_ipc_dedupe_to_editor,
        repair_ipc_decision_visible_state, try_ipc, try_ipc_full_content,
        try_ipc_full_content_operator_mutation_from_source,
    };
    use crate::snapshot;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn doc_in_agent_doc_project(tmp: &TempDir, content: &str) -> std::path::PathBuf {
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("state").join("cycles")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = tmp.path().join("doc.md");
        fs::write(&doc, content).unwrap();
        doc
    }

    struct TsiftDuplicateContentFixture {
        bad_state_before_live_typing: &'static str,
        repaired_snapshot: &'static str,
        live_buffer_after_typing: &'static str,
    }

    fn tsift_md_duplicate_content_corruption_fixture() -> TsiftDuplicateContentFixture {
        TsiftDuplicateContentFixture {
            bad_state_before_live_typing: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "<!-- agent:boundary:tsift-bad -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
            repaired_snapshot: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "<!-- agent:boundary:tsift-repaired -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
            live_buffer_after_typing: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "The duplicate content corrupt document bug happened on tsift.md as I was tying in a prompt. ",
                "What are #next-steps to ensure full-document IPC is not over-eager? #next-steps\n",
                "<!-- agent:boundary:tsift-live -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
        }
    }

    #[test]
    fn ipc_repair_decision_records_prefix_fallback_bad_state() {
        let decision = IpcRepairDecision::content_ours_prefix_fallback(
            "fixed snapshot".to_string(),
            "bad editor state".to_string(),
            &["bad editor state".to_string()],
        );

        assert_eq!(decision.snapshot_content, "fixed snapshot");
        assert_eq!(decision.snap_source, IpcSnapshotSource::ContentOurs);
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergence)
        );
        assert!(decision.redeliver_editor);
        let bad_state = decision
            .editor_bad_state
            .as_ref()
            .expect("prefix fallback should capture bad editor state");
        assert_eq!(bad_state.content(), "bad editor state");
        assert_eq!(bad_state.len, "bad editor state".len());
        assert_eq!(
            bad_state.hash,
            crate::ops_log::content_hash("bad editor state")
        );
        assert_eq!(decision.normalize_prefix_lines, vec!["bad editor state"]);
    }

    #[test]
    fn ipc_repair_decision_preserves_original_bad_state_when_dedupe_follows_prefix_fallback() {
        let decision = IpcRepairDecision::content_ours_prefix_fallback(
            "prefix fallback with duplicate response".to_string(),
            "visible sidecar before fallback".to_string(),
            &["visible sidecar before fallback".to_string()],
        )
        .apply_ipc_dedupe(
            "deduped snapshot".to_string(),
            "prefix fallback with duplicate response".to_string(),
        );

        assert_eq!(decision.snapshot_content, "deduped snapshot");
        assert_eq!(decision.snap_source, IpcSnapshotSource::ContentOurs);
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe)
        );
        assert!(decision.redeliver_editor);
        assert_eq!(
            decision
                .editor_bad_state
                .as_ref()
                .expect("combined repair should keep original bad editor proof")
                .content(),
            "visible sidecar before fallback"
        );
    }

    #[test]
    fn cycle_already_committed_returns_none_when_no_state() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("nonexistent.md");
        assert!(cycle_already_committed(&doc).is_none());
    }

    #[test]
    fn cycle_already_committed_returns_some_for_committed_cycle() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&doc, "test", Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "test", Some(content), Some(content)).unwrap();

        let result = cycle_already_committed(&doc);
        assert!(result.is_some(), "should return Some for committed cycle");
    }

    #[test]
    fn cycle_already_committed_returns_none_for_open_cycle() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        assert!(cycle_already_committed(&doc).is_none());
    }

    #[test]
    fn cleanup_fallback_patch_files_removes_patch_and_writes_sentinel() {
        let tmp = TempDir::new().unwrap();
        let doc =
            doc_in_agent_doc_project(&tmp, "---\nagent_doc_session: test\n---\n\n## Exchange\n");
        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        let patch_content = serde_json::json!({
            "patch_id": "test-patch-123",
            "type": "patch",
        });
        fs::write(
            &patch_path,
            serde_json::to_string_pretty(&patch_content).unwrap(),
        )
        .unwrap();
        assert!(patch_path.exists());

        cleanup_fallback_patch_files(&doc);

        assert!(
            !patch_path.exists(),
            "fallback patch file should be removed"
        );
        let sentinel = tmp
            .path()
            .join(".agent-doc/claimed-patches")
            .join("test-patch-123");
        assert!(sentinel.exists(), "claimed sentinel should be written");
    }

    #[test]
    fn cleanup_fallback_patch_files_noop_when_no_patch() {
        let tmp = TempDir::new().unwrap();
        let doc =
            doc_in_agent_doc_project(&tmp, "---\nagent_doc_session: test\n---\n\n## Exchange\n");
        cleanup_fallback_patch_files(&doc);
    }

    #[test]
    fn try_ipc_marks_committed_cycle_skip_as_not_consumed() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&doc, "test", Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "test", Some(content), Some(content)).unwrap();

        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let stale_patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        fs::write(
            &stale_patch_path,
            serde_json::json!({"patch_id": "late-patch-123"}).to_string(),
        )
        .unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "late response");
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            None,
            None,
            None,
            Some("current-patch-456"),
        )
        .unwrap();

        assert!(
            !result.success,
            "committed-cycle IPC skip must not look like a consumed write"
        );
        assert_eq!(result.patch_id, "current-patch-456");
        assert!(
            result.skipped_committed_cycle,
            "caller must be able to stop terminal fallback handling"
        );
        assert!(
            !stale_patch_path.exists(),
            "stale fallback patch should be removed"
        );
        assert!(
            tmp.path()
                .join(".agent-doc/claimed-patches/late-patch-123")
                .exists(),
            "removed stale patch should be claimed so watchers cannot replay it"
        );

        let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("late_fallback_patch_rejected"));
        assert!(ops_log.contains("patch_id=current-patch-456"));
        assert!(ops_log.contains(
            "flow=closeout stage=terminal_guard outcome=blocked reason=already_committed"
        ));
        assert!(
            !ops_log.contains("ipc_write_consumed"),
            "terminal skip must not be logged as an IPC consume"
        );
    }

    #[test]
    fn full_content_ipc_skips_committed_cycle_before_socket_or_file_fallback() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&doc, "test", Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "test", Some(content), Some(content)).unwrap();

        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let stale_patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        fs::write(
            &stale_patch_path,
            serde_json::json!({"patch_id": "full-content-stale"}).to_string(),
        )
        .unwrap();

        let result = try_ipc_full_content(&doc, "stale full-content repair").unwrap();

        assert!(!result, "committed-cycle full-content IPC must be skipped");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            content,
            "full-content IPC must not dirty an already committed cycle"
        );
        assert!(
            !stale_patch_path.exists(),
            "stale full-content fallback patch should be removed"
        );
        assert!(
            tmp.path()
                .join(".agent-doc/claimed-patches/full-content-stale")
                .exists(),
            "removed full-content fallback patch should be claimed"
        );

        let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("late_fallback_patch_rejected"));
        assert!(ops_log.contains("patch_id=full_content"));
        assert!(ops_log.contains(
            "flow=closeout stage=terminal_guard outcome=blocked reason=already_committed"
        ));
        assert!(
            !ops_log.contains("socket_full_content"),
            "full-content socket diagnostic must not be emitted after committed-cycle skip"
        );
    }

    #[test]
    fn full_content_operator_ipc_is_disabled_before_source_buffer_delivery() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let live = "before\nlive prompt\n";
        let target = "after\n";
        fs::write(&doc, live).unwrap();

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "operator full-content IPC must not be emitted when the disk buffer already contains live drift"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale full-content replacement must not overwrite live prompt drift"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "failed full-content IPC must not save a snapshot"
        );
        let patch_count = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(
            patch_count, 0,
            "disabled full-content path must not hand a patch to file IPC"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled")
                && ops_log.contains("source=compact_exchange"),
            "disabled full-content path should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn full_content_operator_ipc_rejects_late_post_exchange_scratch_comment() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let prompt = "The full-document IPC scratch comment was typed below exchange after target computation. #spec-test-build-install-commit-push";
        let source = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "-->\n"
        );
        let live = source.replace("<!--\n-->", &format!("<!--\n{prompt}\n-->"));
        let target = source.replace(
            "### Re: previous — gpt-5\n\nDone.\n",
            "### Session Summary\n\nCompacted.\n",
        );
        fs::write(&doc, &live).unwrap();

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, &target, source).unwrap();

        assert!(
            !result,
            "operator full-content IPC must not be emitted after a late post-exchange scratch edit"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale full-content replacement must preserve the live scratch comment"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "failed full-content IPC must not save a snapshot"
        );
        let patch_count = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(
            patch_count, 0,
            "scope/source guards must not hand a full-content patch to file IPC"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_scope_rejected")
                && ops_log.contains("source=compact_exchange"),
            "component-scope rejection should be logged before source-buffer proof:\n{ops_log}"
        );
    }

    #[test]
    fn response_fallback_full_content_is_disabled_before_socket_delivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let fallback = "before\n";
        let live = "before\nlive prompt typed after fallback was computed\n";
        fs::write(&doc, live).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let result = try_ipc_full_content(&doc, fallback).unwrap();

        assert!(
            !result,
            "stale response fallback full-content IPC must be skipped before socket delivery"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "socket listener must not receive stale response fallback full-content payloads"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale response fallback must not overwrite live prompt drift"
        );
        assert!(snapshot::load(&doc).unwrap().is_none());
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled")
                && ops_log.contains("source=response_fallback"),
            "disabled full-content path should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn ipc_dedupe_full_content_redelivery_is_disabled() {
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let bad_state = "before\n### Re: issue — gpt-5\nDone.\n### Re: issue — gpt-5\nDone.\n";
        let repaired = "before\n### Re: issue — gpt-5\nDone.\n";
        fs::write(&doc, bad_state).unwrap();

        let seen_payload = Arc::new(Mutex::new(None::<serde_json::Value>));
        let listener_seen = seen_payload.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                *listener_seen.lock().unwrap() = Some(payload.clone());
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let delivered = redeliver_ipc_dedupe_to_editor(&doc, repaired, bad_state);

        assert!(!delivered, "full-content redelivery is disabled");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            bad_state,
            "disabled full-content redelivery must not mutate the editor-visible file"
        );
        assert!(
            seen_payload.lock().unwrap().is_none(),
            "listener should not receive a disabled full-content payload"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled")
                && ops_log.contains("source=response_fallback"),
            "disabled redelivery should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn ipc_dedupe_redelivery_skips_when_bad_state_is_stale() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let bad_state = "before\n### Re: issue — gpt-5\nDone.\n### Re: issue — gpt-5\nDone.\n";
        let live_state = "before\nlive prompt typed after repair planning\n";
        let repaired = "before\n### Re: issue — gpt-5\nDone.\n";
        fs::write(&doc, live_state).unwrap();

        let delivered = redeliver_ipc_dedupe_to_editor(&doc, repaired, bad_state);

        assert!(
            !delivered,
            "redelivery must be skipped when the visible bad-state proof is stale"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live_state,
            "stale redelivery must not overwrite live content"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_dedupe_editor_redelivery_skipped")
                && ops_log.contains("skip=stale_bad_state"),
            "stale redelivery skip should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn template_ipc_dedupe_repair_uses_disk_not_full_content_redelivery() {
        let tmp = TempDir::new().unwrap();
        let bad_state = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: issue — gpt-5\nDone.\n",
            "### Re: issue — gpt-5\nDone.\n",
            "<!-- /agent:exchange -->\n"
        );
        let repaired = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: issue — gpt-5\nDone.\n",
            "<!-- /agent:exchange -->\n"
        );
        let doc = doc_in_agent_doc_project(&tmp, bad_state);
        let agent_doc_dir = tmp.path().join(".agent-doc");

        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let decision = IpcRepairDecision::file_read(bad_state.to_string())
            .apply_ipc_dedupe(repaired.to_string(), bad_state.to_string());
        repair_ipc_decision_visible_state(&doc, &decision, Some("source-patch")).unwrap();

        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "component-scoped template repairs must not send socket fullContent payloads"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            repaired,
            "template duplicate repair should fall back to guarded disk repair"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_scope_rejected")
                && ops_log.contains("scope=template_frontmatter")
                && ops_log.contains("ipc_dedupe_repaired_working_tree"),
            "template fullContent rejection and disk repair should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn tsift_md_duplicate_content_fixture_skips_stale_full_document_redelivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let fixture = tsift_md_duplicate_content_corruption_fixture();
        let doc = tmp.path().join("tasks/software/tsift.md");
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, fixture.live_buffer_after_typing).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let delivered = redeliver_ipc_dedupe_to_editor(
            &doc,
            fixture.repaired_snapshot,
            fixture.bad_state_before_live_typing,
        );

        assert!(
            !delivered,
            "tsift.md fixture must skip full-document redelivery when the visible buffer changed after repair planning"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "stale tsift.md repair proof must be rejected before any socket fullContent payload"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fixture.live_buffer_after_typing,
            "live tsift.md prompt text typed after repair planning must remain untouched"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_dedupe_editor_redelivery_proof")
                && ops_log.contains("redeliver=false")
                && ops_log.contains("ipc_dedupe_editor_redelivery_skipped")
                && ops_log.contains("skip=stale_bad_state"),
            "stale tsift.md fixture should log proof and skip diagnostics:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn socket_full_content_is_disabled_before_payload_delivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let live = "before\nlive prompt typed during compact\n";
        let target = "after\n";
        fs::write(&doc, live).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "disabled full-content path should reject before socket delivery"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "socket listener must not receive stale full-content payloads"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), live);
        assert!(snapshot::load(&doc).unwrap().is_none());
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(ops_log.contains("full_content_ipc_disabled"));

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn socket_full_content_disabled_path_does_not_save_snapshot() {
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let target = "after\n";
        fs::write(&doc, source).unwrap();

        let root = tmp.path().to_path_buf();
        let listener_root = root.clone();
        let ack_root = root.clone();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = payload.get("patch_id")?.as_str()?;
                let ack_dir = ack_root.join(".agent-doc/ack-content");
                fs::create_dir_all(&ack_dir).ok()?;
                fs::write(ack_dir.join(format!("{patch_id}.md")), "wrong\n").ok()?;
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });

        std::thread::sleep(Duration::from_millis(100));
        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "socket full-content IPC must be disabled before payload delivery"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "mismatched socket ack-content must not become the saved snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "socket mismatch rejection must leave disk content untouched"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled"),
            "disabled full-content path should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(&root));
        drop(server);
    }

    fn init_git_repo(root: &Path) {
        use std::process::Command;
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "commit.gpgsign", "false"])
            .output()
            .unwrap();
    }

    fn git_commit_file(root: &Path, rel: &str, content: &str, msg: &str) {
        use std::process::Command;
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "--", rel])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", msg, "--no-verify"])
            .output()
            .unwrap();
    }

    fn head_count(root: &Path) -> usize {
        use std::process::Command;
        let out = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    #[test]
    fn recover_dedupe_only_drift_commits_when_file_matches_dedupe_of_head() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");

        // Simulate what `agent-doc dedupe` produced: file + snapshot both equal
        // the deduped form, HEAD still holds the duplicate.
        let deduped = crate::dedupe::dedupe_responses(duplicated);
        assert_ne!(
            deduped, duplicated,
            "test setup: duplicated content must actually dedupe"
        );
        fs::write(&doc, &deduped).unwrap();
        crate::snapshot::save(&doc, &deduped).unwrap();

        let head_before = head_count(root);
        let recovered =
            recover_dedupe_only_drift(&doc).expect("dedupe-only drift recovery should succeed");
        assert!(
            recovered,
            "file matching dedupe(HEAD) must be recognized as a dedupe-only drift"
        );

        // Commit landed through the binary path.
        let head_after = head_count(root);
        assert_eq!(
            head_after,
            head_before + 1,
            "dedupe-only recovery must produce exactly one new commit"
        );
        let head_content = crate::git::show_head(&doc).unwrap().unwrap();
        assert_eq!(
            head_content.matches("### Re: topic — opus-4-7").count(),
            1,
            "committed HEAD must hold the deduped response"
        );
        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snapshot_after.matches("### Re: topic — opus-4-7").count(),
            1,
            "snapshot must hold the deduped response (boundary markers may differ from disk)"
        );
    }

    #[test]
    fn recover_dedupe_only_drift_skips_when_file_matches_head() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let clean = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", clean, "add clean");
        let doc = root.join("session.md");
        crate::snapshot::save(&doc, clean).unwrap();

        let recovered = recover_dedupe_only_drift(&doc).unwrap();
        assert!(
            !recovered,
            "no drift between file and HEAD should not trigger dedupe-only recovery"
        );
    }

    #[test]
    fn recover_dedupe_only_drift_skips_when_drift_is_not_a_dedupe_outcome() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let original = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", original, "add original");
        let doc = root.join("session.md");

        // Working tree differs from HEAD by an arbitrary user edit, not by
        // dedupe. Recovery must refuse so we don't auto-commit unrelated drift.
        let user_edit = original.replace("Implemented.", "Implemented and tested.");
        fs::write(&doc, &user_edit).unwrap();
        crate::snapshot::save(&doc, &user_edit).unwrap();

        let recovered = recover_dedupe_only_drift(&doc).unwrap();
        assert!(
            !recovered,
            "arbitrary working-tree drift must not be auto-committed as a dedupe recovery"
        );
    }

    // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
    // Phase 4 + Phase 5 regression coverage. Exercises the full
    // `agent-doc dedupe` → `agent-doc write --commit` (empty stdin) recovery
    // path through the strict-closeout entry point that the four `run` /
    // `stream` / `write` call sites use.
    #[test]
    fn recover_empty_response_for_strict_closeout_lands_dedupe_only_drift_through_binary_commit() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");

        let deduped = crate::dedupe::dedupe_responses(duplicated);
        fs::write(&doc, &deduped).unwrap();
        crate::snapshot::save(&doc, &deduped).unwrap();

        let strict = WriteFlags {
            strict_closeout: true,
            ..Default::default()
        };
        let head_before = head_count(root);
        let recovered = recover_empty_response_for_strict_closeout(&doc, &strict)
            .expect("strict-closeout empty-stdin path should recognize dedupe-only drift");
        assert!(
            recovered,
            "empty stdin + strict closeout + dedupe-only drift must commit through the binary path"
        );
        assert_eq!(
            head_count(root),
            head_before + 1,
            "exactly one new commit should land via the dedupe recovery wrapper"
        );

        let head_after = crate::git::show_head(&doc).unwrap().unwrap();
        assert_eq!(
            head_after.matches("### Re: topic — opus-4-7").count(),
            1,
            "committed HEAD must hold the deduped response"
        );
    }

    #[test]
    fn recover_empty_response_for_strict_closeout_refuses_when_not_strict_closeout() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");
        let deduped = crate::dedupe::dedupe_responses(duplicated);
        fs::write(&doc, &deduped).unwrap();
        crate::snapshot::save(&doc, &deduped).unwrap();

        let lenient = WriteFlags::default();
        let head_before = head_count(root);
        let recovered = recover_empty_response_for_strict_closeout(&doc, &lenient).unwrap();
        assert!(
            !recovered,
            "non-strict empty-stdin path must not silently auto-commit dedupe drift"
        );
        assert_eq!(
            head_count(root),
            head_before,
            "non-strict path should not produce a commit"
        );
    }
