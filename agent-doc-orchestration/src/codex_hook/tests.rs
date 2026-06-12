    use super::*;
    use std::fs;
    use std::process::Command as ProcessCommand;

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        dir
    }

    fn write_codex_mcp_config(root: &Path) {
        fs::create_dir_all(root.join(".codex")).unwrap();
        fs::write(
            root.join(".codex/config.toml"),
            format!(
                "[mcp_servers.agent-doc]\ncommand = \"agent-doc\"\nargs = [\"mcp\", \"serve\", \"--project-root\", \"{}\"]\n",
                root.display()
            ),
        )
        .unwrap();
    }

    fn init_git_repo(root: &Path, tracked: &Path) {
        let relative = tracked.strip_prefix(root).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["init"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", relative.to_str().unwrap()])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .status()
            .unwrap();
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = ProcessCommand::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "protocol.file.allow=always",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: stdout={} stderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write_doc(dir: &tempfile::TempDir) -> PathBuf {
        let doc = dir.path().join("task.md");
        let content = "---\nsession: sid\n---\n\n## User\n\nHello\n";
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        doc
    }

    fn write_auto_queue_doc(dir: &tempfile::TempDir, prompts: &[&str]) -> PathBuf {
        let doc = dir.path().join("task.md");
        let queue = prompts
            .iter()
            .map(|prompt| format!("- {prompt}\n"))
            .collect::<String>();
        let content = format!(
            "---\n\
session: sid\n\
agent_doc_format: template\n\
queue_active: true\n\
---\n\n\
## Exchange\n\n\
<!-- agent:exchange patch=append -->\n\
### Re: prior — gpt-5\n\n\
Done.\n\
<!-- /agent:exchange -->\n\n\
## Queue\n\n\
<!-- agent:queue auto -->\n\
{queue}\
<!-- /agent:queue -->\n"
        );
        fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        doc
    }

    fn write_nested_doc(dir: &tempfile::TempDir) -> PathBuf {
        let nested = dir.path().join("nested");
        fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        let doc = nested.join("task.md");
        let content = "---\nsession: sid\n---\n\n## User\n\nHello\n";
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        doc
    }

    fn track_doc(dir: &tempfile::TempDir, doc: &Path, turn_id: &str) {
        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: turn_id.to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!("agent-doc {}", doc.display()),
        })
        .unwrap();
    }

    #[test]
    fn user_prompt_submit_tracks_agent_doc_file() {
        let dir = setup_project();
        let doc = write_doc(&dir);

        track_doc(&dir, &doc, "turn-1");

        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(PathBuf::from(state.doc_path), doc);
        assert_eq!(state.last_turn_id, "turn-1");
        assert_eq!(state.last_prompt, format!("agent-doc {}", doc.display()));
    }

    #[test]
    fn user_prompt_submit_does_not_track_ambient_ancestor_root() {
        let ambient = tempfile::tempdir().unwrap();
        fs::create_dir_all(ambient.path().join(".agent-doc")).unwrap();
        let project = ambient.path().join("project");
        fs::create_dir_all(project.join(".agent-doc/snapshots")).unwrap();
        let doc = project.join("task.md");
        let content = "---\nsession: sid\n---\n\n## User\n\nHello\n";
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: project.display().to_string(),
            prompt: format!("agent-doc {}", doc.display()),
        })
        .unwrap();

        let project_state = load_state(&project, "codex-session").unwrap();
        let ambient_state = load_state(ambient.path(), "codex-session").unwrap();
        assert!(
            project_state.is_some(),
            "nearest project root should receive Codex hook state"
        );
        assert!(
            ambient_state.is_none(),
            "ambient ancestor .agent-doc roots must not receive shared hook state"
        );
    }

    #[test]
    fn user_prompt_submit_tracks_same_line_agent_doc_body() {
        let dir = setup_project();
        let doc = write_doc(&dir);

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!("agent-doc {} #code-review", doc.display()),
        })
        .unwrap();

        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(PathBuf::from(&state.doc_path), doc);
        assert_eq!(
            state.last_prompt,
            format!("agent-doc {} #code-review", doc.display())
        );

        let _lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };
        let loaded = crate::harness_prompt::prompt_body_for_file(&doc).unwrap();
        if let Some(value) = prev {
            unsafe { std::env::set_var("CODEX_THREAD_ID", value) };
        } else {
            unsafe { std::env::remove_var("CODEX_THREAD_ID") };
        }

        assert_eq!(loaded, Some("#code-review".to_string()));
    }

    #[test]
    fn resolve_agent_doc_path_prefers_real_invocation_after_instruction_preamble() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let prompt = format!(
            "# AGENTS.md instructions for {}\n\
\n\
```\n\
agent-doc <FILE>\n\
agent-doc compact <FILE>\n\
```\n\
\n\
Use the harness-native entrypoint below.\n\
\n\
agent-doc {}\n",
            dir.path().display(),
            doc.display()
        );

        let resolved = resolve_agent_doc_path(&prompt, dir.path()).expect("doc path");

        assert_eq!(resolved, doc);
    }

    #[test]
    fn resolve_agent_doc_path_accepts_session_invocation_with_trailing_body() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let prompt = format!("agent-doc {} #agent-doc-bug", doc.display());

        let resolved = resolve_agent_doc_path(&prompt, dir.path()).expect("doc path");

        assert_eq!(resolved, doc);
    }

    #[test]
    fn load_prompt_for_current_session_uses_codex_thread_id() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        track_doc(&dir, &doc, "turn-1");

        let _lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };
        let loaded = load_prompt_for_current_session(&doc).unwrap();
        if let Some(value) = prev {
            unsafe { std::env::set_var("CODEX_THREAD_ID", value) };
        } else {
            unsafe { std::env::remove_var("CODEX_THREAD_ID") };
        }

        assert_eq!(loaded, Some(format!("agent-doc {}", doc.display())));
    }

    #[test]
    fn load_latest_prompt_for_file_picks_most_recent_matching_state() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let root = project_root_for(dir.path()).unwrap();

        save_state(
            &root,
            &SessionState {
                session_id: "codex-session-old".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: None,
                last_context_clear_at: None,
                updated_at: 10,
            },
        )
        .unwrap();
        save_state(
            &root,
            &SessionState {
                session_id: "codex-session-new".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-2".to_string(),
                last_prompt: "/clear".to_string(),
                last_auto_queue_head: None,
                last_context_clear_at: Some(20),
                updated_at: 20,
            },
        )
        .unwrap();

        let loaded = load_latest_prompt_for_file(&doc).unwrap();
        assert_eq!(loaded.as_deref(), Some("/clear"));
    }

    #[test]
    fn load_latest_prompt_for_file_skips_malformed_state_entries() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let root = project_root_for(dir.path()).unwrap();
        let state_dir = root.join(".agent-doc/codex-hooks/sessions");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("bad.json"), "{").unwrap();

        save_state(
            &root,
            &SessionState {
                session_id: "codex-session-good".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: "/clear".to_string(),
                last_auto_queue_head: None,
                last_context_clear_at: Some(20),
                updated_at: 20,
            },
        )
        .unwrap();

        let loaded = load_latest_prompt_for_file(&doc).unwrap();
        assert_eq!(loaded.as_deref(), Some("/clear"));
    }

    #[test]
    fn prompt_requests_clear_matches_only_exact_builtin() {
        assert!(prompt_requests_clear("/clear"));
        assert!(prompt_requests_clear("  /clear  "));
        assert!(prompt_requests_clear("/new"));
        assert!(!prompt_requests_clear("agent-doc tasks/foo.md"));
        assert!(!prompt_requests_clear("/clear please"));
    }

    #[test]
    fn stop_auto_closes_open_cycle_from_last_assistant_message() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final assistant response.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });

        let pending = crate::snapshot::pending_path_for(&doc).unwrap();
        assert!(
            !pending.exists(),
            "pending capture should be cleared after recovery"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Final assistant response."));
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
        let log = ProcessCommand::new("git")
            .current_dir(dir.path())
            .args(["log", "--oneline", "-1"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&log.stdout).contains("agent-doc(task):"),
            "expected auto-close commit, got: {}",
            String::from_utf8_lossy(&log.stdout)
        );
        let root = project_root_for(dir.path()).unwrap();
        assert!(load_state(&root, "codex-session").unwrap().is_none());
    }

    #[test]
    fn stop_auto_closes_prompt_bearing_diff_when_cycle_never_started() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        let current = format!("{original}\n❯ Why was startup missed?\n");
        fs::write(&doc, &current).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "### Re: startup miss — gpt-5\n\nRecovered through Stop.\n"
                .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Why was startup missed?"));
        assert!(content.contains("Recovered through Stop."));
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_blocks_when_parent_submodule_pointer_closeout_fails() {
        let parent_dir = tempfile::tempdir().unwrap();
        let sub_src_dir = tempfile::tempdir().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        git(&sub_src, &["init"]);
        fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init", "--no-verify"]);

        git(&parent, &["init"]);
        fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init", "--no-verify"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );
        git(&parent, &["commit", "-m", "add submodule", "--no-verify"]);

        let submodule_root = parent.join("src/submodule");
        fs::create_dir_all(submodule_root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(submodule_root.join(".agent-doc/state/cycles")).unwrap();
        let doc = submodule_root.join("session.md");
        let original = concat!(
            "---\n",
            "agent_doc_session: sid\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Are the false positives fixed now?\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, original).unwrap();
        crate::snapshot::save(&doc, original).unwrap();
        git(&submodule_root, &["add", "session.md"]);
        git(&submodule_root, &["commit", "-m", "add doc", "--no-verify"]);
        git(&parent, &["add", "src/submodule"]);
        git(
            &parent,
            &["commit", "-m", "record doc commit", "--no-verify"],
        );

        let parent_git_dir = ProcessCommand::new("git")
            .current_dir(&parent)
            .args(["rev-parse", "--absolute-git-dir"])
            .output()
            .unwrap();
        assert!(parent_git_dir.status.success());
        let parent_git_dir = PathBuf::from(String::from_utf8_lossy(&parent_git_dir.stdout).trim());
        fs::write(parent_git_dir.join("index.lock"), "held by test").unwrap();

        track_doc(&parent_dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: parent.display().to_string(),
            last_assistant_message: concat!(
                "<!-- patch:exchange -->\n",
                "### Re: false-positive status — gpt-5\n\n",
                "Yes, the direct-chat answer was written through the Stop hook.\n",
                "<!-- /patch:exchange -->\n",
            )
            .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        let blocked_after_submodule_commit = match response {
            StopResponse::Block { reason, .. } => {
                assert!(
                    reason.contains("could not finish the required commit boundary"),
                    "block reason should name closeout failure, got: {reason}"
                );
                let names_parent_pointer = reason
                    .contains("parent submodule pointer is not committed")
                    && reason.contains("agent-doc commit");
                let names_open_cycle = reason.contains("finalize left cycle")
                    && reason.contains("agent-doc session-check");
                assert!(
                    names_parent_pointer || names_open_cycle,
                    "block reason should name the missing parent layer or the earlier open-cycle closeout boundary, got: {reason}"
                );
                names_parent_pointer
            }
            other => panic!("expected recoverable block response, got {other:?}"),
        };

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("direct-chat answer was written through the Stop hook"));
        if blocked_after_submodule_commit {
            assert!(
                crate::git::submodule_pointer_drift(&doc).unwrap().is_some(),
                "parent gitlink should remain stale while index.lock is held"
            );
        }
        let root = project_root_for(&doc).unwrap();
        assert!(
            load_state(&root, "codex-session").unwrap().is_some(),
            "hook state must remain so a retry can finish closeout"
        );
    }

    #[test]
    fn stop_auto_closes_visible_template_response_without_last_assistant_message() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do [#8zjh]. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, original).unwrap();
        crate::snapshot::save(&doc, original).unwrap();
        init_git_repo(dir.path(), &doc);

        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do [#8zjh]. spec-test-build-install-commit-push\n",
            "### Re: #8zjh — gpt-5\n\n",
            "Recovered from visible response.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, current).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: String::new(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Recovered from visible response."));
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_patch_payload_with_safe_leading_commentary() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ What are some #next-steps?\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, original).unwrap();
        crate::snapshot::save(&doc, original).unwrap();
        init_git_repo(dir.path(), &doc);
        crate::cycle_state::start_preflight(&doc, Some(original), Some(original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let payload = concat!(
            "Reviewing the current plan and repo conventions so I can turn `#next-steps` into concrete backlog items in the session document.\n",
            "I have the plan context. Next I’m checking how this repo formats backlog items so the patch matches existing session-doc conventions instead of inventing a new shape.\n\n",
            "<!-- patch:exchange -->\n",
            "### Re: #next-steps — gpt-5\n\n",
            "Added prioritized follow-up items.\n",
            "<!-- /patch:exchange -->\n\n",
            "<!-- patch:backlog -->\n",
            "- [ ] [#bpcontract] Write the contract first.\n",
            "<!-- /patch:backlog -->\n"
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: payload.to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: #next-steps — gpt-5"));
        assert!(content.contains("[#bpcontract] Write the contract first."));
        assert!(
            !content.contains("Reviewing the current plan and repo conventions"),
            "leading commentary should be stripped from the replayed closeout"
        );
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_guard_prefixed_patch_payload() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, original).unwrap();
        crate::snapshot::save(&doc, original).unwrap();
        init_git_repo(dir.path(), &doc);
        crate::cycle_state::start_preflight(&doc, Some(original), Some(original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let payload = concat!(
            "<!-- no-pending-capture -->\n",
            "<!-- patch:exchange -->\n",
            "### Re: Please reply — gpt-5\n\n",
            "Hook closeout body.\n",
            "<!-- /patch:exchange -->\n"
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: payload.to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: Please reply — gpt-5"));
        assert!(content.contains("Hook closeout body."));
        assert!(
            !dir.path()
                .join(".agent-doc/codex-hooks/blocked-stop")
                .exists(),
            "guard-prefixed patch payload should not be captured as blocked"
        );
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_partial_backlog_patch_against_structured_backlog() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let original = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ What are some #next-steps?\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "### 1. Existing\n",
            "- [ ] [#base] Keep the existing top item.\n",
            "\n",
            "### 2. Later\n",
            "- [ ] [#later] Keep the later section item.\n",
            "<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, original).unwrap();
        crate::snapshot::save(&doc, original).unwrap();
        init_git_repo(dir.path(), &doc);
        crate::cycle_state::start_preflight(&doc, Some(original), Some(original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let payload = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: #next-steps — gpt-5\n\n",
            "Added prioritized follow-up items.\n",
            "<!-- /patch:exchange -->\n\n",
            "<!-- patch:backlog -->\n",
            "### 1. Existing\n",
            "- [ ] [#base] Keep the existing top item.\n",
            "- [ ] [#bpcontract] Write the contract first.\n",
            "<!-- /patch:backlog -->\n"
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: payload.to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: #next-steps — gpt-5"));
        assert!(content.contains("[#bpcontract] Write the contract first."));
        assert!(content.contains("### 2. Later"));
        let capture = crate::capture::latest_committed(&doc)
            .unwrap()
            .expect("committed capture should exist");
        assert!(
            !capture.response_body.contains("<!-- patch:backlog -->"),
            "captured response should be stripped of backlog patches after normalization"
        );
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_active_session_post_commit_drift() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        let drifted = format!("{original}\nPost-closeout active-session drift.\n");
        fs::write(&doc, &drifted).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let _lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };

        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("active harness session changed this document"));
            }
            other => panic!("expected interrupted session-check status, got {other:?}"),
        }

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Recovered post-closeout drift.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        if let Some(value) = prev {
            unsafe { std::env::set_var("CODEX_THREAD_ID", value) };
        } else {
            unsafe { std::env::remove_var("CODEX_THREAD_ID") };
        }

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Post-closeout active-session drift."));
        assert!(content.contains("Recovered post-closeout drift."));
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_open_cycle_across_nested_roots_and_turn_drift() {
        let dir = setup_project();
        let doc = write_nested_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!(
                "agent-doc nested/{}",
                doc.file_name().unwrap().to_string_lossy()
            ),
        })
        .unwrap();

        let nested_root = project_root_for(doc.parent().unwrap()).unwrap();
        assert!(
            load_state(&nested_root, "codex-session").unwrap().is_some(),
            "expected state to be mirrored into nested project root"
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-2".to_string(),
            cwd: doc.parent().unwrap().display().to_string(),
            last_assistant_message: "Recovered from nested root drift.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Recovered from nested root drift."));
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }

        let outer_root = project_root_for(dir.path()).unwrap();
        assert!(load_state(&outer_root, "codex-session").unwrap().is_none());
        assert!(load_state(&nested_root, "codex-session").unwrap().is_none());
    }

    #[test]
    fn stop_auto_closes_active_session_drift_when_prompt_has_instruction_preamble() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&original),
            Some(&original),
        )
        .unwrap();
        fs::write(
            &doc,
            format!("{original}\nVisible drift after committed closeout.\n"),
        )
        .unwrap();

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!(
                "# AGENTS.md instructions for {}\n\n```\nagent-doc <FILE>\n```\n\nagent-doc {}\n",
                dir.path().display(),
                doc.display()
            ),
        })
        .unwrap();

        let _lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };

        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("active harness session changed this document"));
            }
            other => panic!("expected interrupted session-check status, got {other:?}"),
        }

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Recovered after preamble prompt tracking.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        if let Some(value) = prev {
            unsafe { std::env::set_var("CODEX_THREAD_ID", value) };
        } else {
            unsafe { std::env::remove_var("CODEX_THREAD_ID") };
        }

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Visible drift after committed closeout."));
        assert!(content.contains("Recovered after preamble prompt tracking."));
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
    }

    #[test]
    fn stop_blocks_open_cycle_without_recoverable_response() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: String::new(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("unfinished document cycle"));
                assert!(reason.contains("agent-doc repair"));
                assert!(reason.contains("tool-only or authentication step"));
                assert!(reason.contains("blocked-stop"));
            }
            other => panic!("expected block response, got {other:?}"),
        }

        let blocked_dir = dir.path().join(".agent-doc/codex-hooks/blocked-stop");
        let captures: Vec<_> = fs::read_dir(&blocked_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .collect();
        assert_eq!(captures.len(), 1, "expected one blocked-stop capture");
        let blocked_payload = fs::read_to_string(captures[0].path()).unwrap();
        assert!(blocked_payload.contains("\"kind\": \"missing_last_assistant_message\""));
        assert!(blocked_payload.contains(&format!("agent-doc {}", doc.display())));
    }

    #[test]
    fn stop_blocks_transcript_shaped_last_assistant_message() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let transcript_dump = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: hook proof — gpt-5\n",
            "Hook closeout body.\n",
            "<!-- /agent:exchange -->\n",
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: transcript_dump.to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("unfinished document cycle"));
                assert!(reason.contains("refused to replay"));
                assert!(reason.contains("blocked-stop"));
            }
            other => panic!("expected block response, got {other:?}"),
        }

        let pending = crate::snapshot::pending_path_for(&doc).unwrap();
        assert!(
            !pending.exists(),
            "transcript-shaped payload should not be stored as replayable pending content"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(content, original, "document should remain unchanged");

        let blocked_dir = dir.path().join(".agent-doc/codex-hooks/blocked-stop");
        let captures: Vec<_> = fs::read_dir(&blocked_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .collect();
        assert_eq!(captures.len(), 1, "expected one blocked-stop capture");
        let blocked_payload = fs::read_to_string(captures[0].path()).unwrap();
        assert!(blocked_payload.contains("agent:exchange"));
        assert!(blocked_payload.contains("component dump"));
    }

    #[test]
    fn stop_passes_through_committed_cycle() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit", Some(&original), Some(&original))
            .unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let pending = crate::snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn stop_blocks_clean_closeout_when_auto_queue_has_next_prompt() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("agent:queue auto"), "{reason}");
                assert!(reason.contains("do #fix1"), "{reason}");
                assert!(reason.contains("send the final answer"), "{reason}");
                // #codex-self-reinvoke-prevent (Option B): the continuation must
                // drive an in-pane answer + `finalize`, NOT instruct a recursive
                // `agent-doc <FILE>` re-run from the owner pane.
                assert!(reason.contains("in-pane"), "{reason}");
                assert!(reason.contains("agent-doc finalize"), "{reason}");
                assert!(
                    reason.contains("Do NOT run `agent-doc"),
                    "continuation must warn against the recursive self-invocation: {reason}"
                );
            }
            other => panic!("expected auto-queue continuation block, got {other:?}"),
        }

        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix1"));
    }

    #[test]
    fn stop_passes_through_clean_closeout_when_auto_queue_has_clear_command() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["/clear", "do #fix1"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });

        let root = project_root_for(dir.path()).unwrap();
        assert!(load_state(&root, "codex-session").unwrap().is_none());
    }

    #[test]
    fn stop_passes_through_raw_clear_queue_body_with_whitespace() {
        let dir = setup_project();
        let doc = dir.path().join("task.md");
        let content = concat!(
            "---\n",
            "session: sid\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue auto -->\n",
            "\n   /clear   \n\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });

        let root = project_root_for(dir.path()).unwrap();
        assert!(load_state(&root, "codex-session").unwrap().is_none());
    }

    #[test]
    fn stop_blocks_clean_closeout_when_auto_queue_has_generic_slash_command() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["/model sonnet", "do #fix1"]);
        init_git_repo(dir.path(), &doc);
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("queued slash command"), "{reason}");
                assert!(reason.contains("\"/model sonnet\""), "{reason}");
                assert!(!reason.contains("Run `/clear`"), "{reason}");
            }
            other => panic!("expected auto-queue command continuation block, got {other:?}"),
        }
    }

    #[test]
    fn stop_auto_closes_open_cycle_then_blocks_for_next_auto_queue_head() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: concat!(
                "<!-- patch:exchange -->\n",
                "### Re: #fix1 — gpt-5\n\n",
                "Done.\n",
                "<!-- /patch:exchange -->\n",
            )
            .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("do #fix2"), "{reason}");
            }
            other => panic!("expected auto-queue continuation block, got {other:?}"),
        }
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: #fix1 — gpt-5"));
        assert!(content.contains("- ~~do #fix1~~"));
        assert!(content.contains("- do #fix2"));
        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix2"));
    }

    #[test]
    fn stop_auto_queue_continuation_prefers_configured_mcp_tools() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        write_codex_mcp_config(dir.path());
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: concat!(
                "<!-- patch:exchange -->\n",
                "### Re: #fix1 — gpt-5\n\n",
                "Done.\n",
                "<!-- /patch:exchange -->\n",
            )
            .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(
                    reason.contains("configured `agent-doc` MCP server"),
                    "{reason}"
                );
                assert!(reason.contains("agent_doc_preflight"), "{reason}");
                assert!(reason.contains("agent_doc_plan"), "{reason}");
                assert!(reason.contains("agent_doc_finalize"), "{reason}");
                assert!(reason.contains("agent_doc_session_check"), "{reason}");
                assert!(
                    reason.contains("agent-doc finalize")
                        && reason.contains("MCP tools are unavailable"),
                    "{reason}"
                );
                assert!(reason.contains("send the final answer"), "{reason}");
            }
            other => panic!("expected auto-queue continuation block, got {other:?}"),
        }
        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("codex_stop_queue_continuation")
                && ops_log.contains("source=tracked_state")
                && ops_log.contains("mcp_configured=true")
                && ops_log.contains(&crate::ops_log::content_hash("do #fix2")),
            "Stop hook should log tracked queue-continuation proof:\n{ops_log}"
        );
    }

    #[test]
    fn stop_blocks_from_durable_marker_when_session_state_missing() {
        // #codex-auto-queue-stalled-final-gate live regression (monsterrodholders
        // shape): the completed head was consumed, `#seopdp` remains, queue_active
        // is true with `agent:queue auto`, and the document is clean — but the
        // Stop hook has NO tracked in-memory session state (the live failure). The
        // durable continuation marker (written at the prior clean closeout) must
        // still force continuation instead of letting Codex send a final answer.
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        // Prior clean closeout wrote the durable marker.
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");

        // Untracked session id → load_state_any returns None.
        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("durable"), "{reason}");
                assert!(
                    reason.contains("do [#seopdp] deploy product page"),
                    "{reason}"
                );
                assert!(reason.contains("send the final answer"), "{reason}");
                assert!(!reason.contains("agent_doc_finalize"), "{reason}");
            }
            other => panic!("expected durable-marker continuation block, got {other:?}"),
        }
        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("codex_stop_queue_continuation")
                && ops_log.contains("source=durable_marker")
                && ops_log.contains("mcp_configured=false")
                && ops_log.contains(&crate::ops_log::content_hash(
                    "do [#seopdp] deploy product page"
                )),
            "Stop hook should log durable-marker queue-continuation proof:\n{ops_log}"
        );
    }

    #[test]
    fn stop_passes_through_context_clear_from_durable_marker_when_session_state_missing() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["/clear"]);
        init_git_repo(dir.path(), &doc);
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");

        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
    }

    #[test]
    fn stop_marker_fallback_requires_clear_after_exchange_compaction() {
        let dir = setup_project();
        // `#nm1x-codex-clear-parity`: the Codex Stop-hook pre-emptive `/clear`
        // continuation is now gated on the `agent_doc_queue_context_reset` opt-in
        // (off by default, product-wide). This test exercises the fresh-context
        // path, so the project must opt in via `.agent-doc/config.toml`.
        fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_queue_context_reset = true\n",
        )
        .unwrap();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();

        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("Fresh context is required"), "{reason}");
                assert!(
                    reason.contains("Run `/clear` before continuing"),
                    "{reason}"
                );
                assert!(reason.contains("re-invoke `agent-doc"), "{reason}");
                assert!(reason.contains("send the final answer"), "{reason}");
                assert!(!reason.contains("agent_doc_finalize"), "{reason}");
            }
            other => panic!("expected fresh-context continuation block, got {other:?}"),
        }
    }

    /// `#clearcodex`: the Codex Stop-hook continuation now emits structured
    /// proof lines to ops.log when opted in, so an operator can verify the
    /// queue-turn clear decision instead of guessing. The canonical
    /// `[s760] clear-decision` line plus a `[clearcodex] codex-continuation`
    /// companion (with the accretion/compaction reason and the
    /// `clear_instructed` outcome) must both be present.
    #[test]
    fn stop_codex_continuation_logs_structured_clear_proof_when_opted_in() {
        let dir = setup_project();
        fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_queue_context_reset = true\n",
        )
        .unwrap();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();

        apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
            .expect("ops.log should exist after an opted-in continuation");
        assert!(
            ops_log.contains("[s760] clear-decision optIn=true"),
            "missing canonical s760 marker:\n{ops_log}"
        );
        assert!(
            ops_log.contains("pct=none clear=false"),
            "codex ctx% is unsupported, so the s760 gate must report pct=none clear=false:\n{ops_log}"
        );
        assert!(
            ops_log.contains("[clearcodex] codex-continuation optIn=true"),
            "missing codex-continuation companion marker:\n{ops_log}"
        );
        assert!(
            ops_log.contains("clear_instructed=true"),
            "compaction-after-clear should instruct a /clear:\n{ops_log}"
        );
    }

    /// `#clearcodex`: without the `agent_doc_queue_context_reset` opt-in the
    /// Codex continuation must stay silent — no pre-emptive `/clear` and no
    /// structured clear-decision noise in ops.log.
    #[test]
    fn stop_codex_continuation_emits_no_clear_proof_when_not_opted_in() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        init_git_repo(dir.path(), &doc);
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();

        apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        let ops_log =
            fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !ops_log.contains("[s760] clear-decision"),
            "no s760 clear-decision should be logged when not opted in:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("[clearcodex] codex-continuation"),
            "no codex-continuation marker should be logged when not opted in:\n{ops_log}"
        );
    }

    #[test]
    fn stop_auto_queue_allows_in_pane_after_tracked_clear_following_compaction() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();
        let compaction_ts = crate::session_accretion::recent_exchange_compaction_timestamp(&doc)
            .unwrap()
            .expect("compaction marker should be visible");
        let root = project_root_for(dir.path()).unwrap();
        save_state(
            &root,
            &SessionState {
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: "/clear".to_string(),
                last_auto_queue_head: None,
                last_context_clear_at: Some(compaction_ts),
                updated_at: compaction_ts,
            },
        )
        .unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("Continue THIS turn in-pane"), "{reason}");
                assert!(reason.contains("agent-doc finalize"), "{reason}");
                assert!(!reason.contains("Run `/clear`"), "{reason}");
                assert!(reason.contains("do #fix1"), "{reason}");
            }
            other => panic!("expected normal in-pane auto-queue continuation, got {other:?}"),
        }
    }

    #[test]
    fn stop_marker_fallback_continuation_prefers_configured_mcp_tools() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy product page"]);
        write_codex_mcp_config(dir.path());
        init_git_repo(dir.path(), &doc);
        crate::queue_continuation::reconcile_marker(&doc, "commit").expect("continuation required");

        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("durable"), "{reason}");
                assert!(
                    reason.contains("do [#seopdp] deploy product page"),
                    "{reason}"
                );
                assert!(
                    reason.contains("configured `agent-doc` MCP server"),
                    "{reason}"
                );
                assert!(reason.contains("agent_doc_preflight"), "{reason}");
                assert!(reason.contains("agent_doc_finalize"), "{reason}");
                assert!(reason.contains("agent_doc_session_check"), "{reason}");
                assert!(reason.contains("agent-doc write --commit"), "{reason}");
            }
            other => panic!("expected durable-marker continuation block, got {other:?}"),
        }
    }

    #[test]
    fn stop_marker_fallback_replays_plain_final_answer_when_head_does_not_advance() {
        // #codex-auto-queue-stalled-final-gate: a repeated stop (stop_hook_active)
        // whose durable marker already requested this exact head must persist a
        // plain Codex final answer into agent:exchange instead of allowing it to
        // escape as chat-only text.
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do [#seopdp] deploy"]);
        init_git_repo(dir.path(), &doc);
        crate::queue_continuation::reconcile_marker(&doc, "commit").unwrap();
        // The first continuation request recorded this head into the marker.
        crate::queue_continuation::record_requested_head(&doc, "do [#seopdp] deploy").unwrap();

        let response = apply_stop(&StopInput {
            session_id: "untracked-session".to_string(),
            turn_id: "turn-x".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final answer.\n\nVerification: Codex stop-hook simulation."
                .to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: do [#seopdp] deploy — gpt-5"));
        assert!(content.contains("Verification: Codex stop-hook simulation."));
        assert!(!content.contains("- do [#seopdp] deploy"));
        assert!(
            crate::queue_continuation::detect(&doc).unwrap().is_none(),
            "replayed response should drain the only active head"
        );
    }

    #[test]
    fn stop_repair_preserves_auto_queue_when_response_targets_other_prompt() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: concat!(
                "<!-- patch:exchange -->\n",
                "### Re: #next-steps — gpt-5\n\n",
                "Captured unrelated follow-up response.\n",
                "<!-- /patch:exchange -->\n",
            )
            .to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("do #fix1"), "{reason}");
                assert!(!reason.contains("do #fix2"), "{reason}");
            }
            other => panic!("expected auto-queue continuation block, got {other:?}"),
        }
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: #next-steps — gpt-5"));
        assert!(content.contains("<!-- agent:queue auto -->"));
        assert!(content.contains("queue_active: true"));
        assert!(content.contains("- do #fix1"));
        assert!(!content.contains("- ~~do #fix1~~"));
        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix1"));
    }

    #[test]
    fn stop_replays_plain_final_answer_when_auto_queue_continuation_makes_no_progress() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        let root = project_root_for(dir.path()).unwrap();
        save_state(
            &root,
            &SessionState {
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: Some("do #fix1".to_string()),
                last_context_clear_at: None,
                updated_at: 20,
            },
        )
        .unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.\n\nVerification: Codex stop-hook simulation."
                .to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(
                    reason.contains("recovered the previous queue response"),
                    "{reason}"
                );
                assert!(reason.contains("do #fix2"), "{reason}");
            }
            other => panic!("expected recovered no-progress block, got {other:?}"),
        }
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("### Re: do #fix1 — gpt-5"));
        assert!(content.contains("Verification: Codex stop-hook simulation."));
        assert!(content.contains("- ~~do #fix1~~"));
        assert!(content.contains("- do #fix2"));
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix2"));
    }

    #[test]
    fn stop_blocks_when_repeated_auto_queue_head_has_no_replayable_response() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix1", "do #fix2"]);
        init_git_repo(dir.path(), &doc);
        let root = project_root_for(dir.path()).unwrap();
        save_state(
            &root,
            &SessionState {
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: Some("do #fix1".to_string()),
                last_context_clear_at: None,
                updated_at: 20,
            },
        )
        .unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: String::new(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("could not safely replay"), "{reason}");
                assert!(reason.contains("agent-doc finalize"), "{reason}");
                assert!(reason.contains("do #fix1"), "{reason}");
            }
            other => panic!("expected repeated-head recovery block, got {other:?}"),
        }
        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("### Re: do #fix1 — gpt-5"));
        assert!(content.contains("- do #fix1"));
        assert!(!content.contains("- ~~do #fix1~~"));
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix1"));
    }

    #[test]
    fn stop_allows_repeated_auto_queue_blocks_after_head_advances() {
        let dir = setup_project();
        let doc = write_auto_queue_doc(&dir, &["do #fix2", "do #fix3"]);
        init_git_repo(dir.path(), &doc);
        let root = project_root_for(dir.path()).unwrap();
        save_state(
            &root,
            &SessionState {
                session_id: "codex-session".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: Some("do #fix1".to_string()),
                last_context_clear_at: None,
                updated_at: 20,
            },
        )
        .unwrap();

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("do #fix2"), "{reason}");
            }
            other => panic!("expected continued auto-queue block, got {other:?}"),
        }
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(state.last_auto_queue_head.as_deref(), Some("do #fix2"));
    }

    #[test]
    fn stop_fails_closed_after_one_auto_continue() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Still open.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::Stop {
                continue_: false,
                stop_reason,
            } => {
                assert!(stop_reason.contains("already continued once"));
                assert!(stop_reason.contains("cycle is still open"));
            }
            other => panic!("expected stop response, got {other:?}"),
        }
    }

    #[test]
    fn stop_hook_active_blocks_committed_cycle_fresh_prompt_instead_of_stopping() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit", Some(&original), Some(&original))
            .unwrap();
        fs::write(
            &doc,
            format!(
                "{original}\n❯ do #repair-false-closeouts. #spec-test-build-install-commit-push\n"
            ),
        )
        .unwrap();
        track_doc(&dir, &doc, "turn-1");

        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("is `committed`"), "{message}");
                assert!(
                    message.contains("no new agent-doc cycle started")
                        || message.contains("without reopening the binary-owned write/commit path"),
                    "{message}"
                );
            }
            other => panic!("expected interrupted session-check status, got {other:?}"),
        }

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Still working.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(
                    reason.contains("fresh unresolved exchange work"),
                    "{reason}"
                );
                assert!(
                    reason.contains("previous cycle was already committed"),
                    "{reason}"
                );
                assert!(reason.contains("do #repair-false-closeouts"), "{reason}");
                assert!(reason.contains("agent-doc finalize"), "{reason}");
                assert!(!reason.contains("already continued once"), "{reason}");
            }
            other => panic!("expected block response, got {other:?}"),
        }
    }
