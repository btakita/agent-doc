    use super::*;
    use std::cell::RefCell;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::test_support::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    struct FakeLifecycleOps {
        baseline_file: String,
        preflight_calls: RefCell<usize>,
        finalize_calls: RefCell<Vec<String>>,
        session_checks: RefCell<usize>,
    }

    impl LifecycleOps for FakeLifecycleOps {
        fn preflight(&self, file: &Path) -> Result<PreflightOutput> {
            *self.preflight_calls.borrow_mut() += 1;
            let doc = fs::read_to_string(file)?;
            Ok(PreflightOutput {
                diff: Some(format!(
                    "--- snapshot\n+++ document\n+{}",
                    doc.lines().last().unwrap_or("")
                )),
                no_changes: false,
                baseline_file: Some(self.baseline_file.clone()),
                ..PreflightOutput::default()
            })
        }

        fn finalize(
            &self,
            file: &Path,
            _baseline_file: Option<&str>,
            response: &str,
            _mode: ResolvedMode,
        ) -> Result<()> {
            self.finalize_calls.borrow_mut().push(response.to_string());
            write::apply_template_from_string(file, response)
        }

        fn session_check(&self, _file: &Path) -> Result<()> {
            *self.session_checks.borrow_mut() += 1;
            Ok(())
        }
    }

    type AgentEnv = Vec<(String, Option<String>)>;
    type ParallelRunCall = (
        String,
        Vec<parallel::ParallelTask>,
        Option<String>,
        bool,
        bool,
        u64,
        bool,
    );

    fn test_graph_evidence_plan() -> crate::tsift_graph::TsiftGraphEvidencePlan {
        crate::tsift_graph::TsiftGraphEvidencePlan {
            targets: vec!["gkke".to_string()],
            graph_db_status: crate::tsift_graph::TsiftGraphDbStatus {
                root: Some("/tmp/repo".to_string()),
                graph_db: Some("/tmp/repo/.tsift/graph.db".to_string()),
                status: "current".to_string(),
                content_hash: Some("abc".to_string()),
                source_watermark: Some("abc".to_string()),
                diagnostics: Vec::new(),
            },
            prompt_target_handles: vec![crate::tsift_graph::TsiftPromptTargetHandle {
                prompt_target: "do #gkke".to_string(),
                target: "gkke".to_string(),
                contract_version: Some("graph-db-evidence-v1".to_string()),
                evidence_packet_id: "gevd-gkke".to_string(),
                target_node_id: "gbak-gkke".to_string(),
                target_kind: "backlog".to_string(),
                target_label: "#gkke".to_string(),
                projection_hash: Some("abc".to_string()),
                worker_context_handles: vec!["wctx-gkke".to_string()],
                source_handles: vec!["src-gkke".to_string()],
                semantic_handles: Vec::new(),
                next_commands: Vec::new(),
                replay_commands: vec!["tsift graph-db evidence gkke --json".to_string()],
                repair_commands: Vec::new(),
            }],
            conflict_matrix: crate::tsift_graph::TsiftConflictMatrixSummary {
                contract_version: Some("conflict-matrix-v1".to_string()),
                can_parallel: true,
                fail_closed: false,
                inputs: Some(crate::tsift_graph::TsiftConflictMatrixInputs {
                    graph_db_evidence_targets: vec!["gkke".to_string()],
                    evidence_packets: vec![crate::tsift_graph::TsiftConflictMatrixEvidencePacket {
                        target: "gkke".to_string(),
                        packet_id: "gevd-gkke".to_string(),
                        target_node_id: "gbak-gkke".to_string(),
                        projection_hash: Some("abc".to_string()),
                        replay_command: Some("tsift graph-db evidence gkke --json".to_string()),
                    }],
                    context_pack_command: Some("tsift --envelope context-pack session.md --budget normal".to_string()),
                    cached_diff_command: Some("tsift diff-digest --cached /tmp/repo --json".to_string()),
                    impact_command: Some("tsift impact /tmp/repo --cached --limit 20 --json".to_string()),
                }),
                context_pack: Some(crate::tsift_graph::TsiftConflictMatrixContextSummary {
                    target: "session.md".to_string(),
                    target_kind: "agent_doc_session".to_string(),
                    prompt_targets: vec!["do #gkke".to_string()],
                    touched_files: vec!["src/orchestrate.rs".to_string()],
                    touched_symbols: vec!["run_ordered_tasks_internal".to_string()],
                    files_changed: 1,
                    worker_context: vec!["orchestration worker context".to_string()],
                    source_windows: vec!["src/orchestrate.rs:1-80".to_string()],
                    status_reminders: Vec::new(),
                }),
                candidates: vec![crate::tsift_graph::TsiftConflictMatrixCandidate {
                    target: "gkke".to_string(),
                    rank: 1,
                    risk: "low".to_string(),
                    risk_score: 0,
                    risk_reasons: Vec::new(),
                    evidence_packet_id: "gevd-gkke".to_string(),
                    target_node_id: "gbak-gkke".to_string(),
                    target_kind: "backlog".to_string(),
                    target_label: "#gkke".to_string(),
                    owned_files: vec!["src/orchestrate.rs".to_string()],
                    owned_symbols: vec!["run_ordered_tasks_internal".to_string()],
                    config_files: Vec::new(),
                    affected_tests: vec!["cargo test orchestrate".to_string()],
                    staged_files: Vec::new(),
                    staged_symbols: Vec::new(),
                    staged_tests: Vec::new(),
                    staged_config_files: Vec::new(),
                    semantic_dispatch_score: 4,
                    semantic_dispatch_reasons: vec!["source handle matched orchestration".to_string()],
                    worker_feedback: Some(crate::tsift_graph::TsiftWorkerFeedbackSummary {
                        total: 1,
                        completed: 1,
                        blocked: 0,
                        touched_files: vec!["src/orchestrate.rs".to_string()],
                        expected_tests: vec!["cargo test orchestrate".to_string()],
                        follow_up_ids: Vec::new(),
                        outcome_history: vec!["completed #gkke".to_string()],
                        repeated_blockage: false,
                        stale_expected_tests: Vec::new(),
                        follow_up_debt: Vec::new(),
                        closure_rank_score: 0,
                        closure_rank_reasons: Vec::new(),
                        warnings: Vec::new(),
                    }),
                }],
                conflicts: Vec::new(),
                evidence_packet_ids: vec!["gevd-gkke".to_string()],
                decisions: vec!["candidate #1 gkke risk=low".to_string()],
                worker_ownership_blocks: vec!["Worker 1 owns gkke (#gkke)".to_string()],
                worker_prompt_packets: vec![crate::tsift_graph::TsiftWorkerPromptPacket {
                    contract_version: Some("worker-prompt-packet-v1".to_string()),
                    packet_id: Some("wpp-gkke".to_string()),
                    target: "gkke".to_string(),
                    rank: 1,
                    risk: "low".to_string(),
                    projection_hash: Some("abc".to_string()),
                    title: "Worker 1 owns gkke (#gkke)".to_string(),
                    owned_files: vec!["src/orchestrate.rs".to_string()],
                    owned_symbols: vec!["run_ordered_tasks_internal".to_string()],
                    read_only_context: vec!["src-gkke".to_string()],
                    forbidden_files: Vec::new(),
                    expected_tests: vec!["cargo test orchestrate".to_string()],
                    expansion_commands: vec!["tsift graph-db evidence gkke --json".to_string()],
                    token_budget: Some(crate::tsift_graph::TsiftWorkerPromptTokenBudget {
                        prompt_estimated_tokens: 32,
                        max_prompt_tokens: 256,
                        source_window_count: 1,
                        source_window_lines: 80,
                        max_context_bytes: 9600,
                    }),
                    semantic_dispatch_score: 4,
                    semantic_dispatch_reasons: vec![
                        "source handle matched orchestration".to_string()
                    ],
                    worker_feedback: Some(crate::tsift_graph::TsiftWorkerFeedbackSummary {
                        total: 1,
                        completed: 1,
                        blocked: 0,
                        touched_files: vec!["src/orchestrate.rs".to_string()],
                        expected_tests: vec!["cargo test orchestrate".to_string()],
                        follow_up_ids: Vec::new(),
                        outcome_history: vec!["completed #gkke".to_string()],
                        repeated_blockage: false,
                        stale_expected_tests: Vec::new(),
                        follow_up_debt: Vec::new(),
                        closure_rank_score: 0,
                        closure_rank_reasons: Vec::new(),
                        warnings: Vec::new(),
                    }),
                    prompt: Some(
                        "Worker 1 owns gkke (#gkke)\n\nFail closed if the task requires a forbidden/shared file."
                            .to_string(),
                    ),
                }],
                next_commands: Vec::new(),
                warnings: Vec::new(),
            },
            dispatch_trace: Some(crate::tsift_graph::TsiftDispatchTraceSummary {
                contract_version: Some("dispatch-trace-v1".to_string()),
                projection_freshness: crate::tsift_graph::TsiftProjectionFreshness {
                    status: "current".to_string(),
                    fail_closed: false,
                    content_hash: Some("abc".to_string()),
                    source_watermark: Some("abc".to_string()),
                    diagnostics: Vec::new(),
                },
                projection_hashes: vec!["abc".to_string()],
                evidence_packet_ids: vec!["gevd-gkke".to_string()],
                worker_prompt_packets: vec![crate::tsift_graph::TsiftWorkerPromptPacket {
                    contract_version: Some("worker-prompt-packet-v1".to_string()),
                    packet_id: Some("wpp-gkke".to_string()),
                    target: "gkke".to_string(),
                    rank: 1,
                    risk: "low".to_string(),
                    projection_hash: Some("abc".to_string()),
                    title: "Worker 1 owns gkke (#gkke)".to_string(),
                    owned_files: vec!["src/orchestrate.rs".to_string()],
                    owned_symbols: vec!["run_ordered_tasks_internal".to_string()],
                    read_only_context: vec!["src-gkke".to_string()],
                    forbidden_files: Vec::new(),
                    expected_tests: vec!["cargo test orchestrate".to_string()],
                    expansion_commands: vec!["tsift graph-db evidence gkke --json".to_string()],
                    token_budget: Some(crate::tsift_graph::TsiftWorkerPromptTokenBudget {
                        prompt_estimated_tokens: 32,
                        max_prompt_tokens: 256,
                        source_window_count: 1,
                        source_window_lines: 80,
                        max_context_bytes: 9600,
                    }),
                    semantic_dispatch_score: 4,
                    semantic_dispatch_reasons: vec![
                        "source handle matched orchestration".to_string()
                    ],
                    worker_feedback: Some(crate::tsift_graph::TsiftWorkerFeedbackSummary {
                        total: 1,
                        completed: 1,
                        blocked: 0,
                        touched_files: vec!["src/orchestrate.rs".to_string()],
                        expected_tests: vec!["cargo test orchestrate".to_string()],
                        follow_up_ids: Vec::new(),
                        outcome_history: vec!["completed #gkke".to_string()],
                        repeated_blockage: false,
                        stale_expected_tests: Vec::new(),
                        follow_up_debt: Vec::new(),
                        closure_rank_score: 0,
                        closure_rank_reasons: Vec::new(),
                        warnings: Vec::new(),
                    }),
                    prompt: Some(
                        "Worker 1 owns gkke (#gkke)\n\nFail closed if the task requires a forbidden/shared file."
                            .to_string(),
                    ),
                }],
                worker_feedback: vec![crate::tsift_graph::TsiftWorkerFeedbackSummary {
                    total: 1,
                    completed: 1,
                    blocked: 0,
                    touched_files: vec!["src/orchestrate.rs".to_string()],
                    expected_tests: vec!["cargo test orchestrate".to_string()],
                    follow_up_ids: Vec::new(),
                    outcome_history: vec!["completed #gkke".to_string()],
                    repeated_blockage: false,
                    stale_expected_tests: Vec::new(),
                    follow_up_debt: Vec::new(),
                    closure_rank_score: 0,
                    closure_rank_reasons: Vec::new(),
                    warnings: Vec::new(),
                }],
                graph_nodes: vec![
                    crate::tsift_graph::TsiftDispatchTraceGraphNode {
                        id: "gbak-gkke".to_string(),
                        kind: "backlog".to_string(),
                        label: "#gkke".to_string(),
                        properties: std::collections::BTreeMap::new(),
                    },
                    crate::tsift_graph::TsiftDispatchTraceGraphNode {
                        id: "wres-gkke".to_string(),
                        kind: "worker_result".to_string(),
                        label: "completed #gkke".to_string(),
                        properties: std::collections::BTreeMap::new(),
                    },
                ],
                graph_edges: vec![crate::tsift_graph::TsiftDispatchTraceGraphEdge {
                    from_id: "gbak-gkke".to_string(),
                    to_id: "wres-gkke".to_string(),
                    kind: "has_result".to_string(),
                }],
                replay_commands: vec!["tsift conflict-matrix --path /tmp/repo gkke --json".to_string()],
                repair_commands: vec!["tsift graph-db --path /tmp/repo refresh --json".to_string()],
                warnings: Vec::new(),
            }),
            next_commands: Vec::new(),
        }
    }

    struct FakeAgentRunner {
        prompts: RefCell<Vec<String>>,
        envs: RefCell<Vec<AgentEnv>>,
        fresh_calls: RefCell<usize>,
        streaming_calls: RefCell<usize>,
        response: String,
        streaming_chunks: Option<Vec<StreamChunk>>,
    }

    struct MutatingAgentRunner {
        fresh_calls: RefCell<usize>,
        response: String,
    }

    impl FreshAgentRunner for FakeAgentRunner {
        fn send_fresh(
            &self,
            _file: &Path,
            prompt: &str,
            _agent_name: &str,
            _agent_config: Option<&AgentConfig>,
            _env: Vec<(String, Option<String>)>,
            _model: Option<&str>,
        ) -> Result<String> {
            *self.fresh_calls.borrow_mut() += 1;
            self.prompts.borrow_mut().push(prompt.to_string());
            self.envs.borrow_mut().push(_env);
            Ok(self.response.clone())
        }

        fn send_fresh_streaming(
            &self,
            _file: &Path,
            prompt: &str,
            _agent_name: &str,
            _agent_config: Option<&AgentConfig>,
            _env: Vec<(String, Option<String>)>,
            _model: Option<&str>,
        ) -> Result<Option<Box<dyn Iterator<Item = Result<StreamChunk>>>>> {
            let Some(chunks) = &self.streaming_chunks else {
                return Ok(None);
            };
            *self.streaming_calls.borrow_mut() += 1;
            self.prompts.borrow_mut().push(prompt.to_string());
            self.envs.borrow_mut().push(_env);
            Ok(Some(Box::new(chunks.clone().into_iter().map(Ok))))
        }
    }

    impl FreshAgentRunner for MutatingAgentRunner {
        fn send_fresh(
            &self,
            file: &Path,
            _prompt: &str,
            _agent_name: &str,
            _agent_config: Option<&AgentConfig>,
            _env: Vec<(String, Option<String>)>,
            _model: Option<&str>,
        ) -> Result<String> {
            let mut calls = self.fresh_calls.borrow_mut();
            *calls += 1;
            if *calls == 1 {
                let doc = fs::read_to_string(file)?;
                let updated = doc.replace(
                    "- do #first\n- do #second",
                    "- do #first\n- do #inserted\n- do #second",
                );
                fs::write(file, updated)?;
            }
            Ok(self.response.clone())
        }
    }

    struct CaptureAgent {
        seen_prompt: RefCell<Vec<String>>,
        seen_session_id: RefCell<Vec<Option<String>>>,
        seen_fork: RefCell<Vec<bool>>,
    }

    #[derive(Default)]
    struct FakeParallelRunner {
        calls: RefCell<Vec<ParallelRunCall>>,
    }

    impl ParallelRunner for FakeParallelRunner {
        fn run(&self, file: &Path, config: parallel::ParallelConfig) -> Result<()> {
            self.calls.borrow_mut().push((
                file.display().to_string(),
                config.tasks,
                config.model,
                config.no_git,
                config.no_worktree,
                config.timeout_secs,
                config.dry_run,
            ));
            Ok(())
        }
    }

    #[test]
    fn agent_doc_binary_resolution_works_without_path_when_current_exe_exists() {
        let dir = TempDir::new().unwrap();
        let current = dir.path().join("current-agent-doc");
        std::fs::write(&current, "current").unwrap();

        let resolved =
            resolve_agent_doc_binary_from_env(Some(current.clone()), None, None, dir.path())
                .unwrap();

        assert_eq!(resolved, current);
    }

    #[test]
    fn agent_doc_binary_resolution_falls_back_when_current_exe_is_stale() {
        let dir = TempDir::new().unwrap();
        let path_bin_dir = dir.path().join("bin");
        let path_bin = path_bin_dir.join("agent-doc");
        std::fs::create_dir_all(&path_bin_dir).unwrap();
        std::fs::write(&path_bin, "path").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            Some(dir.path().join("deleted-agent-doc")),
            Some(OsString::from("agent-doc")),
            Some(path_bin_dir.into_os_string()),
            dir.path(),
        )
        .unwrap();

        assert_eq!(resolved, path_bin);
    }

    #[test]
    fn internal_spawn_context_names_binary_cwd_and_path_presence() {
        let context = internal_command_spawn_context("finalize", Path::new("/tmp/agent-doc"));

        assert!(context.contains("agent-doc finalize"));
        assert!(context.contains("binary=/tmp/agent-doc"));
        assert!(context.contains("cwd="));
        assert!(context.contains("PATH_present="));
    }

    impl agent::Agent for CaptureAgent {
        fn send(
            &self,
            prompt: &str,
            session_id: Option<&str>,
            fork: bool,
            _model: Option<&str>,
        ) -> Result<agent::AgentResponse> {
            self.seen_prompt.borrow_mut().push(prompt.to_string());
            self.seen_session_id
                .borrow_mut()
                .push(session_id.map(str::to_string));
            self.seen_fork.borrow_mut().push(fork);
            Ok(agent::AgentResponse {
                text: "ok".to_string(),
                session_id: None,
            })
        }
    }

    fn template_doc() -> String {
        "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nBody\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n".to_string()
    }

    #[test]
    fn extract_tasks_prefers_last_fenced_list() {
        let text = "Notes\n\n- old one\n\n```md\n- do first\n- do second\n```\n";
        assert_eq!(
            extract_tasks_from_text(text),
            vec!["do first".to_string(), "do second".to_string()]
        );
    }

    #[test]
    fn extract_tasks_uses_last_markdown_list() {
        let text = "alpha\n\n- first\n- second\n\nTail\n";
        assert_eq!(
            extract_tasks_from_text(text),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn resolve_task_batch_collects_exchange_prompt_presets() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            "---\nprompt_presets:\n  \"#1\": |\n    Keep the work tree clean.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\npreset #1\n- do #prep\n- do #report\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
        )
        .unwrap();

        assert_eq!(
            batch.tasks,
            vec!["do #prep".to_string(), "do #report".to_string()]
        );
        assert_eq!(batch.requested_presets, vec!["#1".to_string()]);
    }

    #[test]
    fn resolve_task_batch_canonicalizes_bare_hashtag_prompt_preset() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            "---\nprompt_presets:\n  \"#spec-test\": |\n    Run checks.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\npreset spec-test\n- do #prep\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
        )
        .unwrap();

        assert_eq!(batch.requested_presets, vec!["#spec-test".to_string()]);
        assert_eq!(
            load_prompt_preset_block(&doc, &batch.requested_presets)
                .unwrap()
                .as_deref(),
            Some("(preset #spec-test)\nRun checks.\n")
        );
    }

    #[test]
    fn resolve_task_batch_collects_active_queue_for_auto_dag() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            "---\nqueue_active: true\nprompt_presets:\n  \"#spec-test\": |\n    Run checks.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto -->\npreset spec-test\n- do #prep\n- do #report after #prep\n<!-- /agent:queue -->\n",
        )
        .unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Dag,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: false,
                from_queue: true,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
        )
        .unwrap();

        assert_eq!(
            batch.tasks,
            vec!["do #prep".to_string(), "do #report after #prep".to_string()]
        );
        assert_eq!(batch.requested_presets, vec!["#spec-test".to_string()]);
    }

    #[test]
    fn apply_prompt_preset_block_prefixes_task_prompt() {
        let rendered = apply_prompt_preset_block(
            "do #prep",
            Some("(preset #1)\nToday is 2026-04-25.\nKeep the work tree clean.\n"),
        );
        assert_eq!(
            rendered,
            "(preset #1)\nToday is 2026-04-25.\nKeep the work tree clean.\ndo #prep"
        );
    }

    #[test]
    fn exchange_task_source_fingerprint_detects_list_mutations() {
        let original = ExchangeTaskSourceFingerprint {
            tasks: vec!["do #first".to_string(), "do #second".to_string()],
            requested_presets: vec!["#spec".to_string()],
        };
        let source = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #first\n- do #second\n<!-- agent:boundary:keep -->\n",
            &original,
        )
        .unwrap();
        assert_eq!(source, original);

        let boundary_only = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #first\n- do #second\n<!-- agent:boundary:new -->\n",
            &source,
        )
        .unwrap();
        assert_eq!(boundary_only, source);

        let inserted = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #first\n- do #inserted\n- do #second\n",
            &source,
        )
        .unwrap();
        assert_ne!(inserted, source);

        let reordered = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #second\n- do #first\n",
            &source,
        );
        assert!(reordered.is_none());

        let quoted_later = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #first\n- do #second\n\n### Re: response — gpt-5\n\n- do #first\n- do #extra\n- do #second\n",
            &source,
        )
        .unwrap();
        assert_eq!(quoted_later, source);
    }

    #[test]
    fn inject_prompt_inserts_before_boundary() {
        let updated = inject_prompt_into_doc(&template_doc(), "do #gkke").unwrap();
        let prompt_pos = updated.find("❯ do #gkke").unwrap();
        let boundary_pos = updated.find("<!-- agent:boundary:keep -->").unwrap();
        assert!(prompt_pos < boundary_pos);
    }

    #[test]
    fn close_open_preflight_handoff_cycle_snapshots_before_injection() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        Command::new("git")
            .current_dir(dir.path())
            .arg("init")
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let snapshot = template_doc();
        fs::write(&doc, &snapshot).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let handoff = snapshot.replace(
            "<!-- agent:boundary:keep -->",
            "synchronous orchestra\npreset #spec-test\n- do #first\n<!-- agent:boundary:keep -->",
        );
        fs::write(&doc, &handoff).unwrap();
        agent_doc_orchestration::cycle_state::start_preflight(
            &doc,
            Some(&snapshot),
            Some(&handoff),
        )
        .unwrap();

        close_open_preflight_handoff_cycle(&doc).unwrap();
        inject_prompt(&doc, "do #first").unwrap();

        let snap = snapshot::load(&doc).unwrap().unwrap();
        let live = fs::read_to_string(&doc).unwrap();
        assert!(snap.contains("synchronous orchestra"));
        assert!(!snap.contains("❯ do #first"));
        assert!(live.contains("❯ do #first"));
        assert_eq!(
            agent_doc_orchestration::cycle_state::load(&doc)
                .unwrap()
                .unwrap()
                .phase,
            agent_doc_orchestration::cycle_state::CyclePhase::Abandoned
        );
    }

    #[test]
    fn send_fresh_response_uses_no_resume() {
        let agent = CaptureAgent {
            seen_prompt: RefCell::new(Vec::new()),
            seen_session_id: RefCell::new(Vec::new()),
            seen_fork: RefCell::new(Vec::new()),
        };
        let response = send_fresh_response(&agent, "prompt", Some("gpt-5")).unwrap();
        assert_eq!(response.text, "ok");
        assert_eq!(agent.seen_session_id.borrow().as_slice(), &[None]);
        assert_eq!(agent.seen_fork.borrow().as_slice(), &[false]);
        assert_eq!(
            agent.seen_prompt.borrow().as_slice(),
            &["prompt".to_string()]
        );
    }

    #[test]
    fn sequential_orchestration_injects_prompt_and_finalizes() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        fs::write(&doc, template_doc()).unwrap();
        fs::write(&baseline, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response:
                "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /patch:exchange -->\n"
                    .to_string(),
            streaming_chunks: None,
        };

        let tasks = vec![ExecutionTask {
            label: "do #gkke".to_string(),
            prompt: "do #gkke".to_string(),
        }];

        run_ordered_tasks_internal(
            &doc,
            &tasks,
            OrderedTaskRunOptions {
                exchange_source: None,
                agent_override: None,
                model_override: Some("gpt-5"),
            },
            &Config::default(),
            &lifecycle,
            &agent,
            None,
        )
        .unwrap();

        let final_doc = fs::read_to_string(&doc).unwrap();
        assert!(final_doc.contains("❯ do #gkke"));
        assert!(final_doc.contains("### Re: task — gpt-5"));
        assert_eq!(*lifecycle.preflight_calls.borrow(), 1);
        assert_eq!(lifecycle.finalize_calls.borrow().len(), 1);
        assert_eq!(*lifecycle.session_checks.borrow(), 1);
        assert!(
            agent.prompts.borrow()[0].contains("<diff>"),
            "sequential prompt should include the document diff"
        );
        assert!(
            agent.prompts.borrow()[0].contains("❯ do #gkke"),
            "fresh agent prompt should include the injected task"
        );
        assert_eq!(*agent.fresh_calls.borrow(), 1);
        assert_eq!(*agent.streaming_calls.borrow(), 0);
    }

    #[test]
    fn sequential_orchestration_attaches_tsift_graph_context_to_agent_prompt() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        fs::write(&doc, template_doc()).unwrap();
        fs::write(&baseline, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response:
                "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented.\n<!-- /patch:exchange -->\n"
                    .to_string(),
            streaming_chunks: None,
        };
        let tasks = vec![ExecutionTask {
            label: "do #gkke".to_string(),
            prompt: "do #gkke".to_string(),
        }];
        let graph_evidence = test_graph_evidence_plan();

        run_ordered_tasks_internal(
            &doc,
            &tasks,
            OrderedTaskRunOptions {
                exchange_source: None,
                agent_override: None,
                model_override: Some("gpt-5"),
            },
            &Config::default(),
            &lifecycle,
            &agent,
            Some(&graph_evidence),
        )
        .unwrap();

        let prompt = &agent.prompts.borrow()[0];
        assert!(prompt.contains("<tsift_graph_evidence>"));
        assert!(prompt.contains("\"evidence_packet_id\": \"gevd-gkke\""));
        assert!(prompt.contains("Worker 1 owns gkke (#gkke)"));
        assert!(prompt.contains("\"context_pack\""));
        assert!(prompt.contains("\"candidates\""));
        assert!(prompt.contains("Fail closed if the task requires a forbidden/shared file"));
        assert!(prompt.contains("\"token_budget\""));
        assert!(prompt.contains("\"lower_agent_job_packet\""));
        assert!(prompt.contains("\"owned_files\": ["));
        assert!(prompt.contains("\"read_only_context\": ["));
        assert!(prompt.contains("\"forbidden_files\": []"));
        assert!(prompt.contains("\"expected_tests\": ["));
        assert!(prompt.contains("\"expansion_commands\": ["));
        assert!(prompt.contains("\"fail_closed_prompt\""));
        let finalize_calls = lifecycle.finalize_calls.borrow();
        assert_eq!(finalize_calls.len(), 1);
        assert!(
            finalize_calls[0].contains("worker_result: completed #gkke"),
            "child closeout should include a tsift-projectable worker_result line:\n{}",
            finalize_calls[0]
        );
        assert!(finalize_calls[0].contains("src/orchestrate.rs"));
        assert!(finalize_calls[0].contains("`cargo test orchestrate`"));
        let final_doc = fs::read_to_string(&doc).unwrap();
        assert!(!final_doc.contains("<tsift_graph_evidence>"));
    }

    #[test]
    fn parallel_graph_context_maps_worker_packet_to_lower_agent_job_packet() {
        let graph_evidence = test_graph_evidence_plan();
        let prompt =
            apply_parallel_graph_context("do #gkke", "do #gkke".to_string(), Some(&graph_evidence));

        assert!(prompt.contains("\"lower_agent_job_packet\""));
        assert!(prompt.contains("\"contract_version\": \"agent-doc-lower-agent-job-v1\""));
        assert!(prompt.contains("\"source_contract_version\": \"worker-prompt-packet-v1\""));
        assert!(prompt.contains("\"packet_id\": \"wpp-gkke\""));
        assert!(prompt.contains("\"owned_files\": ["));
        assert!(prompt.contains("\"read_only_context\": ["));
        assert!(prompt.contains("\"forbidden_files\": []"));
        assert!(prompt.contains("\"expected_tests\": ["));
        assert!(prompt.contains("\"expansion_commands\": ["));
        assert!(prompt.contains("\"fail_closed_prompt\""));
        assert!(prompt.contains("Fail closed if the task requires a forbidden/shared file"));
    }

    #[test]
    fn sequential_orchestration_always_reruns_preflight_after_injection() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        fs::write(&doc, template_doc()).unwrap();
        fs::write(&baseline, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response:
                "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /patch:exchange -->\n"
                    .to_string(),
            streaming_chunks: None,
        };

        let tasks = vec![ExecutionTask {
            label: "do #opcc".to_string(),
            prompt: "do #opcc".to_string(),
        }];

        run_ordered_tasks_internal(
            &doc,
            &tasks,
            OrderedTaskRunOptions {
                exchange_source: None,
                agent_override: None,
                model_override: Some("gpt-5"),
            },
            &Config::default(),
            &lifecycle,
            &agent,
            None,
        )
        .unwrap();

        let final_doc = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            *lifecycle.preflight_calls.borrow(),
            1,
            "sequential mode should always rerun preflight after prompt injection"
        );
        assert!(final_doc.contains("❯ do #opcc"));
        assert!(agent.prompts.borrow()[0].contains("❯ do #opcc"));
        assert_eq!(lifecycle.finalize_calls.borrow().len(), 1);
        assert_eq!(*lifecycle.session_checks.borrow(), 1);
        assert_eq!(*agent.fresh_calls.borrow(), 1);
        assert_eq!(*agent.streaming_calls.borrow(), 0);
    }

    #[test]
    fn sequential_orchestration_expands_prompt_presets_into_task_prompt() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        let content = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nprompt_presets:\n  \"#1\": |\n    Today is 2026-04-25.\n    Keep the work tree clean.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\npreset #1\n- do #prep\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n";
        fs::write(&doc, content).unwrap();
        fs::write(&baseline, content).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response:
                "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented.\n<!-- /patch:exchange -->\n"
                    .to_string(),
            streaming_chunks: None,
        };

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: Some("gpt-5".to_string()),
                no_git: false,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: false,
                plan: false,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &FakeParallelRunner::default(),
            false,
        )
        .unwrap();

        let prompt = agent.prompts.borrow()[0].clone();
        assert!(prompt.contains("(preset #1)\nToday is 2026-04-25.\nKeep the work tree clean."));
        assert!(prompt.contains("❯ (preset #1)\nToday is 2026-04-25."));
    }

    #[test]
    fn sequential_orchestration_stops_when_exchange_task_list_changes() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        let content = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsync orchestra\npreset #spec\n- do #first\n- do #second\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n";
        fs::write(&doc, content).unwrap();
        fs::write(&baseline, content).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = MutatingAgentRunner {
            fresh_calls: RefCell::new(0),
            response: "<!-- patch:exchange -->\n### Re: first — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n"
                .to_string(),
        };
        let tasks = vec![
            ExecutionTask {
                label: "do #first".to_string(),
                prompt: "do #first".to_string(),
            },
            ExecutionTask {
                label: "do #second".to_string(),
                prompt: "do #second".to_string(),
            },
        ];
        let source = ExchangeTaskSourceFingerprint {
            tasks: vec!["do #first".to_string(), "do #second".to_string()],
            requested_presets: vec!["#spec".to_string()],
        };

        let err = run_ordered_tasks_internal(
            &doc,
            &tasks,
            OrderedTaskRunOptions {
                exchange_source: Some(&source),
                agent_override: None,
                model_override: Some("gpt-5"),
            },
            &Config::default(),
            &lifecycle,
            &agent,
            None,
        )
        .unwrap_err()
        .to_string();

        let final_doc = fs::read_to_string(&doc).unwrap();
        assert!(err.contains("orchestration batch changed during run"));
        assert!(final_doc.contains("- do #inserted"));
        assert!(final_doc.contains("### Re: first — gpt-5"));
        assert!(final_doc.contains("### Re: orchestration batch changed — gpt-5"));
        assert!(!final_doc.contains("❯ do #second"));
        assert_eq!(*agent.fresh_calls.borrow(), 1);
        assert_eq!(lifecycle.finalize_calls.borrow().len(), 2);
        assert_eq!(*lifecycle.session_checks.borrow(), 2);
    }

    #[test]
    fn render_streamed_exchange_inserts_response_before_boundary() {
        let seed = ExchangeStreamSeed {
            prefix: "❯ do #4qja\n".to_string(),
            suffix: "<!-- agent:boundary:keep -->\n".to_string(),
        };

        let rendered = render_streamed_exchange(
            &seed,
            "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n<!-- /patch:exchange -->\n",
        );
        let response_pos = rendered.find("### Re: streamed — gpt-5").unwrap();
        let boundary_pos = rendered.find("<!-- agent:boundary:keep -->").unwrap();
        assert!(response_pos < boundary_pos);
    }

    #[test]
    fn finalize_suffix_uses_only_unseen_stream_tail() {
        let streamed = "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n";
        let full = "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->\n";
        let delta = finalize_suffix_from_streamed_prefix(streamed, full).unwrap();
        assert!(!delta.contains("### Re: streamed — gpt-5"));
        assert!(delta.contains("Implemented and verified."));
    }

    #[test]
    fn orchestrate_template_finalize_wraps_plain_response_as_exchange_patch() {
        let finalize = orchestrate_finalize_text_for_template(
            "### Re: plain orch — gpt-5\n\nImplemented and verified.".to_string(),
        );

        assert!(finalize.starts_with("<!-- patch:exchange -->"));
        assert!(finalize.contains("### Re: plain orch"));
        assert!(finalize.ends_with("<!-- /patch:exchange -->\n"));
    }

    #[test]
    fn orchestrate_template_finalize_does_not_wrap_transcript_response() {
        let transcript = "❯ do #next\n### Re: malformed — gpt-5\nBody";
        let finalize = orchestrate_finalize_text_for_template(transcript.to_string());

        assert_eq!(finalize, transcript);
    }

    #[test]
    fn streamed_flush_waits_for_exchange_patch_marker() {
        assert!(!should_stream_exchange_patch(
            "### Re: malformed streaming closeout — gpt-5\nBody"
        ));
        assert!(should_stream_exchange_patch(
            "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n"
        ));
    }

    #[test]
    fn sequential_orchestration_uses_streaming_backend_for_crdt_docs() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        fs::write(&doc, template_doc()).unwrap();
        fs::write(&baseline, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: String::new(),
            streaming_chunks: Some(vec![
                StreamChunk {
                    text: "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n".to_string(),
                    thinking: None,
                    is_final: false,
                    session_id: None,
                },
                StreamChunk {
                    text: "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /patch:exchange -->\n".to_string(),
                    thinking: None,
                    is_final: true,
                    session_id: Some("sess-stream".to_string()),
                },
            ]),
        };

        let tasks = vec![ExecutionTask {
            label: "do #4qja".to_string(),
            prompt: "do #4qja".to_string(),
        }];

        run_ordered_tasks_internal(
            &doc,
            &tasks,
            OrderedTaskRunOptions {
                exchange_source: None,
                agent_override: None,
                model_override: Some("gpt-5"),
            },
            &Config::default(),
            &lifecycle,
            &agent,
            None,
        )
        .unwrap();

        let final_doc = fs::read_to_string(&doc).unwrap();
        assert!(final_doc.contains("❯ do #4qja"));
        assert!(final_doc.contains("### Re: streamed — gpt-5"));
        assert_eq!(final_doc.matches("### Re: streamed — gpt-5").count(), 1);
        assert_eq!(*agent.streaming_calls.borrow(), 1);
        assert_eq!(*agent.fresh_calls.borrow(), 0);
    }

    #[test]
    fn resolve_dag_tasks_supports_fan_in_dependencies() {
        let tasks = [
            "do #prep. Prepare context",
            "[after=#prep] do #bench. Run benchmarks",
            "[id=report after=#prep,#bench] Summarize both results",
        ];

        let parsed = tasks
            .iter()
            .enumerate()
            .map(|(idx, task)| parse_dag_task_line(task, idx).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(parsed[0].id, "#prep");
        assert!(parsed[0].deps.is_empty());
        assert_eq!(parsed[1].id, "#bench");
        assert_eq!(parsed[1].deps, vec!["#prep".to_string()]);
        assert_eq!(parsed[2].id, "report");
        assert_eq!(
            parsed[2].deps,
            vec!["#prep".to_string(), "#bench".to_string()]
        );
        assert_eq!(parsed[2].prompt, "Summarize both results");
    }

    #[test]
    fn dag_schedule_rejects_unknown_dependency() {
        let tasks = vec![DagTask {
            id: "#prep".to_string(),
            prompt: "do #prep".to_string(),
            deps: vec!["#missing".to_string()],
        }];

        let err = plan_dag_execution(&tasks).unwrap_err().to_string();
        assert!(
            err.contains("unknown task `#missing`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn dag_schedule_rejects_cycles() {
        let tasks = vec![
            DagTask {
                id: "#a".to_string(),
                prompt: "do #a".to_string(),
                deps: vec!["#b".to_string()],
            },
            DagTask {
                id: "#b".to_string(),
                prompt: "do #b".to_string(),
                deps: vec!["#a".to_string()],
            },
        ];

        let err = plan_dag_execution(&tasks).unwrap_err().to_string();
        assert!(
            err.contains("dag dependency cycle detected"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn dag_orchestration_runs_topological_order() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        fs::write(&doc, template_doc()).unwrap();
        fs::write(&baseline, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response:
                "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n"
                    .to_string(),
            streaming_chunks: None,
        };
        let dag_tasks = vec![
            DagTask {
                id: "#prep".to_string(),
                prompt: "do #prep".to_string(),
                deps: Vec::new(),
            },
            DagTask {
                id: "#report".to_string(),
                prompt: "do #report".to_string(),
                deps: vec!["#prep".to_string(), "#bench".to_string()],
            },
            DagTask {
                id: "#bench".to_string(),
                prompt: "do #bench".to_string(),
                deps: vec!["#prep".to_string()],
            },
        ];

        let execution = plan_dag_execution(&dag_tasks).unwrap();
        assert_eq!(
            execution
                .iter()
                .map(|task| task.prompt.as_str())
                .collect::<Vec<_>>(),
            vec!["do #prep", "do #bench", "do #report"]
        );

        run_ordered_tasks_internal(
            &doc,
            &execution,
            OrderedTaskRunOptions {
                exchange_source: None,
                agent_override: None,
                model_override: Some("gpt-5"),
            },
            &Config::default(),
            &lifecycle,
            &agent,
            None,
        )
        .unwrap();

        assert_eq!(lifecycle.finalize_calls.borrow().len(), 3);
        assert_eq!(*lifecycle.session_checks.borrow(), 3);
        let prompts = agent.prompts.borrow();
        assert!(prompts[0].contains("❯ do #prep"));
        assert!(prompts[1].contains("❯ do #bench"));
        assert!(prompts[2].contains("❯ do #report"));
    }

    #[test]
    fn parallel_mode_uses_shared_parallel_runner() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };
        let parallel_runner = FakeParallelRunner::default();

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Parallel,
                tasks_explicit: vec!["  ❯ do #9pw9  ".to_string()],
                from_file: None,
                from_exchange: false,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: Some("gpt-5".to_string()),
                no_git: true,
                no_worktree: true,
                timeout_secs: 45,
                dry_run: true,
                plan: false,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &parallel_runner,
            false,
        )
        .unwrap();

        let calls = parallel_runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].1,
            vec![parallel::ParallelTask {
                description: "do #9pw9".to_string(),
                prompt: "do #9pw9".to_string(),
            }]
        );
        assert_eq!(calls[0].2.as_deref(), Some("gpt-5"));
        assert!(calls[0].3);
        assert!(calls[0].4);
        assert_eq!(calls[0].5, 45);
        assert!(calls[0].6);
        assert!(lifecycle.finalize_calls.borrow().is_empty());
        assert!(agent.prompts.borrow().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn parallel_mode_continues_without_graph_evidence_when_tsift_is_stale() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let script = dir.path().join("fake-tsift-stale.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
if echo "$*" | grep -q 'graph-db.*--json status'; then
  cat <<'JSON'
{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{"status":"stale","fail_closed":true,"diagnostics":["graph.db is stale"]}}
JSON
  exit 0
fi
echo "unexpected fake tsift args: $*" >&2
exit 2
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", script.to_str().unwrap());

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };
        let parallel_runner = FakeParallelRunner::default();

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Parallel,
                tasks_explicit: vec!["do #gkke".to_string()],
                from_file: None,
                from_exchange: false,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: false,
                plan: false,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &parallel_runner,
            false,
        )
        .unwrap();

        let calls = parallel_runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1[0].description, "do #gkke");
        assert_eq!(calls[0].1[0].prompt, "do #gkke");
        assert!(!calls[0].1[0].prompt.contains("<tsift_graph_evidence>"));
    }

    #[test]
    fn parallel_mode_expands_prompt_presets_into_task_prompt_only() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            "---\nprompt_presets:\n  \"#1\": |\n    Keep the work tree clean.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\npreset #1\n- do #prep\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };
        let parallel_runner = FakeParallelRunner::default();

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Parallel,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &parallel_runner,
            false,
        )
        .unwrap();

        let call = &parallel_runner.calls.borrow()[0];
        assert_eq!(call.1[0].description, "do #prep");
        assert_eq!(
            call.1[0].prompt,
            "(preset #1)\nKeep the work tree clean.\ndo #prep"
        );
    }

    #[test]
    fn legacy_parallel_compat_allows_empty_task_list() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };
        let parallel_runner = FakeParallelRunner::default();

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Parallel,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: false,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: false,
                no_worktree: false,
                timeout_secs: 600,
                dry_run: false,
                plan: false,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &parallel_runner,
            true,
        )
        .unwrap();

        let calls = parallel_runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].1.is_empty());
    }

    #[test]
    fn plan_flag_sequential_prints_expanded_prompts_without_executing() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: vec!["do #prep".to_string(), "do #report".to_string()],
                from_file: None,
                from_exchange: false,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: false,
                no_worktree: false,
                timeout_secs: 30,
                dry_run: false,
                plan: true,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &FakeParallelRunner::default(),
            false,
        )
        .unwrap();

        assert!(lifecycle.finalize_calls.borrow().is_empty());
        assert!(agent.prompts.borrow().is_empty());
    }

    #[test]
    fn plan_flag_sequential_expands_preset_in_output() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let preset_doc = "---\nagent_doc_format: template\nagent_doc_write: crdt\nagent: claude\nprompt_presets:\n  \"#1\": \"Today is 2026-04-25.\\nKeep the work tree clean.\"\n---\n<!-- agent:exchange -->\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n<!-- agent:pending -->\n<!-- /agent:pending -->\n";
        fs::write(&doc, preset_doc).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: vec!["do #prep".to_string()],
                from_file: None,
                from_exchange: false,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: false,
                no_worktree: false,
                timeout_secs: 30,
                dry_run: false,
                plan: true,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &FakeParallelRunner::default(),
            false,
        )
        .unwrap();

        assert!(lifecycle.finalize_calls.borrow().is_empty());
        assert!(agent.prompts.borrow().is_empty());
    }

    #[test]
    fn plan_flag_parallel_exits_without_calling_runner() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };
        let parallel_runner = FakeParallelRunner::default();

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Parallel,
                tasks_explicit: vec!["do #a".to_string(), "do #b".to_string()],
                from_file: None,
                from_exchange: false,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: false,
                no_worktree: false,
                timeout_secs: 30,
                dry_run: false,
                plan: true,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &parallel_runner,
            false,
        )
        .unwrap();

        assert!(parallel_runner.calls.borrow().is_empty());
    }

    #[test]
    fn resolve_orchestrate_agent_args_claude_frontmatter() {
        let fm = frontmatter::Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let config = Config::default();
        let result = resolve_orchestrate_agent_args(&fm, "claude", &config);
        assert_eq!(result.as_deref(), Some("--dangerously-skip-permissions"));
    }

    #[test]
    fn resolve_orchestrate_agent_args_codex_frontmatter() {
        let fm = frontmatter::Frontmatter {
            codex_args: Some("-s danger-full-access".into()),
            ..Default::default()
        };
        let config = Config::default();
        let result = resolve_orchestrate_agent_args(&fm, "codex", &config);
        assert_eq!(result.as_deref(), Some("-s danger-full-access"));
    }

    #[test]
    fn resolve_orchestrate_agent_args_opencode_frontmatter() {
        let fm = frontmatter::Frontmatter {
            opencode_args: Some("--dangerously-skip-permissions".into()),
            codex_args: Some("-s danger-full-access".into()),
            ..Default::default()
        };
        let config = Config::default();
        let result = resolve_orchestrate_agent_args(&fm, "opencode", &config);
        assert_eq!(result.as_deref(), Some("--dangerously-skip-permissions"));
    }

    #[test]
    fn resolve_orchestrate_agent_args_agent_args_beats_harness_specific() {
        let fm = frontmatter::Frontmatter {
            agent_args: Some("--model sonnet".into()),
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let config = Config::default();
        let result = resolve_orchestrate_agent_args(&fm, "claude", &config);
        assert_eq!(result.as_deref(), Some("--model sonnet"));
    }

    #[test]
    fn resolve_orchestrate_agent_args_falls_through_to_config() {
        let fm = frontmatter::Frontmatter::default();
        let config = Config {
            claude_args: Some("--from-config".into()),
            ..Default::default()
        };
        let result = resolve_orchestrate_agent_args(&fm, "claude", &config);
        assert_eq!(result.as_deref(), Some("--from-config"));
    }

    #[test]
    fn resolve_orchestrate_agent_args_none_when_no_args() {
        let fm = frontmatter::Frontmatter::default();
        let config = Config::default();
        let result = resolve_orchestrate_agent_args(&fm, "claude", &config);
        assert!(result.is_none());
    }

    #[test]
    fn build_effective_config_claude_with_frontmatter_args() {
        let config = Config::default();
        let effective =
            build_effective_agent_config("claude", Some("--dangerously-skip-permissions"), &config);
        let effective = effective.unwrap();
        assert_eq!(effective.command, "claude");
        assert_eq!(
            effective.args,
            vec![
                "-p",
                "--output-format",
                "json",
                "--dangerously-skip-permissions"
            ]
        );
    }

    #[test]
    fn build_effective_config_codex_with_frontmatter_args() {
        let config = Config::default();
        let effective =
            build_effective_agent_config("codex", Some("-s danger-full-access"), &config);
        let effective = effective.unwrap();
        assert_eq!(effective.command, "codex");
        assert_eq!(
            effective.args,
            vec!["exec", "--json", "-s", "danger-full-access"]
        );
    }

    #[test]
    fn build_effective_config_none_without_frontmatter_args() {
        let config = Config::default();
        let effective = build_effective_agent_config("claude", None, &config);
        assert!(effective.is_none());
    }

    #[test]
    fn sequential_orchestration_adds_codex_network_override_to_child_env() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        let content = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\ncodex_args: \"-s danger-full-access\"\ncodex_network_access: enabled\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n";
        fs::write(&doc, content).unwrap();
        fs::write(&baseline, content).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            envs: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response:
                "<!-- patch:exchange -->\n### Re: network — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n"
                    .to_string(),
            streaming_chunks: None,
        };

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: vec!["do #net".to_string()],
                from_file: None,
                from_exchange: false,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: false,
                no_worktree: false,
                timeout_secs: 30,
                dry_run: false,
                plan: false,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &FakeParallelRunner::default(),
            false,
        )
        .unwrap();

        let envs = agent.envs.borrow();
        assert_eq!(envs.len(), 1);
        assert!(envs[0].iter().any(|(key, value)| {
            key == agent_doc_orchestration::agent::CODEX_SANDBOX_NETWORK_DISABLED_ENV
                && value.is_none()
        }));
    }

    // --- parse_list_item prompt-prefix stripping (bug #orch3) ---

    #[test]
    fn parse_list_item_strips_prompt_prefix() {
        let result = parse_list_item("❯ - do #task1");
        assert_eq!(result, Some("do #task1".to_string()));
    }

    #[test]
    fn parse_list_item_strips_prompt_prefix_with_star() {
        let result = parse_list_item("❯ * do #task2");
        assert_eq!(result, Some("do #task2".to_string()));
    }

    #[test]
    fn parse_list_item_without_prefix_still_works() {
        let result = parse_list_item("- do #task3");
        assert_eq!(result, Some("do #task3".to_string()));
    }

    #[test]
    fn parse_list_item_strips_prompt_prefix_numbered() {
        let result = parse_list_item("❯ 1. do #task4");
        assert_eq!(result, Some("do #task4".to_string()));
    }

    #[test]
    fn collect_markdown_list_blocks_with_prompt_prefix() {
        let text = "❯ - do #a\n❯ - do #b\n\nsome other text\n";
        let blocks = collect_markdown_list_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], vec!["do #a".to_string(), "do #b".to_string()]);
    }

    fn setup_snapshot_dir(dir: &Path) {
        fs::create_dir_all(dir.join(".agent-doc")).unwrap();
    }

    #[test]
    fn from_exchange_scopes_to_tail_bare_directive() {
        let dir = TempDir::new().unwrap();
        setup_snapshot_dir(dir.path());
        let doc = dir.path().join("session.md");

        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "Compacted summary.\n\n",
            "❯ Previous prompt\n\n",
            "### Re: response — opus-4-6\n\n",
            "Recommendations:\n\n",
            "- **#stale1** — fix stale-task parsing\n",
            "- **#envt1** — fix env test\n\n",
            "All marked [recommended].\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let current_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "Compacted summary.\n\n",
            "❯ Previous prompt\n\n",
            "### Re: response — opus-4-6 (HEAD)\n\n",
            "Recommendations:\n\n",
            "- **#stale1** — fix stale-task parsing\n",
            "- **#envt1** — fix env test\n\n",
            "All marked [recommended].\n\n",
            "do #stale1\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        fs::write(&doc, current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
        )
        .unwrap();

        assert_eq!(
            batch.tasks,
            vec!["do #stale1".to_string()],
            "should extract user's bare directive, not response list items"
        );
    }

    #[test]
    fn from_exchange_scopes_to_tail_list_directive() {
        let dir = TempDir::new().unwrap();
        setup_snapshot_dir(dir.path());
        let doc = dir.path().join("session.md");

        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "❯ Old prompt\n\n",
            "### Re: old response — opus-4-6\n\n",
            "Old list:\n\n",
            "- old item 1\n",
            "- old item 2\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let current_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "❯ Old prompt\n\n",
            "### Re: old response — opus-4-6 (HEAD)\n\n",
            "Old list:\n\n",
            "- old item 1\n",
            "- old item 2\n\n",
            "- do #new1\n",
            "- do #new2\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        fs::write(&doc, current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
        )
        .unwrap();

        assert_eq!(
            batch.tasks,
            vec!["do #new1".to_string(), "do #new2".to_string()],
            "should extract user's new list items, not old response list items"
        );
    }

    #[test]
    fn from_exchange_falls_back_without_snapshot() {
        let dir = TempDir::new().unwrap();
        setup_snapshot_dir(dir.path());
        let doc = dir.path().join("session.md");

        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "- do #task1\n",
            "- do #task2\n",
            "<!-- /agent:exchange -->\n",
        );

        fs::write(&doc, content).unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
        )
        .unwrap();

        assert_eq!(
            batch.tasks,
            vec!["do #task1".to_string(), "do #task2".to_string()],
            "without snapshot should fall back to full exchange extraction"
        );
    }

    #[test]
    fn from_exchange_multiple_responses_picks_latest_directive() {
        let dir = TempDir::new().unwrap();
        setup_snapshot_dir(dir.path());
        let doc = dir.path().join("session.md");

        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "Compacted: old orchestration ran tasks #a, #b, #c.\n\n",
            "❯ What should we do next?\n\n",
            "### Re: next steps — opus-4-6\n\n",
            "I recommend:\n\n",
            "1. Fix stale-task parsing\n",
            "2. Fix env test\n",
            "3. Manual test orchestrate\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let current_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "Compacted: old orchestration ran tasks #a, #b, #c.\n\n",
            "❯ What should we do next?\n\n",
            "### Re: next steps — opus-4-6 (HEAD)\n\n",
            "I recommend:\n\n",
            "1. Fix stale-task parsing\n",
            "2. Fix env test\n",
            "3. Manual test orchestrate\n\n",
            "do #stale1\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        fs::write(&doc, current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
        )
        .unwrap();

        assert_eq!(
            batch.tasks,
            vec!["do #stale1".to_string()],
            "should extract user's directive, not the numbered list from the response"
        );
    }

    #[test]
    fn build_agent_prompt_carries_forward_active_format_requirements() {
        let doc = concat!(
            "❯ Please organize the backlog into a 2-level list. ",
            "Place the urgent-security matters at the top. ",
            "Use a numeric list where appropriate.\n",
            "### Re: backlog organization — gpt-5\n",
            "Done.\n",
        );

        let prompt = build_agent_prompt(
            Path::new("session.md"),
            ResolvedMode {
                format: crate::frontmatter::AgentDocFormat::Template,
                write: crate::frontmatter::AgentDocWrite::Crdt,
            },
            Some("diff"),
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
    fn build_agent_prompt_uses_bounded_context_pack_for_warn_level_prompt_targets() {
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
        let report = agent_doc_orchestration::session_accretion::SessionAccretionReport {
            level: agent_doc_orchestration::session_accretion::SessionAccretionLevel::Warn,
            reasons: vec!["document hit 2 no-op closeouts in the last 30 minutes".to_string()],
            ..Default::default()
        };

        let prompt = build_agent_prompt(
            Path::new("session.md"),
            ResolvedMode {
                format: crate::frontmatter::AgentDocFormat::Template,
                write: crate::frontmatter::AgentDocWrite::Crdt,
            },
            Some(diff),
            doc,
            Some(&report),
        );
        assert!(prompt.contains("<response_context level=\"warn\">"));
        assert!(!prompt.contains("<document>\n## Exchange"));
    }
