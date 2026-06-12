    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_no_frontmatter() {
        let content = "# Hello\n\nBody text.\n";
        let (fm, body) = parse(content).unwrap();
        assert!(fm.session.is_none());
        assert!(fm.agent.is_none());
        assert!(fm.model.is_none());
        assert!(fm.branch.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn parse_all_fields() {
        let content =
            "---\nsession: abc-123\nagent: claude\nmodel: opus\nbranch: main\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc-123"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert_eq!(fm.model.as_deref(), Some("opus"));
        assert_eq!(fm.branch.as_deref(), Some("main"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn queue_control_parse_aliases() {
        assert_eq!(QueueControl::parse("start"), Some(QueueControl::Start));
        assert_eq!(QueueControl::parse("go"), Some(QueueControl::Start));
        assert_eq!(QueueControl::parse("STOP"), Some(QueueControl::Stop));
        assert_eq!(QueueControl::parse("  Start  "), Some(QueueControl::Start));
        assert_eq!(QueueControl::parse("auto"), None);
        assert_eq!(QueueControl::parse(""), None);
        assert!(QueueControl::Start.is_active());
        assert!(!QueueControl::Stop.is_active());
    }

    #[test]
    fn parse_queue_start_activates() {
        // `queue: start` (canonical) folds onto the deprecated queue_active bool
        // so every existing reader honors it (#queue-state-unify).
        let (fm, _) = parse("---\nagent_doc_format: template\nqueue: start\n---\n\n").unwrap();
        assert_eq!(fm.queue.as_deref(), Some("start"));
        assert_eq!(fm.queue_active, Some(true));
    }

    #[test]
    fn parse_queue_go_alias_activates() {
        let (fm, _) = parse("---\nqueue: go\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, Some(true));
    }

    #[test]
    fn parse_queue_stop_deactivates() {
        let (fm, _) = parse("---\nqueue: stop\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, Some(false));
    }

    #[test]
    fn parse_queue_canonical_wins_over_stale_queue_active() {
        // An explicit `queue:` control overrides a stale `queue_active:` line.
        let (fm, _) = parse("---\nqueue: stop\nqueue_active: true\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, Some(false));

        let (fm, _) = parse("---\nqueue: start\nqueue_active: false\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, Some(true));
    }

    #[test]
    fn pipeline_is_empty_default() {
        let p = AgentDocPipeline::default();
        assert!(p.is_empty());
    }

    #[test]
    fn parse_pipeline_all_fields() {
        let content = "---\nagent_doc_pipeline:\n  turn_id: \"#fm-run-id-step\"\n  run_id: cycle-1780600334196\n  step: response_captured\n  queue_task_id: \"#fm-run-id-step\"\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.pipeline.turn_id.as_deref(), Some("#fm-run-id-step"));
        assert_eq!(fm.pipeline.run_id.as_deref(), Some("cycle-1780600334196"));
        assert_eq!(fm.pipeline.step.as_deref(), Some("response_captured"));
        assert_eq!(
            fm.pipeline.queue_task_id.as_deref(),
            Some("#fm-run-id-step")
        );
        assert!(!fm.pipeline.is_empty());
    }

    #[test]
    fn empty_pipeline_is_not_serialized() {
        // A drained/terminal cycle must leave no `agent_doc_pipeline:` block behind.
        let fm = Frontmatter {
            session: Some("abc".to_string()),
            ..Default::default()
        };
        let out = write(&fm, "Body\n").unwrap();
        assert!(!out.contains("agent_doc_pipeline"));
    }

    #[test]
    fn set_pipeline_state_merges_without_disturbing_other_fields() {
        let content =
            "---\nagent_doc_session: abc-123\nagent: claude\nqueue: start\n---\n\n## Body\n";
        // Set run_id + step, leave turn_id/queue_task_id untouched.
        let updated =
            set_pipeline_state(content, Some("cycle-99"), Some("write_applied"), None, None)
                .unwrap();
        let (fm, body) = parse(&updated).unwrap();
        assert_eq!(fm.pipeline.run_id.as_deref(), Some("cycle-99"));
        assert_eq!(fm.pipeline.step.as_deref(), Some("write_applied"));
        assert!(fm.pipeline.turn_id.is_none());
        // Other frontmatter fields and the body survive untouched.
        assert_eq!(fm.session.as_deref(), Some("abc-123"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert_eq!(fm.queue_active, Some(true));
        assert!(body.contains("## Body"));

        // A second merge updates step + turn_id while preserving the prior run_id.
        let updated2 = set_pipeline_state(
            &updated,
            None,
            Some("committed"),
            Some("#fm-run-id-step"),
            None,
        )
        .unwrap();
        let (fm2, _) = parse(&updated2).unwrap();
        assert_eq!(fm2.pipeline.run_id.as_deref(), Some("cycle-99"));
        assert_eq!(fm2.pipeline.step.as_deref(), Some("committed"));
        assert_eq!(fm2.pipeline.turn_id.as_deref(), Some("#fm-run-id-step"));
    }

    #[test]
    fn splice_pipeline_block_is_byte_precise_and_strip_cancels() {
        // #22a8: the write-side splice must touch ONLY the pipeline block, and
        // the diff-side strip must cancel it back to the original bytes.
        let base = "---\nagent_doc_session: abc-123\nagent: claude\nprompt_presets:\n  '#x': do the thing\nqueue: start\n---\n\n## Body\n- item\n";
        let pipeline = AgentDocPipeline {
            run_id: Some("cycle-99".into()),
            step: Some("response_captured".into()),
            turn_id: Some("#22a8".into()),
            queue_task_id: None,
        };
        let spliced = splice_pipeline_block(base, &pipeline).unwrap();
        // The block is present and parseable…
        assert!(spliced.contains("agent_doc_pipeline:"));
        let (fm, _) = parse(&spliced).unwrap();
        assert_eq!(fm.pipeline.run_id.as_deref(), Some("cycle-99"));
        assert_eq!(fm.pipeline.turn_id.as_deref(), Some("#22a8"));
        // …every non-block field/byte is preserved (presets, order, body).
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        // Stripping the block yields the ORIGINAL bytes exactly.
        assert_eq!(strip_pipeline_block_lines(&spliced), base);
        // Clearing (empty pipeline) also returns the original bytes.
        assert_eq!(
            splice_pipeline_block(&spliced, &AgentDocPipeline::default()).unwrap(),
            base
        );
        // Idempotent: re-splicing the same state does not stack blocks.
        let twice = splice_pipeline_block(&spliced, &pipeline).unwrap();
        assert_eq!(twice.matches("agent_doc_pipeline:").count(), 1);
    }

    #[test]
    fn clear_pipeline_state_drops_block_only() {
        let content = "---\nagent_doc_session: abc-123\nagent_doc_pipeline:\n  run_id: cycle-1\n  step: response_captured\n---\n\nBody\n";
        let cleared = clear_pipeline_state(content).unwrap();
        assert!(!cleared.contains("agent_doc_pipeline"));
        let (fm, body) = parse(&cleared).unwrap();
        assert!(fm.pipeline.is_empty());
        assert_eq!(fm.session.as_deref(), Some("abc-123"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn parse_queue_unknown_value_leaves_queue_active_untouched() {
        // A typo must never silently flip activation.
        let (fm, _) = parse("---\nqueue: bogus\nqueue_active: true\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, Some(true));
        let (fm, _) = parse("---\nqueue: bogus\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, None);
    }

    #[test]
    fn write_emits_canonical_queue_for_direct_set_queue_active() {
        // #writer-emits-deprecated-queue-active-false: a path that sets
        // `queue_active` directly on the struct (bypassing merge_queue_state) and
        // then calls write() must still emit the canonical `queue:` control, never
        // the deprecated `queue_active:` line.
        let mut fm = Frontmatter {
            queue_active: Some(true),
            ..Default::default()
        };
        let out = write(&fm, "body\n").unwrap();
        assert!(out.contains("queue: start"), "{out}");
        assert!(!out.contains("queue_active:"), "{out}");

        fm.queue_active = Some(false);
        let out = write(&fm, "body\n").unwrap();
        assert!(out.contains("queue: stop"), "{out}");
        assert!(!out.contains("queue_active:"), "{out}");

        // A doc parsed from canonical `queue:` (queue_active mirrored on) must
        // not double-emit the deprecated line on re-serialize.
        let (fm, body) =
            parse("---\nagent_doc_format: template\nqueue: start\n---\n\nx\n").unwrap();
        let out = write(&fm, &body).unwrap();
        assert!(out.contains("queue: start"), "{out}");
        assert!(!out.contains("queue_active:"), "{out}");

        // No queue state → neither key is emitted.
        let fm = Frontmatter::default();
        let out = write(&fm, "body\n").unwrap();
        assert!(
            !out.contains("queue_active:") && !out.contains("queue:"),
            "{out}"
        );
    }

    #[test]
    fn merge_queue_state_writes_canonical_and_drops_legacy() {
        // #queue-state-unify phase 4: writer emits `queue: start|stop` and
        // removes any legacy `queue_active:` line so `queue:` is the sole field.
        let legacy = "---\nagent_doc_format: template\nqueue_active: true\n---\n\nbody\n";
        let active = merge_queue_state(legacy, true).unwrap();
        assert!(active.contains("queue: start"), "{active}");
        assert!(!active.contains("queue_active:"), "{active}");

        let stopped = merge_queue_state(legacy, false).unwrap();
        assert!(stopped.contains("queue: stop"), "{stopped}");
        assert!(!stopped.contains("queue_active:"), "{stopped}");

        // Round-trips back to the internal queue_active for readers.
        let (fm, _) = parse(&active).unwrap();
        assert_eq!(fm.queue_active, Some(true));
        let (fm, _) = parse(&stopped).unwrap();
        assert_eq!(fm.queue_active, Some(false));
    }

    #[test]
    fn strip_deprecated_queue_active_line_drops_legacy_when_canonical_present() {
        // #queue-active-deprecated-line-stuck: a doc carrying BOTH `queue:` and the
        // deprecated `queue_active:` (the stuck shape the diff layer never lets a
        // normal write remove) has the legacy line dropped byte-precisely, leaving
        // every other field untouched.
        let stuck = "---\nagent: claude\nqueue: start\nqueue_active: true\n---\n\nbody\n";
        let out = strip_deprecated_queue_active_line(stuck);
        assert_eq!(
            out, "---\nagent: claude\nqueue: start\n---\n\nbody\n",
            "legacy line dropped, canonical queue + other fields preserved"
        );
        // Idempotent.
        assert_eq!(strip_deprecated_queue_active_line(&out), out);
    }

    #[test]
    fn strip_deprecated_queue_active_line_keeps_legacy_without_canonical() {
        // Without a canonical `queue:` the legacy line is the only state — never
        // drop it (that would lose the queue state).
        let legacy_only = "---\nqueue_active: true\n---\n\nbody\n";
        assert_eq!(strip_deprecated_queue_active_line(legacy_only), legacy_only);
        // No frontmatter at all → unchanged.
        assert_eq!(
            strip_deprecated_queue_active_line("no frontmatter\nqueue_active: true\n"),
            "no frontmatter\nqueue_active: true\n"
        );
    }

    #[test]
    fn parse_legacy_queue_active_still_honored() {
        // Back-compat: docs without the canonical key keep working.
        let (fm, _) = parse("---\nqueue_active: true\n---\n\n").unwrap();
        assert_eq!(fm.queue.as_deref(), None);
        assert_eq!(fm.queue_active, Some(true));
    }

    #[test]
    fn parse_partial_fields() {
        let content = "---\nsession: xyz\n---\n# Doc\n";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("xyz"));
        assert!(fm.agent.is_none());
        assert!(body.contains("# Doc"));
    }

    #[test]
    fn parse_model_tier_high() {
        let content = "---\nagent_doc_model_tier: high\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model_tier, Some(Tier::High));
    }

    #[test]
    fn parse_collaboration_mode_shared() {
        let content = "---\nagent_doc_collaboration: shared\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.collaboration_mode(), CollaborationMode::Shared);
    }

    #[test]
    fn parse_security_review_roundtrip() {
        let content = "---\nagent_doc_collaboration: shared\nagent_doc_security_review: sec-2026-04-29\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.collaboration_mode(), CollaborationMode::Shared);
        assert_eq!(fm.security_review.as_deref(), Some("sec-2026-04-29"));
        assert!(fm.has_security_review());
        let written = write(&fm, "Body\n").unwrap();
        assert!(written.contains("agent_doc_collaboration: shared"));
        assert!(written.contains("agent_doc_security_review: sec-2026-04-29"));
    }

    #[test]
    fn parse_model_tier_low() {
        let content = "---\nagent_doc_model_tier: low\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model_tier, Some(Tier::Low));
    }

    #[test]
    fn parse_model_tier_med() {
        let content = "---\nagent_doc_model_tier: med\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model_tier, Some(Tier::Med));
    }

    #[test]
    fn parse_model_tier_auto() {
        let content = "---\nagent_doc_model_tier: auto\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model_tier, Some(Tier::Auto));
    }

    #[test]
    fn parse_model_tier_absent() {
        let content = "---\nagent: claude\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model_tier, None);
    }

    #[test]
    fn parse_pending_capture_guard_strict() {
        let content = "---\npending_capture_guard: strict\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(
            fm.pending_capture_guard,
            Some(PendingCaptureGuardMode::Strict)
        );
    }

    #[test]
    fn parse_pending_capture_guard_invalid_rejected() {
        let content = "---\npending_capture_guard: loudly\n---\nBody\n";
        assert!(parse(content).is_err());
    }

    #[test]
    fn parse_pending_done_guard_strict() {
        let content = "---\npending_done_guard: strict\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.pending_done_guard, Some(PendingCaptureGuardMode::Strict));
    }

    #[test]
    fn parse_pending_done_guard_invalid_rejected() {
        let content = "---\npending_done_guard: loudly\n---\nBody\n";
        assert!(parse(content).is_err());
    }

    #[test]
    fn parse_model_tier_invalid_rejected() {
        let content = "---\nagent_doc_model_tier: ultra\n---\nBody\n";
        let result = parse(content);
        assert!(result.is_err(), "invalid tier value should fail to parse");
    }

    #[test]
    fn write_model_tier_roundtrip() {
        let fm = Frontmatter {
            model_tier: Some(Tier::High),
            ..Default::default()
        };
        let doc = write(&fm, "Body\n").unwrap();
        let (parsed, _) = parse(&doc).unwrap();
        assert_eq!(parsed.model_tier, Some(Tier::High));
        assert!(doc.contains("agent_doc_model_tier: high"));
    }

    #[test]
    fn parse_null_fields() {
        let content = "---\nsession: null\nagent: null\nmodel: null\nbranch: null\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert!(fm.session.is_none());
        assert!(fm.agent.is_none());
        assert!(fm.model.is_none());
        assert!(fm.branch.is_none());
        assert!(body.contains("Body"));
    }

    #[test]
    fn parse_unterminated_frontmatter() {
        let content = "---\nsession: abc\nno closing block";
        let err = parse(content).unwrap_err();
        assert!(err.to_string().contains("Unterminated frontmatter"));
    }

    #[test]
    fn parse_closing_at_eof() {
        let content = "---\nsession: abc\n---";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc"));
        assert_eq!(body, "");
    }

    #[test]
    fn parse_empty_body() {
        let content = "---\nsession: abc\n---\n";
        let (fm, _body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc"));
    }

    #[test]
    fn write_roundtrip() {
        // Start from write output to ensure consistent formatting
        let fm = Frontmatter {
            session: Some("test-id".to_string()),
            resume: Some("resume-id".to_string()),
            agent: Some("claude".to_string()),
            model: Some("opus".to_string()),
            claude_model: None,
            codex_model: None,
            opencode_model: None,
            branch: Some("dev".to_string()),
            tmux_session: None,
            mode: None,
            format: None,
            write_mode: None,
            stream_config: None,
            agent_args: None,
            claude_args: None,
            codex_args: None,
            opencode_args: None,
            codex_network_access: None,
            required_ssh_targets: Vec::new(),
            required_ssh_profile: None,
            managed_proof_max_attempts: None,
            managed_proof_retry_backoff_secs: None,
            managed_proof_probe_timeout_secs: None,
            no_mcp: None,
            enable_tool_search: None,
            debounce_ms: None,
            links: vec![],
            auto_compact: None,
            queue_context_reset: None,
            gate_autoverify: None,
            clear_threshold: None,
            model_tier: None,
            pending_capture_guard: None,
            pending_done_guard: None,
            review_done_guard: None,
            auto_done: None,
            agent_doc_lint_dialect: None,
            hooks: std::collections::HashMap::new(),
            env: indexmap::IndexMap::new(),
            prompt_presets: indexmap::IndexMap::new(),
            dispatch: None,
            agent_doc_env_inherit: None,
            cwd: None,
            queue: None,
            queue_active: None,
            queue_prompt_echo_max_chars: None,
            collaboration: None,
            security_review: None,
            pipeline: AgentDocPipeline::default(),
        };
        let body = "# Hello\n\nBody text.\n";
        let written = write(&fm, body).unwrap();
        let (fm2, body2) = parse(&written).unwrap();
        assert_eq!(fm2.session, fm.session);
        assert_eq!(fm2.agent, fm.agent);
        assert_eq!(fm2.model, fm.model);
        assert_eq!(fm2.claude_model, fm.claude_model);
        assert_eq!(fm2.codex_model, fm.codex_model);
        assert_eq!(fm2.opencode_model, fm.opencode_model);
        assert_eq!(fm2.branch, fm.branch);
        assert_eq!(fm2.collaboration, fm.collaboration);
        assert_eq!(fm2.security_review, fm.security_review);
        assert_eq!(body2, body);
    }

    #[test]
    fn repeated_parse_write_roundtrip_preserves_body_prefix() {
        let fm = Frontmatter {
            session: Some("abc".to_string()),
            agent: Some("codex".to_string()),
            ..Default::default()
        };
        let original = write(&fm, "## Status\n\nBody\n").unwrap();
        let mut current = original.clone();

        for _ in 0..3 {
            let (fm, body) = parse(&current).unwrap();
            assert_eq!(body, "## Status\n\nBody\n");
            current = write(&fm, body).unwrap();
        }

        let (fm, body) = parse(&current).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc"));
        assert_eq!(fm.agent.as_deref(), Some("codex"));
        assert_eq!(body, "## Status\n\nBody\n");
        assert_eq!(current, original);
    }

    #[test]
    fn write_default_frontmatter() {
        let fm = Frontmatter::default();
        let result = write(&fm, "body\n").unwrap();
        assert!(result.starts_with("---\n"));
        assert!(result.ends_with("---\nbody\n"));
    }

    #[test]
    fn write_preserves_body_content() {
        let fm = Frontmatter::default();
        let body = "# Title\n\nSome **markdown** with `code`.\n";
        let result = write(&fm, body).unwrap();
        assert!(result.contains("# Title"));
        assert!(result.contains("Some **markdown** with `code`."));
    }

    #[test]
    fn set_session_id_creates_frontmatter() {
        let content = "# No frontmatter\n\nJust body.\n";
        let result = set_session_id(content, "new-session").unwrap();
        let (fm, body) = parse(&result).unwrap();
        assert_eq!(fm.session.as_deref(), Some("new-session"));
        assert!(body.contains("# No frontmatter"));
    }

    #[test]
    fn set_session_id_updates_existing() {
        let content = "---\nsession: old-id\nagent: claude\n---\nBody\n";
        let result = set_session_id(content, "new-id").unwrap();
        let (fm, body) = parse(&result).unwrap();
        assert_eq!(fm.session.as_deref(), Some("new-id"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn set_session_id_preserves_other_fields() {
        let content = "---\nsession: old\nagent: claude\nmodel: opus\nbranch: dev\n---\nBody\n";
        let result = set_session_id(content, "new").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.session.as_deref(), Some("new"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert_eq!(fm.model.as_deref(), Some("opus"));
        assert_eq!(fm.branch.as_deref(), Some("dev"));
    }

    #[test]
    fn ensure_session_no_frontmatter() {
        let content = "# Hello\n\nBody.\n";
        let (updated, sid) = ensure_session(content).unwrap();
        // Should have generated a UUID
        assert_eq!(sid.len(), 36); // UUID v4 string length
        let (fm, body) = parse(&updated).unwrap();
        assert_eq!(fm.session.as_deref(), Some(sid.as_str()));
        assert!(body.contains("# Hello"));
    }

    #[test]
    fn ensure_session_null_session() {
        let content = "---\nsession:\nagent: claude\n---\nBody\n";
        let (updated, sid) = ensure_session(content).unwrap();
        assert_eq!(sid.len(), 36);
        let (fm, body) = parse(&updated).unwrap();
        assert_eq!(fm.session.as_deref(), Some(sid.as_str()));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn ensure_session_existing_session() {
        let content = "---\nagent_doc_session: existing-id\nagent: claude\n---\nBody\n";
        let (updated, sid) = ensure_session(content).unwrap();
        assert_eq!(sid, "existing-id");
        // Content should be unchanged
        assert_eq!(updated, content);
    }

    #[test]
    fn parse_legacy_session_field() {
        // Old `session:` field should still parse via serde alias
        let content = "---\nsession: legacy-id\nagent: claude\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("legacy-id"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn parse_agent_doc_mode_canonical() {
        let content = "---\nagent_doc_mode: template\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.mode.as_deref(), Some("template"));
    }

    #[test]
    fn parse_mode_shorthand_alias() {
        let content = "---\nmode: template\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.mode.as_deref(), Some("template"));
    }

    #[test]
    fn parse_response_mode_legacy_alias() {
        let content = "---\nresponse_mode: template\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.mode.as_deref(), Some("template"));
    }

    #[test]
    fn write_uses_agent_doc_mode_field() {
        #[allow(deprecated)]
        let fm = Frontmatter {
            mode: Some("template".to_string()),
            ..Default::default()
        };
        let result = write(&fm, "body\n").unwrap();
        assert!(result.contains("agent_doc_mode:"));
        assert!(!result.contains("response_mode:"));
        assert!(!result.contains("\nmode:"));
    }

    #[test]
    fn write_uses_new_field_name() {
        let fm = Frontmatter {
            session: Some("test-id".to_string()),
            ..Default::default()
        };
        let result = write(&fm, "body\n").unwrap();
        assert!(result.contains("agent_doc_session:"));
        assert!(!result.contains("\nsession:"));
    }

    #[test]
    fn write_pending_capture_guard_roundtrip() {
        let fm = Frontmatter {
            pending_capture_guard: Some(PendingCaptureGuardMode::Strict),
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        assert!(written.contains("pending_capture_guard: strict"));
        let (parsed, _) = parse(&written).unwrap();
        assert_eq!(
            parsed.pending_capture_guard,
            Some(PendingCaptureGuardMode::Strict)
        );
    }

    #[test]
    fn write_pending_done_guard_roundtrip() {
        let fm = Frontmatter {
            pending_done_guard: Some(PendingCaptureGuardMode::Strict),
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        assert!(written.contains("pending_done_guard: strict"));
        let (parsed, _) = parse(&written).unwrap();
        assert_eq!(
            parsed.pending_done_guard,
            Some(PendingCaptureGuardMode::Strict)
        );
    }

    #[test]
    fn parse_review_done_guard_error_alias() {
        let content = "---\nreview_done_guard: error\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.review_done_guard, Some(PendingCaptureGuardMode::Strict));
    }

    #[test]
    fn write_review_done_guard_roundtrip() {
        let fm = Frontmatter {
            review_done_guard: Some(PendingCaptureGuardMode::Strict),
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        assert!(written.contains("review_done_guard: strict"));
        let (parsed, _) = parse(&written).unwrap();
        assert_eq!(
            parsed.review_done_guard,
            Some(PendingCaptureGuardMode::Strict)
        );
    }

    #[test]
    fn parse_auto_done_bool() {
        let content = "---\nauto_done: true\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.auto_done, Some(true));
    }

    #[test]
    fn write_auto_done_roundtrip() {
        let fm = Frontmatter {
            auto_done: Some(true),
            ..Default::default()
        };
        let written = write(&fm, "Body\n").unwrap();
        assert!(written.contains("auto_done: true"));
        let (parsed, _) = parse(&written).unwrap();
        assert_eq!(parsed.auto_done, Some(true));
    }

    #[test]
    fn parse_prompt_presets_roundtrip() {
        let content = "---\nprompt_presets:\n  \"#1\": |\n    Today is 2026-04-25.\n    Keep the work tree clean.\n  release-check: |\n    Run cargo test.\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(body, "Body\n");
        assert_eq!(
            fm.prompt_presets.get("#1").map(String::as_str),
            Some("Today is 2026-04-25.\nKeep the work tree clean.\n")
        );
        assert_eq!(
            fm.prompt_presets.get("release-check").map(String::as_str),
            Some("Run cargo test.")
        );

        let written = write(&fm, body).unwrap();
        let (parsed, body2) = parse(&written).unwrap();
        assert_eq!(body2, "Body\n");
        assert_eq!(parsed.prompt_presets, fm.prompt_presets);
    }

    #[test]
    fn resolve_prompt_preset_key_accepts_bare_hashtag_alias() {
        let mut prompt_presets = indexmap::IndexMap::new();
        prompt_presets.insert("#spec-test".to_string(), "Run checks.".to_string());
        prompt_presets.insert("release-check".to_string(), "Publish.".to_string());

        assert_eq!(
            resolve_prompt_preset_key(&prompt_presets, "spec-test").as_deref(),
            Some("#spec-test")
        );
        assert_eq!(
            resolve_prompt_preset_key(&prompt_presets, "#spec-test").as_deref(),
            Some("#spec-test")
        );
        assert_eq!(
            resolve_prompt_preset_key(&prompt_presets, "release-check").as_deref(),
            Some("release-check")
        );
        assert!(resolve_prompt_preset_key(&prompt_presets, "missing").is_none());
    }

    // --- resolve_mode tests ---

    #[test]
    fn resolve_mode_defaults() {
        let fm = Frontmatter::default();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Template);
        assert_eq!(resolved.write, AgentDocWrite::Crdt);
    }

    #[test]
    fn resolve_mode_from_deprecated_append() {
        let content = "---\nagent_doc_mode: append\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Append);
        assert_eq!(resolved.write, AgentDocWrite::Crdt);
    }

    #[test]
    fn resolve_mode_from_deprecated_template() {
        let content = "---\nagent_doc_mode: template\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Template);
        assert_eq!(resolved.write, AgentDocWrite::Crdt);
    }

    #[test]
    fn resolve_mode_from_deprecated_stream() {
        let content = "---\nagent_doc_mode: stream\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Template);
        assert_eq!(resolved.write, AgentDocWrite::Crdt);
    }

    #[test]
    fn resolve_mode_new_fields_override_deprecated() {
        let content = "---\nagent_doc_mode: append\nagent_doc_format: template\nagent_doc_write: merge\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Template);
        assert_eq!(resolved.write, AgentDocWrite::Merge);
    }

    #[test]
    fn resolve_mode_explicit_new_fields_only() {
        let content = "---\nagent_doc_format: append\nagent_doc_write: crdt\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Append);
        assert_eq!(resolved.write, AgentDocWrite::Crdt);
    }

    #[test]
    fn resolve_mode_partial_new_field_format_only() {
        let content = "---\nagent_doc_format: append\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Append);
        assert_eq!(resolved.write, AgentDocWrite::Crdt); // default
    }

    #[test]
    fn resolve_mode_partial_new_field_write_only() {
        let content = "---\nagent_doc_write: merge\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Template); // default
        assert_eq!(resolved.write, AgentDocWrite::Merge);
    }

    #[test]
    fn resolve_mode_helper_methods() {
        let fm = Frontmatter::default();
        let resolved = fm.resolve_mode();
        assert!(resolved.is_template());
        assert!(!resolved.is_append());
        assert!(resolved.is_crdt());
    }

    #[test]
    fn parse_new_format_field() {
        let content = "---\nagent_doc_format: template\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.format, Some(AgentDocFormat::Template));
    }

    #[test]
    fn parse_new_write_field() {
        let content = "---\nagent_doc_write: crdt\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.write_mode, Some(AgentDocWrite::Crdt));
    }

    #[test]
    fn write_uses_new_format_write_fields() {
        let fm = Frontmatter {
            format: Some(AgentDocFormat::Template),
            write_mode: Some(AgentDocWrite::Crdt),
            ..Default::default()
        };
        let result = write(&fm, "body\n").unwrap();
        assert!(result.contains("agent_doc_format:"));
        assert!(result.contains("agent_doc_write:"));
        assert!(!result.contains("agent_doc_mode:"));
    }

    #[test]
    fn set_format_and_write_clears_deprecated_mode() {
        let content = "---\nagent_doc_mode: stream\n---\nBody\n";
        let result =
            set_format_and_write(content, AgentDocFormat::Template, AgentDocWrite::Crdt).unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert!(fm.mode.is_none());
        assert_eq!(fm.format, Some(AgentDocFormat::Template));
        assert_eq!(fm.write_mode, Some(AgentDocWrite::Crdt));
    }

    // --- merge_fields tests ---

    #[test]
    fn merge_fields_adds_new_field() {
        let content = "---\nagent_doc_session: abc\n---\nBody\n";
        let result = merge_fields(content, "model: opus").unwrap();
        let (fm, body) = parse(&result).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc"));
        assert_eq!(fm.model.as_deref(), Some("opus"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn merge_fields_updates_existing_field() {
        let content = "---\nagent_doc_session: abc\nmodel: sonnet\n---\nBody\n";
        let result = merge_fields(content, "model: opus").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("opus"));
        assert_eq!(fm.session.as_deref(), Some("abc"));
    }

    #[test]
    fn merge_fields_multiple_fields() {
        let content = "---\nagent_doc_session: abc\n---\nBody\n";
        let result = merge_fields(content, "model: opus\nagent: claude\nbranch: main").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("opus"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert_eq!(fm.branch.as_deref(), Some("main"));
    }

    #[test]
    fn merge_fields_format_enum() {
        let content = "---\nagent_doc_session: abc\n---\nBody\n";
        let result = merge_fields(content, "agent_doc_format: append").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.format, Some(AgentDocFormat::Append));
    }

    #[test]
    fn merge_fields_write_enum() {
        let content = "---\nagent_doc_session: abc\n---\nBody\n";
        let result = merge_fields(content, "agent_doc_write: merge").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.write_mode, Some(AgentDocWrite::Merge));
    }

    #[test]
    fn merge_fields_ignores_unknown() {
        let content = "---\nagent_doc_session: abc\n---\nBody\n";
        let result = merge_fields(content, "unknown_field: value\nmodel: opus").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("opus"));
    }

    #[test]
    fn merge_fields_preserves_body() {
        let content = "---\nagent_doc_session: abc\n---\n# Title\n\nSome **markdown** content.\n";
        let result = merge_fields(content, "model: opus").unwrap();
        assert!(result.contains("# Title"));
        assert!(result.contains("Some **markdown** content."));
    }

    #[test]
    fn set_format_and_write_clears_deprecated() {
        let content = "---\nagent_doc_mode: append\n---\nBody\n";
        let result =
            set_format_and_write(content, AgentDocFormat::Template, AgentDocWrite::Crdt).unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert!(fm.mode.is_none());
        assert_eq!(fm.format, Some(AgentDocFormat::Template));
        assert_eq!(fm.write_mode, Some(AgentDocWrite::Crdt));
    }

    #[test]
    fn hooks_roundtrip() {
        let content = "---\nhooks:\n  session_start:\n    - \"echo start {{session_id}}\"\n  post_write:\n    - \"notify {{file}}\"\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(
            fm.hooks.get("session_start"),
            Some(&vec!["echo start {{session_id}}".to_string()])
        );
        assert_eq!(
            fm.hooks.get("post_write"),
            Some(&vec!["notify {{file}}".to_string()])
        );
    }

    #[test]
    fn hooks_omitted_when_empty() {
        let fm = Frontmatter::default();
        let result = write(&fm, "body\n").unwrap();
        assert!(!result.contains("hooks"));
    }

    #[test]
    fn hooks_absent_parses_as_empty() {
        let content = "---\nsession: abc\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert!(fm.hooks.is_empty());
    }

    #[test]
    fn parse_no_mcp_field() {
        let content = "---\nno_mcp: true\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.no_mcp, Some(true));
    }

    #[test]
    fn parse_enable_tool_search_field() {
        let content = "---\nenable_tool_search: true\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.enable_tool_search, Some(true));
    }

    #[test]
    fn parse_missing_flags_default_none() {
        let content = "---\nsession: abc\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert!(fm.no_mcp.is_none());
        assert!(fm.enable_tool_search.is_none());
    }

    #[test]
    fn parse_env_map() {
        let content = "---\nenv:\n  FOO: bar\n  BAZ: \"$(echo hello)\"\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.env.len(), 2);
        assert_eq!(fm.env["FOO"], Some("bar".to_string()));
        assert_eq!(fm.env["BAZ"], Some("$(echo hello)".to_string()));
        // Verify order is preserved
        let keys: Vec<&String> = fm.env.keys().collect();
        assert_eq!(keys, vec!["FOO", "BAZ"]);
    }

    #[test]
    fn parse_env_empty_default() {
        let content = "---\nsession: abc\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert!(fm.env.is_empty());
    }

    #[test]
    fn parse_env_unset_via_null() {
        let content = "---\nenv:\n  SET_ME: value\n  UNSET_ME: null\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.env.len(), 2);
        assert_eq!(fm.env["SET_ME"], Some("value".to_string()));
        assert_eq!(fm.env["UNSET_ME"], None);
    }

    #[test]
    fn write_roundtrip_with_env() {
        let mut env: indexmap::IndexMap<String, Option<String>> = indexmap::IndexMap::new();
        env.insert("KEY1".to_string(), Some("value1".to_string()));
        env.insert("KEY2".to_string(), Some("$KEY1".to_string()));
        env.insert("KEY3".to_string(), None);
        let fm = Frontmatter {
            env,
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        let (fm2, _) = parse(&written).unwrap();
        assert_eq!(fm2.env.len(), 3);
        assert_eq!(fm2.env["KEY1"], Some("value1".to_string()));
        assert_eq!(fm2.env["KEY2"], Some("$KEY1".to_string()));
        assert_eq!(fm2.env["KEY3"], None);
    }

    #[test]
    fn parse_agent_args_field() {
        let content = "---\nagent_args: \"--json -s workspace-write\"\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("--json -s workspace-write"));
        assert!(fm.claude_args.is_none());
        assert!(fm.codex_args.is_none());
        assert!(fm.opencode_args.is_none());
    }

    #[test]
    fn parse_agent_args_and_harness_aliases() {
        let content = "---\nagent_args: \"--json\"\nclaude_args: \"--dangerously-skip-permissions\"\ncodex_args: \"-s danger-full-access\"\nopencode_args: \"--dangerously-skip-permissions\"\ncodex_network_access: enabled\nrequired_ssh_targets:\n  - monsterrodholders-server\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("--json"));
        assert_eq!(
            fm.claude_args.as_deref(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(fm.codex_args.as_deref(), Some("-s danger-full-access"));
        assert_eq!(
            fm.opencode_args.as_deref(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(fm.codex_network_access, Some(CodexNetworkAccess::Enabled));
        assert_eq!(
            fm.required_ssh_targets,
            vec!["monsterrodholders-server".to_string()]
        );
    }

    #[test]
    fn write_roundtrip_agent_args() {
        let fm = Frontmatter {
            agent_args: Some("--json -s workspace-write".to_string()),
            codex_args: Some("-s danger-full-access".to_string()),
            opencode_args: Some("--dangerously-skip-permissions".to_string()),
            codex_network_access: Some(CodexNetworkAccess::Enabled),
            required_ssh_targets: vec!["monsterrodholders-server".to_string()],
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        let (fm2, _) = parse(&written).unwrap();
        assert_eq!(fm2.agent_args.as_deref(), Some("--json -s workspace-write"));
        assert_eq!(fm2.codex_args.as_deref(), Some("-s danger-full-access"));
        assert_eq!(
            fm2.opencode_args.as_deref(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(fm2.codex_network_access, Some(CodexNetworkAccess::Enabled));
        assert_eq!(
            fm2.required_ssh_targets,
            vec!["monsterrodholders-server".to_string()]
        );
    }

    #[test]
    fn merge_fields_agent_args() {
        let content = "---\nclaude_args: old\ncodex_args: old-codex\nopencode_args: old-opencode\n---\nBody\n";
        let result = merge_fields(content, "agent_args: new").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("new"));
        assert_eq!(fm.claude_args.as_deref(), Some("old"));
        assert_eq!(fm.codex_args.as_deref(), Some("old-codex"));
        assert_eq!(fm.opencode_args.as_deref(), Some("old-opencode"));
    }

    #[test]
    fn merge_fields_codex_args() {
        let content = "---\nagent_args: old\n---\nBody\n";
        let result = merge_fields(content, "codex_args: new").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("old"));
        assert_eq!(fm.codex_args.as_deref(), Some("new"));
    }

    #[test]
    fn merge_fields_opencode_args() {
        let content = "---\nagent_args: old\n---\nBody\n";
        let result = merge_fields(content, "opencode_args: new").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("old"));
        assert_eq!(fm.opencode_args.as_deref(), Some("new"));
    }

    #[test]
    fn merge_fields_codex_network_access() {
        let content = "Body\n";
        let result = merge_fields(content, "codex_network_access: disabled").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.codex_network_access, Some(CodexNetworkAccess::Disabled));
    }

    #[test]
    fn merge_fields_required_ssh_targets() {
        let content = "Body\n";
        let result = merge_fields(
            content,
            "required_ssh_targets:\n  - monsterrodholders-server\n  - root@50.28.2.199",
        )
        .unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(
            fm.required_ssh_targets,
            vec![
                "monsterrodholders-server".to_string(),
                "root@50.28.2.199".to_string(),
            ]
        );
    }

    #[test]
    fn merge_fields_required_ssh_profile() {
        let content = "Body\n";
        let result = merge_fields(content, "required_ssh_profile: monsterrodholders").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(
            fm.required_ssh_profile.as_deref(),
            Some("monsterrodholders")
        );
    }

    // --- resolve_harness_model tests ---

    #[test]
    fn resolve_harness_model_claude_code_uses_claude_model() {
        let fm = Frontmatter {
            model: Some("gpt-5".to_string()),
            claude_model: Some("opus".to_string()),
            codex_model: Some("o3-pro".to_string()),
            ..Default::default()
        };
        assert_eq!(fm.resolve_harness_model("claude-code"), Some("opus"));
    }

    #[test]
    fn resolve_harness_model_codex_uses_codex_model() {
        let fm = Frontmatter {
            model: Some("gpt-5".to_string()),
            claude_model: Some("opus".to_string()),
            codex_model: Some("o3-pro".to_string()),
            opencode_model: Some("zai/glm-5".to_string()),
            ..Default::default()
        };
        assert_eq!(fm.resolve_harness_model("codex"), Some("o3-pro"));
    }

    #[test]
    fn resolve_harness_model_opencode_uses_opencode_model() {
        let fm = Frontmatter {
            model: Some("gpt-5".to_string()),
            claude_model: Some("opus".to_string()),
            codex_model: Some("o3-pro".to_string()),
            opencode_model: Some("zai/glm-5".to_string()),
            ..Default::default()
        };
        assert_eq!(fm.resolve_harness_model("opencode"), Some("zai/glm-5"));
    }

    #[test]
    fn resolve_harness_model_falls_back_to_generic() {
        let fm = Frontmatter {
            model: Some("gpt-5".to_string()),
            ..Default::default()
        };
        assert_eq!(fm.resolve_harness_model("claude-code"), Some("gpt-5"));
        assert_eq!(fm.resolve_harness_model("codex"), Some("gpt-5"));
        assert_eq!(fm.resolve_harness_model("opencode"), Some("gpt-5"));
        assert_eq!(fm.resolve_harness_model("default"), Some("gpt-5"));
    }

    #[test]
    fn resolve_harness_model_none_when_no_model() {
        let fm = Frontmatter::default();
        assert_eq!(fm.resolve_harness_model("claude-code"), None);
    }

    #[test]
    fn resolve_harness_model_unknown_harness_uses_generic() {
        let fm = Frontmatter {
            model: Some("sonnet".to_string()),
            claude_model: Some("opus".to_string()),
            ..Default::default()
        };
        assert_eq!(fm.resolve_harness_model("unknown"), Some("sonnet"));
    }

    #[test]
    fn parse_harness_model_fields() {
        let content = "---\nmodel: gpt-5\nclaude_model: opus\ncodex_model: o3-pro\nopencode_model: zai/glm-5\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model.as_deref(), Some("gpt-5"));
        assert_eq!(fm.claude_model.as_deref(), Some("opus"));
        assert_eq!(fm.codex_model.as_deref(), Some("o3-pro"));
        assert_eq!(fm.opencode_model.as_deref(), Some("zai/glm-5"));
    }

    #[test]
    fn write_roundtrip_harness_model_fields() {
        let fm = Frontmatter {
            model: Some("gpt-5".to_string()),
            claude_model: Some("opus".to_string()),
            codex_model: Some("o3-pro".to_string()),
            opencode_model: Some("zai/glm-5".to_string()),
            ..Default::default()
        };
        let doc = write(&fm, "Body\n").unwrap();
        let (fm2, body) = parse(&doc).unwrap();
        assert_eq!(fm2.model.as_deref(), Some("gpt-5"));
        assert_eq!(fm2.claude_model.as_deref(), Some("opus"));
        assert_eq!(fm2.codex_model.as_deref(), Some("o3-pro"));
        assert_eq!(fm2.opencode_model.as_deref(), Some("zai/glm-5"));
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn merge_fields_claude_model() {
        let content = "---\nmodel: gpt-5\n---\nBody\n";
        let result = merge_fields(content, "claude_model: opus").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("gpt-5"));
        assert_eq!(fm.claude_model.as_deref(), Some("opus"));
    }

    #[test]
    fn merge_fields_codex_model() {
        let content = "---\nmodel: gpt-5\n---\nBody\n";
        let result = merge_fields(content, "codex_model: o3-pro").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("gpt-5"));
        assert_eq!(fm.codex_model.as_deref(), Some("o3-pro"));
    }

    #[test]
    fn merge_fields_opencode_model() {
        let content = "---\nmodel: gpt-5\n---\nBody\n";
        let result = merge_fields(content, "opencode_model: zai/glm-5").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("gpt-5"));
        assert_eq!(fm.opencode_model.as_deref(), Some("zai/glm-5"));
    }
