    use super::*;
    use agent_kit::detect::Environment;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: test-only process-local env mutation, restored in Drop.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: test-only process-local env mutation, restored in Drop.
            unsafe { std::env::remove_var(key) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => {
                    // SAFETY: test-only process-local env mutation, restored to prior value.
                    unsafe { std::env::set_var(self.key, value) };
                }
                None => {
                    // SAFETY: test-only process-local env mutation, restored to prior absence.
                    unsafe { std::env::remove_var(self.key) };
                }
            }
        }
    }

    #[test]
    fn bundled_skill_is_not_empty() {
        assert!(!SKILL_TEMPLATE.is_empty());
    }

    #[test]
    fn bundled_skill_contains_agent_doc() {
        assert!(SKILL_TEMPLATE.contains("agent-doc"));
    }

    #[test]
    fn bundled_skill_hot_path_stays_compact() {
        assert!(
            line_count(SKILL_TEMPLATE) <= 140,
            "SKILL.md hot path grew to {} lines",
            line_count(SKILL_TEMPLATE)
        );
    }

    #[test]
    fn rendered_claude_skill_includes_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::ClaudeCode);
        assert!(
            rendered.contains("## Auto-loop while queue is active (Claude Code)"),
            "Claude-rendered SKILL.md must include the auto-loop section header"
        );
        assert!(
            rendered.contains("Skill") && rendered.contains("skill: \"loop\""),
            "auto-loop section must instruct invoking the Skill tool with skill: \"loop\""
        );
        assert!(
            rendered.contains("AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP"),
            "auto-loop section must name the env hard-cap"
        );
        assert!(
            rendered.contains("queue_active") && rendered.contains("queue_prompts"),
            "auto-loop section must reference preflight queue fields"
        );
        assert!(
            rendered.contains("\"persisted\""),
            "auto-loop section must make persisted-active queues continuation-eligible (#active-queue-persisted-no-continue)"
        );
    }

    #[test]
    fn rendered_codex_skill_omits_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::Codex);
        assert!(
            !rendered.contains("## Auto-loop while queue is active (Claude Code)"),
            "Codex-rendered SKILL.md must NOT include the Claude-only auto-loop section (Codex uses its own Stop hook)"
        );
    }

    #[test]
    fn rendered_opencode_skill_omits_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::OpenCode);
        assert!(
            !rendered.contains("## Auto-loop while queue is active (Claude Code)"),
            "OpenCode-rendered SKILL.md must NOT include the Claude-only auto-loop section"
        );
    }

    #[test]
    fn rendered_generic_skill_omits_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::Generic);
        assert!(
            !rendered.contains("## Auto-loop while queue is active (Claude Code)"),
            "Generic-rendered SKILL.md must NOT include the Claude-only auto-loop section"
        );
    }

    #[test]
    fn rendered_harness_content_stays_compact() {
        for env in [
            Environment::ClaudeCode,
            Environment::OpenCode,
            Environment::Codex,
            Environment::Generic,
        ] {
            let content = super::content_for_env(env);
            assert!(
                line_count(&content) <= 150,
                "{env:?} rendered instruction surface grew to {} lines",
                line_count(&content)
            );
        }
    }

    #[test]
    fn bundled_skill_template_contains_auto_update_line() {
        assert!(SKILL_TEMPLATE.contains(AUTO_UPDATE_LINE));
    }

    #[test]
    fn detect_install_env_treats_codex_thread_id_as_codex() {
        let _env_lock = crate::test_support::env_lock();
        let _claude = EnvVarGuard::unset("CLAUDE_CODE");
        let _claude_ep = EnvVarGuard::unset("CLAUDE_CODE_ENTRYPOINT");
        let _opencode = EnvVarGuard::unset("OPENCODE");
        let _cursor = EnvVarGuard::unset("CURSOR_SESSION_ID");
        let _cursor2 = EnvVarGuard::unset("CURSOR");
        let _code = EnvVarGuard::unset("CODEX");
        let _code_cli = EnvVarGuard::unset("CODEX_CLI");
        let _thread = EnvVarGuard::set("CODEX_THREAD_ID", "thread-123");
        let _ci = EnvVarGuard::unset("CODEX_CI");

        assert_eq!(super::detect_install_env(), Environment::Codex);
    }

    /// Use an explicit ClaudeCode environment for deterministic test paths.
    /// Environment::detect() is non-deterministic in CI (depends on env vars).
    fn test_config() -> SkillConfig {
        SkillConfig::with_environment(
            "agent-doc",
            content_for_env(Environment::ClaudeCode),
            VERSION,
            Environment::ClaudeCode,
        )
    }

    /// Resolve expected skill path using the explicit test environment.
    fn expected_path(dir: &std::path::Path) -> std::path::PathBuf {
        test_config().skill_path(Some(dir))
    }

    fn install_test(root: Option<&std::path::Path>) -> anyhow::Result<()> {
        test_config().install(root)
    }

    fn line_count(content: &str) -> usize {
        content.lines().count()
    }

    fn assert_codex_mcp_config(config: &toml::Value, root: &std::path::Path) {
        let server = &config["mcp_servers"][CODEX_MCP_SERVER_NAME];
        assert_eq!(server["command"].as_str(), Some(CODEX_MCP_COMMAND));
        let args: Vec<&str> = server["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap())
            .collect();
        let canonical_root = std::fs::canonicalize(root).unwrap();
        assert_eq!(
            args,
            vec![
                "mcp",
                "serve",
                "--project-root",
                canonical_root.to_str().unwrap()
            ]
        );
    }

    #[test]
    fn install_creates_file() {
        let dir = tempfile::tempdir().unwrap();

        install_test(Some(dir.path())).unwrap();

        let path = expected_path(dir.path());
        assert!(path.exists(), "skill not found at {}", path.display());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, content_for_env(Environment::ClaudeCode));
        assert!(content.contains(&format!("agent-doc-version: \"{VERSION}\"")));
    }

    #[test]
    fn install_idempotent() {
        let dir = tempfile::tempdir().unwrap();

        install_test(Some(dir.path())).unwrap();
        install_test(Some(dir.path())).unwrap();

        let path = expected_path(dir.path());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, content_for_env(Environment::ClaudeCode));
    }

    #[test]
    fn check_not_installed() {
        let dir = tempfile::tempdir().unwrap();

        let path = expected_path(dir.path());
        assert!(!path.exists());
    }

    #[test]
    fn install_creates_runbooks_claude() {
        let dir = tempfile::tempdir().unwrap();

        install_test(Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        let runbook_path = dir
            .path()
            .join(".claude/skills/agent-doc/runbooks/compact-exchange.md");
        assert!(
            runbook_path.exists(),
            "runbook not found at {}",
            runbook_path.display()
        );
        let content = std::fs::read_to_string(&runbook_path).unwrap();
        assert!(content.contains("Compact Exchange"));
        assert!(content.contains("agent-doc compact <FILE> --component exchange --commit"));
        assert!(content.contains("VCS refresh signal"));
    }

    #[test]
    fn install_creates_runbooks_codex() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let runbook_path = dir.path().join(".codex/runbooks/compact-exchange.md");
        assert!(
            runbook_path.exists(),
            "codex runbook not found at {}",
            runbook_path.display()
        );
        let content = std::fs::read_to_string(&runbook_path).unwrap();
        assert!(content.contains("agent-doc compact <FILE> --component exchange --commit"));
        assert!(content.contains("VCS refresh signal"));
    }

    #[test]
    fn install_runbooks_reaps_stale_files() {
        let dir = tempfile::tempdir().unwrap();
        let runbooks_dir = dir.path().join(".claude/skills/agent-doc/runbooks");
        std::fs::create_dir_all(&runbooks_dir).unwrap();

        let sentinel = runbooks_dir.join("sentinel-stale.md");
        std::fs::write(&sentinel, "# stale runbook removed in a later release\n").unwrap();
        let plugin_install = runbooks_dir.join("plugin-install.md");
        std::fs::write(&plugin_install, "# stale runbook\n").unwrap();
        let kept_non_md = runbooks_dir.join("README.txt");
        std::fs::write(&kept_non_md, "non-md file should survive\n").unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        assert!(
            !sentinel.exists(),
            "sentinel stale runbook should be reaped: {}",
            sentinel.display()
        );
        assert!(
            !plugin_install.exists(),
            "stale plugin-install.md should be reaped"
        );
        assert!(
            kept_non_md.exists(),
            "non-md files in runbooks dir must not be reaped"
        );

        let installed: std::collections::HashSet<String> = std::fs::read_dir(&runbooks_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("md"))
            .filter_map(|entry| entry.file_name().to_str().map(|s| s.to_string()))
            .collect();
        let canonical: std::collections::HashSet<String> = super::BUNDLED_RUNBOOKS
            .iter()
            .map(|(name, _)| name.to_string())
            .collect();
        assert_eq!(
            installed, canonical,
            "post-install runbook set must match canonical embedded set"
        );
    }

    #[test]
    fn install_runbooks_is_no_op_on_clean_tree() {
        let dir = tempfile::tempdir().unwrap();
        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        let runbooks_dir = dir.path().join(".claude/skills/agent-doc/runbooks");
        let first: Vec<(String, std::time::SystemTime)> = std::fs::read_dir(&runbooks_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let mtime = entry.metadata().ok()?.modified().ok()?;
                let name = entry.file_name().to_str()?.to_string();
                Some((name, mtime))
            })
            .collect();

        std::thread::sleep(std::time::Duration::from_millis(20));
        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        for (name, mtime_before) in &first {
            let path = runbooks_dir.join(name);
            let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
            assert_eq!(
                *mtime_before, mtime_after,
                "second install must not rewrite unchanged runbook {}",
                name
            );
        }
    }

    #[test]
    fn install_runbooks_reaps_for_codex_and_opencode() {
        let dir = tempfile::tempdir().unwrap();
        for (env, rel) in [
            (Environment::Codex, ".codex/runbooks"),
            (Environment::OpenCode, ".opencode/skills/agent-doc/runbooks"),
        ] {
            let runbooks_dir = dir.path().join(rel);
            std::fs::create_dir_all(&runbooks_dir).unwrap();
            let sentinel = runbooks_dir.join("legacy-runbook.md");
            std::fs::write(&sentinel, "# legacy\n").unwrap();

            super::install_runbooks_for(env, Some(dir.path())).unwrap();

            assert!(
                !sentinel.exists(),
                "stale runbook under {} should be reaped",
                rel
            );
            assert!(runbooks_dir.join("commit.md").exists());
        }
    }

    #[test]
    fn installed_harness_runbooks_include_commit_invariant() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/commit.md"),
        )
        .unwrap();
        let codex = std::fs::read_to_string(dir.path().join(".codex/runbooks/commit.md")).unwrap();
        let opencode = std::fs::read_to_string(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/commit.md"),
        )
        .unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("Every appended `agent-doc` response must be committed"));
            assert!(content.contains("agent-doc finalize <FILE>"));
            assert!(content.contains("agent-doc write --commit <FILE>"));
            assert!(content.contains("agent-doc session-check <FILE>"));
            assert!(content.contains("bare `agent-doc write`"));
        }
    }

    #[test]
    fn installed_harness_pending_ops_runbooks_cover_plan_backed_items() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/pending-ops.md"),
        )
        .unwrap();
        let codex =
            std::fs::read_to_string(dir.path().join(".codex/runbooks/pending-ops.md")).unwrap();
        let opencode = std::fs::read_to_string(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/pending-ops.md"),
        )
        .unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("create the plan file"));
            assert!(content.contains("include that exact plan"));
            assert!(content.contains("file path in the item text"));
            assert!(content.contains("plan-spec2-rollout.md"));
            assert!(content.contains("one flush-left backlog item per"));
            assert!(content.contains("queue entries and closeouts should target"));
        }
    }

    #[test]
    fn installed_harness_runbooks_share_manual_repair_rule() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/harness-invocation.md"),
        )
        .unwrap();
        let codex =
            std::fs::read_to_string(dir.path().join(".codex/runbooks/harness-invocation.md"))
                .unwrap();
        let opencode = std::fs::read_to_string(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/harness-invocation.md"),
        )
        .unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("## Harness-Native Entrypoints"));
            assert!(content.contains("executable workflow entry"));
            assert!(content.contains(
                "Do **not** end a normal harness-native `agent-doc` turn with \"not committed\""
            ));
            assert!(content.contains("## Manual Repair Default"));
            assert!(content.contains("For **Claude Code**, **Codex**, and **OpenCode**"));
            assert!(content.contains("agent-doc write --commit <FILE>"));
            assert!(content.contains("bare `agent-doc write`"));
        }
        assert!(codex.contains("agent-doc session-check <FILE>"));
        assert!(codex.contains("Do **not** report success or stop"));
        assert!(opencode.contains("## OpenCode"));
        assert!(opencode.contains("Write-back"));
    }

    #[test]
    fn install_for_codex_writes_codex_specific_content() {
        let dir = tempfile::tempdir().unwrap();

        super::config_for_env(Environment::Codex)
            .install_for(Environment::Codex, Some(dir.path()))
            .unwrap();

        let path = dir.path().join(".codex/AGENTS.md");
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("Codex CLI will reject it"));
        assert!(content.contains("agent-doc skill install --harness codex --reload restart"));
        assert!(content.contains("SKILL_RELOAD=restart"));
        assert!(content.contains("stale instruction drift"));
        assert!(content.contains("continue this turn"));
        assert!(content.contains(&format!("agent-doc-version: \"{VERSION}\"")));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
    }

    #[test]
    fn install_for_opencode_writes_opencode_specific_content() {
        let dir = tempfile::tempdir().unwrap();

        super::config_for_env(Environment::OpenCode)
            .install_for(Environment::OpenCode, Some(dir.path()))
            .unwrap();

        let path = dir.path().join(".opencode/skills/agent-doc/SKILL.md");
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(content.contains("Interactive markdown session for OpenCode"));
        assert!(content.contains("/agent-doc <FILE>"));
        assert!(content.contains("agent-doc skill install --harness opencode"));
        assert!(content.contains("installed OpenCode skill"));
        assert!(content.contains("agent-doc finalize <FILE>"));
        assert!(content.contains("Use `agent-doc write --commit <FILE>`"));
        assert!(content.contains(&format!("agent-doc-version: \"{VERSION}\"")));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
    }

    #[test]
    fn install_for_codex_refreshes_managed_root_agents_mirror() {
        let dir = tempfile::tempdir().unwrap();
        let stale_root = super::content_for_env(Environment::Generic).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        std::fs::write(dir.path().join("AGENTS.md"), stale_root).unwrap();

        super::config_for_env(Environment::Codex)
            .install_for(Environment::Codex, Some(dir.path()))
            .unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();
        super::sync_managed_root_agents(Some(dir.path())).unwrap();

        let root = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        let codex = std::fs::read_to_string(dir.path().join(".codex/AGENTS.md")).unwrap();
        assert_eq!(root, super::content_for_env(Environment::Generic));
        assert!(codex.contains("agent-doc skill install --harness codex --reload restart"));
    }

    #[test]
    fn install_for_codex_preserves_custom_root_agents() {
        let dir = tempfile::tempdir().unwrap();
        let custom = "# Custom Project Instructions\n\nKeep this file untouched.\n";
        std::fs::write(dir.path().join("AGENTS.md"), custom).unwrap();

        super::config_for_env(Environment::Codex)
            .install_for(Environment::Codex, Some(dir.path()))
            .unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();
        super::sync_managed_root_agents(Some(dir.path())).unwrap();

        let root = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert_eq!(root, custom);
    }

    #[test]
    fn audit_managed_instruction_surfaces_rejects_stale_root_agents_mirror() {
        let dir = tempfile::tempdir().unwrap();
        let stale_root = super::content_for_env(Environment::Generic).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        std::fs::write(dir.path().join("AGENTS.md"), stale_root).unwrap();

        let err = super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("managed agent-doc instruction surface is stale"));
        assert!(message.contains("AGENTS.md"));
        assert!(message.contains("agent-doc skill install --all"));
    }

    #[test]
    fn audit_managed_instruction_surfaces_allows_tsift_navigation_block() {
        let dir = tempfile::tempdir().unwrap();
        let mut root = super::content_for_env(Environment::Generic);
        root.push_str(
            "\n<!-- tsift:code-navigation v=0.1.42 -->\n## Code Navigation\n\nRun `tsift status`.\n<!-- /tsift:code-navigation -->\n",
        );
        std::fs::write(dir.path().join("AGENTS.md"), root).unwrap();

        super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap();
    }

    #[test]
    fn audit_managed_instruction_surfaces_treats_tsift_root_mirror_as_customized() {
        let dir = tempfile::tempdir().unwrap();
        let mut root = super::content_for_env(Environment::Generic).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        root.push_str(
            "\n<!-- tsift:code-navigation v=0.1.42 -->\n## Code Navigation\n\nRun `tsift status`.\n<!-- /tsift:code-navigation -->\n",
        );
        std::fs::write(dir.path().join("AGENTS.md"), root).unwrap();

        super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap();
    }

    #[test]
    fn audit_managed_instruction_surfaces_rejects_stale_codex_agents() {
        let dir = tempfile::tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let stale_codex = super::content_for_env(Environment::Codex).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        std::fs::write(codex_dir.join("AGENTS.md"), stale_codex).unwrap();

        let err = super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("managed agent-doc instruction surface is stale"));
        assert!(message.contains(".codex"));
    }

    #[test]
    fn audit_managed_instruction_surfaces_preserves_custom_root_agents() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "# Custom Project Instructions\n\nKeep this file untouched.\n",
        )
        .unwrap();

        super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap();
    }

    #[test]
    fn install_for_codex_writes_hooks_json_and_feature_flag() {
        let dir = tempfile::tempdir().unwrap();

        super::config_for_env(Environment::Codex)
            .install_for(Environment::Codex, Some(dir.path()))
            .unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();

        let hooks_path = dir.path().join(".codex/hooks.json");
        let config_path = dir.path().join(".codex/config.toml");
        assert!(hooks_path.exists(), "missing {}", hooks_path.display());
        assert!(config_path.exists(), "missing {}", config_path.display());

        let hooks: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
        let stop_hooks = hooks["hooks"]["Stop"][0]["hooks"].as_array().unwrap();
        assert!(stop_hooks.iter().any(|hook| {
            hook["command"].as_str() == Some(CODEX_STOP_COMMAND)
                && hook["type"].as_str() == Some("command")
        }));
        let submit_hooks = hooks["hooks"]["UserPromptSubmit"][0]["hooks"]
            .as_array()
            .unwrap();
        assert!(
            submit_hooks
                .iter()
                .any(|hook| hook["command"].as_str() == Some(CODEX_USER_PROMPT_COMMAND))
        );

        let config: toml::Value =
            toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(config["features"]["hooks"].as_bool(), Some(true));
        assert!(config["features"].get("codex_hooks").is_none());
        assert_codex_mcp_config(&config, dir.path());
    }

    #[test]
    fn install_turn_status_hooks_all_harnesses_idempotent_and_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        // A pre-existing unrelated Claude hook (autoclaim SessionStart) must survive.
        let claude_settings = dir.path().join(".claude/settings.json");
        std::fs::create_dir_all(claude_settings.parent().unwrap()).unwrap();
        std::fs::write(
            &claude_settings,
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"agent-doc autoclaim"}]}]}}"#,
        )
        .unwrap();

        super::install_turn_status_hooks(Some(dir.path()), false, false).unwrap();
        super::install_turn_status_hooks(Some(dir.path()), false, false).unwrap(); // idempotent re-run

        let cmds = |v: &serde_json::Value, event: &str| -> Vec<String> {
            v["hooks"][event]
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .flat_map(|e| e["hooks"].as_array().cloned().unwrap_or_default())
                        .filter_map(|h| h["command"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };

        // Claude: start + (Stop, SessionStart) end hooks; autoclaim preserved; idempotent.
        let claude: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_settings).unwrap()).unwrap();
        assert!(
            cmds(&claude, "UserPromptSubmit").contains(&TURN_STATUS_ACTIVE_COMMAND.to_string())
        );
        assert!(cmds(&claude, "Stop").contains(&TURN_STATUS_IDLE_COMMAND.to_string()));
        let ss = cmds(&claude, "SessionStart");
        assert!(
            ss.contains(&"agent-doc autoclaim".to_string()),
            "autoclaim preserved: {ss:?}"
        );
        assert!(ss.contains(&TURN_STATUS_IDLE_COMMAND.to_string()));
        assert_eq!(
            ss.iter()
                .filter(|c| c.as_str() == TURN_STATUS_IDLE_COMMAND)
                .count(),
            1,
            "idempotent: {ss:?}"
        );

        // Codex: start + Stop end (no SessionStart event for Codex).
        let codex: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
        )
        .unwrap();
        assert!(cmds(&codex, "UserPromptSubmit").contains(&TURN_STATUS_ACTIVE_COMMAND.to_string()));
        assert!(cmds(&codex, "Stop").contains(&TURN_STATUS_IDLE_COMMAND.to_string()));

        // OpenCode: plugin file wired to both turn-status commands via chat.message + session.idle.
        let oc =
            std::fs::read_to_string(dir.path().join(".opencode/plugin/agent-doc-turn-status.js"))
                .unwrap();
        assert!(oc.contains("turn-status active"), "{oc}");
        assert!(oc.contains("session.idle"), "{oc}");
        assert!(oc.contains("turn-status idle"), "{oc}");
    }

    #[test]
    fn install_for_codex_preserves_existing_hook_and_config_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::write(
            dir.path().join(".codex/hooks.json"),
            serde_json::json!({
                "hooks": {
                    "Stop": [
                        {
                            "hooks": [
                                { "type": "command", "command": "echo existing-stop" }
                            ]
                        }
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".codex/config.toml"),
            "[sandbox]\ndefault = \"workspace-write\"\n",
        )
        .unwrap();

        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();

        let hooks: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
        )
        .unwrap();
        let stop_hooks = hooks["hooks"]["Stop"].as_array().unwrap();
        assert!(stop_hooks.iter().any(|entry| {
            entry["hooks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hook| hook["command"].as_str() == Some("echo existing-stop"))
        }));
        assert!(stop_hooks.iter().any(|entry| {
            entry["hooks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hook| hook["command"].as_str() == Some(CODEX_STOP_COMMAND))
        }));

        let config: toml::Value = toml::from_str(
            &std::fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            config["sandbox"]["default"].as_str(),
            Some("workspace-write")
        );
        assert_eq!(config["features"]["hooks"].as_bool(), Some(true));
        assert!(config["features"].get("codex_hooks").is_none());
        assert_codex_mcp_config(&config, dir.path());
    }

    #[test]
    fn install_runbooks_all_creates_for_each_env() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_all(Some(dir.path())).unwrap();

        assert!(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/compact-exchange.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".codex/runbooks/compact-exchange.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/compact-exchange.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".cursor/rules/runbooks/compact-exchange.md")
                .exists()
        );
    }

    #[test]
    fn bundled_skill_contains_harness_preamble() {
        assert!(SKILL_TEMPLATE.contains("Harness Compatibility"));
        assert!(SKILL_TEMPLATE.contains("harness-invocation.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/commit.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/command-synonyms.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/compound-task-steering.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/planning-dispatch.md"));
    }

    #[test]
    fn bundled_skill_contains_pending_capture_rules() {
        assert!(SKILL_TEMPLATE.contains("Pending capture rule"));
        assert!(SKILL_TEMPLATE.contains("[recommended]"));
        assert!(SKILL_TEMPLATE.contains("beginning of `agent:backlog`"));
        assert!(SKILL_TEMPLATE.contains("adjacent to its predecessor"));
        assert!(SKILL_TEMPLATE.contains("multi-phase implementation work"));
        assert!(SKILL_TEMPLATE.contains("prefer one backlog ID per actionable phase"));
        assert!(SKILL_TEMPLATE.contains("`do #id` closeout rule"));
        assert!(SKILL_TEMPLATE.contains("--done <id>"));
        assert!(SKILL_TEMPLATE.contains("pending_done_guard"));
    }

    #[test]
    fn bundled_skill_treats_imperative_document_edits_as_executable_work() {
        assert!(SKILL_TEMPLATE.contains("Imperative edits are executable directives"));
        assert!(
            SKILL_TEMPLATE.contains("Do not require the same instruction to be repeated in chat")
        );
        assert!(
            SKILL_TEMPLATE
                .contains("MCP auth / OAuth steps are sub-steps, not closeout boundaries")
        );
        assert!(SKILL_TEMPLATE.contains(
            "Do not keep appending \"starting/continuing\" status prose while the requested work remains undone"
        ));
    }

    #[test]
    fn bundled_skill_treats_harness_native_entrypoints_as_binary_owned_cycles() {
        assert!(SKILL_TEMPLATE.contains(
            "Harness-native `agent-doc` entrypoints start the binary-owned response cycle"
        ));
        assert!(SKILL_TEMPLATE.contains("executable workflow start"));
        assert!(SKILL_TEMPLATE.contains("generic document-editing request"));
        assert!(
            SKILL_TEMPLATE
                .contains("Do not manually patch the final assistant response into the document")
        );
        assert!(SKILL_TEMPLATE.contains("agent-doc write --commit <FILE>` completes"));
        assert!(
            SKILL_TEMPLATE
                .contains("stage and commit only the intended non-session repo files first")
        );
        assert!(SKILL_TEMPLATE.contains("code-enforced-directives.md"));
    }

    #[test]
    fn bundled_skill_contains_manual_repair_write_commit_rule() {
        assert!(SKILL_TEMPLATE.contains("Manual repair / missed patchback rule (all harnesses)"));
        assert!(
            SKILL_TEMPLATE
                .contains("do **not** patch the assistant response directly into the file")
        );
        assert!(SKILL_TEMPLATE.contains("Use `agent-doc write --commit <FILE>`"));
        assert!(SKILL_TEMPLATE.contains("bare `agent-doc write`"));
    }

    #[test]
    fn bundled_skill_contains_finalize_commit_invariant() {
        assert!(SKILL_TEMPLATE.contains("agent-doc finalize <FILE>"));
        assert!(
            SKILL_TEMPLATE
                .contains("unless the user explicitly told you to leave the response uncommitted")
        );
        assert!(SKILL_TEMPLATE.contains("requires the cycle to reach `committed`"));
        assert!(SKILL_TEMPLATE.contains("agent-doc session-check <FILE>"));
        assert!(SKILL_TEMPLATE.contains("final document-mutation boundary for the cycle"));
        assert!(SKILL_TEMPLATE.contains(
            "After `finalize` / `write --commit`, do not start more long-running task work"
        ));
    }

    #[test]
    fn bundled_skill_compact_entry_uses_commit_closeout() {
        assert!(SKILL_TEMPLATE.contains("agent-doc compact <FILE> --commit"));
        assert!(SKILL_TEMPLATE.contains("compact exchange <FILE>"));
    }

    #[test]
    fn bundled_skill_contains_model_short_name_attribution_rule() {
        assert!(SKILL_TEMPLATE.contains("### Re: topic — gpt-5"));
        assert!(SKILL_TEMPLATE.contains("### Re: topic — opus-4-6"));
        assert!(SKILL_TEMPLATE.contains("Never use the harness label (`codex`, `claude`)"));
    }

    #[test]
    fn bundled_skill_requires_oldest_first_exchange_tail_reconciliation() {
        assert!(SKILL_TEMPLATE.contains("Do not stop at the newest question"));
        assert!(SKILL_TEMPLATE.contains("each unresolved prompt in that tail"));
    }

    #[test]
    fn codex_content_uses_plain_text_invocation() {
        let content = super::content_for_env(Environment::Codex);

        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("Codex CLI will reject it"));
        assert!(content.contains("agent-doc skill install --harness codex --reload restart"));
        assert!(content.contains("SKILL_RELOAD=restart"));
        assert!(content.contains("stale instruction drift"));
        assert!(content.contains("continue this turn"));
        assert!(content.contains("Use `agent-doc write --commit <FILE>`"));
        assert!(content.contains("agent-doc session-check <FILE>"));
        assert!(content.contains("binary-owned response cycle"));
        assert!(content.contains("generic document-editing request"));
        assert!(content.contains("final document-mutation boundary for the cycle"));
        assert!(content.contains("do not start more long-running task work for that same turn"));
        assert!(content.contains(".codex/hooks.json"));
        assert!(content.contains(".codex/config.toml"));
        assert!(content.contains("fail-closed backstop"));
        assert!(content.contains("MCP auth / OAuth steps are sub-steps"));
        assert!(content.contains("Project-scoped remote hosts"));
        assert!(content.contains("globally approved SSH commands"));
        assert!(content.contains("project-local `.agent-doc/config.toml`"));
        assert!(content.contains("### Re: topic — gpt-5"));
        assert!(content.contains("Never use the harness label (`codex`, `claude`)"));
        assert!(content.contains("Imperative edits are executable directives"));
        assert!(content.contains("Do not require the same instruction to be repeated in chat"));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
    }

    #[test]
    fn opencode_content_uses_slash_command_invocation() {
        let content = super::content_for_env(Environment::OpenCode);

        assert!(content.contains("Interactive markdown session for OpenCode"));
        assert!(content.contains("/agent-doc <FILE>"));
        assert!(content.contains("agent-doc skill install --harness opencode"));
        assert!(content.contains("installed OpenCode skill"));
        assert!(content.contains("Use `agent-doc write --commit <FILE>`"));
        assert!(content.contains("binary-owned response cycle"));
        assert!(content.contains("final document-mutation boundary for the cycle"));
        assert!(content.contains("MCP auth / OAuth steps are sub-steps"));
        assert!(content.contains("Imperative edits are executable directives"));
        assert!(content.contains("Do not require the same instruction to be repeated in chat"));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
        assert!(!content.contains("Codex CLI will reject it"));
        assert!(!content.contains("In OpenCode, invoke agent-doc by writing"));
    }

    #[test]
    fn install_for_opencode_creates_command_file() {
        let dir = tempfile::tempdir().unwrap();

        super::install_opencode_command_file(Some(dir.path())).unwrap();

        let path = dir.path().join(".opencode/commands/agent-doc.md");
        assert!(path.exists(), "command file should exist at {path:?}");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("agent-doc $ARGUMENTS"));
        assert!(content.contains("description:"));
    }

    #[test]
    fn install_for_opencode_command_file_idempotent() {
        let dir = tempfile::tempdir().unwrap();

        super::install_opencode_command_file(Some(dir.path())).unwrap();
        let first =
            std::fs::read_to_string(dir.path().join(".opencode/commands/agent-doc.md")).unwrap();

        super::install_opencode_command_file(Some(dir.path())).unwrap();
        let second =
            std::fs::read_to_string(dir.path().join(".opencode/commands/agent-doc.md")).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn install_env_artifacts_creates_opencode_command() {
        let dir = tempfile::tempdir().unwrap();

        super::install_env_artifacts(Environment::OpenCode, Some(dir.path())).unwrap();

        assert!(dir.path().join(".opencode/commands/agent-doc.md").exists());
    }

    #[test]
    fn claude_content_keeps_slash_invocation() {
        let content = super::content_for_env(Environment::ClaudeCode);

        assert!(content.contains("/agent-doc <FILE>"));
        assert!(content.contains("TRIGGER: user invokes /agent-doc <file>"));
        assert!(content.contains("agent-doc skill install --harness claude --reload restart"));
        assert!(content.contains("SKILL_RELOAD=restart"));
        assert!(content.contains("agent_doc_auto_compact"));
        assert!(content.contains("Use `--reload compact`"));
        assert!(content.contains("stale instruction drift"));
        assert!(content.contains("continue this turn"));
        assert!(content.contains("binary-owned response cycle"));
        assert!(content.contains("Do not manually patch the final assistant response"));
    }

    #[test]
    fn generic_content_handles_stale_duplicate_instructions() {
        let content = super::content_for_env(Environment::Generic);

        assert!(content.contains("Claude Code: `/agent-doc <FILE>`"));
        assert!(content.contains("Codex: `agent-doc <FILE>`"));
        assert!(content.contains("OpenCode: `/agent-doc <FILE>`"));
        assert!(content.contains("agent-doc skill install --harness claude --reload restart"));
        assert!(content.contains("agent_doc_auto_compact"));
        assert!(content.contains("agent-doc skill install --harness codex --reload restart"));
        assert!(content.contains("agent-doc skill install --harness opencode"));
        assert!(content.contains("stale duplicate instructions"));
        assert!(content.contains("continue with the task"));
    }

    #[test]
    fn generated_harness_content_shares_hot_path_outside_invocation() {
        let claude = super::content_for_env(Environment::ClaudeCode);
        let codex = super::content_for_env(Environment::Codex);
        let opencode = super::content_for_env(Environment::OpenCode);

        let claude_shared = super::remove_markdown_section(&claude, "## Invocation");
        let codex_shared = super::remove_markdown_section(&codex, "## Invocation");
        let opencode_shared = super::remove_markdown_section(&opencode, "## Invocation");
        let claude_shared = super::remove_markdown_section(
            &claude_shared,
            "## Auto-loop while queue is active (Claude Code)",
        );
        let claude_shared = claude_shared.replace(CLAUDE_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");
        let codex_shared = codex_shared.replace(CODEX_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");
        let opencode_shared = opencode_shared.replace(OPENCODE_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");

        assert_eq!(
            claude_shared.replace(CLAUDE_DESCRIPTION, "<DESC>"),
            codex_shared.replace(CODEX_DESCRIPTION, "<DESC>")
        );
        assert_eq!(
            claude_shared.replace(CLAUDE_DESCRIPTION, "<DESC>"),
            opencode_shared.replace(OPENCODE_DESCRIPTION, "<DESC>")
        );
    }

    #[test]
    fn bundled_runbooks_include_harness_invocation() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "harness-invocation.md"),
            "harness-invocation.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_commit_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "commit.md"),
            "commit.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_command_synonyms_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "command-synonyms.md"),
            "command-synonyms.md should be in BUNDLED_RUNBOOKS"
        );
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "compound-task-steering.md"),
            "compound-task-steering.md should be in BUNDLED_RUNBOOKS"
        );
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "planning-dispatch.md"),
            "planning-dispatch.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_split_spec_files_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "split-spec-files.md"),
            "split-spec-files.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_baseline_drift_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "baseline-drift.md"),
            "baseline-drift.md should be in BUNDLED_RUNBOOKS"
        );
        assert!(
            SKILL_TEMPLATE.contains("baseline-drift"),
            "SKILL.md should list baseline-drift in the runbook catalog"
        );
    }

    #[test]
    fn harness_invocation_runbook_content() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "harness-invocation.md")
            .expect("harness-invocation.md not found");
        assert!(content.contains("## Directive Semantics"));
        assert!(content.contains(
            "Imperative user edits inside an `agent-doc` session document are executable directives"
        ));
        assert!(content.contains("Do **not** emit status-only progress prose while doing neither"));
        assert!(content.contains("## Manual Repair Default"));
        assert!(content.contains("For **Claude Code**, **Codex**, and **OpenCode**"));
        assert!(content.contains("Claude Code"));
        assert!(content.contains("Codex"));
        assert!(content.contains("OpenCode"));
        assert!(content.contains("Harness Detection"));
        assert!(content.contains("Response Header Attribution"));
        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("bare `agent-doc write`"));
        assert!(content.contains("### Re: topic — gpt-5"));
        assert!(content.contains("### Re: topic — opus-4-6"));
        assert!(content.contains("### Re: topic — codex"));
        assert!(content.contains("### Re: topic — claude"));
        assert!(content.contains("Manual repair / missed patchback"));
        assert!(content.contains("agent-doc write --commit <FILE>"));
        assert!(content.contains("agent-doc session-check <FILE>"));
        assert!(
            content.contains(
                "Do not patch the document early and then keep working for the same turn"
            )
        );
        assert!(
            content.contains("the manual repo commit must exclude the active session document")
        );
        assert!(content.contains("Resolve the intended non-session path set first"));
        assert!(content.contains("stop immediately on any stage failure"));
        assert!(content.contains("verify the staged diff still matches the intended set"));
        assert!(content.contains(".codex/hooks.json"));
        assert!(content.contains("UserPromptSubmit"));
        assert!(content.contains("agent-doc hook codex-stop"));
    }

    #[test]
    fn harness_invocation_runbook_opencode_section_requires_session_check() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "harness-invocation.md")
            .expect("harness-invocation.md not found");

        let opencode_start = content
            .find("## OpenCode")
            .expect("## OpenCode section not found");
        let next_section = content[opencode_start + 1..]
            .find("\n## ")
            .map(|i| opencode_start + 1 + i)
            .unwrap_or(content.len());
        let opencode_section = &content[opencode_start..next_section];

        assert!(
            opencode_section.contains("agent-doc session-check"),
            "OpenCode section must require session-check after finalize: {opencode_section}"
        );
        assert!(
            opencode_section.contains("Fail closed"),
            "OpenCode section must include fail-closed guard: {opencode_section}"
        );
        assert!(
            opencode_section.contains("response text visible in the console but absent"),
            "OpenCode section must name the CLI-only-output anti-pattern: {opencode_section}"
        );
    }

    #[test]
    fn commit_runbook_content() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "commit.md")
            .expect("commit.md not found");
        assert!(content.contains("Every appended `agent-doc` response must be committed"));
        assert!(content.contains("agent-doc finalize <FILE>"));
        assert!(content.contains("agent-doc write --commit <FILE>"));
        assert!(content.contains("bare `agent-doc write`"));
        assert!(content.contains("keep the active session document out of that manual git commit"));
        assert!(content.contains("Resolve the exact intended non-session path set first"));
        assert!(content.contains("verify `git diff --cached --name-only`"));
        assert!(content.contains(
            "Do **not** continue to `git commit` after a narrowed `git add` / stage failure"
        ));
        assert!(content.contains(
            "Do **not** stage the active session document into an ordinary repo `git commit`"
        ));
    }

    #[test]
    fn compound_task_runbook_defers_session_doc_commit_until_finalize() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "compound-task-steering.md")
            .expect("compound-task-steering.md not found");
        assert!(content.contains(
            "run `agent-doc finalize` / `write --commit` so the session document gets its own binary-owned closeout commit"
        ));
        assert!(content.contains(
            "validate and commit only the intended non-session repo files, finalize the session document, then push"
        ));
        assert!(content.contains("stop on any stage failure"));
    }

    #[test]
    fn compact_exchange_runbook_content_uses_binary_owned_closeout() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "compact-exchange.md")
            .expect("compact-exchange.md not found");
        assert!(content.contains("agent-doc compact <FILE> --component exchange --commit"));
        assert!(content.contains("binary-owned `agent-doc commit` path"));
        assert!(content.contains("VCS refresh signal"));
        assert!(content.contains("agent:backlog"));
        assert!(content.contains("agent:queue"));
        assert!(content.contains("agent:icebox"));
        assert!(content.contains("prompt_presets"));
    }

    #[test]
    fn pending_ops_runbook_content_contains_pending_capture_rules() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "pending-ops.md")
            .expect("pending-ops.md not found");
        assert!(content.contains("beginning of the list"));
        assert!(content.contains("[recommended]"));
        assert!(content.contains("preserve the order you presented them in"));
        assert!(content.contains("follow-on step from an ordered batch"));
        assert!(content.contains("--pending-reorder gkke,9pw9,step3"));
        assert!(content.contains("Existing `do #id` work that completed this cycle"));
        assert!(content.contains("--done <id>"));
    }

    #[test]
    fn pending_ops_runbook_content_contains_custom_id_docs() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "pending-ops.md")
            .expect("pending-ops.md not found");
        assert!(content.contains("id=<custom>"));
        assert!(content.contains("id=#spec1"));
        assert!(content.contains("ASCII alphanumeric"));
    }

    #[test]
    fn pending_ops_runbook_content_covers_plan_backed_items() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "pending-ops.md")
            .expect("pending-ops.md not found");
        assert!(content.contains("create the plan file"));
        assert!(content.contains("include that exact plan"));
        assert!(content.contains("file path in the item text"));
        assert!(content.contains("plan-spec2-rollout.md"));
        assert!(content.contains("one flush-left backlog item per"));
        assert!(content.contains("queue entries and closeouts should target"));
    }

    #[test]
    fn command_synonyms_runbook_content_covers_orchestrate_modes() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "command-synonyms.md")
            .expect("command-synonyms.md not found");
        assert!(content.contains("agent-doc orchestrate <FILE> --mode sequential"));
        assert!(content.contains("agent-doc orchestrate <FILE> --mode parallel"));
        assert!(content.contains("agent-doc orchestrate <FILE> --mode dag"));
        assert!(content.contains("Run these in order"));
        assert!(content.contains("fan out"));
        assert!(content.contains("after X do Y"));
        assert!(content.contains("default to `--mode sequential`"));
    }

    #[test]
    fn compound_task_steering_runbook_covers_explicit_normalization() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "compound-task-steering.md")
            .expect("compound-task-steering.md not found");
        assert!(content.contains("do #ntoc. Add to today's news. commit + push"));
        assert!(content.contains("agent-doc orchestrate <FILE> --mode sequential"));
        assert!(content.contains("Do not invent binary-owned prose grammar"));
        assert!(content.contains("commit + push"));
        assert!(content.contains("Preserve it. Do not rewrite explicit orchestration"));
    }

    #[test]
    fn planning_dispatch_runbook_content_covers_plan_contract() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "planning-dispatch.md")
            .expect("planning-dispatch.md not found");
        assert!(content.contains("agent-doc plan <FILE>"));
        assert!(content.contains("prompt_targets"));
        assert!(content.contains("repo_actions"));
        assert!(content.contains("required_commands"));
        assert!(content.contains("pending_mutations"));
        assert!(content.contains("handoff"));
        assert!(content.contains("blockers"));
        assert!(content.contains("handoff=orchestrate"));
    }

    #[test]
    fn install_overwrites_outdated() {
        let dir = tempfile::tempdir().unwrap();

        let path = expected_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "old content").unwrap();

        install_test(Some(dir.path())).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, content_for_env(Environment::ClaudeCode));
    }

    #[test]
    fn installed_harness_skill_content_shares_completion_boundary_text() {
        let dir = tempfile::tempdir().unwrap();

        super::config_for_env(Environment::ClaudeCode)
            .install_for(Environment::ClaudeCode, Some(dir.path()))
            .unwrap();
        super::config_for_env(Environment::OpenCode)
            .install_for(Environment::OpenCode, Some(dir.path()))
            .unwrap();
        super::config_for_env(Environment::Codex)
            .install_for(Environment::Codex, Some(dir.path()))
            .unwrap();

        let claude =
            std::fs::read_to_string(dir.path().join(".claude/skills/agent-doc/SKILL.md")).unwrap();
        let opencode =
            std::fs::read_to_string(dir.path().join(".opencode/skills/agent-doc/SKILL.md"))
                .unwrap();
        let codex = std::fs::read_to_string(dir.path().join(".codex/AGENTS.md")).unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("agent-doc finalize <FILE>"));
            assert!(content.contains("Use `agent-doc write --commit <FILE>`"));
            assert!(content.contains("requires the cycle to reach `committed`"));
            assert!(content.contains("agent-doc session-check <FILE>"));
            assert!(content.contains("final document-mutation boundary for the cycle"));
            assert!(content.contains("Imperative edits are executable directives"));
            assert!(content.contains("Never use the harness label (`codex`, `claude`)"));
            assert!(content.contains("Agent harnesses own full-suite verification"));
            assert!(content.contains("Do not waive red suites as \"unrelated\" or \"flaky\""));
            assert!(content.contains("Do not rely on a pre-commit hook"));
        }
        assert!(claude.contains("final document-mutation boundary for the cycle"));
        assert!(codex.contains(".codex/hooks.json"));
        assert!(codex.contains("fail-closed backstop"));
    }
