    use super::*;
    use std::sync::{Arc, Mutex};

    fn empty_operator_status(
        record: Option<agent_doc_orchestration::session_actor::ActorRecord>,
    ) -> agent_doc_orchestration::project_controller::SessionOperatorStatus {
        agent_doc_orchestration::project_controller::SessionOperatorStatus {
            record,
            transitions: Vec::new(),
            supervisor_lease: None,
            dispatch_attempts: Vec::new(),
            projection_diagnostics: Vec::new(),
        }
    }

    fn test_actor_record(state: ActorState) -> agent_doc_orchestration::session_actor::ActorRecord {
        agent_doc_orchestration::session_actor::ActorRecord {
            document_id: "/tmp/doc.md".to_string(),
            session_id: "session-1".to_string(),
            generation: 7,
            pane_id: "%7".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state,
            last_transition: agent_doc_orchestration::session_actor::ActorLastTransition {
                caller: "test".to_string(),
                reason: "test".to_string(),
                timestamp: 1,
                prior_generation: 7,
                new_generation: 7,
            },
        }
    }

    fn test_supervisor_runtime(actor_state: Option<ActorState>) -> SupervisorRuntime {
        SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state,
            actor_session_id: Some("session-1".to_string()),
            actor_pane_id: Some("%7".to_string()),
            actor_generation: Some(7),
            supervisor_state: Some("healthy".to_string()),
            restart_count: 0,
            supervisor_pid: Some(100),
            supervisor_instance_id: Some("sup-1".to_string()),
            child_pid: Some(101),
            cwd_source: Some("config".to_string()),
        }
    }

    fn test_session_context(
        record: agent_doc_orchestration::session_actor::ActorRecord,
        runtime: SupervisorRuntime,
        lease_state: Option<&str>,
    ) -> SessionContext {
        let lease = lease_state.map(|state| {
            agent_doc_orchestration::project_controller::SupervisorLeaseStatus {
                generation: 7,
                supervisor_pid: Some(100),
                supervisor_socket: Some("/tmp/supervisor.sock".to_string()),
                last_heartbeat: Some(1),
                runtime_state: Some(state.to_string()),
            }
        });
        SessionContext {
            canonical_file: PathBuf::from("/tmp/doc.md"),
            base_dir: PathBuf::from("/tmp"),
            session_id: "session-1".to_string(),
            harness: "codex".to_string(),
            actor_record: Some(record.clone()),
            operator_status: agent_doc_orchestration::project_controller::SessionOperatorStatus {
                record: Some(record),
                transitions: Vec::new(),
                supervisor_lease: lease,
                dispatch_attempts: Vec::new(),
                projection_diagnostics: Vec::new(),
            },
            registry_entry: None,
            startup_miss: None,
            log_status: None,
            supervisor_runtime: runtime,
            supervisor_socket: PathBuf::from("/tmp/supervisor.sock"),
        }
    }

    #[test]
    fn parse_actor_state_handles_known_values() {
        assert_eq!(parse_actor_state("ready"), Some(ActorState::Ready));
        assert_eq!(
            parse_actor_state("waiting_input"),
            Some(ActorState::WaitingInput)
        );
        assert_eq!(parse_actor_state("unknown"), None);
    }

    #[test]
    fn live_pane_prompt_ready_detects_idle_opencode_prompt() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();

        assert!(live_pane_prompt_ready(&harness, "work complete\n>\n"));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_opencode_status_chrome_without_proof_output() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();

        assert!(live_pane_prompt_ready(
            &harness,
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_opencode_idle_splash_without_prompt_glyph() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();

        assert!(live_pane_prompt_ready(
            &harness,
            "\
                                                                                                     ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▄ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀
                                                                                   ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                                   ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                                   ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                                   tab agents  ctrl+p commands
                                                                                        ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_status_chrome_only_output() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_xhigh_status_chrome_only_output() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 41% used\n"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_footer_below_prior_output() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "\
### Re: prior turn
The valid choices in that state are wait for the prompt, refresh after it returns idle, or use explicit clear.
gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_codex_drafted_input_above_footer() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(!live_pane_prompt_ready(
            &harness,
            "\
› investigate this issue
gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_default_placeholder() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "\
› Ask Codex to do anything
gpt-5.5 high · ~/work/btakita/agent-loop · Context 55% used
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_codex_write_tests_placeholder() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(live_pane_prompt_ready(
            &harness,
            "\
› Write tests for @filename
gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_codex_working_status_above_placeholder() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(!live_pane_prompt_ready(
            &harness,
            "\
• Working (1m 34s • esc to interrupt)

› Write tests for @filename
gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used
"
        ));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_active_output_after_prompt() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();

        assert!(!live_pane_prompt_ready(
            &harness,
            "›\nexploring repository\n"
        ));
    }

    // #jb-stale-busy-idle-footer: a genuinely idle Claude pane (composer + status
    // + permissions) must project ready, while a mid-turn pane (spinner) must not.
    #[test]
    fn live_pane_prompt_ready_accepts_idle_claude_composer() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::claude();
        let idle = concat!(
            "────────────────────\n",
            "❯\n",
            "────────────────────\n",
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@cachyos-x8664\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)\n",
        );
        assert!(live_pane_prompt_ready(&harness, idle));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_busy_claude_turn() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::claude();
        // Mid-turn: spinner above an otherwise-idle-looking composer. The busy cue
        // must win so the live turn is never clobbered by dispatch/clear.
        let busy = concat!(
            "· Roosting… (14s · ↓ 487 tokens · thinking with high effort)\n",
            "────────────────────\n",
            "❯\n",
            "────────────────────\n",
            "  Opus 4.8 (1M context) ctx:23% ~/work/btakita/agent-loop/resume main brian@host\n",
            "  ⏵⏵ bypass permissions on · 1 shell\n",
        );
        assert!(!live_pane_prompt_ready(&harness, busy));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_claude_idle_footer_after_stale_busy_scrollback() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::claude();
        let idle_after_clear = concat!(
            "✶ Generating… (3s · esc to interrupt)\n",
            "  ❯ /clear\n",
            "────────────────────\n",
            "❯ Press up to edit queued messages\n",
            "────────────────────\n",
            "  Opus 4.8 ctx:10% ~/work/btakita/agent-loop main brian@host\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents\n",
        );

        assert!(live_pane_prompt_ready(&harness, idle_after_clear));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_claude_active_spinner_footer() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::claude();
        let active = concat!(
            "✶ Generating… (3s · esc to interrupt)\n",
            "❯\n",
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@host\n",
            "  ⏵⏵ bypass permissions on · 1 shell\n",
        );

        assert!(!live_pane_prompt_ready(&harness, active));
    }

    #[test]
    fn live_pane_prompt_ready_accepts_idle_claude_when_status_line_is_last() {
        // The plan's question-1 state: no trailing `⏵⏵` line, status line last.
        // With the status line ignorable and no busy cue, the `⏵⏵` composer above
        // it becomes the candidate → ready.
        let harness = agent_doc_orchestration::harness::HarnessConfig::claude();
        let idle = concat!(
            "❯\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)\n",
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@cachyos-x8664\n",
        );
        assert!(live_pane_prompt_ready(&harness, idle));
    }

    #[test]
    fn live_pane_prompt_ready_opencode_context_bar_idle_hint() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();
        let idle = "⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13\n";
        assert!(live_pane_prompt_ready(&harness, idle));
    }

    #[test]
    fn live_pane_prompt_ready_opencode_context_bar_with_scrollback() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();
        let idle = concat!(
            "Thought: I need to check the files\n",
            "Click to expand\n",
            "  ~/work/btakita/agent-loop:main                                        1.15.13\n",
            "⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13\n",
        );
        assert!(live_pane_prompt_ready(&harness, idle));
    }

    #[test]
    fn live_pane_prompt_ready_rejects_opencode_active_turn_with_context_bar() {
        let harness = agent_doc_orchestration::harness::HarnessConfig::opencode();
        let busy = concat!(
            "Working (14s - esc to interrupt)\n",
            "⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13\n",
        );
        assert!(!live_pane_prompt_ready(&harness, busy));
    }

    #[test]
    fn protected_clear_refusal_points_to_interrupt_clear() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("gpt-5.5 high · ~/work/btakita/agent-loop · Context 85% used".to_string()),
        };

        let message = protected_clear_refusal_message(
            Path::new("/tmp/doc.md"),
            &evidence,
            "drafted prompt input",
        );

        assert!(message.contains("session_clear refused"));
        assert!(message.contains("pane %7 contains protected prompt input"));
        assert!(message.contains("reason=drafted prompt input"));
        assert!(message.contains("agent-doc session interrupt-clear /tmp/doc.md"));
    }

    #[test]
    fn operator_clear_allows_agent_doc_wrapper_without_busy_cue() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 41% used".to_string()),
        };

        let state = operator_clear_input_state_for_evidence(&evidence, false, false, false);

        assert_eq!(state, OperatorClearInputState::IdlePrompt);
        assert_eq!(
            agent_doc_orchestration::flow::operator_clear::clear_guard_outcome(state),
            agent_doc_orchestration::flow::types::FlowOutcome::Completed
        );
    }

    #[test]
    fn operator_clear_blocks_explicit_busy_cue() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("Working...".to_string()),
        };

        let state = operator_clear_input_state_for_evidence(&evidence, false, false, true);

        assert_eq!(state, OperatorClearInputState::Busy);
        assert_eq!(
            agent_doc_orchestration::flow::operator_clear::clear_guard_outcome(state),
            agent_doc_orchestration::flow::types::FlowOutcome::Blocked
        );
        let message =
            busy_clear_refusal_message(Path::new("/tmp/doc.md"), &evidence, "active codex turn");
        assert!(message.contains("session_clear refused"));
        assert!(message.contains("pane %7 is alive-busy"));
        assert!(message.contains("reason=active codex turn"));
        assert!(message.contains("agent-doc session interrupt-clear /tmp/doc.md"));
    }

    #[test]
    fn force_interrupt_clear_summary_reports_destructive_cleanup() {
        let message = force_interrupt_clear_summary(
            Path::new("/tmp/doc.md"),
            Some("%7"),
            ForceInterruptClearReport {
                actor_closed: true,
                registry_removed: true,
                supervisor_signaled: true,
                child_signaled: false,
                pane_killed: true,
                socket_removed: true,
                cooldown_written: true,
            },
        );

        assert!(message.contains("Force-cleared session for /tmp/doc.md"));
        assert!(message.contains("pane=%7"));
        assert!(message.contains("actor_closed=true"));
        assert!(message.contains("registry_removed=true"));
        assert!(message.contains("supervisor_signaled=true"));
        assert!(message.contains("pane_killed=true"));
        assert!(message.contains("socket_removed=true"));
        assert!(message.contains("cooldown_written=true"));
    }

    #[test]
    fn restart_busy_refusal_points_to_force() {
        let message = restart_busy_refusal_message(
            Path::new("/tmp/doc.md"),
            "%7",
            "authoritative_actor",
            "agent-doc",
            Some("• Working (7m 47s · esc to interrupt)"),
            "⏵⏵ bypass permissions on (shift+tab to cycle)",
        );

        assert!(message.contains("session_restart refused"));
        assert!(message.contains("pane %7 is alive-busy"));
        assert!(message.contains("source=authoritative_actor"));
        assert!(message.contains("current_command=agent-doc"));
        // Busy-proof line is surfaced so the busy state is self-evident, not the
        // ambiguous permission footer (#session-restart-refusal-shows-busy-proof).
        assert!(message.contains("busy_proof=\"• Working (7m 47s · esc to interrupt)\""));
        assert!(message.contains("bypass permissions"));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
        assert!(message.contains("pass `--force`"));
        assert!(message.contains("interrupt the running turn and restart anyway"));
    }

    #[test]
    fn restart_editor_holds_pane_refusal_names_editor_and_close_guidance() {
        // #hj7s: the refusal must carry the parseable header + fields the JB plugin
        // keys off, name the editor, and tell the operator to close it manually.
        let message = restart_editor_holds_pane_refusal_message(
            Path::new("/tmp/doc.md"),
            "%7",
            "authoritative_actor",
            "nvim",
            "-- INSERT --",
        );

        assert!(message.contains("session_restart refused for /tmp/doc.md"));
        assert!(message.contains("pane %7 is held by editor nvim"));
        assert!(message.contains("source=authoritative_actor"));
        assert!(message.contains("current_command=nvim"));
        assert!(message.contains("ctrl+g"));
        // UX: close the editor manually — do NOT force-quit or SIGINT around it.
        assert!(message.contains("Close the editor"));
        assert!(message.contains(":wq"));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
        // Must not advise --force: --force does not bypass the editor guard.
        assert!(!message.contains("--force"));
    }

    #[test]
    fn operator_clear_allows_clean_exit_prompt() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("Press Enter to restart...".to_string()),
        };

        let state = operator_clear_input_state_for_evidence(&evidence, false, true, false);

        assert_eq!(state, OperatorClearInputState::CleanExit);
        assert_eq!(
            agent_doc_orchestration::flow::operator_clear::clear_guard_outcome(state),
            agent_doc_orchestration::flow::types::FlowOutcome::Completed
        );
    }

    #[test]
    fn terminal_editor_command_detects_vim_family_processes() {
        for command in [
            "vi",
            "view",
            "vim",
            "vim.basic",
            "vimdiff",
            "nvim",
            "nvimdiff",
        ] {
            assert!(
                terminal_editor_command(command),
                "{command} should trigger interrupt-clear editor recovery"
            );
        }
        assert!(!terminal_editor_command("codex"));
        assert!(!terminal_editor_command("agent-doc"));
        assert!(!terminal_editor_command("vim-addon-manager"));
    }

    #[test]
    fn operator_interrupt_key_plan_omits_ctrl_g_for_codex_composer() {
        // #codex-interrupt-clear-ctrl-g-opens-editor: C-g opens the external
        // editor (nvim) in the Codex composer, so the normal interrupt path must
        // not send it — Escape + C-c is the safe interrupt.
        assert_eq!(
            operator_interrupt_key_plan("codex", false),
            vec!["Escape", "C-c"]
        );
        assert!(!operator_interrupt_key_plan("codex", false).contains(&"C-g"));
    }

    #[test]
    fn operator_interrupt_key_plan_sends_ctrl_g_only_for_codex_shell_search() {
        // C-g is safe (aborts the search) only when the Codex pane is in a shell
        // reverse-i-search / history-search state.
        assert_eq!(
            operator_interrupt_key_plan("codex", true),
            vec!["C-g", "Escape", "C-c"]
        );
    }

    #[test]
    fn operator_interrupt_key_plan_unchanged_for_other_harnesses() {
        // The codex_shell_search flag is codex-scoped and must not perturb other
        // harnesses' interrupt sequences.
        assert_eq!(
            operator_interrupt_key_plan("opencode", false),
            vec!["Escape", "Escape"]
        );
        assert_eq!(
            operator_interrupt_key_plan("opencode", true),
            vec!["Escape", "Escape"]
        );
        assert_eq!(operator_interrupt_key_plan("claude", false), vec!["C-c"]);
        assert_eq!(operator_interrupt_key_plan("claude", true), vec!["C-c"]);
    }

    #[test]
    fn interrupt_clear_timeout_message_reports_editor_recovery() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("vim".to_string()),
            prompt_ready: Some(false),
            tail: Some("-- INSERT --".to_string()),
        };
        let message =
            interrupt_clear_timeout_message(Path::new("/tmp/doc.md"), "%7", &evidence, true);

        assert!(message.contains("forced editor recovery"));
        assert!(message.contains("stayed alive-busy"));
        assert!(message.contains("source=authoritative_actor"));
        assert!(message.contains("current_command=vim"));
        assert!(message.contains("prompt_ready=false"));
        assert!(message.contains("tail=\"-- INSERT --\""));
        assert!(message.contains(":qa!"));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
    }

    #[test]
    fn interrupt_clear_timeout_message_reports_last_command_without_editor_recovery() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("codex".to_string()),
            prompt_ready: Some(false),
            tail: Some("⏵⏵ bypass permissions on".to_string()),
        };
        let message =
            interrupt_clear_timeout_message(Path::new("/tmp/doc.md"), "%7", &evidence, false);

        assert!(!message.contains("forced editor recovery"));
        assert!(message.contains("stayed alive-busy"));
        assert!(message.contains("source=authoritative_actor"));
        assert!(message.contains("current_command=codex"));
        assert!(message.contains("prompt_ready=false"));
        assert!(message.contains("tail=\"⏵⏵ bypass permissions on\""));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
    }

    #[test]
    fn interrupt_clear_initial_action_skips_interrupt_keys_for_idle_pane() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveIdle,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(true),
            tail: Some("gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used".to_string()),
        };

        assert_eq!(
            interrupt_clear_initial_action(&evidence),
            InterruptClearInitialAction::SkipInterruptAlreadyIdle
        );
    }

    #[test]
    fn interrupt_clear_initial_action_keeps_interrupt_for_busy_pane() {
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveBusy,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(false),
            tail: Some("Working...".to_string()),
        };

        assert_eq!(
            interrupt_clear_initial_action(&evidence),
            InterruptClearInitialAction::SendInterrupt
        );
    }

    #[test]
    fn interrupt_clear_timeout_outcome_reports_final_blocking_evidence() {
        let outcome = InterruptClearSettleOutcome::TimedOut {
            evidence: LivePaneEvidence {
                pane_id: Some("%7".to_string()),
                source: "authoritative_actor",
                state: LivePaneState::AliveBusy,
                current_command: Some("agent-doc".to_string()),
                prompt_ready: Some(false),
                tail: Some("reverse-i-search".to_string()),
            },
            editor_recovery_attempted: false,
        };

        assert_eq!(outcome.as_str(), "timed_out");
        assert_eq!(outcome.blocking_state(), "alive-busy");
        assert_eq!(outcome.blocking_source(), "authoritative_actor");
        assert_eq!(outcome.prompt_ready(), "false");
        assert_eq!(outcome.last_command(), Some("agent-doc"));
        assert_eq!(outcome.tail(), Some("reverse-i-search"));
    }

    #[test]
    fn operator_starting_guard_sees_supervisor_runtime_starting() {
        let record = test_actor_record(ActorState::Ready);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Starting)),
            None,
        );

        assert!(operator_command_has_starting_actor(&ctx));
    }

    #[test]
    fn live_evidence_target_ignores_closed_actor_pane() {
        let mut record = test_actor_record(ActorState::Closed);
        record.pane_id = "%stale".to_string();
        let mut ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Ready)),
            None,
        );
        ctx.registry_entry = Some(SessionEntry {
            pane: "%registry".to_string(),
            pid: 123,
            cwd: "/tmp".to_string(),
            started: "1".to_string(),
            session_id: "session-1".to_string(),
            file: "/tmp/doc.md".to_string(),
            window: "@1".to_string(),
            supervisor_instance_id: "sup-1".to_string(),
        });

        assert_eq!(
            live_evidence_target(&ctx),
            (Some("%registry".to_string()), "registry")
        );
    }

    #[test]
    fn operator_starting_guard_lets_matching_runtime_ready_override_stale_record() {
        let record = test_actor_record(ActorState::Starting);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Ready)),
            Some("ready"),
        );

        assert!(!operator_command_has_starting_actor(&ctx));
    }

    #[test]
    fn operator_starting_guard_accepts_legacy_session_scoped_runtime_ready() {
        let record = test_actor_record(ActorState::Starting);
        let mut runtime = test_supervisor_runtime(Some(ActorState::Ready));
        runtime.actor_session_id = None;
        runtime.actor_pane_id = None;
        runtime.actor_generation = None;
        let ctx = test_session_context(record, runtime, Some("starting"));

        assert!(!operator_command_has_starting_actor(&ctx));
    }

    #[test]
    fn operator_starting_guard_sees_supervisor_lease_starting() {
        let record = test_actor_record(ActorState::Ready);
        let ctx = test_session_context(record, test_supervisor_runtime(None), Some("starting"));

        assert!(operator_command_has_starting_actor(&ctx));
    }

    #[test]
    fn starting_operator_guard_does_not_gate_clear() {
        let record = test_actor_record(ActorState::Starting);
        let ctx = test_session_context(
            record,
            test_supervisor_runtime(Some(ActorState::Starting)),
            Some("starting"),
        );
        let tmux = Tmux::default_server();

        guard_starting_actor_operator_command(&ctx, &tmux, OperatorAction::Clear)
            .expect("session clear must not be gated by stale starting actor projections");
    }

    #[test]
    fn supervisor_clear_legacy_unsupported_error_matches_old_supervisor_parse_failure() {
        let legacy_error = "parse error: unknown variant `clear`, expected one of `restart`, `inject`, `state`, `pid`, `stop` at line 1 column 17";

        assert!(supervisor_clear_legacy_unsupported_error(legacy_error));
        assert!(!supervisor_clear_legacy_unsupported_error(
            "parse error: unknown variant `nonsense`, expected one of `restart`, `inject`, `state`, `pid`, `stop` at line 1 column 17"
        ));
        assert!(!supervisor_clear_legacy_unsupported_error(
            "supervisor response timeout (2s)"
        ));
    }

    #[test]
    fn starting_operator_guard_reason_blocks_restart_at_clean_exit_prompt() {
        assert_eq!(
            starting_operator_guard_reason(OperatorAction::Restart, false, false, true),
            "the pane has not reached a dispatch-ready prompt (`prompt_ready=true`)"
        );
    }

    #[test]
    fn starting_restart_refusal_points_to_force() {
        let message = starting_operator_guard_refusal_message(
            OperatorAction::Restart,
            Path::new("/tmp/doc.md"),
            "the document changed after the last committed cycle",
        );

        assert!(message.contains("session_restart refused"));
        assert!(message.contains("the authoritative actor is still starting"));
        assert!(message.contains("the document changed after the last committed cycle"));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
        assert!(message.contains("Pass `--force`"));
        assert!(message.contains("interrupt the running turn and restart anyway"));
    }

    #[test]
    fn document_dirty_after_committed_cycle_detects_post_commit_edit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/doc.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let committed = "---\nagent_doc_session: session-1\n---\n\nDone.\n";
        std::fs::write(&doc, committed).unwrap();
        agent_doc_orchestration::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        assert!(!document_dirty_after_committed_cycle(&doc).unwrap());

        std::fs::write(&doc, format!("{committed}\nnew prompt\n")).unwrap();

        assert!(document_dirty_after_committed_cycle(&doc).unwrap());
    }

    #[test]
    fn last_meaningful_pane_line_trims_ansi_and_blank_lines() {
        assert_eq!(
            last_meaningful_pane_line("\x1b[32mworking\x1b[0m\n\n").as_deref(),
            Some("working")
        );
    }

    #[test]
    fn idle_direct_pane_evidence_supersedes_stale_busy_projection() {
        let record = agent_doc_orchestration::session_actor::ActorRecord {
            document_id: "/tmp/doc.md".to_string(),
            session_id: "session-1".to_string(),
            generation: 7,
            pane_id: "%7".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Busy,
            last_transition: agent_doc_orchestration::session_actor::ActorLastTransition {
                caller: "supervisor".to_string(),
                reason: "work_started".to_string(),
                timestamp: 1,
                prior_generation: 7,
                new_generation: 7,
            },
        };
        let ctx = SessionContext {
            canonical_file: PathBuf::from("/tmp/doc.md"),
            base_dir: PathBuf::from("/tmp"),
            session_id: "session-1".to_string(),
            harness: "codex".to_string(),
            actor_record: Some(record.clone()),
            operator_status: agent_doc_orchestration::project_controller::SessionOperatorStatus {
                record: Some(record),
                transitions: Vec::new(),
                supervisor_lease: Some(
                    agent_doc_orchestration::project_controller::SupervisorLeaseStatus {
                        generation: 7,
                        supervisor_pid: Some(100),
                        supervisor_socket: Some("/tmp/supervisor.sock".to_string()),
                        last_heartbeat: Some(1),
                        runtime_state: Some("busy".to_string()),
                    },
                ),
                dispatch_attempts: Vec::new(),
                projection_diagnostics: Vec::new(),
            },
            registry_entry: None,
            startup_miss: None,
            log_status: None,
            supervisor_runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(ActorState::Busy),
                actor_session_id: Some("session-1".to_string()),
                actor_pane_id: Some("%7".to_string()),
                actor_generation: Some(7),
                supervisor_state: Some("healthy".to_string()),
                restart_count: 0,
                supervisor_pid: Some(100),
                supervisor_instance_id: Some("sup-1".to_string()),
                child_pid: Some(101),
                cwd_source: Some("config".to_string()),
            },
            supervisor_socket: PathBuf::from("/tmp/supervisor.sock"),
        };
        let evidence = LivePaneEvidence {
            pane_id: Some("%7".to_string()),
            source: "authoritative_actor",
            state: LivePaneState::AliveIdle,
            current_command: Some("agent-doc".to_string()),
            prompt_ready: Some(true),
            tail: Some(">".to_string()),
        };

        assert!(idle_projection_needs_reconciliation(&ctx, &evidence));
        let busy_evidence = LivePaneEvidence {
            state: LivePaneState::AliveBusy,
            prompt_ready: Some(false),
            ..evidence
        };
        assert!(!idle_projection_needs_reconciliation(&ctx, &busy_evidence));
    }

    #[test]
    fn status_context_refreshes_controller_lease_from_matching_live_supervisor() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/status.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-status\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_orchestration::session_actor::record_session_start_direct(
            &doc,
            "session-status",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_orchestration::session_actor::transition_state_direct(
            &doc,
            "session-status",
            "%41",
            Some(1),
            ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();
        agent_doc_orchestration::project_controller::refresh_supervisor_lease(
            dir.path(),
            agent_doc_orchestration::project_controller::SupervisorHeartbeatRequest {
                file: doc.clone(),
                session_id: "session-status".to_string(),
                pane_id: "%41".to_string(),
                generation: 1,
                supervisor_pid: Some(999),
                supervisor_socket: Some("/tmp/stale.sock".to_string()),
                runtime_state: "starting".to_string(),
            },
        )
        .unwrap();

        let sock = agent_doc_orchestration::supervisor::ipc::SupervisorIpc::start(
            dir.path(),
            "session-status",
            {
                move |method| match method {
                    IpcMethod::State => agent_doc_orchestration::supervisor::ipc::IpcResponse::ok(
                        serde_json::json!({
                            "running": true,
                            "state": "healthy",
                            "actor_state": "ready",
                            "actor_session_id": "session-status",
                            "actor_pane_id": "%41",
                            "actor_generation": 1,
                            "restart_count": 0,
                            "supervisor_pid": 1001,
                            "supervisor_instance_id": "sup-status",
                            "child_pid": 1002,
                            "cwd_source": "config",
                        }),
                    ),
                    _ => agent_doc_orchestration::supervisor::ipc::IpcResponse::ok_empty(),
                }
            },
        )
        .unwrap();

        let ctx = build_context(&doc).unwrap();
        let lease = ctx.operator_status.supervisor_lease.unwrap();
        let expected_socket = sock.path().to_string_lossy().to_string();
        assert_eq!(lease.runtime_state.as_deref(), Some("ready"));
        assert_eq!(lease.supervisor_pid, Some(1001));
        assert_eq!(
            lease.supervisor_socket.as_deref(),
            Some(expected_socket.as_str())
        );
        assert_eq!(ctx.operator_status.transitions.len(), 2);
    }

    #[test]
    fn doctor_flags_missing_actor_and_registry() {
        let runtime = SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
            actor_session_id: None,
            actor_pane_id: None,
            actor_generation: None,
            supervisor_state: None,
            restart_count: 0,
            supervisor_pid: None,
            supervisor_instance_id: None,
            child_pid: None,
            cwd_source: None,
        };
        let ctx = SessionContext {
            canonical_file: PathBuf::from("/tmp/doc.md"),
            base_dir: PathBuf::from("/tmp"),
            session_id: "session-1".to_string(),
            harness: "codex".to_string(),
            actor_record: None,
            operator_status: empty_operator_status(None),
            registry_entry: None,
            startup_miss: None,
            log_status: None,
            supervisor_runtime: runtime,
            supervisor_socket: PathBuf::from("/tmp/missing.sock"),
        };
        let issues = collect_doctor_issues(&ctx);
        assert!(issues.iter().any(|issue| issue.contains("actor record")));
        assert!(issues.iter().any(|issue| issue.contains("registry entry")));
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("supervisor socket"))
        );
    }

    #[test]
    fn doctor_flags_closed_actor_with_stale_pane() {
        let record = test_actor_record(ActorState::Closed);
        let ctx = test_session_context(record, test_supervisor_runtime(None), None);

        let issues = collect_doctor_issues(&ctx);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("closed actor record still references pane %7"))
        );
    }

    #[test]
    fn harness_clear_command_maps_opencode_to_new() {
        // OpenCode has no `/clear` command; its clear-context equivalent is
        // `/new` (#opencode-clear-uses-new). Claude/Codex keep `/clear`.
        assert_eq!(harness_clear_command("opencode"), "/new");
        assert_eq!(harness_clear_command("claude"), "/clear");
        assert_eq!(harness_clear_command("codex"), "/clear");
        assert_eq!(harness_clear_command("unknown"), "/clear");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn send_clear_to_pane_submits_clear_command() {
        let dir = tempfile::tempdir().unwrap();
        let socket = format!("session-clear-direct-pane-{}", uuid::Uuid::new_v4());
        let iso = tmux_router::IsolatedTmux::new(&socket);
        let pane = iso.new_session("test", dir.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
        let output_path = dir.path().join("clear.txt");
        let done_path = dir.path().join("clear.done");
        iso.send_keys(
            &pane,
            &format!(
                "sh -lc 'IFS= read -r line; printf \"%s\" \"$line\" > \"{}\"; touch \"{}\"'",
                output_path.display(),
                done_path.display()
            ),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));

        send_clear_to_pane(&iso, &pane, Path::new("/tmp/doc.md"), "claude").unwrap();
        for _ in 0..40 {
            if done_path.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            done_path.exists(),
            "expected `/clear` to submit through pane input"
        );
        assert_eq!(std::fs::read_to_string(&output_path).unwrap(), "/clear");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn clear_falls_back_to_supervisor_inject_when_authoritative_pane_is_not_on_default_tmux() {
        let dir = tempfile::tempdir().unwrap();
        let iso = tmux_router::IsolatedTmux::new("session-clear-direct-pane");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-clear\nagent: codex\n---\n",
        )
        .unwrap();
        let captured = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured_for_ipc = captured.clone();
        let sock = agent_doc_orchestration::supervisor::ipc::SupervisorIpc::start(
            dir.path(),
            "session-clear",
            {
                move |method| match method {
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        captured_for_ipc.lock().unwrap().push(bytes);
                        agent_doc_orchestration::supervisor::ipc::IpcResponse::ok_empty()
                    }
                    IpcMethod::State => agent_doc_orchestration::supervisor::ipc::IpcResponse::ok(
                        serde_json::json!({
                            "running": true,
                            "state": "healthy",
                            "actor_state": "ready",
                            "restart_count": 0,
                        }),
                    ),
                    _ => agent_doc_orchestration::supervisor::ipc::IpcResponse::ok_empty(),
                }
            },
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let pane_window = iso.pane_window(&pane).unwrap();
        agent_doc_orchestration::sessions::register("session-clear", &pane, &doc.to_string_lossy())
            .unwrap();
        agent_doc_orchestration::session_actor::record_session_start(
            &doc,
            "session-clear",
            &pane,
            &pane_window,
            1,
        )
        .unwrap();
        clear(&doc).unwrap();
        let latest = agent_doc_orchestration::codex_hook::load_latest_prompt_for_file(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(latest, "/clear");
        assert_eq!(
            captured.lock().unwrap().as_slice(),
            &[agent_doc_orchestration::supervisor::ipc::normalize_submit_text("/clear")]
        );
        drop(sock);
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_direct_submit_pane_prefers_authoritative_actor() {
        let dir = tempfile::tempdir().unwrap();
        let iso = tmux_router::IsolatedTmux::new("session-clear-pane-select-actor");
        let actor_pane = iso.new_session("test", dir.path()).unwrap();
        let registry_pane = iso.new_window("test", dir.path()).unwrap();
        let actor_record = agent_doc_orchestration::session_actor::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session-clear".to_string(),
            generation: 3,
            pane_id: actor_pane.clone(),
            window_id: iso.pane_window(&actor_pane).unwrap(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: agent_doc_orchestration::session_actor::ActorLastTransition {
                caller: "test".to_string(),
                reason: "actor".to_string(),
                timestamp: 1,
                prior_generation: 2,
                new_generation: 3,
            },
        };
        let ctx = SessionContext {
            canonical_file: dir.path().join("doc.md"),
            base_dir: dir.path().to_path_buf(),
            session_id: "session-clear".to_string(),
            harness: "codex".to_string(),
            actor_record: Some(actor_record.clone()),
            operator_status: empty_operator_status(Some(actor_record)),
            registry_entry: Some(SessionEntry {
                pane: registry_pane.clone(),
                pid: 1,
                cwd: dir.path().display().to_string(),
                started: "now".to_string(),
                session_id: "session-clear".to_string(),
                file: dir.path().join("doc.md").display().to_string(),
                window: iso.pane_window(&registry_pane).unwrap(),
                supervisor_instance_id: "sup".to_string(),
            }),
            startup_miss: None,
            log_status: None,
            supervisor_runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(ActorState::Ready),
                actor_session_id: None,
                actor_pane_id: None,
                actor_generation: None,
                supervisor_state: Some("healthy".to_string()),
                restart_count: 0,
                supervisor_pid: Some(1),
                supervisor_instance_id: Some("sup".to_string()),
                child_pid: Some(2),
                cwd_source: Some("config".to_string()),
            },
            supervisor_socket: dir.path().join("session-clear.sock"),
        };

        assert_eq!(
            resolve_direct_submit_pane(&ctx, &iso),
            Some((actor_pane, DirectSubmitPaneSource::AuthoritativeActor))
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_direct_submit_pane_falls_back_to_registry() {
        let dir = tempfile::tempdir().unwrap();
        let iso = tmux_router::IsolatedTmux::new("session-clear-pane-select-registry");
        let registry_pane = iso.new_session("test", dir.path()).unwrap();
        let actor_record = agent_doc_orchestration::session_actor::ActorRecord {
            document_id: "doc".to_string(),
            session_id: "session-clear".to_string(),
            generation: 3,
            pane_id: "%9999".to_string(),
            window_id: "@9999".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: agent_doc_orchestration::session_actor::ActorLastTransition {
                caller: "test".to_string(),
                reason: "actor".to_string(),
                timestamp: 1,
                prior_generation: 2,
                new_generation: 3,
            },
        };
        let ctx = SessionContext {
            canonical_file: dir.path().join("doc.md"),
            base_dir: dir.path().to_path_buf(),
            session_id: "session-clear".to_string(),
            harness: "codex".to_string(),
            actor_record: Some(actor_record.clone()),
            operator_status: empty_operator_status(Some(actor_record)),
            registry_entry: Some(SessionEntry {
                pane: registry_pane.clone(),
                pid: 1,
                cwd: dir.path().display().to_string(),
                started: "now".to_string(),
                session_id: "session-clear".to_string(),
                file: dir.path().join("doc.md").display().to_string(),
                window: iso.pane_window(&registry_pane).unwrap(),
                supervisor_instance_id: "sup".to_string(),
            }),
            startup_miss: None,
            log_status: None,
            supervisor_runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(ActorState::Ready),
                actor_session_id: None,
                actor_pane_id: None,
                actor_generation: None,
                supervisor_state: Some("healthy".to_string()),
                restart_count: 0,
                supervisor_pid: Some(1),
                supervisor_instance_id: Some("sup".to_string()),
                child_pid: Some(2),
                cwd_source: Some("config".to_string()),
            },
            supervisor_socket: dir.path().join("session-clear.sock"),
        };

        assert_eq!(
            resolve_direct_submit_pane(&ctx, &iso),
            Some((registry_pane, DirectSubmitPaneSource::Registry))
        );
    }

    fn clear_reclaim_project() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "# Doc\n\n## User\n\nDo the thing\n").unwrap();
        (dir, doc)
    }

    #[test]
    fn clear_reclaims_orphaned_empty_preflight_cycle() {
        let (_dir, doc) = clear_reclaim_project();
        let content = std::fs::read_to_string(&doc).unwrap();
        agent_doc_orchestration::cycle_state::start_preflight(&doc, Some(&content), Some(&content))
            .unwrap();

        // The clear path reclaims the orphaned cycle so the next Run Agent Doc
        // is not wedged by a stale open cycle.
        assert_eq!(
            reclaim_orphaned_cycle_on_clear(&doc),
            agent_doc_orchestration::repair::CancelOutcome::Abandoned
        );
        let state = agent_doc_orchestration::cycle_state::load(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            state.phase,
            agent_doc_orchestration::cycle_state::CyclePhase::Abandoned
        );
    }

    #[test]
    fn clear_protects_cycle_that_already_captured_a_response() {
        let (_dir, doc) = clear_reclaim_project();
        let content = std::fs::read_to_string(&doc).unwrap();
        agent_doc_orchestration::cycle_state::start_preflight(&doc, Some(&content), Some(&content))
            .unwrap();
        agent_doc_orchestration::capture::capture_response(
            &doc,
            "### Re: do — opus-4-8\n\nDone.\n",
        )
        .unwrap();

        // A cycle that owns a captured response must not be discarded by clear.
        assert_eq!(
            reclaim_orphaned_cycle_on_clear(&doc),
            agent_doc_orchestration::repair::CancelOutcome::Protected
        );
        assert!(
            agent_doc_orchestration::cycle_state::load(&doc)
                .unwrap()
                .unwrap()
                .is_open(),
            "clear must protect a cycle that already captured a response"
        );
    }

    #[test]
    fn clear_reclaim_is_noop_without_an_open_cycle() {
        let (_dir, doc) = clear_reclaim_project();
        assert_eq!(
            reclaim_orphaned_cycle_on_clear(&doc),
            agent_doc_orchestration::repair::CancelOutcome::NoOpenCycle
        );
    }
