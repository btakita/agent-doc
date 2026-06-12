    use super::*;
    use crate::component::is_backlog_component;
    use agent_doc_core::topic::parse_topic_sections;

    #[test]
    fn parse_exchanges_basic() {
        let body = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\nBye\n\n## Assistant\n\nGoodbye\n\n## User\n\n";
        let exchanges = parse_exchanges(body);
        assert_eq!(exchanges.len(), 2);
        assert_eq!(exchanges[0].user, "Hello");
        assert_eq!(exchanges[0].assistant, "Hi there");
        assert_eq!(exchanges[1].user, "Bye");
        assert_eq!(exchanges[1].assistant, "Goodbye");
    }

    #[test]
    fn parse_exchanges_with_code_blocks() {
        let body = "## User\n\nHere's code:\n\n```\n## User\n## Assistant\n```\n\n## Assistant\n\nNice code\n\n## User\n\n";
        let exchanges = parse_exchanges(body);
        assert_eq!(exchanges.len(), 1);
        assert!(exchanges[0].user.contains("```"));
        assert!(exchanges[0].user.contains("## User"));
    }

    #[test]
    fn parse_exchanges_trailing_user_not_counted() {
        let body = "## User\n\nHello\n\n## Assistant\n\nHi\n\n## User\n\nPending question\n";
        let exchanges = parse_exchanges(body);
        // Only the complete exchange is counted, not the trailing User block
        assert_eq!(exchanges.len(), 1);
    }

    #[test]
    fn build_archive_format() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc_path = dir.path().join("session.md");
        std::fs::write(&doc_path, "---\nsession: test\n---\n").unwrap();
        let exchanges = vec![Exchange {
            user: "Hello".to_string(),
            assistant: "Hi there".to_string(),
        }];
        let archive = build_archive(&doc_path, "---\nsession: test\n---\n", &exchanges);
        assert!(archive.contains("archived_from: compact"));
        assert!(archive.contains("component: exchange"));
        assert!(archive.contains("session: test"));
        assert!(archive.contains("## User\n\nHello"));
        assert!(archive.contains("## Assistant\n\nHi there"));
    }

    #[test]
    fn build_compacted_format() {
        let kept = vec![Exchange {
            user: "Recent question".to_string(),
            assistant: "Recent answer".to_string(),
        }];
        let compacted = build_compacted(
            "---\ntest: true\n---\n\n",
            "\n",
            &kept,
            Path::new("archive.md"),
            3,
        );
        assert!(compacted.contains("3 earlier exchange(s) archived"));
        assert!(compacted.contains("## User\n\nRecent question"));
        assert!(compacted.contains("## Assistant\n\nRecent answer"));
        assert!(compacted.ends_with("## User\n\n"));
    }

    #[test]
    fn chrono_timestamp_format() {
        let ts = chrono_timestamp();
        // Should be YYYYMMDD-HHMMSS format
        assert_eq!(ts.len(), 15);
        assert_eq!(&ts[8..9], "-");
    }

    #[test]
    fn build_component_archive_format() {
        let doc = "---\nagent_doc_session: abc-123\nagent_doc_mode: stream\n---\n\n<!-- agent:exchange -->\nOld conversation\n<!-- /agent:exchange -->\n";
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc_path = dir.path().join("docs/session.md");
        std::fs::create_dir_all(doc_path.parent().unwrap()).unwrap();
        std::fs::write(&doc_path, doc).unwrap();
        let archive = build_component_archive(&doc_path, doc, "exchange", "\nOld conversation\n");
        assert!(archive.contains("archived_from: compact"));
        assert!(archive.contains("component: exchange"));
        assert!(archive.contains("document: docs/session.md"));
        assert!(archive.contains("session: abc-123"));
        assert!(archive.contains("Old conversation"));
    }

    #[test]
    fn parse_topic_sections_basic() {
        let content = "### Session Summary\n\nSome preamble.\n\n### Re: first topic\n\nFirst response.\n\n### Re: second topic\n\nSecond response.\n";
        let (preamble, sections) = parse_topic_sections(content);
        assert!(preamble.contains("Session Summary"));
        assert!(preamble.contains("Some preamble."));
        assert_eq!(sections.len(), 2);
        assert!(sections[0].starts_with("### Re: first topic"));
        assert!(sections[0].contains("First response."));
        assert!(sections[1].starts_with("### Re: second topic"));
        assert!(sections[1].contains("Second response."));
    }

    #[test]
    fn parse_topic_sections_keep_threshold() {
        let content = "### Re: topic 1\nResponse 1.\n### Re: topic 2\nResponse 2.\n";
        let (_, sections) = parse_topic_sections(content);
        // 2 sections ≤ keep=3 → no-op when called from run_component_compact_partial
        assert_eq!(sections.len(), 2);
    }

    #[test]
    fn parse_topic_sections_strips_boundary_marker() {
        let content = "### Re: last topic\n\nContent.\n<!-- agent:boundary:abc123 -->\n";
        let (_, sections) = parse_topic_sections(content);
        assert_eq!(sections.len(), 1);
        assert!(!sections[0].contains("agent:boundary"));
    }

    #[test]
    fn parse_topic_sections_no_re_headings() {
        let content = "Just preamble text.\nNo Re: headings here.\n";
        let (preamble, sections) = parse_topic_sections(content);
        assert!(preamble.contains("Just preamble text."));
        assert_eq!(sections.len(), 0);
    }

    #[test]
    fn partial_compact_preserves_trailing_prompt_after_boundary() {
        let doc = concat!(
            "---\nagent_doc_session: test-tail\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\nExisting summary.\n\n",
            "### Re: first topic\n\nResponse one.\n\n",
            "### Re: second topic\n\nResponse two.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "do #autocmp. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        run_component_compact_partial(&file, doc, "exchange", 1, None, false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let exchange = crate::component::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert!(exchange.contains("### Re: second topic"));
        assert!(!exchange.contains("### Re: first topic"));
        assert!(exchange.contains("do #autocmp. spec-test-build-install-commit-push"));

        let snapshot_after = snapshot::load(&file).unwrap().unwrap();
        assert!(
            !snapshot_after.contains("do #autocmp. spec-test-build-install-commit-push"),
            "unresolved trailing prompt must remain live drift after compact, not committed snapshot state:\n{snapshot_after}"
        );
    }

    #[test]
    fn full_exchange_compact_preserves_trailing_prompt_after_boundary() {
        let prompt = "do #fullcmp. spec-test-build-install-commit-push";
        let doc = format!(
            concat!(
                "---\nagent_doc_session: test-full-tail\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\nExisting summary.\n\n",
                "### Re: archived topic\n\nResponse body.\n",
                "<!-- agent:boundary:abc123 -->\n",
                "{prompt}\n",
                "<!-- /agent:exchange -->\n",
            ),
            prompt = prompt
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, &doc).unwrap();

        run_component_compact(&file, &doc, "exchange", None, false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let exchange = crate::component::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert!(exchange.contains("*Compacted. Content archived to `"));
        assert!(exchange.contains(prompt));
        assert!(
            !exchange.contains("Trailing prompt/context"),
            "compact summary must not summarize unresolved live prompt text:\n{exchange}"
        );

        let snapshot_after = snapshot::load(&file).unwrap().unwrap();
        assert!(
            !snapshot_after.contains(prompt),
            "unresolved trailing prompt must remain live drift after full compact, not committed snapshot state:\n{snapshot_after}"
        );

        let archive_dir = agent_doc_dir.join("archives");
        let archive_path = std::fs::read_dir(&archive_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "md"))
            .expect("compact should write an archive");
        let archive = std::fs::read_to_string(archive_path).unwrap();
        assert!(
            !archive.contains(prompt),
            "unresolved trailing prompt must not be archived:\n{archive}"
        );
    }

    #[test]
    fn component_compact_preserves_non_target_components() {
        let doc = concat!(
            "---\nagent_doc_session: test-123\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Status: active\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nLong response about topic one.\n\n",
            "### Re: topic two\n\nLong response about topic two.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending\n\n",
            "<!-- agent:pending -->\n",
            "- Task A: do something important\n",
            "- Task B: do something else\n",
            "- Task C: critical item\n",
            "<!-- /agent:pending -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        // Set up .agent-doc dirs
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        // Capture pending content before compact
        let components_before = component::parse(doc).unwrap();
        let pending_before = components_before
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(doc)
            .to_string();
        let status_before = components_before
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(doc)
            .to_string();

        // Run compact on exchange only
        run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false).unwrap();

        // Read the result and verify non-target components are byte-identical
        let result = std::fs::read_to_string(&file).unwrap();
        let components_after = component::parse(&result).unwrap();
        let pending_after = components_after
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&result)
            .to_string();
        let status_after = components_after
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(
            pending_before, pending_after,
            "pending component must be byte-identical after compact"
        );
        assert_eq!(
            status_before, status_after,
            "status component must be byte-identical after compact"
        );

        // Verify exchange was actually compacted
        let exchange_after = components_after
            .iter()
            .find(|c| c.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();
        assert!(exchange_after.contains("Compacted summary."));
        assert!(!exchange_after.contains("### Re: topic one"));
    }

    #[test]
    fn component_compact_preserves_summary_leading_code_fence() {
        let doc = concat!(
            "---\nagent_doc_session: test-compact-fence\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ show fenced prompt\n",
            "```\n",
            "prompt body\n",
            "```\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        run_component_compact(
            &file,
            doc,
            "exchange",
            Some("```\ncompacted summary\n```"),
            false,
        )
        .unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let exchange_after = crate::component::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(
            exchange_after.matches("```").count(),
            2,
            "compact summary fences must survive:\n{exchange_after}"
        );
        assert!(
            exchange_after.starts_with("```\ncompacted summary\n```\n"),
            "compact summary leading fence must remain first content:\n{exchange_after}"
        );
    }

    #[test]
    fn component_compact_preserves_post_exchange_scratch_comment() {
        let prompt = "The compact exchange scratch comment should not be deleted. #spec-test-build-install-commit-push";
        let doc = format!(
            concat!(
                "---\nagent_doc_session: test-compact-scratch\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: topic one\n\nResponse one.\n\n",
                "### Re: topic two\n\nResponse two.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Keep compact scratch notes visible.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#aaaa] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, &doc).unwrap();

        run_component_compact(&file, &doc, "exchange", Some("Compacted summary."), false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        assert!(
            result.contains(&format!(
                "<!--\n{prompt}\n#spec-test-build-install-commit-push\n---\nKeep compact scratch notes visible.\n-->"
            )),
            "compact exchange must leave post-exchange scratch comments outside the compacted component:\n{result}"
        );
        let snapshot_after = snapshot::load(&file).unwrap().unwrap();
        assert!(
            snapshot_after.contains("Keep compact scratch notes visible."),
            "compact snapshot should preserve owned post-exchange scratch comments:\n{snapshot_after}"
        );
    }

    #[test]
    fn component_compact_uses_guarded_direct_write_when_patches_dir_exists() {
        let doc = concat!(
            "---\nagent_doc_session: test-ipc\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        let patches_dir = agent_doc_dir.join("patches");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(&patches_dir).unwrap();
        snapshot::save(&file, doc).unwrap();

        run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false).unwrap();
        let compacted = std::fs::read_to_string(&file).unwrap();
        let patch_count = std::fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            })
            .count();

        assert!(compacted.contains("Compacted summary."));
        assert!(!compacted.contains("### Re: topic one"));
        assert_eq!(patch_count, 0, "compact must not emit fullContent IPC");
        assert_eq!(snapshot::load(&file).unwrap().unwrap(), compacted);
    }

    #[test]
    fn component_compact_direct_write_is_not_blocked_by_previous_cycle_committed() {
        let doc = concat!(
            "---\nagent_doc_session: test-ipc-committed\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        let patches_dir = agent_doc_dir.join("patches");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        std::fs::create_dir_all(&patches_dir).unwrap();
        snapshot::save(&file, doc).unwrap();
        crate::cycle_state::start_preflight(&file, Some(doc), Some(doc)).unwrap();
        crate::cycle_state::mark_response_captured(
            &file,
            "test",
            Some(doc),
            Some(doc),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&file, "test", Some(doc), Some(doc)).unwrap();
        crate::cycle_state::mark_committed(&file, "test", Some(doc), Some(doc)).unwrap();

        run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false).unwrap();
        let compacted = std::fs::read_to_string(&file).unwrap();
        let patch_count = std::fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            })
            .count();

        assert!(compacted.contains("Compacted summary."));
        assert!(!compacted.contains("### Re: topic one"));
        assert_eq!(patch_count, 0, "compact must not emit fullContent IPC");
        assert_eq!(snapshot::load(&file).unwrap().unwrap(), compacted);
        let ops_log =
            std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            !ops_log.contains("late_fallback_patch_rejected"),
            "operator compact is not a stale response fallback"
        );
    }

    #[test]
    fn component_compact_direct_fallback_rejects_late_visible_edit() {
        let doc = concat!(
            "---\nagent_doc_session: test-compact-cas\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n",
        );
        let live = doc.replace(
            "<!-- /agent:exchange -->",
            "live prompt typed during compact\n<!-- /agent:exchange -->",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &live).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        snapshot::save(&file, doc).unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        assert!(
            err.to_string().contains("document changed after"),
            "compact fallback should fail with visible-current CAS error: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            live,
            "compact fallback must not overwrite a prompt typed after compaction was computed"
        );
        assert_eq!(
            snapshot::load(&file).unwrap().unwrap(),
            doc,
            "failed compact fallback must not advance the snapshot"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("visible_write_deferred_current_changed")
                && ops_log.contains("source=compact_exchange_direct_write"),
            "compact CAS rejection should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn component_compact_direct_fallback_rejects_idle_unsaved_editor_buffer() {
        let doc = concat!(
            "---\nagent_doc_session: test-compact-live-buffer\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n",
        );
        let live_buffer = doc.replace(
            "<!-- /agent:exchange -->",
            "prompt typed in JetBrains but not saved\n<!-- /agent:exchange -->",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        snapshot::save(&file, doc).unwrap();
        let file_str = file.canonicalize().unwrap().to_string_lossy().to_string();
        crate::debounce::record_live_buffer_digest(
            &file_str,
            live_buffer.len(),
            &crate::debounce::content_hash(&live_buffer),
        )
        .unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        assert!(
            err.to_string().contains("visible editor buffer"),
            "compact should reject idle unsaved editor drift before disk write: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            doc,
            "compact must not rewrite disk while the editor-visible buffer is unsaved"
        );
        assert_eq!(
            snapshot::load(&file).unwrap().unwrap(),
            doc,
            "failed compact must not advance the snapshot"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("visible_write_deferred_live_buffer_changed")
                && ops_log.contains("source=compact_exchange_direct_write"),
            "compact live-buffer rejection should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn component_compact_rejects_stale_editor_cache_when_snapshot_is_stale() {
        let stale_snapshot = concat!(
            "---\nagent_doc_session: test-compact-stale-cache\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = stale_snapshot.replace(
            "<!-- /agent:exchange -->",
            "### Re: topic two\n\nResponse two.\n<!-- /agent:exchange -->",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &current).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        snapshot::save(&file, stale_snapshot).unwrap();
        let file_str = file.canonicalize().unwrap().to_string_lossy().to_string();
        crate::debounce::record_live_buffer_digest(
            &file_str,
            stale_snapshot.len(),
            &crate::debounce::content_hash(stale_snapshot),
        )
        .unwrap();

        let err = run_component_compact(
            &file,
            &current,
            "exchange",
            Some("Compacted summary."),
            false,
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("visible editor buffer"),
            "compact should reject a stale editor cache before writing: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            current,
            "compact must not overwrite the current file when JetBrains still advertises stale cache content"
        );
        assert_eq!(
            snapshot::load(&file).unwrap().unwrap(),
            stale_snapshot,
            "failed compact must not advance a stale snapshot"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("visible_write_deferred_live_buffer_changed")
                && ops_log.contains("source=compact_exchange_direct_write"),
            "stale-cache compact rejection should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn component_compact_direct_fallback_rejects_late_post_exchange_scratch_comment() {
        let prompt = "The post-exchange scratch comment was typed while compact exchange was being computed. #spec-test-build-install-commit-push";
        let doc = concat!(
            "---\nagent_doc_session: test-compact-comment-cas\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "-->\n"
        );
        let live = doc.replace("<!--\n-->", &format!("<!--\n{prompt}\n-->"));

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &live).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        snapshot::save(&file, doc).unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        assert!(
            err.to_string().contains("document changed after"),
            "compact fallback should fail with visible-current CAS error: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            live,
            "compact fallback must not overwrite scratch comments typed after compaction was computed"
        );
        assert_eq!(
            snapshot::load(&file).unwrap().unwrap(),
            doc,
            "failed compact fallback must not advance the snapshot"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("visible_write_deferred_current_changed")
                && ops_log.contains("source=compact_exchange_direct_write"),
            "compact CAS rejection should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn component_compact_rejects_cycle_1779845677327_scratch_directive_race() {
        let scratch_prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Reproduce bugs with tests first that fail and fix the implementation.";
        let scratch_directive = "#spec-test-build-install-commit-push";
        let scratch_dispatch = "dispatch #spec-test-build-install-commit-push";
        let doc = concat!(
            "---\nagent_doc_session: cycle-1779845677327\nagent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "-->\n\n",
            "<!-- agent:queue auto -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- do [#liveipcrace]\n",
            "<!-- /agent:queue -->\n"
        );
        let live_scratch =
            format!("<!--\n{scratch_prompt}\n{scratch_directive}\n---\n{scratch_dispatch}\n-->");
        let live = doc.replace("<!--\n-->", &live_scratch);

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &live).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        let patches_dir = agent_doc_dir.join("patches");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        std::fs::create_dir_all(&patches_dir).unwrap();
        snapshot::save(&file, doc).unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        assert!(
            err.to_string().contains("document changed after"),
            "compact fallback should fail with visible-current CAS error: {err}"
        );
        let file_after = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            file_after, live,
            "compact fallback must not overwrite cycle-1779845677327 scratch directives"
        );
        assert_eq!(
            file_after.matches(scratch_prompt).count(),
            1,
            "scratch prompt text must not be duplicated or deleted:\n{file_after}"
        );
        assert_eq!(
            file_after.matches(&live_scratch).count(),
            1,
            "prompt preset and dispatch directives in the scratch comment must remain intact:\n{file_after}"
        );
        assert_eq!(
            snapshot::load(&file).unwrap().unwrap(),
            doc,
            "failed compact fallback must not advance the snapshot to the shorter or live buffer"
        );
        let patch_count = std::fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            })
            .count();
        assert_eq!(
            patch_count, 0,
            "compact race handling must not emit file IPC or fullContent payloads"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("visible_write_deferred_current_changed")
                && ops_log.contains("source=compact_exchange_direct_write"),
            "compact CAS rejection should be logged:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("snapshot_absorb"),
            "compact race handling must not silently absorb a shorter disk snapshot:\n{ops_log}"
        );
    }

    #[test]
    fn exchange_compact_default_summary_includes_archived_content_digest() {
        let doc = concat!(
            "---\nagent_doc_session: test-summary\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do #next. run targeted test\n",
            "- do #later. build and install\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#rpaj] Make compact exchange synthesize backlog-aware context\n",
            "- [/release] [#ship] Wait for release window\n",
            "- [x] [#done] Already handled\n",
            "<!-- /agent:backlog -->\n\n",
            "## Icebox\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#parked] Parked follow-up for later\n",
            "<!-- /agent:icebox -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        run_component_compact(&file, doc, "exchange", None, false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let components = component::parse(&result).unwrap();
        let exchange = components
            .iter()
            .find(|c| c.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert!(exchange.contains("### Session Summary"));
        assert!(exchange.contains("*Compacted. Content archived to `"));
        assert!(exchange.contains("Compacted content:"));
        assert!(exchange.contains("Archived 2 response topic(s): topic one; topic two"));
        assert!(!exchange.contains("Active backlog:"));
        assert!(!exchange.contains("[#rpaj]"));
        assert!(!exchange.contains("Queue:"));
        assert!(!exchange.contains("Icebox:"));
        assert!(!exchange.contains("### Re: topic one"));
    }

    #[test]
    fn exchange_compact_reconciles_stale_top_backlog_status() {
        let doc = concat!(
            "---\nagent_doc_session: test-summary\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Top backlog item: #done.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        run_component_compact(&file, doc, "exchange", Some("Compacted."), false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        assert!(result.contains("No open backlog items."));
        assert!(!result.contains("Top backlog item: #done."));
        let snap = snapshot::load(&file).unwrap().unwrap();
        assert!(snap.contains("No open backlog items."));
    }

    #[test]
    fn crdt_compact_preserves_pending_with_state_refresh() {
        // Test that CRDT state refresh prevents pending items from being lost
        let doc = concat!(
            "---\nagent_doc_session: test-crdt-123\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n\n",
            "### Re: topic three\n\nResponse three.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending\n\n",
            "<!-- agent:pending -->\n",
            "- ✅ completed task\n",
            "- 🔄 in-progress work\n",
            "- 🆕 new task to add\n",
            "<!-- /agent:pending -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        // Set up .agent-doc dirs and save initial CRDT state
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        snapshot::save(&file, doc).unwrap();

        // Create and save initial CRDT state
        let initial_crdt = crate::crdt::CrdtDoc::from_text(doc).encode_state();
        snapshot::save_document_crdt(&file, &initial_crdt, doc).unwrap();

        // Capture pending before compact
        let components_before = component::parse(doc).unwrap();
        let pending_before = components_before
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(doc)
            .to_string();

        // Run compact with CRDT mode enabled (is_crdt=true)
        run_component_compact(&file, doc, "exchange", Some("Compacted."), true).unwrap();

        // Read result and verify pending survives
        let result = std::fs::read_to_string(&file).unwrap();
        let components_after = component::parse(&result).unwrap();
        let pending_after = components_after
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(
            pending_before, pending_after,
            "pending component must survive CRDT state refresh during compact"
        );
        assert!(pending_after.contains("completed task"));
        assert!(pending_after.contains("in-progress work"));
        assert!(pending_after.contains("new task to add"));
    }

    #[test]
    fn compact_preserves_boundary_marker() {
        // Test that boundary markers (❯) survive compact operations
        let doc = concat!(
            "---\nagent_doc_session: test-boundary\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first topic\n\nResponse one.\n\n",
            "### Re: second topic\n\nResponse two.\n",
            "<!-- agent:boundary:abc123def456 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "❯ Critical item: verify preservation\n",
            "<!-- /agent:status -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        // Capture status with ❯ before compact
        let components_before = component::parse(doc).unwrap();
        let status_before = components_before
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(doc)
            .to_string();

        run_component_compact(&file, doc, "exchange", Some("Archived."), false).unwrap();

        // Verify ❯ marker preserved in non-target component
        let result = std::fs::read_to_string(&file).unwrap();
        let components_after = component::parse(&result).unwrap();
        let status_after = components_after
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(status_before, status_after);
        assert!(status_after.contains("❯"));
        assert!(status_after.contains("Critical item"));
    }

    #[test]
    fn compact_working_tree_consistency() {
        // Test that compact leaves working tree in consistent state
        // (file unchanged vs disk, snapshot updated, no stale CRDT)
        let doc = concat!(
            "---\nagent_doc_session: test-wt\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic A\n\nResponse A.\n\n",
            "### Re: topic B\n\nResponse B.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        snapshot::save(&file, doc).unwrap();

        let file_before = std::fs::read_to_string(&file).unwrap();

        run_component_compact(&file, doc, "exchange", Some("Summary."), false).unwrap();

        // After compact: file and snapshot should match
        let file_after = std::fs::read_to_string(&file).unwrap();
        let snap_path = snapshot::path_for(&file).unwrap();
        let snapshot_content = std::fs::read_to_string(&snap_path).unwrap();

        assert_eq!(
            file_after, snapshot_content,
            "file and snapshot must match after compact"
        );

        // Verify the document was actually modified
        assert_ne!(file_before, file_after);
        assert!(file_after.contains("Summary."));
    }

    #[test]
    fn compact_with_commit_writes_vcs_refresh_signal() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        std::process::Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let file = root.join("session.md");
        let doc = concat!(
            "---\nagent_doc_session: test-compact\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&file, doc).unwrap();
        snapshot::save(&file, doc).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        run(
            &file,
            None,
            Some("exchange"),
            Some("Compacted summary."),
            Some("skip"),
            true,
        )
        .unwrap();

        let signal = root.join(".agent-doc/patches/vcs-refresh.signal");
        assert!(
            signal.exists(),
            "expected VCS refresh signal at {}",
            signal.display()
        );

        let log = std::process::Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-n", "1", "--", "session.md"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&log.stdout);
        assert!(
            log.contains("agent-doc(session):"),
            "compact closeout should use agent-doc commit, got:\n{log}"
        );

        let committed = crate::git::show_head(&file).unwrap().unwrap();
        assert!(committed.contains("Compacted summary."));
        assert!(!committed.contains("### Re: topic one"));
    }

    #[test]
    fn malformed_compact_summary_detects_prompt_prefixed_summary() {
        // #jb-compact-malformed-response-commit: the live repro shape.
        let doc = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ *Compacted. Content archived to `.agent-doc/archives/x.md`*\n",
            "### Re: #next-steps-2 — gpt-5\n\nLeftover archived body.\n",
            "<!-- /agent:exchange -->\n",
        );
        let malformed = malformed_compact_summary_lines(doc);
        assert_eq!(malformed.len(), 1, "{malformed:?}");
        assert!(malformed[0].contains("Compacted"), "{malformed:?}");
    }

    #[test]
    fn malformed_compact_summary_accepts_clean_summary() {
        let doc = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Session Summary\n\n",
            "*Compacted. Content archived to `.agent-doc/archives/x.md`*\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(
            malformed_compact_summary_lines(doc).is_empty(),
            "clean compact summary must not be flagged"
        );
    }

    #[test]
    fn apply_compacted_document_fails_closed_on_malformed_summary() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let file = root.join("doc.md");
        let source = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n❯ prompt\n### Re: prompt — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, source).unwrap();
        let malformed_compacted = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n❯ *Compacted. Content archived to `x.md`*\n<!-- /agent:exchange -->\n",
        );
        let err = apply_compacted_document(
            &file,
            malformed_compacted,
            malformed_compacted,
            source,
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("post-compact exchange is malformed"),
            "{err}"
        );
        // The malformed content must NOT have been written.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), source);
    }

    #[test]
    fn compact_without_commit_records_uncommitted_warning() {
        // #jb-compact-repair-left-uncommitted: compact without --commit rewrites
        // the doc but must not silently leave it dirty — it records an explicit
        // uncommitted-state diagnostic with the recovery command.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let file = root.join("session.md");
        let doc = concat!(
            "---\nagent_doc_session: test-compact\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();
        snapshot::save(&file, doc).unwrap();

        run(
            &file,
            None,
            Some("exchange"),
            Some("Compacted summary."),
            Some("skip"),
            false, // no --commit
        )
        .unwrap();

        let after = std::fs::read_to_string(&file).unwrap();
        assert!(
            after.contains("Compacted summary."),
            "doc should be compacted"
        );
        let ops_log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("compact_left_uncommitted"),
            "uncommitted compact must be recorded, got:\n{ops_log}"
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {:?} failed", args);
    }

    #[test]
    fn create_pre_mutation_tag_auto_increments_ordinal_per_slug() {
        // #misfire-recovery-snapshot: the shared pre-mutation checkpoint tag
        // auto-generates `agent-doc/<doc>/<slug>-N`, incrementing N per slug.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@t.t"]);
        git(root, &["config", "user.name", "t"]);
        let doc_path = root.join("session.md");
        std::fs::write(&doc_path, "---\nsession: test\n---\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "init"]);

        create_pre_mutation_tag(&doc_path, "pre-auto-run", None).unwrap();
        create_pre_mutation_tag(&doc_path, "pre-auto-run", None).unwrap();
        // A different slug starts its own ordinal series.
        create_pre_mutation_tag(&doc_path, "pre-compact", None).unwrap();

        let tags = String::from_utf8(
            Command::new("git")
                .current_dir(root)
                .args(["tag", "-l"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert!(
            tags.contains("agent-doc/session/pre-auto-run-1"),
            "tags: {tags}"
        );
        assert!(
            tags.contains("agent-doc/session/pre-auto-run-2"),
            "tags: {tags}"
        );
        assert!(
            tags.contains("agent-doc/session/pre-compact-1"),
            "tags: {tags}"
        );
    }

    #[test]
    fn create_pre_mutation_tag_honors_override_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@t.t"]);
        git(root, &["config", "user.name", "t"]);
        let doc_path = root.join("session.md");
        std::fs::write(&doc_path, "x").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "init"]);

        create_pre_mutation_tag(&doc_path, "pre-auto-run", Some("my-checkpoint")).unwrap();
        let tags = String::from_utf8(
            Command::new("git")
                .current_dir(root)
                .args(["tag", "-l"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert!(tags.contains("my-checkpoint"), "tags: {tags}");
    }
