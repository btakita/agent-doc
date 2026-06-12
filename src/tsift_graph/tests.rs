    use super::*;
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

    fn setup_doc() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let doc = dir.path().join("tasks.md");
        std::fs::write(&doc, "# Tasks\n").unwrap();
        (dir, doc)
    }

    #[cfg(unix)]
    fn fake_tsift(dir: &Path, log: &Path, stale: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = dir.join("fake-tsift.sh");
        let status = if stale {
            r#"{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{"status":"stale","fail_closed":true,"content_hash":"old","source_watermark":"old","diagnostics":["graph.db is stale"]},"next_commands":["tsift graph-db --path /tmp/repo refresh --json"]}"#
        } else {
            r#"{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]},"next_commands":["tsift graph-db --path /tmp/repo status --json"]}"#
        };
        let script_body = format!(
            r##"#!/bin/sh
printf '%s\n' "$*" >> "{}"
case "$*" in
  *"graph-db"*"--json status"*)
    cat <<'JSON'
{}
JSON
    ;;
  *"graph-db"*"--json refresh"*)
    cat <<'JSON'
{{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","operation":"refresh","status":"current","freshness":{{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]}},"next_commands":["tsift graph-db --path /tmp/repo doctor --json"]}}
JSON
    ;;
  *"graph-db"*"--json evidence agbr"*)
    if grep -q -- '--json refresh' "{}"; then
      cat <<'JSON'
{{"contract_version":"graph-db-evidence-v1","root":"/tmp/repo","backend":"sqlite","target":"agbr","packet_id":"gevd-agbr","projection_hash":"abc","freshness":{{"status":"current","fail_closed":false,"diagnostics":[]}},"target_node":{{"id":"gbak-agbr","kind":"backlog","label":"#agbr"}},"worker_context":[{{"id":"wctx-agbr"}}],"source_handles":[{{"id":"src-agbr"}}],"semantic_related":[{{"id":"sem-agbr"}}],"next_commands":["tsift graph-db --path /tmp/repo evidence agbr --json"],"replay_commands":["tsift graph-db --path /tmp/repo evidence agbr --json"],"repair_commands":["tsift graph-db --path /tmp/repo refresh --json"]}}
JSON
    else
      cat <<'JSON'
{{"contract_version":"graph-db-evidence-v1","root":"/tmp/repo","backend":"sqlite","target":"agbr","packet_id":"gevd-old","projection_hash":"old","freshness":{{"status":"current","fail_closed":false,"diagnostics":[]}},"target_node":{{"id":"gbak-agbr","kind":"backlog","label":"#agbr"}},"worker_context":[{{"id":"wctx-agbr"}}],"source_handles":[{{"id":"src-agbr"}}],"semantic_related":[{{"id":"sem-agbr"}}],"next_commands":["tsift graph-db --path /tmp/repo evidence agbr --json"],"replay_commands":["tsift graph-db --path /tmp/repo evidence agbr --json"],"repair_commands":["tsift graph-db --path /tmp/repo refresh --json"]}}
JSON
    fi
    ;;
  *"conflict-matrix"*)
    cat <<'JSON'
{{"contract_version":"conflict-matrix-v1","targets":["agbr"],"can_parallel":true,"fail_closed":false,"inputs":{{"graph_db_evidence_targets":["agbr"],"evidence_packets":[{{"target":"agbr","packet_id":"gevd-agbr","target_node_id":"gbak-agbr","projection_hash":"abc","replay_command":"tsift graph-db evidence agbr --json"}}],"context_pack_command":"tsift --envelope context-pack tasks.md --budget normal","cached_diff_command":"tsift diff-digest --cached /tmp/repo --json","impact_command":"tsift impact /tmp/repo --cached --limit 20 --json"}},"context_pack":{{"target":"tasks.md","target_kind":"agent_doc_session","prompt_targets":["do #agbr"],"touched_files":["tasks.md"],"touched_symbols":["Exchange"],"files_changed":1,"worker_context":["summary"],"source_windows":["tasks.md:1-20"],"status_reminders":[]}},"candidates":[{{"target":"agbr","rank":1,"risk":"low","risk_score":0,"risk_reasons":[],"evidence_packet_id":"gevd-agbr","target_node_id":"gbak-agbr","target_kind":"backlog","target_label":"#agbr","owned_files":["tasks.md"],"owned_symbols":["Exchange"],"config_files":[],"affected_tests":["cargo test"],"staged_overlap":{{"files":[],"symbols":[],"tests":[],"config_files":[]}},"semantic_dispatch_score":3,"semantic_dispatch_reasons":["semantic match"],"worker_feedback":{{"total":1,"completed":1,"blocked":0,"touched_files":["tasks.md"],"expected_tests":["cargo test"],"follow_up_ids":["next1"],"outcome_history":["completed #agbr"],"repeated_blockage":false,"stale_expected_tests":[],"follow_up_debt":[],"closure_rank_score":0,"closure_rank_reasons":[]}}}}],"conflicts":[],"orchestration":{{"evidence_packet_ids":["gevd-agbr"],"conflict_matrix_decisions":["candidate #1 agbr risk=low"],"worker_ownership_blocks":["Worker 1 owns agbr (#agbr)"],"projection_hashes":["abc"],"projection_freshness":{{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]}}}},"worker_prompt_packets":[{{"contract_version":"worker-prompt-packet-v1","packet_id":"wpp-agbr","target":"agbr","rank":1,"risk":"low","projection_hash":"abc","title":"Worker 1 owns agbr (#agbr)","owned_files":["tasks.md"],"owned_symbols":["Exchange"],"read_only_context":["src-agbr","semantic_rank: semantic match"],"forbidden_files":[],"expected_tests":["cargo test"],"expansion_commands":["tsift graph-db evidence agbr --json"],"token_budget":{{"prompt_estimated_tokens":20,"max_prompt_tokens":200,"source_window_count":1,"source_window_lines":20,"max_context_bytes":2400}},"semantic_dispatch_score":3,"semantic_dispatch_reasons":["semantic match"],"worker_feedback":{{"total":1,"completed":1,"blocked":0,"touched_files":["tasks.md"],"expected_tests":["cargo test"],"follow_up_ids":["next1"],"outcome_history":["completed #agbr"],"repeated_blockage":false,"stale_expected_tests":[],"follow_up_debt":[],"closure_rank_score":0,"closure_rank_reasons":[]}},"prompt":"Worker 1 owns agbr (#agbr)\n\nFail closed if the task requires a forbidden/shared file."}}],"next_commands":["tsift conflict-matrix --path /tmp/repo agbr --json"],"warnings":[]}}
JSON
    ;;
  *"dispatch-trace"*)
    cat <<'JSON'
{{"contract_version":"dispatch-trace-v1","root":"/tmp/repo","targets":["agbr"],"projection_freshness":{{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]}},"projection_hashes":["abc"],"evidence_packet_ids":["gevd-agbr"],"worker_prompt_packets":[{{"contract_version":"worker-prompt-packet-v1","packet_id":"wpp-agbr","target":"agbr","rank":1,"risk":"low","projection_hash":"abc","title":"Worker 1 owns agbr (#agbr)","owned_files":["tasks.md"],"owned_symbols":["Exchange"],"read_only_context":["src-agbr","semantic_rank: semantic match"],"forbidden_files":[],"expected_tests":["cargo test"],"expansion_commands":["tsift graph-db evidence agbr --json"],"token_budget":{{"prompt_estimated_tokens":20,"max_prompt_tokens":200,"source_window_count":1,"source_window_lines":20,"max_context_bytes":2400}},"semantic_dispatch_score":3,"semantic_dispatch_reasons":["semantic match"],"worker_feedback":{{"total":1,"completed":1,"blocked":0,"touched_files":["tasks.md"],"expected_tests":["cargo test"],"follow_up_ids":["next1"],"outcome_history":["completed #agbr"],"repeated_blockage":false,"stale_expected_tests":[],"follow_up_debt":[],"closure_rank_score":0,"closure_rank_reasons":[]}},"prompt":"Worker 1 owns agbr (#agbr)\n\nFail closed if the task requires a forbidden/shared file."}}],"worker_feedback":[{{"total":1,"completed":1,"blocked":0,"touched_files":["tasks.md"],"expected_tests":["cargo test"],"follow_up_ids":["next1"],"outcome_history":["completed #agbr"],"repeated_blockage":false,"stale_expected_tests":[],"follow_up_debt":[],"closure_rank_score":0,"closure_rank_reasons":[]}}],"summary":{{"backlog":1,"job_packet":1,"worker_result":1,"worker_context":1,"source_handle":1,"semantic_rows":0}},"nodes":[{{"id":"gbak-agbr","kind":"backlog","label":"#agbr","properties":{{"ref_id":"agbr"}}}},{{"id":"job-agbr","kind":"job_packet","label":"do #agbr","properties":{{"ref_id":"agbr"}}}},{{"id":"wres-agbr","kind":"worker_result","label":"completed #agbr","properties":{{"status":"completed","touched_files":"tasks.md","expected_tests":"cargo test","follow_up_ids":"next1"}}}}],"edges":[{{"from_id":"job-agbr","to_id":"gbak-agbr","kind":"targets","properties":{{}}}},{{"from_id":"gbak-agbr","to_id":"wres-agbr","kind":"has_result","properties":{{}}}}],"conflict_matrix_decisions":["candidate #1 agbr risk=low"],"replay_commands":["tsift conflict-matrix --path /tmp/repo agbr --json"],"repair_commands":["tsift graph-db --path /tmp/repo refresh --json"],"truncated":false,"warnings":[]}}
JSON
    ;;
  *)
    echo "unexpected fake tsift args: $*" >&2
    exit 2
    ;;
esac
"##,
            log.display(),
            status,
            log.display()
        );
        std::fs::write(&script, script_body).unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        script
    }

    #[test]
    fn extracts_do_targets_from_common_task_shapes() {
        assert_eq!(
            extract_do_target("do #agbr. spec"),
            Some("agbr".to_string())
        );
        assert_eq!(
            extract_do_target("do [#agbr]. spec"),
            Some("agbr".to_string())
        );
        assert_eq!(
            extract_do_target("[prep] do #agbr"),
            Some("agbr".to_string())
        );
        assert_eq!(extract_do_target("run tests"), None);
        assert_eq!(
            extract_do_targets("do [#x63e] [#v4v0]. spec-test"),
            vec!["x63e".to_string(), "v4v0".to_string()]
        );
        assert_eq!(
            extract_do_target("do #inline-done-signal. spec-test"),
            Some("inline-done-signal".to_string())
        );
    }

    #[test]
    fn classifies_recoverable_graph_db_access_errors() {
        let locked = anyhow::anyhow!(
            "`tsift graph-db status` exited: Error code 5: The database file is locked"
        );
        assert!(is_recoverable_graph_db_access_error(&locked));

        let hot_journal =
            anyhow::anyhow!("sqlite hot-journal/read-only recovery prevented graph-db status");
        assert!(is_recoverable_graph_db_access_error(&hot_journal));

        let stale = anyhow::anyhow!("tsift graph.db is not current: graph.db is stale");
        assert!(!is_recoverable_graph_db_access_error(&stale));
    }

    #[cfg(unix)]
    #[test]
    fn collect_for_do_items_attaches_graph_handles() {
        let (dir, doc) = setup_doc();
        let log = dir.path().join("calls.log");
        let fake = fake_tsift(dir.path(), &log, false);
        let _env = EnvGuard::set(TSIFT_BIN_ENV, fake.to_str().unwrap());

        let plan = collect_for_do_items(&doc, &["do [#agbr]. spec-test".to_string()])
            .unwrap()
            .unwrap();

        assert_eq!(plan.targets, vec!["agbr"]);
        assert_eq!(plan.graph_db_status.status, "current");
        assert_eq!(
            plan.prompt_target_handles[0].evidence_packet_id,
            "gevd-agbr"
        );
        assert_eq!(plan.conflict_matrix.evidence_packet_ids, vec!["gevd-agbr"]);
        assert_eq!(
            plan.conflict_matrix.contract_version.as_deref(),
            Some("conflict-matrix-v1")
        );
        assert_eq!(
            plan.conflict_matrix.candidates[0].owned_files,
            vec!["tasks.md"]
        );
        assert_eq!(
            plan.conflict_matrix
                .context_pack
                .as_ref()
                .unwrap()
                .touched_files,
            vec!["tasks.md"]
        );
        assert_eq!(
            plan.conflict_matrix.worker_prompt_packets[0]
                .token_budget
                .as_ref()
                .unwrap()
                .source_window_count,
            1
        );
        let context = plan
            .prompt_context_for_task("do #agbr")
            .unwrap()
            .expect("expected prompt context");
        assert!(context.contains("<tsift_graph_evidence>"));
        assert!(context.contains("\"source_handles\": ["));
        assert!(context.contains("\"Worker 1 owns agbr (#agbr)\""));
        assert!(context.contains("\"context_pack\""));
        assert!(context.contains("\"candidates\""));
        assert!(context.contains("Fail closed if the task requires a forbidden/shared file"));
        assert!(context.contains("\"lower_agent_job_packet\""));
        assert!(context.contains("\"dispatch_trace\""));
        assert!(context.contains("\"contract_version\": \"dispatch-trace-v1\""));
        assert!(context.contains("\"worker_feedback\""));
        assert!(context.contains("\"follow_up_ids\": ["));
        assert!(context.contains("\"graph_nodes\": ["));
        assert!(context.contains("\"graph_edges\": ["));
        assert!(context.contains("\"replay_commands\": ["));
        assert!(context.contains("\"repair_commands\": ["));

        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains("graph-db"));
        assert!(calls.contains("refresh"));
        assert!(calls.contains("evidence agbr"));
        assert!(calls.contains("conflict-matrix"));
        assert!(calls.contains("dispatch-trace"));
    }

    #[cfg(unix)]
    #[test]
    fn collect_for_do_items_fails_closed_on_stale_graph_db() {
        let (dir, doc) = setup_doc();
        let log = dir.path().join("calls.log");
        let fake = fake_tsift(dir.path(), &log, true);
        let _env = EnvGuard::set(TSIFT_BIN_ENV, fake.to_str().unwrap());

        let err = collect_for_do_items(&doc, &["do #agbr".to_string()])
            .unwrap_err()
            .to_string();

        assert!(err.contains("not current"));
        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains("status"));
        assert!(!calls.contains("evidence agbr"));
        assert!(!calls.contains("conflict-matrix"));
    }

    #[cfg(unix)]
    #[test]
    fn collect_for_do_items_fails_closed_on_missing_graph_contract_fields() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, doc) = setup_doc();
        let log = dir.path().join("calls.log");
        let script = dir.path().join("fake-tsift-missing-contracts.sh");
        let script_body = format!(
            r##"#!/bin/sh
printf '%s\n' "$*" >> "{}"
case "$*" in
  *"graph-db"*"--json status"*)
    cat <<'JSON'
{{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]}}}}
JSON
    ;;
  *"graph-db"*"--json refresh"*)
    cat <<'JSON'
{{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","operation":"refresh","status":"current","freshness":{{"status":"current","fail_closed":false,"content_hash":"abc","source_watermark":"abc","diagnostics":[]}}}}
JSON
    ;;
  *"graph-db"*"--json evidence agbr"*)
    cat <<'JSON'
{{"root":"/tmp/repo","backend":"sqlite","target":"agbr","freshness":{{"status":"current","fail_closed":false,"diagnostics":[]}},"target_node":{{"id":"gbak-agbr","kind":"backlog","label":"#agbr"}},"worker_context":[],"source_handles":[]}}
JSON
    ;;
  *"conflict-matrix"*)
    cat <<'JSON'
{{"targets":["agbr"],"can_parallel":true,"fail_closed":false,"worker_prompt_packets":[]}}
JSON
    ;;
  *)
    echo "unexpected fake tsift args: $*" >&2
    exit 2
    ;;
esac
"##,
            log.display()
        );
        std::fs::write(&script, script_body).unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        let _env = EnvGuard::set(TSIFT_BIN_ENV, script.to_str().unwrap());

        let err = collect_for_do_items(&doc, &["do #agbr".to_string()])
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("graph-db evidence agbr missing contract_version"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn conflict_matrix_blocks_parallel_when_report_does_not_approve_dispatch() {
        let summary = TsiftConflictMatrixSummary {
            contract_version: Some("conflict-matrix-v1".to_string()),
            can_parallel: false,
            fail_closed: false,
            inputs: None,
            context_pack: None,
            candidates: Vec::new(),
            conflicts: vec![TsiftConflictMatrixConflict {
                left: "left".to_string(),
                right: "right".to_string(),
                risk: "high".to_string(),
                risk_score: 40,
                shared_files: Vec::new(),
                shared_symbols: vec!["shared_symbol".to_string()],
                shared_tests: Vec::new(),
                shared_config_files: Vec::new(),
                verdict: "split by file or serialize".to_string(),
            }],
            evidence_packet_ids: Vec::new(),
            decisions: vec!["pair left<->right risk=high".to_string()],
            worker_ownership_blocks: Vec::new(),
            worker_prompt_packets: Vec::new(),
            next_commands: Vec::new(),
            warnings: Vec::new(),
        };

        let blocker = summary.parallel_dispatch_blocker().unwrap();

        assert!(blocker.contains("can_parallel=false"));
        assert!(blocker.contains("shared_symbols=shared_symbol"));
    }
