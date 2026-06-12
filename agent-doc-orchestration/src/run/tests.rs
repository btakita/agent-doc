    use super::*;
    use crate::config::Config;
    use std::sync::{Arc, Barrier};
    use tempfile::TempDir;

    #[test]
    fn owned_pane_queue_handoff_diagnostic_names_head_and_recovery() {
        // #codex-owned-pane-auto-queue-stuck: the fail-closed handoff diagnostic
        // must name the live head + id, the in-owner-turn recovery path, and warn
        // against re-running the same direct command.
        let continuation = crate::queue_continuation::QueueContinuation {
            head_prompt: "do [#codex-owned-pane-auto-queue-stuck]".to_string(),
            head_id: Some("codex-owned-pane-auto-queue-stuck".to_string()),
            reason: "active `agent:queue auto` still has a ready head prompt".to_string(),
        };
        let msg = owned_pane_queue_handoff_diagnostic(
            Path::new("tasks/x.md"),
            "current_pane=%9 session_id=sess actor_generation=3 actor_state=alive-busy actor_pane=%9",
            &continuation,
        );
        assert!(msg.contains("active auto-queue head"));
        assert!(msg.contains("do [#codex-owned-pane-auto-queue-stuck]"));
        assert!(msg.contains("(id #codex-owned-pane-auto-queue-stuck)"));
        assert!(msg.contains("THIS owner pane"));
        assert!(msg.contains("agent-doc finalize tasks/x.md"));
        assert!(msg.contains("Do NOT re-run"));
        assert!(msg.contains("No pre-commit, snapshot, or queue mutation was made"));
    }

    #[test]
    fn owned_pane_queue_handoff_diagnostic_uses_supervisor_for_slash_command() {
        let continuation = crate::queue_continuation::QueueContinuation {
            head_prompt: "  /clear  ".to_string(),
            head_id: None,
            reason: "active `agent:queue auto` still has a ready head prompt".to_string(),
        };
        let msg = owned_pane_queue_handoff_diagnostic(
            Path::new("tasks/x.md"),
            "current_pane=%9 session_id=sess actor_generation=3 actor_state=alive-busy actor_pane=%9",
            &continuation,
        );
        assert!(msg.contains("slash command"));
        assert!(msg.contains("\"/clear\""));
        assert!(msg.contains("managed owner-pane supervisor will submit"));
        assert!(msg.contains("No pre-commit, snapshot, exchange, or queue mutation was made"));
        assert!(msg.contains("Do NOT answer this queue head in the exchange"));
        assert!(
            !msg.contains("agent-doc finalize"),
            "slash-command handoff must not instruct an assistant closeout: {msg}"
        );
    }

    #[test]
    fn active_queue_prompt_diff_ignores_slash_command_head() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "-   /clear  \n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        assert_eq!(
            active_queue_prompt_diff(&doc).unwrap(),
            None,
            "slash-only active queue heads are command handoffs, not child-agent prompts"
        );
    }

    #[test]
    fn owned_pane_queue_wedge_halt_diagnostic_names_halt_and_both_recoveries() {
        // #recguard-wedge-escape-live-verify (deterministic core): when the
        // owner-pane self-invocation guard trips WEDGE_THRESHOLD times in a row,
        // the escalated diagnostic must (a) state the auto-queue was HALTED
        // (queue: stop), (b) state the head stays live / no drift committed,
        // (c) name BOTH recovery actions (answer+finalize+queue:go, or
        // agent-doc start from OUTSIDE the pane), and (d) warn against re-running
        // the same direct command. The end-to-end verification on a real wedged
        // Codex pane stays a recommended live-verify (#recguard-wedge-escape-live-verify).
        let continuation = crate::queue_continuation::QueueContinuation {
            head_prompt: "do [#recguard-wedge-escape]".to_string(),
            head_id: Some("recguard-wedge-escape".to_string()),
            reason: "active `agent:queue auto` still has a ready head prompt".to_string(),
        };
        let msg = owned_pane_queue_wedge_halt_diagnostic(
            Path::new("tasks/x.md"),
            "current_pane=%9 session_id=sess actor_generation=3 actor_state=alive-busy actor_pane=%9",
            &continuation,
            crate::recguard_wedge::WEDGE_THRESHOLD,
        );
        assert!(msg.contains("WEDGE"));
        assert!(msg.contains("HALTED (`queue: stop`)"));
        assert!(msg.contains("do [#recguard-wedge-escape]"));
        assert!(msg.contains("(id #recguard-wedge-escape)"));
        assert!(msg.contains("stays live"));
        assert!(msg.contains("agent-doc finalize tasks/x.md"));
        assert!(msg.contains("queue: go"));
        assert!(msg.contains("agent-doc start tasks/x.md"));
        assert!(msg.contains("OUTSIDE this pane"));
        assert!(msg.contains("Do NOT re-run"));
    }

    #[test]
    fn recursive_start_diagnostic_refuses_and_names_out_of_pane_recovery() {
        // #recursion-guard-wedge-escape (part 1): `agent-doc start <FILE>` inside
        // the Codex pane that already owns the doc must fail closed with a message
        // that (a) names the deadlock as a recursive self-owned-pane start, (b)
        // explains it would loop re-injecting `agent-doc <FILE>`, (c) points at an
        // out-of-pane recovery (session status reconcile, then interrupt-clear,
        // escalating to interrupt-clear --force), and (d) warns against re-running
        // `agent-doc start` from this pane.
        let msg = format_recursive_start_diagnostic(
            Path::new("tasks/x.md"),
            "current_pane=%9 session_id=sess actor_generation=3 actor_state=alive-busy actor_pane=%9",
        );
        assert!(msg.contains("recursive self-owned-pane start would deadlock"));
        assert!(msg.contains("agent-doc start tasks/x.md"));
        assert!(msg.contains("loop re-injecting `agent-doc tasks/x.md`"));
        assert!(msg.contains("DIFFERENT pane"));
        assert!(msg.contains("agent-doc session status tasks/x.md"));
        assert!(msg.contains("agent-doc session interrupt-clear tasks/x.md"));
        assert!(msg.contains("agent-doc session interrupt-clear tasks/x.md --force"));
        assert!(msg.contains("Do NOT re-run"));
    }

    #[test]
    fn no_change_after_recursive_block_reports_typed_diagnostic() {
        // #nochange-after-stall: a direct run that finds no diff but whose latest
        // cycle was abandoned by the recursive-owner guard must surface the prior
        // state + recovery instead of plain "Nothing changed".
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-1","file":"x.md","phase":"abandoned","last_event":"recursive_direct_invocation_blocked recursive direct invocation would deadlock","started_at":0,"updated_at":0}"#,
        )
        .unwrap();
        match classify_no_change_cycle_state(Some(&st)) {
            NoChangeVerdict::Abnormal { summary, recovery } => {
                assert!(summary.contains("recursive direct invocation"));
                assert!(summary.contains("cycle-1"));
                assert!(recovery.contains("managed pane"));
                // #stale-busy-recursion-recovery-discoverability: a stale busy
                // idle pane must be recoverable without a pane kill via the
                // existing idle-reconcile path, so the diagnostic must surface
                // `session status` / `session clear` ahead of the heavy restart.
                assert!(recovery.contains("agent-doc session status"));
                assert!(recovery.contains("agent-doc session clear"));
                assert!(recovery.contains("without killing the pane"));
            }
            NoChangeVerdict::Clean => panic!("expected an abnormal no-change verdict"),
        }
    }

    #[test]
    fn no_change_after_generic_abandoned_cycle_reports_typed_diagnostic() {
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-2","file":"x.md","phase":"abandoned","last_event":"stale_preflight","started_at":0,"updated_at":0}"#,
        )
        .unwrap();
        assert!(matches!(
            classify_no_change_cycle_state(Some(&st)),
            NoChangeVerdict::Abnormal { .. }
        ));
    }

    #[test]
    fn no_change_with_committed_cycle_stays_clean() {
        // Normal healthy completed session: no-change behavior must be unchanged.
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-3","file":"x.md","phase":"committed","last_event":"commit","started_at":0,"updated_at":0}"#,
        )
        .unwrap();
        assert_eq!(
            classify_no_change_cycle_state(Some(&st)),
            NoChangeVerdict::Clean
        );
        assert_eq!(classify_no_change_cycle_state(None), NoChangeVerdict::Clean);
    }

    #[test]
    fn no_change_after_committed_bookkeeping_only_cycle_reports_abnormal() {
        // #jb-codex-nochange-after-repair: when a committed cycle has no
        // response body but carried bookkeeping-only mutations (repair/reap
        // following an abandoned recursive invocation), the "Nothing changed"
        // output must surface the prior abnormal state instead of Clean.
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-repair-1","file":"tasks/monsterrodholders.md","phase":"committed","last_event":"commit_success","started_at":0,"updated_at":0,"had_pending_mutations":true,"reaped_pending_ids":["stale-item"]}"#,
        )
        .unwrap();
        match classify_no_change_cycle_state(Some(&st)) {
            NoChangeVerdict::Abnormal { summary, recovery } => {
                assert!(summary.contains("cycle-repair-1"));
                assert!(summary.contains("bookkeeping-only"));
                assert!(summary.contains("commit_success"));
                assert!(recovery.contains("tasks/monsterrodholders.md"));
                assert!(recovery.contains("non-owner pane"));
                assert!(recovery.contains("agent-doc start"));
            }
            NoChangeVerdict::Clean => {
                panic!("expected Abnormal for committed no-response bookkeeping cycle")
            }
        }
    }

    #[test]
    fn no_change_committed_no_response_no_bookkeeping_stays_clean() {
        // A committed no-response cycle with no bookkeeping is not suspicious.
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-4","file":"x.md","phase":"committed","last_event":"commit_success","started_at":0,"updated_at":0}"#,
        )
        .unwrap();
        assert_eq!(
            classify_no_change_cycle_state(Some(&st)),
            NoChangeVerdict::Clean
        );
    }

    #[test]
    fn no_change_committed_with_response_stays_clean() {
        // A committed cycle WITH a response body is healthy regardless of bookkeeping.
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-5","file":"x.md","phase":"committed","last_event":"commit_success","started_at":0,"updated_at":0,"capture_id":"cap-1","response_sha256":"abc123","had_pending_mutations":true}"#,
        )
        .unwrap();
        assert_eq!(
            classify_no_change_cycle_state(Some(&st)),
            NoChangeVerdict::Clean
        );
    }

    #[test]
    fn build_prompt_defaults_to_template_mode() {
        let fm = frontmatter::Frontmatter::default();
        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::from_frontmatter(&fm),
            &fm,
            "diff",
            "doc",
            None,
        );
        assert!(prompt.contains("patch:exchange"));
        assert!(!prompt.contains("## Assistant heading"));
    }

    #[test]
    fn build_prompt_append_mode_uses_inline_contract() {
        let fm = frontmatter::Frontmatter {
            format: Some(frontmatter::AgentDocFormat::Append),
            ..Default::default()
        };
        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::from_frontmatter(&fm),
            &fm,
            "diff",
            "doc",
            None,
        );
        assert!(prompt.contains("Do not include a ## Assistant heading"));
        assert!(!prompt.contains("patch:exchange"));
    }

    #[test]
    fn build_prompt_places_turn_churn_after_cache_boundary() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
          Done.\n\
          +do [#pcache-boundary]. keep volatile queue churn below the boundary\n\
          <!-- /agent:exchange -->\n";
        let doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: older topic - gpt-5\n\n",
            "Older response body.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#pcache-boundary] Prompt-cache boundary work\n",
            "<!-- /agent:backlog -->\n",
        );
        let report = crate::session_accretion::SessionAccretionReport {
            level: crate::session_accretion::SessionAccretionLevel::Warn,
            reasons: vec!["document hit 4 no-op closeouts in the last 30 minutes".to_string()],
            ..Default::default()
        };

        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            diff,
            doc,
            Some(&report),
        );
        let boundary = prompt
            .find(crate::prompt_cache::PROMPT_CACHE_BOUNDARY)
            .expect("direct-run prompt should expose cache boundary");
        for volatile in [
            "<diff>",
            "do [#pcache-boundary]. keep volatile queue churn below the boundary",
            "User-authored prompt-bearing changes (oldest first):",
            "Accretion reason: document hit 4 no-op closeouts in the last 30 minutes.",
            "<response_context level=\"warn\">",
        ] {
            let pos = prompt
                .find(volatile)
                .unwrap_or_else(|| panic!("missing volatile fragment {volatile:?}:\n{prompt}"));
            assert!(
                pos > boundary,
                "volatile fragment {volatile:?} must stay after cache boundary:\n{prompt}"
            );
        }
        assert!(
            prompt.starts_with("<agent_doc_prompt_stable_prefix>"),
            "stable prefix should be the first prompt block:\n{prompt}"
        );
    }

    #[test]
    fn prompt_cache_boundary_contract_separates_durable_and_volatile_blocks() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let diff = "--- snapshot\n+++ document\n@@ -1,4 +1,6 @@\n\
old status\n\
+new status\n\
+do [#pcache-boundary-contract]\n";
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "new status\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older topic - gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#pcache-boundary-contract]\n",
            "<!-- /agent:queue -->\n",
        );
        let report = crate::session_accretion::SessionAccretionReport {
            level: crate::session_accretion::SessionAccretionLevel::Warn,
            reasons: vec!["document closed 7 cycles in the last 30 minutes".to_string()],
            recent_noop_closeouts: 5,
            ..Default::default()
        };

        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            diff,
            doc,
            Some(&report),
        );
        let boundary = crate::prompt_cache::PROMPT_CACHE_BOUNDARY;
        let (stable, volatile) = prompt
            .split_once(boundary)
            .expect("direct-run prompt should expose cache boundary");

        for durable in [
            "<agent_doc_prompt_stable_prefix>",
            "<response_contract>",
            "<turn_payload_contract>",
            "Format your response as patch blocks",
            "Read the volatile turn payload after the cache boundary",
        ] {
            assert!(
                stable.contains(durable),
                "stable prefix must keep durable fragment {durable:?}:\n{prompt}"
            );
        }

        for volatile_fragment in [
            "<diff>",
            "new status",
            "do [#pcache-boundary-contract]",
            "<response_context level=\"warn\">",
            "Accretion reason: document closed 7 cycles in the last 30 minutes.",
        ] {
            assert!(
                !stable.contains(volatile_fragment),
                "volatile fragment {volatile_fragment:?} must not enter stable prefix:\n{prompt}"
            );
            assert!(
                volatile.contains(volatile_fragment),
                "volatile suffix must contain {volatile_fragment:?}:\n{prompt}"
            );
        }
    }

    #[test]
    fn prompt_cache_replay_key_survives_session_churn_and_invalidates_on_durable_contract() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let report = crate::session_accretion::SessionAccretionReport {
            level: crate::session_accretion::SessionAccretionLevel::Warn,
            reasons: vec!["document closed 3 no-op cycles in the last hour".to_string()],
            recent_noop_closeouts: 3,
            ..Default::default()
        };
        let base_diff = "--- snapshot\n+++ document\n@@ -1,3 +1,5 @@\n\
           complete\n\
           +do [#pcache-replaygate]\n\
           +queue_active: true\n";
        let churn_diff = "--- snapshot\n+++ document\n@@ -1,5 +1,7 @@\n\
           -complete\n\
           +working\n\
           +do [#pcache-replaygate]\n\
           +<!-- agent:boundary:churn -->\n";
        let base_doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "complete\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior topic - gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#pcache-replaygate]\n",
            "- do [#pcache-missrank]\n",
            "<!-- /agent:queue -->\n"
        );
        let churn_doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "working\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior topic - gpt-5\n\nDone.\n",
            "<!-- agent:boundary:churn -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#pcache-replaygate]\n",
            "- do [#pcache-missrank]\n",
            "- do [#pcache-ci-history]\n",
            "<!-- /agent:queue -->\n"
        );

        let base_prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            base_diff,
            base_doc,
            Some(&report),
        );
        let churn_prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            churn_diff,
            churn_doc,
            Some(&report),
        );
        assert_ne!(
            base_prompt, churn_prompt,
            "precondition: volatile session churn should change the full prompt"
        );

        let routing_affinity =
            prompt_cache_routing_affinity(RunMode::Template, "codex", Some("gpt-5"));
        let base_key = crate::prompt_cache::PromptCacheBlocks::from_rendered(&base_prompt)
            .expect("template prompt should expose prompt-cache blocks")
            .replay_key(&routing_affinity);
        let churn_key = crate::prompt_cache::PromptCacheBlocks::from_rendered(&churn_prompt)
            .expect("churn prompt should expose prompt-cache blocks")
            .replay_key(&routing_affinity);

        assert_eq!(base_key, churn_key);
        assert_eq!(
            base_key.cache_control,
            crate::prompt_cache::PROMPT_CACHE_CONTROL
        );
        assert_eq!(
            base_key.routing_affinity,
            "agent_doc_run:v1;agent=codex;model=gpt-5;mode=template"
        );

        let churn_boundary = churn_prompt
            .find(crate::prompt_cache::PROMPT_CACHE_BOUNDARY)
            .expect("prompt-cache boundary should be present");
        for volatile in [
            "working",
            "do [#pcache-replaygate]",
            "agent:boundary:churn",
            "Accretion reason: document closed 3 no-op cycles in the last hour.",
        ] {
            let pos = churn_prompt
                .find(volatile)
                .unwrap_or_else(|| panic!("missing volatile fragment {volatile:?}"));
            assert!(
                pos > churn_boundary,
                "volatile fragment {volatile:?} must remain after cache boundary:\n{churn_prompt}"
            );
        }

        let append_prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Append,
            &fm,
            base_diff,
            base_doc,
            Some(&report),
        );
        let append_same_route_key =
            crate::prompt_cache::PromptCacheBlocks::from_rendered(&append_prompt)
                .expect("append prompt should expose prompt-cache blocks")
                .replay_key(&base_key.routing_affinity);
        assert_eq!(
            append_same_route_key.routing_affinity,
            base_key.routing_affinity
        );
        assert_ne!(
            append_same_route_key.stable_prefix_sha256, base_key.stable_prefix_sha256,
            "changing the durable response contract should invalidate the stable-prefix fingerprint"
        );
        assert_ne!(
            append_same_route_key.provider_cache_key, base_key.provider_cache_key,
            "provider cache key must change when durable instructions change"
        );
    }

    #[test]
    fn build_prompt_resume_lists_required_response_targets() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,5 @@\n\
           ctx\n\
           +❯ First unresolved question?\n\
           +\n\
           +❯ Second unresolved question?\n";
        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            diff,
            "doc",
            None,
        );
        assert!(prompt.contains("User-authored prompt-bearing changes (oldest first):"));
        assert!(prompt.contains("Do not stop at the newest question"));
        assert!(prompt.contains("kind=\"prompt_target\""));
        assert!(prompt.contains("❯ First unresolved question?"));
        assert!(prompt.contains("❯ Second unresolved question?"));
    }

    #[test]
    fn build_prompt_carries_forward_active_format_requirements() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let doc = concat!(
            "❯ Please organize the backlog into a 2-level list. ",
            "Place the urgent-security matters at the top. ",
            "Use a numeric list where appropriate.\n",
            "### Re: backlog organization — gpt-5\n",
            "Done.\n",
        );

        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            "diff",
            doc,
            None,
        );
        assert!(
            prompt.contains(
                "Active document-level formatting / structure requirements carried forward"
            )
        );
        assert!(prompt.contains(
            "Please organize the backlog into a 2-level list. Place the urgent-security matters at the top. Use a numeric list where appropriate."
        ));
    }

    #[test]
    fn build_prompt_uses_bounded_context_pack_for_warn_level_prompt_targets() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
           Done.\n\
           +do [#ctxpack]. spec-test-build-install-commit-push\n\
           <!-- /agent:exchange -->\n";
        let doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: older topic — gpt-5\n\n",
            "Older response body.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ctxpack] Add bounded context pack\n",
            "<!-- /agent:backlog -->\n",
        );
        let report = crate::session_accretion::SessionAccretionReport {
            level: crate::session_accretion::SessionAccretionLevel::Warn,
            reasons: vec!["document hit 2 no-op closeouts in the last 30 minutes".to_string()],
            ..Default::default()
        };

        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            diff,
            doc,
            Some(&report),
        );
        assert!(prompt.contains("<response_context level=\"warn\">"));
        assert!(prompt.contains("<recent_exchange_turns limit=\"2\">"));
        assert!(!prompt.contains("<document>\n## Exchange"));
    }

    #[test]
    fn apply_template_response_normalizes_legacy_backlog_patch_before_enforcement() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] existing item\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: backlog follow-up — gpt-5\n\n",
            "Captured the requested backlog update.\n",
            "<!-- /patch:exchange -->\n\n",
            "<!-- patch:backlog -->\n",
            "- [ ] [#new1] added item\n",
            "- [ ] [#keep1] existing item\n",
            "<!-- /patch:backlog -->\n",
        );

        apply_template_response(&doc, baseline, response, false)
            .expect("run path should normalize legacy backlog patches before enforcement");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("### Re: backlog follow-up — gpt-5"));
        assert!(updated.contains("- [ ] [#new1] added item"));
        assert!(updated.contains("- [ ] [#keep1] existing item"));
    }

    #[test]
    fn apply_template_response_normalizes_monsterrodholders_style_backlog_patch() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "### 2. Revenue / Fulfillment / Store Operations\n",
            "- [ ] [#2xcx] Verify ShipStation polling resumes after Cloudflare fix\n",
            "- [ ] [#yckq] [#ss01] ShipStation fix\n",
            "\n",
            "### 4. Internal Tooling / Documentation Carry-Forward\n",
            "- [ ] [#2gdt] [#wpmem] WP memory limits\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: monsterrodholders backlog follow-up — gpt-5\n\n",
            "Captured the requested backlog update.\n",
            "<!-- /patch:exchange -->\n\n",
            "<!-- patch:backlog -->\n",
            "### 2. Revenue / Fulfillment / Store Operations\n",
            "- [ ] [#new1] Verify direct rerun completed cleanly\n",
            "- [ ] [#2xcx] Verify ShipStation polling resumes after Cloudflare fix\n",
            "- [ ] [#yckq] [#ss01] ShipStation fix\n",
            "\n",
            "### 4. Internal Tooling / Documentation Carry-Forward\n",
            "- [ ] [#2gdt] [#wpmem] WP memory limits\n",
            "<!-- /patch:backlog -->\n",
        );

        apply_template_response(&doc, baseline, response, false)
            .expect("run path should normalize monsterrodholders-style backlog patches");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("### Re: monsterrodholders backlog follow-up — gpt-5"));
        assert!(updated.contains("- [ ] [#new1] Verify direct rerun completed cleanly"));
        assert!(updated.contains("- [ ] [#yckq] [#ss01] ShipStation fix"));
        assert!(updated.contains("- [ ] [#2gdt] [#wpmem] WP memory limits"));
    }

    #[test]
    fn apply_template_response_prefixes_direct_run_prompt_with_image_line() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "Read the image.\n",
            "![img_7.png](img_7.png)\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        snapshot::save(&doc, snapshot).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: image read — gpt-5\n\n",
            "The image line was handled.\n",
            "<!-- /patch:exchange -->\n",
        );

        apply_template_response(&doc, baseline, response, false)
            .expect("direct-run template write should normalize prompt lines");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("❯ Read the image.\n❯ ![img_7.png](img_7.png)\n"),
            "raw direct-run prompt block must be prefixed:\n{updated}"
        );
        assert!(
            updated.contains("### Re: image read — gpt-5 (HEAD)\n\nThe image line was handled."),
            "assistant response should be preserved:\n{updated}"
        );
        assert!(
            !updated.contains("❯ ### Re: image read"),
            "assistant response heading must not receive prompt prefix:\n{updated}"
        );
    }

    #[test]
    fn normalize_direct_run_prompt_prefixes_updates_baseline_before_precommit() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "Read the image.\n",
            "![img_7.png](img_7.png)\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        snapshot::save(&doc, snapshot).unwrap();

        let diff_text = crate::diff::unified_diff_from_contents(snapshot, baseline)
            .expect("snapshot and baseline differ");
        let normalized = normalize_direct_run_prompt_prefixes(&doc, baseline, &diff_text)
            .expect("direct-run baseline prompt normalization should succeed");
        let on_disk = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(normalized, on_disk);
        assert!(
            on_disk.contains("❯ Read the image.\n❯ ![img_7.png](img_7.png)\n"),
            "precommit baseline should be written with prompt prefixes:\n{on_disk}"
        );
    }

    #[test]
    fn run_rejects_bare_compact_exchange_directive() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n\n",
            "compact exchange\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let err = run(&doc, false, None, None, true, true, &Config::default())
            .expect_err("run should fail closed on unresolved compaction directive");
        let msg = err.to_string();
        assert!(msg.contains("compact exchange"));
        assert!(msg.contains("agent-doc compact"));
    }

    #[test]
    fn acquire_doc_lock_succeeds() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();
        let lock = acquire_doc_lock(&doc);
        assert!(lock.is_ok());
    }

    #[test]
    fn doc_lock_released_on_drop() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();
        {
            let _lock = acquire_doc_lock(&doc).unwrap();
        }
        // After drop, second acquire should succeed
        let lock2 = acquire_doc_lock(&doc);
        assert!(lock2.is_ok());
    }

    #[test]
    fn atomic_write_correct_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("atomic.md");
        atomic_write(&path, "hello world").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("overwrite.md");
        std::fs::write(&path, "old content").unwrap();
        atomic_write(&path, "new content").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
    }

    /// #ipc-drift-writeprovenance: the direct-run document-write path records the
    /// same write-provenance as the IPC/finalize `write.rs::atomic_write`, so a
    /// foreign-looking disk change from a direct-run write is positively
    /// attributed to agent-doc instead of inferred from the mtime heuristic.
    #[test]
    fn direct_run_atomic_write_records_provenance() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let path = dir.path().join("prov-direct-run.md");
        atomic_write(&path, "direct run body").unwrap();
        let key = path
            .canonicalize()
            .unwrap_or_else(|_| path.clone())
            .to_string_lossy()
            .to_string();
        let prov = crate::debounce::write_provenance(&key)
            .expect("direct-run document write should record provenance");
        assert_eq!(prov.len, "direct run body".len());
        assert_eq!(prov.hash, crate::debounce::content_hash("direct run body"));
        assert_eq!(prov.actor, "agent");
        assert!(!prov.write_id.is_empty());
    }

    /// 08b end state (removal rung complete): the direct-run document-write path
    /// is no longer a parallel direct-disk writer — it routes through the session
    /// actor's ordered write queue, the SAME chokepoint as the IPC/finalize path
    /// (no flag). The routed write re-enters `atomic_write` on the owner thread;
    /// the owner-scope re-entrancy guard keeps that inner write on the raw path,
    /// so this must not deadlock, the content must land, and the routed decision
    /// must be reported to `ops.log` (proving no surviving direct-disk writer
    /// bypasses the queue).
    #[test]
    fn direct_run_atomic_write_routes_through_queue() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc").join("logs")).unwrap();
        let path = dir.path().join("routed-direct-run.md");
        atomic_write(&path, "routed direct-run body").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "routed direct-run body"
        );
        let ops =
            std::fs::read_to_string(dir.path().join(".agent-doc").join("logs").join("ops.log"))
                .unwrap_or_default();
        assert!(
            ops.contains("write_authority action=routed"),
            "direct-run write must route through the queue and \
             report it to ops.log: {ops:?}"
        );
    }

    #[test]
    fn concurrent_atomic_writes_no_corruption() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("concurrent.md");
        std::fs::write(&path, "initial").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for i in 0..n {
            let p = path.clone();
            let bar = Arc::clone(&barrier);
            let content = format!("writer-{}-content", i);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                atomic_write(&p, &content).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Final content should be exactly one of the valid writes
        let final_content = std::fs::read_to_string(&path).unwrap();
        assert!(final_content.starts_with("writer-"));
        assert!(final_content.ends_with("-content"));
    }

    // -----------------------------------------------------------------------
    // Lazy parallelization: functional tests
    // -----------------------------------------------------------------------

    /// Simulate two document cycles on different files running in parallel.
    /// Both should complete without interference — no shared lock contention.
    #[test]
    fn parallel_different_files_no_interference() {
        let dir = TempDir::new().unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "initial-a").unwrap();
        std::fs::write(&doc_b, "initial-b").unwrap();

        let barrier = Arc::new(Barrier::new(2));

        let bar_a = Arc::clone(&barrier);
        let path_a = doc_a.clone();
        let ha = std::thread::spawn(move || {
            let _lock = acquire_doc_lock(&path_a).unwrap();
            bar_a.wait(); // both threads hold their own lock simultaneously
            // Simulate read-modify-write cycle
            let content = std::fs::read_to_string(&path_a).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
            atomic_write(&path_a, &format!("{}\n## Assistant\nResponse A", content)).unwrap();
        });

        let bar_b = Arc::clone(&barrier);
        let path_b = doc_b.clone();
        let hb = std::thread::spawn(move || {
            let _lock = acquire_doc_lock(&path_b).unwrap();
            bar_b.wait(); // both threads hold their own lock simultaneously
            let content = std::fs::read_to_string(&path_b).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
            atomic_write(&path_b, &format!("{}\n## Assistant\nResponse B", content)).unwrap();
        });

        ha.join().unwrap();
        hb.join().unwrap();

        let a = std::fs::read_to_string(&doc_a).unwrap();
        let b = std::fs::read_to_string(&doc_b).unwrap();
        assert!(a.contains("Response A"), "Doc A missing response: {}", a);
        assert!(b.contains("Response B"), "Doc B missing response: {}", b);
        assert!(!a.contains("Response B"), "Doc A has B's response");
        assert!(!b.contains("Response A"), "Doc B has A's response");
    }

    /// Simulate two document cycles on the SAME file running concurrently.
    /// flock serializes them — both writes land, no corruption.
    #[test]
    fn same_file_serialized_by_flock() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("shared.md");
        std::fs::write(&doc, "# Shared Doc\n").unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();

        for i in 0..2 {
            let path = doc.clone();
            let bar = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                bar.wait(); // both start at the same time
                let lock = acquire_doc_lock(&path).unwrap();
                // Critical section: read, modify, write
                let content = std::fs::read_to_string(&path).unwrap();
                let updated = format!("{}writer-{}\n", content, i);
                std::thread::sleep(std::time::Duration::from_millis(5));
                atomic_write(&path, &updated).unwrap();
                drop(lock);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let final_content = std::fs::read_to_string(&doc).unwrap();
        // Both writers should have appended (serialized by flock)
        assert!(
            final_content.contains("writer-0") && final_content.contains("writer-1"),
            "Both writes should land (flock serializes): {}",
            final_content
        );
    }

    /// Verify that a locked document cycle prevents concurrent reads of
    /// partial state — the second reader waits for the lock to be released.
    #[test]
    fn flock_prevents_partial_read_during_write() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("partial.md");
        std::fs::write(&doc, "before").unwrap();

        let path_w = doc.clone();
        let path_r = doc.clone();
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();

        // Writer: acquire lock, pause, then write
        let writer = std::thread::spawn(move || {
            let lock = acquire_doc_lock(&path_w).unwrap();
            locked_tx.send(()).unwrap();
            // Hold lock while "processing"
            std::thread::sleep(std::time::Duration::from_millis(50));
            atomic_write(&path_w, "after").unwrap();
            drop(lock);
        });

        // Reader: wait until writer definitely holds the lock, then block until release.
        locked_rx.recv().unwrap();
        let reader = std::thread::spawn(move || {
            let _lock = acquire_doc_lock(&path_r).unwrap();
            // By the time we get the lock, writer has finished
            std::fs::read_to_string(&path_r).unwrap()
        });

        writer.join().unwrap();
        let read_content = reader.join().unwrap();
        assert_eq!(
            read_content, "after",
            "Reader should see completed write, not partial state"
        );
    }

    #[test]
    fn merge_clean_no_conflicts() {
        // merge_contents spawns `git merge-file` which inherits CWD.
        // Other tests may invalidate CWD via TempDir drops, so we
        // perform the merge manually using temp files + Command with
        // an explicit current_dir to avoid CWD pollution.
        let dir = TempDir::new().unwrap();
        let base_path = dir.path().join("base");
        let ours_path = dir.path().join("ours");
        let theirs_path = dir.path().join("theirs");

        let base = "line 1\nline 2\nline 3\n";
        let ours = "line 1\nline 2\nline 3\n\n## Assistant\n\nResponse here.\n";
        let theirs = "line 1\nline 2\nline 3\n";

        std::fs::write(&base_path, base).unwrap();
        std::fs::write(&ours_path, ours).unwrap();
        std::fs::write(&theirs_path, theirs).unwrap();

        let output = std::process::Command::new("git")
            .current_dir(dir.path())
            .args([
                "merge-file",
                "-p",
                "--diff3",
                "-L",
                "agent-response",
                "-L",
                "original",
                "-L",
                "your-edits",
            ])
            .arg(&ours_path)
            .arg(&base_path)
            .arg(&theirs_path)
            .output()
            .unwrap();

        let merged = String::from_utf8(output.stdout).unwrap();
        assert!(output.status.success(), "merge should be clean");
        assert!(merged.contains("Response here."));
    }
