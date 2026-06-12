    use super::*;
    use agent_doc_orchestration::snapshot;
    use std::io::Write;
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

        fn unset(key: &'static str) -> Self {
            let lock = crate::test_support::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
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

    fn write_cycles_log(
        doc: &std::path::Path,
        entries: &[agent_doc_orchestration::ops_log::CycleEntry],
    ) {
        let log_path = doc.parent().unwrap().join(".agent-doc/logs/cycles.jsonl");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(log_path).unwrap();
        for entry in entries {
            writeln!(file, "{}", serde_json::to_string(entry).unwrap()).unwrap();
        }
    }

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    #[test]
    fn build_plan_detects_orchestration_handoff_and_existing_pending_item() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:pending -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

synchronous orcestra
- do #1g42 Option A. update spec + tests. build + install for local testing. commit + push
- do #1g42 Option B. update spec + tests. build + install for local testing. commit + push
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:pending -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.handoff, HandoffTarget::Orchestrate);
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc orchestrate")
                    && cmd.contains("--mode sequential")),
            "expected orchestrate handoff command, got: {:?}",
            plan.required_commands
        );
        assert_eq!(plan.repo_actions.len(), 2);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "1g42");
        assert_eq!(
            plan.pending_mutations[0].kind,
            PendingMutationKind::ResolveExisting
        );
        assert_eq!(plan.prompt_targets.len(), 2);
    }

    #[test]
    fn build_plan_includes_finalize_placeholder_for_template_docs() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
What changed?
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.required_commands.iter().any(|cmd| {
                cmd.contains("agent-doc finalize")
                    && cmd.contains("--baseline-file <preflight.baseline_file>")
                    && cmd.contains("--stream")
            }),
            "expected finalize placeholder command, got: {:?}",
            plan.required_commands
        );
        assert_eq!(plan.handoff, HandoffTarget::None);
        assert!(plan.blockers.is_empty());
    }

    #[test]
    fn build_plan_uses_active_queue_prompt_when_document_has_no_diff() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue auto -->
- do [#oobpmt]
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#oobpmt] Fix OOB prompt absorption.
<!-- /agent:backlog -->
"#;
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "active queue prompt should not plan as a no-op"
        );
        assert_eq!(plan.repo_actions, vec!["do [#oobpmt]"]);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "oobpmt");
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--done oobpmt")),
            "queue do item should require closeout with --done oobpmt: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_treats_active_queue_slash_command_as_command_handoff() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue auto -->
-   /clear
<!-- /agent:queue -->
"#;
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let plan = build(&doc).unwrap();

        assert!(plan.prompt_targets.is_empty(), "{plan:?}");
        assert!(plan.repo_actions.is_empty(), "{plan:?}");
        assert!(plan.pending_mutations.is_empty(), "{plan:?}");
        assert_eq!(plan.handoff, HandoffTarget::Other);
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("`/clear`")),
            "slash command handoff should remain visible as a command requirement: {:?}",
            plan.required_commands
        );
        assert!(
            plan.required_commands
                .iter()
                .all(|cmd| !cmd.contains("agent-doc finalize")),
            "slash-only command handoff must not require assistant finalization: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_warns_on_semantic_completion_match_for_free_text_queue() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#cachefix] Repair cache duplication\n",
        )
        .unwrap();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
queue_active: true
---

<!-- agent:queue auto -->
- Repair cache duplication
<!-- /agent:queue -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#;
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let plan = build(&doc).unwrap();
        assert_eq!(plan.prompt_targets, vec!["Repair cache duplication"]);
        assert!(
            plan.warnings.iter().any(|warning| {
                warning.contains("semantic completion candidate") && warning.contains("#cachefix")
            }),
            "{:?}",
            plan.warnings
        );
    }

    #[test]
    fn build_plan_ignores_inactive_queue_edit_as_repo_action() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: false
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue -->
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#gdbpropscan] Inspect graph DB properties.
<!-- /agent:backlog -->
"#;
        let current = baseline.replace(
            "<!-- agent:queue -->\n<!-- /agent:queue -->",
            "<!-- agent:queue -->\n- do [#gdbpropscan]\n<!-- /agent:queue -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.prompt_targets.is_empty(),
            "inactive queue edit must not become prompt targets: {:?}",
            plan.prompt_targets
        );
        assert!(
            plan.repo_actions.is_empty(),
            "inactive queue edit must not become repo actions: {:?}",
            plan.repo_actions
        );
        assert!(
            plan.pending_mutations.is_empty(),
            "inactive queue edit must not resolve or capture pending work: {:?}",
            plan.pending_mutations
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_plan_downgrades_locked_graph_db_to_manual_packet_only_warning() {
        use std::os::unix::fs::PermissionsExt;

        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#jobslock] Create job packets when graph.db is locked.
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#jobslock]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let script = dir.path().join("fake-tsift-lock.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nif echo \"$*\" | grep -q 'graph-db.*--json status'; then echo 'Error code 5: The database file is locked' >&2; exit 1; fi\necho \"unexpected fake tsift args: $*\" >&2\nexit 2\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", script.to_str().unwrap());

        let plan = build(&doc).unwrap();

        assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan);
        assert!(plan.manual_packet_only);
        assert!(plan.graph_evidence.is_none());
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("database file is locked")),
            "expected lock warning, got {:?}",
            plan.warnings
        );
        assert_eq!(
            plan.repo_actions,
            vec!["do [#jobslock]. spec-test-build-install-commit-push"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_plan_downgrades_stale_graph_db_to_manual_packet_only_warning() {
        use std::os::unix::fs::PermissionsExt;

        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#staleg] Keep turns running when graph.db is stale.
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#staleg]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

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

        let plan = build(&doc).unwrap();

        assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan);
        assert!(plan.manual_packet_only);
        assert!(plan.graph_evidence.is_none());
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("graph.db is stale")
                    && warning.contains("manual_packet_only=true")),
            "expected stale graph warning, got {:?}",
            plan.warnings
        );
    }

    #[test]
    fn build_plan_includes_pending_done_for_bracketed_do_directive() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#dodone] Close the matching backlog item
<!-- /agent:pending -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
do [#dodone]. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#dodone] Close the matching backlog item
<!-- /agent:pending -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.repo_actions.len(), 1);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(
            plan.pending_mutations[0].kind,
            PendingMutationKind::ResolveExisting
        );
        assert_eq!(plan.pending_mutations[0].id, "dodone");
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc finalize")
                    && cmd.contains("--done dodone")
                    && cmd.contains("--stream")),
            "expected finalize command to carry --done, got: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_resolves_each_id_in_compound_do_directive() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#x63e] First packet target
- [ ] [#v4v0] Second packet target
<!-- /agent:pending -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#x63e] [#v4v0]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(
            plan.pending_mutations
                .iter()
                .map(|mutation| mutation.id.as_str())
                .collect::<Vec<_>>(),
            vec!["x63e", "v4v0"]
        );
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--done x63e") && cmd.contains("--done v4v0")),
            "expected finalize command to carry both --done flags, got: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_emits_lower_agent_routing_fields() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_dispatch: auto
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:backlog -->
- [ ] [#jobp1] Define lower-agent job packet spec and runbook.
- [ ] [#jobp2] Add tsift context packet tests.
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#jobp1]\ndo [#jobp2]\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(plan.dispatch_candidate);
        assert_eq!(plan.dispatch_mode, "auto");
        assert_eq!(plan.task_class, "lower_agent_orchestration");
        assert_eq!(plan.risk, "high");
        assert!(plan.parallelizable);
        assert_eq!(plan.model_tier, "high");
        assert_eq!(plan.context_budget_tokens, 10_000);
        assert!(
            plan.write_scope
                .contains(&"src/agent-doc/specs/".to_string())
        );
        assert!(plan.write_scope.contains(&"src/tsift/".to_string()));
        assert!(plan.required_proof.contains(&"verification".to_string()));
        assert_eq!(plan.tsift_context.status, "missing");
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--done jobp1") && cmd.contains("--done jobp2"))
        );
    }

    #[test]
    fn build_plan_resolves_existing_icebox_item_for_do_directive() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
<!-- /agent:pending -->

## Icebox

<!-- agent:icebox -->
- [ ] [#ice01] Parked follow-up
<!-- /agent:icebox -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
do #ice01. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
<!-- /agent:pending -->

## Icebox

<!-- agent:icebox -->
- [ ] [#ice01] Parked follow-up
<!-- /agent:icebox -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(
            plan.pending_mutations[0].kind,
            PendingMutationKind::ResolveExisting
        );
        assert_eq!(plan.pending_mutations[0].id, "ice01");
    }

    #[test]
    fn build_plan_dispatches_compact_exchange_request() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

compact exchange
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.handoff, HandoffTarget::Compact);
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc compact") && cmd.contains("--commit")),
            "expected compact handoff command, got: {:?}",
            plan.required_commands
        );
        assert!(
            !plan
                .required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc finalize")),
            "compact handoff should not advertise finalize: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn test_plan_detects_backlog_request() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#abc1] Existing item
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

add to backlog: what tasks remain?
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#abc1] Existing item
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_some(),
            "expected ExpectAdd mutation, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn plan_expect_add_carries_explicit_backlog_target() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let target = dir.path().join("bugs.md");
        std::fs::write(
            &target,
            "<!-- agent:backlog -->\n- [ ] [#old1] Existing\n<!-- /agent:backlog -->\n",
        )
        .unwrap();

        let baseline = format!(
            r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan. Add to the backlog of {}
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#,
            target.display()
        );

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "#agent-doc-bug\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, &baseline).unwrap();

        let plan = build(&doc).unwrap();
        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd)
            .expect("expected ExpectAdd mutation");
        assert_eq!(
            expect_add.target_files,
            vec![
                std::fs::canonicalize(&target)
                    .unwrap()
                    .display()
                    .to_string()
            ]
        );
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--pending-add-to") && cmd.contains("bugs.md")),
            "expected finalize hint to include --pending-add-to, got {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn plan_preserves_agent_doc_bug_declaration_order_for_target_adds() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let target = dir.path().join("bugs.md");
        std::fs::write(
            &target,
            "<!-- agent:backlog -->\n- [ ] [#old1] Existing\n<!-- /agent:backlog -->\n",
        )
        .unwrap();

        let baseline = format!(
            r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan. Add to the backlog of {}
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#,
            target.display()
        );

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "First captured bug. #agent-doc-bug\n---\nSecond captured bug. #agent-doc-bug\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, &baseline).unwrap();

        let plan = build(&doc).unwrap();
        let expect_adds = plan
            .pending_mutations
            .iter()
            .filter(|mutation| mutation.kind == PendingMutationKind::ExpectAdd)
            .collect::<Vec<_>>();

        assert_eq!(expect_adds.len(), 2);
        assert_eq!(expect_adds[0].text, "First captured bug. #agent-doc-bug");
        assert_eq!(expect_adds[1].text, "Second captured bug. #agent-doc-bug");
        assert_eq!(expect_adds[0].target_files, expect_adds[1].target_files);
    }

    #[test]
    fn test_plan_detects_recommendation_request() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

What should we do next? Any recommendations?
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_some(),
            "expected ExpectAdd for recommendation request, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn test_plan_detects_backlog_request_via_prompt_preset_expansion() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#code-review': Please review the codebase. '#follow-up-backlog'
  '#follow-up-backlog': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#code-review': Please review the codebase. '#follow-up-backlog'
  '#follow-up-backlog': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

❯ #code-review
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_some(),
            "expected ExpectAdd for preset-expanded backlog request, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn plan_classifies_embedded_next_steps_domain_prompt_as_actionable_backlog_request() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#next-steps': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let prompt =
            "Please analyze failed orders and bot traffic on monsterrodholders.com. #next-steps";
        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            &format!("{prompt}\n<!-- /agent:exchange -->"),
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.task_class, "prompt_response");
        assert_eq!(plan.execution_scope, ExecutionScope::Normal);
        assert!(
            plan.prompt_targets
                .iter()
                .any(|target| target.contains(prompt)),
            "expected embedded #next-steps domain prompt target, got {:?}",
            plan.prompt_targets
        );
        assert!(
            plan.pending_mutations
                .iter()
                .any(|mutation| mutation.kind == PendingMutationKind::ExpectAdd),
            "expected ExpectAdd from embedded #next-steps preset expansion, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn test_plan_no_false_positive_on_questions() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#xyz1] Some item
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

How does the CRDT merge work?
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#xyz1] Some item
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_none(),
            "should not emit ExpectAdd for a plain question, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_uses_harness_prompt_when_snapshot_matches_document() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#code-review': Please review the codebase. '#follow-up-backlog'
  '#follow-up-backlog': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!("agent-doc {} #code-review", doc.display()),
        );
        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "unexpected blockers: {:?}",
            plan.blockers
        );
        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ExpectAdd),
            "expected ExpectAdd from harness prompt preset expansion, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_blocks_shared_doc_plan_reference_without_security_review() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #spec2. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "spec2");
        assert_eq!(plan.blockers.len(), 1);
        assert!(plan.blockers[0].contains("agent_doc_security_review"));
    }

    #[test]
    fn build_plan_allows_shared_doc_plan_reference_with_security_review() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
agent_doc_security_review: sec-1
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
agent_doc_security_review: sec-1
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #spec2. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "spec2");
    }

    #[test]
    fn build_plan_resolves_existing_pending_item_from_harness_prompt() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!(
                "agent-doc {}\ndo #1g42. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "unexpected blockers: {:?}",
            plan.blockers
        );
        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| { m.kind == PendingMutationKind::ResolveExisting && m.id == "1g42" }),
            "expected ResolveExisting for harness prompt do-directive, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_resolves_explicit_inline_done_signal() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#inline-done-signal] Inline done signal
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "Done.\n<!-- /agent:exchange -->",
            "Done.\n\nmark #inline-done-signal done\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.pending_mutations.iter().any(|m| {
                m.kind == PendingMutationKind::ResolveExisting && m.id == "inline-done-signal"
            }),
            "expected ResolveExisting for explicit inline done signal, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_resolves_plain_done_to_single_review_item_when_auto_done_enabled() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
auto_done: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Waiting.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rev1] Await user acceptance
<!-- /agent:review -->
"#;

        let current = baseline.replace(
            "Waiting.\n<!-- /agent:exchange -->",
            "Waiting.\n\ndone\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ResolveExisting && m.id == "rev1"),
            "expected ResolveExisting for the single review item, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_does_not_resolve_plain_done_without_auto_done() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Waiting.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rev1] Await user acceptance
<!-- /agent:review -->
"#;

        let current = baseline.replace(
            "Waiting.\n<!-- /agent:exchange -->",
            "Waiting.\n\ndone\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            !plan
                .pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ResolveExisting && m.id == "rev1"),
            "plain done should require auto_done, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn plan_backlog_only_deferral_warning_names_suppressed_directive() {
        // #lr-queue-response-miss step 2: a runnable directive deferred by
        // plan_backlog_only must be surfaced by name.
        let diff = "--- snapshot\n+++ document\n@@ -0,0 +1 @@\n+do [#lr-queue-response-miss]\n";
        let warning =
            plan_backlog_only_deferral_warning(ExecutionScope::PlanBacklogOnly, diff).unwrap();
        assert!(warning.contains("plan_backlog_only"));
        assert!(
            warning.contains("lr-queue-response-miss"),
            "deferral warning must name the suppressed directive: {warning}"
        );
    }

    #[test]
    fn plan_backlog_only_deferral_warning_quiet_without_directive_or_in_normal_scope() {
        // Pure bug-capture (no imperative directive) stays quiet.
        assert!(
            plan_backlog_only_deferral_warning(
                ExecutionScope::PlanBacklogOnly,
                "+ just a clarifying question about the design?\n"
            )
            .is_none()
        );
        // Normal scope never warns even with a directive.
        assert!(plan_backlog_only_deferral_warning(ExecutionScope::Normal, "+do [#x]\n").is_none());
    }

    #[test]
    fn build_plan_marks_agent_doc_bug_prompt_as_plan_backlog_only() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _prompt = EnvGuard::set("AGENT_DOC_HARNESS_PROMPT", "#agent-doc-bug");
        let plan = build(&doc).unwrap();

        assert_eq!(plan.execution_scope, ExecutionScope::PlanBacklogOnly);
        assert!(plan.repo_actions.is_empty(), "{:?}", plan.repo_actions);
        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ExpectAdd),
            "expected ExpectAdd from #agent-doc-bug preset expansion, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_does_not_treat_backlog_text_as_agent_doc_bug_prompt_scope() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #pbct. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#pbct] Respect `#agent-doc-bug` preset scope and fail closed before implementation.
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.execution_scope, ExecutionScope::Normal);
        assert_eq!(
            plan.repo_actions,
            vec!["do #pbct. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn build_plan_keeps_copied_prompt_preset_definitions_out_of_prompt_scope() {
        let dir = setup_project();
        let doc = dir.path().join("tmux-router.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #tmuxreprocmd. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tmuxreprocmd] Capture the exact command, crate root, and tooling context that produced the tmux-router diagnostic.
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.execution_scope, ExecutionScope::Normal);
        assert_eq!(
            plan.repo_actions,
            vec!["do #tmuxreprocmd. spec-test-build-install-commit-push".to_string()]
        );
        assert!(
            !plan
                .pending_mutations
                .iter()
                .any(|mutation| mutation.kind == PendingMutationKind::ExpectAdd),
            "copied preset definitions must not require agent-doc-bug backlog capture: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_does_not_block_on_session_accretion_guard() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let long_exchange = (0..260)
            .map(|idx| format!("context line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->
"#;

        let current = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{long_exchange}\ndo #ctxacc. spec-test-build-install-commit-push\n<!-- /agent:exchange -->\n"
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan);
        assert_eq!(
            plan.repo_actions,
            vec!["do #ctxacc. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn build_plan_keeps_repeated_noop_closeout_churn_advisory() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #nooploop. spec-test-build-install-commit-push
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();
        write_cycles_log(
            &doc,
            &[
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(20).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(10).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );

        let plan = build(&doc).unwrap();

        assert_eq!(plan.handoff, HandoffTarget::None);
        assert_eq!(
            plan.repo_actions,
            vec!["do #nooploop. spec-test-build-install-commit-push".to_string()],
            "session-accretion no-op churn should remain advisory unless compact is explicit"
        );
        assert!(
            !plan
                .required_commands
                .iter()
                .any(|command| command.contains("agent-doc compact")),
            "session-accretion no-op churn must not force compaction: {:?}",
            plan.required_commands
        );
        assert!(
            plan.required_commands
                .iter()
                .any(|command| command.contains("agent-doc finalize")),
            "normal closeout should still be requested after repo work: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_allows_turn_after_recent_compaction_recovery() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
do #cmpclr. spec-test-build-install-commit-push
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();
        write_cycles_log(
            &doc,
            &[
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(120).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(110).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(100).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(90).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(80).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(70).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(60).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(50).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(40).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(30).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );
        agent_doc_orchestration::session_accretion::record_recent_exchange_compaction(&doc)
            .unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "recent exchange compaction should clear closeout-churn blockers: {:?}",
            plan.blockers
        );
        assert_eq!(
            plan.repo_actions,
            vec!["do #cmpclr. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn build_plan_allows_post_compaction_rerun_noop_closeouts() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
do #aftercmp. spec-test-build-install-commit-push
<!-- /agent:exchange -->
"#;

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();
        agent_doc_orchestration::session_accretion::record_recent_exchange_compaction(&doc)
            .unwrap();
        write_cycles_log(
            &doc,
            &[
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(5).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(4).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );

        let plan = build(&doc).unwrap();

        assert_eq!(
            plan.handoff,
            HandoffTarget::None,
            "preflight no-op closeouts immediately after compact must not trap the rerun in another compact handoff"
        );
        assert_eq!(
            plan.repo_actions,
            vec!["do #aftercmp. spec-test-build-install-commit-push".to_string()]
        );
    }
