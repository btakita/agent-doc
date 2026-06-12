    use super::*;
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn write_fake_codex_script(script: &str) -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("fake-codex.sh");
        fs::write(&path, script).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        (dir, path.to_string_lossy().into_owned())
    }

    fn write_fake_ssh_script(script: &str) -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ssh");
        let dir_path = dir.path().to_string_lossy().into_owned();
        fs::write(&path, script).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        (dir, dir_path)
    }

    #[test]
    fn text_file_busy_detection_matches_unix_os_error() {
        #[cfg(unix)]
        assert!(is_text_file_busy(&std::io::Error::from_raw_os_error(
            libc::ETXTBSY
        )));
        assert!(!is_text_file_busy(&std::io::Error::from(
            std::io::ErrorKind::NotFound
        )));
    }

    fn init_repo(root: &Path) {
        let init = Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        assert!(
            init.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        for (key, value) in [("user.email", "test@test.com"), ("user.name", "Test")] {
            let config = Command::new("git")
                .current_dir(root)
                .args(["config", key, value])
                .output()
                .unwrap();
            assert!(
                config.status.success(),
                "git config failed: {}",
                String::from_utf8_lossy(&config.stderr)
            );
        }
    }

    fn commit_file(root: &Path, rel: &str, content: &str, message: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        let add = Command::new("git")
            .current_dir(root)
            .args(["add", rel])
            .output()
            .unwrap();
        assert!(
            add.status.success(),
            "git add failed: {}",
            String::from_utf8_lossy(&add.stderr)
        );
        let commit = Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", message, "--no-verify"])
            .output()
            .unwrap();
        assert!(
            commit.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
    }

    fn add_submodule(repo: &Path, origin: &Path, target: &str) {
        let url = format!("file://{}", origin.display());
        let add = Command::new("git")
            .current_dir(repo)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &url,
                target,
            ])
            .output()
            .unwrap();
        assert!(
            add.status.success(),
            "git submodule add failed: {}",
            String::from_utf8_lossy(&add.stderr)
        );
        let commit = Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();
        assert!(
            commit.status.success(),
            "git commit submodule failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
    }

    #[test]
    fn streaming_stderr_surfaced_on_nonzero_exit() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                "cat >/dev/null; echo >&2 'sandbox violation'; exit 1".into(),
            ]),
        );
        let iter = codex.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<_> = iter.collect();
        assert_eq!(chunks.len(), 1);
        let err = chunks[0].as_ref().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("sandbox violation"), "got: {msg}");
        assert!(msg.contains("codex subprocess exited with"), "got: {msg}");
    }

    #[test]
    fn codex_stderr_filter_drops_marketplace_manifest_noise_only() {
        let stderr = "\
2026-05-04T02:58:49Z WARN codex_core_plugins::manifest: ignoring interface.defaultPrompt: prompt must be at most 128 characters path=/home/brian/.codex/.tmp/plugins/plugins/build-ios-apps/.codex-plugin/plugin.json
2026-05-04T02:58:49Z WARN codex_core_skills::loader: ignoring interface.icon_small: icon path must not contain '..'
2026-05-04T02:58:49Z WARN codex_core_skills::loader: ignoring interface.icon_large: icon path must not contain '..'
real stderr
";

        let filtered = filter_codex_stderr_noise(stderr);
        let report = codex_stderr_noise_report(stderr);

        assert_eq!(filtered, "real stderr\n");
        assert_eq!(report.filtered, "real stderr\n");
        assert_eq!(report.suppressed_marketplace_manifest_warnings, 3);
    }

    #[test]
    fn codex_stderr_filter_keeps_local_plugin_manifest_warnings() {
        let stderr = "WARN codex_core_plugins::manifest: ignoring interface.defaultPrompt: prompt must be at most 128 characters path=/home/brian/work/btakita/agent-loop/src/agent-doc/.codex-plugin/plugin.json\n";

        let filtered = filter_codex_stderr_noise(stderr);
        let report = codex_stderr_noise_report(stderr);

        assert_eq!(filtered, stderr);
        assert_eq!(report.suppressed_marketplace_manifest_warnings, 0);
    }

    #[test]
    fn looks_like_codex_transport_403_429_detects_ws_403() {
        assert!(looks_like_codex_transport_403_429(
            "403 Forbidden on wss://chatgpt.com/backend-api/codex/responses"
        ));
        assert!(looks_like_codex_transport_403_429(
            "WebSocket handshake failed: 403"
        ));
    }

    #[test]
    fn looks_like_codex_transport_403_429_detects_https_429() {
        assert!(looks_like_codex_transport_403_429("429 Too Many Requests"));
        assert!(looks_like_codex_transport_403_429(
            "rate limit exceeded 429"
        ));
    }

    #[test]
    fn looks_like_codex_transport_403_429_rejects_unrelated() {
        assert!(!looks_like_codex_transport_403_429("sandbox violation"));
        assert!(!looks_like_codex_transport_403_429("permission denied"));
    }

    #[test]
    fn format_transport_403_429_diagnostic_names_both_rejections() {
        let msg = format_transport_403_429_diagnostic(
            "403 on wss://example.com then 429 Too Many Requests",
        );
        assert!(msg.contains("403 Forbidden on WebSocket"));
        assert!(msg.contains("429 Too Many Requests"));
        assert!(msg.contains("restart the codex session"));
    }

    #[test]
    fn send_surfaces_transport_403_429_diagnostic() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                "cat >/dev/null; echo >&2 '403 Forbidden on wss://chatgpt.com/backend-api/codex/responses'; exit 1".into(),
            ]),
        );
        let result = codex.send("ignored", None, false, None);
        assert!(
            result.is_err(),
            "expected error for 403 transport rejection"
        );
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("transport rejection"), "got: {msg}");
        assert!(msg.contains("403 Forbidden"), "got: {msg}");
    }

    #[test]
    fn streaming_surfaces_transport_403_429_diagnostic() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                "cat >/dev/null; echo >&2 '429 Too Many Requests'; exit 1".into(),
            ]),
        );
        let iter = codex.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<_> = iter.collect();
        assert_eq!(chunks.len(), 1);
        let err = chunks[0].as_ref().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("transport rejection"), "got: {msg}");
        assert!(msg.contains("429"), "got: {msg}");
    }

    #[test]
    fn streaming_stderr_logged_on_zero_exit() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                r#"cat >/dev/null; echo '{"type":"thread.started","thread_id":"t1"}'; echo '{"type":"turn.completed","usage":{}}'; echo >&2 'deprecation warning'"#
                    .into(),
            ]),
        );
        let iter = codex.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<Result<StreamChunk>> = iter.collect();
        assert!(
            chunks.iter().all(|c| c.is_ok()),
            "expected no errors, got: {chunks:?}"
        );
        let final_chunk = chunks
            .iter()
            .find(|c| c.as_ref().map(|sc| sc.is_final).unwrap_or(false));
        assert!(final_chunk.is_some(), "expected final chunk");
    }

    #[test]
    fn streaming_synthesizes_final_chunk_when_successful_eof_follows_agent_message() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                "cat >/dev/null; echo '{\"type\":\"thread.started\",\"thread_id\":\"t1\"}'; echo '{\"type\":\"item.completed\",\"item\":{\"id\":\"msg-1\",\"type\":\"agent_message\",\"text\":\"hello\"}}'".into(),
            ]),
        );
        let iter = codex.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<Result<StreamChunk>> = iter.collect();
        assert!(
            chunks.iter().all(|c| c.is_ok()),
            "expected no errors, got: {chunks:?}"
        );
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().unwrap().text, "hello");
        assert!(chunks[1].as_ref().unwrap().is_final);
        assert_eq!(
            chunks[1].as_ref().unwrap().session_id.as_deref(),
            Some("t1")
        );
    }

    #[test]
    fn streaming_stderr_empty_on_nonzero_exit() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec!["-c".into(), "cat >/dev/null; exit 42".into()]),
        );
        let iter = codex.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<_> = iter.collect();
        assert_eq!(chunks.len(), 1);
        let err = chunks[0].as_ref().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("codex subprocess exited with"), "got: {msg}");
    }

    fn command_args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(OsStr::to_string_lossy)
            .map(|s| s.into_owned())
            .collect()
    }

    #[test]
    fn parse_thread_started() {
        let line =
            r#"{"type":"thread.started","thread_id":"019db613-e57b-77d2-844c-9e7dca83ad01"}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
        assert_eq!(
            chunk.session_id.as_deref(),
            Some("019db613-e57b-77d2-844c-9e7dca83ad01")
        );
    }

    #[test]
    fn parse_agent_message() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"hello world"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "hello world");
        assert!(!chunk.is_final);
        assert!(chunk.session_id.is_none());
    }

    #[test]
    fn parse_command_execution() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"command_execution","command":"ls","aggregated_output":"foo\n","exit_code":0,"status":"completed"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
    }

    #[test]
    fn parse_turn_completed() {
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":100,"output_tokens":20}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_turn_started() {
        let line = r#"{"type":"turn.started"}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(!chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_item_started() {
        let line = r#"{"type":"item.started","item":{"id":"item_0","type":"command_execution","command":"ls","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(!chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_unknown_event() {
        let line = r#"{"type":"some.future.event","data":42}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(!chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_malformed_json() {
        let result = parse_codex_line("not json");
        assert!(result.is_err());
    }

    #[test]
    fn parse_agent_message_missing_text() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
    }

    #[test]
    fn stream_iterator_propagates_session_id_to_final() {
        // Simulate the iterator behavior: session_id from thread.started
        // should appear on the final (turn.completed) chunk
        let lines = vec![
            r#"{"type":"thread.started","thread_id":"abc-123"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"hi"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}"#,
        ];

        // Parse individually and verify the propagation logic
        let mut session_id: Option<String> = None;
        let mut chunks: Vec<StreamChunk> = Vec::new();

        for line in &lines {
            let mut chunk = parse_codex_line(line).unwrap();
            if chunk.session_id.is_some() && !chunk.is_final {
                session_id = chunk.session_id.take();
            }
            if chunk.is_final {
                chunk.session_id = session_id.take();
            }
            // Filter same as iterator: skip empty non-final
            if !chunk.is_final
                && chunk.text.is_empty()
                && chunk.thinking.is_none()
                && chunk.session_id.is_none()
            {
                continue;
            }
            chunks.push(chunk);
        }

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "hi");
        assert!(!chunks[0].is_final);
        assert!(chunks[1].is_final);
        assert_eq!(chunks[1].session_id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn build_command_exec_preserves_default_sandbox_flag() {
        let codex = Codex::new(None, None);
        let cmd = codex.build_command(None, false, None);

        assert_eq!(
            command_args(&cmd),
            vec!["exec", "--json", "-s", "workspace-write"]
        );
    }

    #[test]
    fn build_command_resume_translates_short_sandbox_flag() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "-s".into(),
                "workspace-write".into(),
                "--skip-git-repo-check".into(),
            ]),
        );
        let cmd = codex.build_command(Some("thread-123"), false, None);

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "resume",
                "thread-123",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "--skip-git-repo-check",
            ]
        );
    }

    #[test]
    fn build_command_fork_starts_fresh_exec_session() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "--sandbox=danger-full-access".into(),
                "--ignore-user-config".into(),
            ]),
        );
        let cmd = codex.build_command(None, true, Some("gpt-5.4"));

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "--json",
                "--sandbox=danger-full-access",
                "--ignore-user-config",
                "-m",
                "gpt-5.4",
            ]
        );
    }

    #[test]
    fn build_command_exec_preserves_add_dir() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "-s".into(),
                "workspace-write".into(),
                "--add-dir".into(),
                "/home/user/.git/modules/sub".into(),
            ]),
        );
        let cmd = codex.build_command(None, false, None);

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "--json",
                "-s",
                "workspace-write",
                "--add-dir",
                "/home/user/.git/modules/sub",
            ]
        );
    }

    #[test]
    fn build_command_resume_strips_add_dir() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "-s".into(),
                "workspace-write".into(),
                "--add-dir".into(),
                "/home/user/.git/modules/sub".into(),
                "--skip-git-repo-check".into(),
            ]),
        );
        let cmd = codex.build_command(Some("thread-456"), false, None);

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "resume",
                "thread-456",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "--skip-git-repo-check",
            ]
        );
    }

    #[test]
    fn build_command_resume_strips_add_dir_equals_form() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "-s".into(),
                "workspace-write".into(),
                "--add-dir=/home/user/.git".into(),
            ]),
        );
        let cmd = codex.build_command(Some("thread-789"), false, None);

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "resume",
                "thread-789",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
            ]
        );
    }

    #[test]
    fn build_command_resume_strips_multiple_add_dirs() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "-s".into(),
                "workspace-write".into(),
                "--add-dir".into(),
                "/home/user/.git/modules/sub".into(),
                "--add-dir".into(),
                "/home/user/.git".into(),
            ]),
        );
        let cmd = codex.build_command(Some("thread-abc"), false, None);

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "resume",
                "thread-abc",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
            ]
        );
    }

    #[test]
    fn managed_capability_contract_requires_network_ssh_or_writable_roots() {
        let config = crate::config::Config::default();
        let mut fm = Frontmatter::default();
        assert!(!managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "codex"
        ));
        assert!(!managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "opencode"
        ));

        fm.codex_network_access = Some(CodexNetworkAccess::Enabled);
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "codex"
        ));
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "opencode"
        ));

        fm.codex_network_access = None;
        fm.required_ssh_targets = vec!["example-host".to_string()];
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "codex"
        ));
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "opencode"
        ));

        fm.required_ssh_targets.clear();
        assert!(managed_capability_contract_required(
            &[
                "exec".to_string(),
                "--json".to_string(),
                "--add-dir".to_string(),
                "/tmp/example".to_string()
            ],
            &fm,
            &config,
            "codex"
        ));
        assert!(!managed_capability_contract_required(
            &[
                "exec".to_string(),
                "--json".to_string(),
                "--add-dir".to_string(),
                "/tmp/example".to_string()
            ],
            &fm,
            &config,
            "opencode"
        ));
    }

    #[test]
    fn managed_network_child_proof_cache_key_includes_environment() {
        let mut first_env = std::collections::HashMap::new();
        first_env.insert("HTTPS_PROXY".to_string(), "http://proxy-a".to_string());
        let mut second_env = std::collections::HashMap::new();
        second_env.insert("HTTPS_PROXY".to_string(), "http://proxy-b".to_string());

        let args = vec!["exec".to_string(), "--json".to_string()];
        let first = managed_network_child_proof_cache_key("codex", &args, &first_env, "codex");
        let second = managed_network_child_proof_cache_key("codex", &args, &second_env, "codex");

        assert_ne!(first, second);
        assert!(!managed_network_child_proof_is_cached(&first));
        remember_managed_network_child_proof(first.clone());
        assert!(managed_network_child_proof_is_cached(&first));
    }

    #[test]
    fn opencode_probe_args_use_run_format_json_and_preserve_safe_flags() {
        let args = opencode_run_args_for_probe(
            &[
                "--model".to_string(),
                "zai/glm-5".to_string(),
                "--continue".to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--dir".to_string(),
                "/tmp/project".to_string(),
            ],
            "probe".to_string(),
        );

        assert_eq!(
            args,
            vec![
                "run",
                "--format",
                "json",
                "--model",
                "zai/glm-5",
                "--dangerously-skip-permissions",
                "probe"
            ]
        );
    }

    #[test]
    fn opencode_probe_args_drop_tui_only_prompt_flag_and_preserve_run_command_flag() {
        let args = opencode_run_args_for_probe(
            &[
                "--prompt".to_string(),
                "tui-only".to_string(),
                "--command".to_string(),
                "session".to_string(),
                "--file".to_string(),
                "SPEC.md".to_string(),
            ],
            "probe".to_string(),
        );

        assert_eq!(
            args,
            vec![
                "run",
                "--format",
                "json",
                "--command",
                "session",
                "--file",
                "SPEC.md",
                "probe"
            ]
        );
    }

    #[test]
    fn opencode_child_probe_classifies_cli_usage_separately_from_network_failure() {
        let err = validate_opencode_child_probe_marker_output(
            "opencode run [message..]\n\nPositionals:\n  message\n\nOptions:\n  --format\n",
            "",
            CODEX_CHILD_NETWORK_PROBE_MARKER,
            "network",
            "OpenCode",
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("printed CLI usage/help instead of running the network probe"),
            "{message}"
        );
        assert!(
            !message.contains("sandbox/network capability denied outbound access"),
            "{message}"
        );
    }

    #[test]
    fn opencode_child_probe_classifies_socket_eperm_as_ssh_sandbox_denial() {
        let err = validate_opencode_child_probe_marker_output(
            r#"{"type":"message","text":"ssh monsterrodholders-server true\nsocket: Operation not permitted"}"#,
            "",
            OPENCODE_CHILD_SSH_PROBE_MARKER,
            "ssh",
            "OpenCode",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("SSH unavailable inside managed OpenCode pane"),
            "{err}"
        );
    }

    #[test]
    fn managed_capability_proof_runs_opencode_child_ssh_probe() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname $2"
  echo "port 22"
  exit 0
fi
exit 0
"#,
        );
        let (_opencode_dir, opencode) = write_fake_codex_script(&format!(
            r#"#!/bin/sh
printf '%s\n' '{{"type":"message","text":"{}\n"}}'
"#,
            OPENCODE_CHILD_SSH_PROBE_MARKER
        ));
        let mut env = std::collections::HashMap::new();
        let old_path = std::env::var("PATH").unwrap_or_default();
        env.insert("PATH".to_string(), format!("{path_dir}:{old_path}"));
        let mut fm = Frontmatter::default();
        fm.required_ssh_targets = vec!["monsterrodholders-server".to_string()];

        let event = prove_managed_session_capabilities(
            &opencode,
            &[
                "--model".to_string(),
                "zai/glm-5".to_string(),
                "--dangerously-skip-permissions".to_string(),
            ],
            &env,
            &fm,
            &crate::config::Config::default(),
            "opencode",
            crate::agent::DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT,
        )
        .unwrap()
        .unwrap();

        assert!(event.contains("opencode_capability_proof status=proven"));
        assert!(event.contains("ssh_targets=1"));
        assert!(event.contains("writable_roots=0"));
    }

    #[test]
    fn managed_capability_proof_checks_writable_add_dirs() {
        let dir = TempDir::new().unwrap();
        let (_script_dir, script) = write_fake_codex_script(&format!(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{{"type":"thread.started","thread_id":"probe-thread"}}'
printf '%s\n' '{{"type":"item.completed","item":{{"type":"command_execution","command":"sh -lc probe","aggregated_output":"{}\n","exit_code":0}}}}'
printf '%s\n' '{{"type":"turn.completed","usage":{{}}}}'
"#,
            CODEX_CHILD_WRITABLE_ROOT_PROBE_MARKER
        ));
        let args = vec![
            "exec".to_string(),
            "--json".to_string(),
            "--add-dir".to_string(),
            dir.path().to_string_lossy().into_owned(),
        ];
        let fm = Frontmatter::default();
        let env = std::collections::HashMap::new();

        let event = prove_managed_session_capabilities(
            &script,
            &args,
            &env,
            &fm,
            &crate::config::Config::default(),
            "codex",
            crate::agent::DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT,
        )
        .unwrap()
        .unwrap();

        assert!(event.contains("codex_capability_proof status=proven"));
        assert!(event.contains("writable_roots=1"));
        assert!(event.contains("timings_ms="), "{event}");
        assert!(event.contains("writable_launcher:"), "{event}");
        assert!(event.contains("writable_child:"), "{event}");
    }

    #[test]
    fn managed_capability_proof_records_writable_root_contract() {
        let dir = TempDir::new().unwrap();
        let (_script_dir, script) = write_fake_codex_script(&format!(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{{"type":"thread.started","thread_id":"probe-thread"}}'
printf '%s\n' '{{"type":"item.completed","item":{{"type":"command_execution","command":"sh -lc probe","aggregated_output":"{}\n","exit_code":0}}}}'
printf '%s\n' '{{"type":"turn.completed","usage":{{}}}}'
"#,
            CODEX_CHILD_WRITABLE_ROOT_PROBE_MARKER
        ));
        let root = dir.path().canonicalize().unwrap();
        let args = vec![
            "exec".to_string(),
            "--json".to_string(),
            "--add-dir".to_string(),
            root.to_string_lossy().into_owned(),
        ];
        let fm = Frontmatter::default();
        let env = std::collections::HashMap::new();
        let expected_contract = writable_root_contract_id(&[root]).unwrap();

        let event = prove_managed_session_capabilities(
            &script,
            &args,
            &env,
            &fm,
            &crate::config::Config::default(),
            "codex",
            crate::agent::DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT,
        )
        .unwrap()
        .unwrap();

        assert!(
            event.contains(&format!("writable_root_contract={expected_contract}")),
            "{event}"
        );
    }

    #[test]
    fn send_fresh_exec_when_resume_has_writable_add_dirs() {
        let dir = TempDir::new().unwrap();
        let (_script_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "${2:-}" = "resume" ]; then
  printf '%s\n' 'resume path must not be used when --add-dir roots are required' >&2
  exit 44
fi
cat >/dev/null
printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"fresh response"}}'
printf '%s\n' '{"type":"turn.completed","usage":{}}'
"#,
        );
        let codex = Codex::new(
            Some(script),
            Some(vec![
                "exec".to_string(),
                "--json".to_string(),
                "--add-dir".to_string(),
                dir.path().to_string_lossy().into_owned(),
            ]),
        );

        let response = codex
            .send("prompt", Some("resume-123"), false, None)
            .unwrap();

        assert_eq!(response.text, "fresh response");
        assert_eq!(response.session_id.as_deref(), Some("fresh-thread"));
    }

    #[test]
    fn send_selects_last_agent_message_as_final_closeout() {
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"I am checking the document now.\n"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"make check","aggregated_output":"ok\n","exit_code":0,"status":"completed"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"<!-- patch:exchange -->\n### Re: final — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->"}}'
printf '%s\n' '{"type":"turn.completed","usage":{}}'
"#,
        );
        let codex = Codex::new(Some(script), None);

        let response = codex.send("prompt", None, false, None).unwrap();

        assert_eq!(
            response.text,
            "<!-- patch:exchange -->\n### Re: final — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->"
        );
        assert!(
            !response.text.contains("I am checking"),
            "progress chatter must not be captured in patchback: {}",
            response.text
        );
        assert_eq!(response.session_id.as_deref(), Some("fresh-thread"));
    }

    #[test]
    fn send_rejects_multiple_agent_messages_without_final_boundary() {
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"progress"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"possible final"}}'
"#,
        );
        let codex = Codex::new(Some(script), None);

        let err = match codex.send("prompt", None, false, None) {
            Ok(response) => panic!("expected ambiguous response error, got {}", response.text),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("ambiguous Codex response"),
            "got: {err}"
        );
    }

    #[test]
    fn managed_capability_contract_for_doc_requires_auto_submodule_gitdirs() {
        let outer_dir = TempDir::new().unwrap();
        let outer = outer_dir.path();
        init_repo(outer);
        commit_file(outer, "README.md", "# outer\n", "init outer");

        let sub_origin_dir = TempDir::new().unwrap();
        let sub_origin = sub_origin_dir.path();
        init_repo(sub_origin);
        commit_file(sub_origin, "README.md", "# sub\n", "init sub");

        add_submodule(outer, sub_origin, "src/sub");
        let doc = outer.join("src/sub/tasks/session.md");
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, "test\n").unwrap();

        let fm = Frontmatter::default();
        assert!(managed_capability_contract_required_for_doc_and_harness(
            &doc,
            &fm,
            &crate::config::Config::default(),
            "codex"
        ));
    }

    #[test]
    fn codex_child_writable_probe_classifies_read_only_gitdir_lock() {
        let stdout = "{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"command\":\"sh -lc probe\",\"aggregated_output\":\"sh: cannot create /repo/.git/modules/sub/index.lock: Read-only file system\\n\",\"exit_code\":1}}\n";

        let err = validate_codex_child_writable_root_probe_output(stdout, "", "Codex").unwrap_err();
        assert!(
            err.to_string()
                .contains("Codex sandbox/write capability denied git metadata access"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn codex_network_probe_exec_args_wrap_interactive_launch_args() {
        let args = codex_exec_args_for_probe(&[
            "-s".to_string(),
            "danger-full-access".to_string(),
            "--add-dir".to_string(),
            "/tmp/repo".to_string(),
        ]);

        assert_eq!(
            args,
            vec![
                "exec",
                "--json",
                "-s",
                "danger-full-access",
                "--add-dir",
                "/tmp/repo",
            ]
        );
    }

    #[test]
    fn codex_child_network_probe_requires_command_marker() {
        let stdout = format!(
            "{{\"type\":\"item.completed\",\"item\":{{\"type\":\"command_execution\",\"command\":\"sh -lc probe\",\"aggregated_output\":\"{}\\n\",\"exit_code\":0}}}}\n",
            CODEX_CHILD_NETWORK_PROBE_MARKER
        );

        validate_codex_child_network_probe_output(&stdout, "", "codex").unwrap();
    }

    #[test]
    fn codex_child_network_probe_classifies_sandbox_denial() {
        let stdout = "{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"command\":\"sh -lc probe\",\"aggregated_output\":\"socket: Operation not permitted\\n\",\"exit_code\":1}}\n";

        let err = validate_codex_child_network_probe_output(stdout, "", "Codex").unwrap_err();
        assert!(
            err.to_string()
                .contains("Codex sandbox/network capability denied outbound access"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn opencode_child_network_probe_classifies_sandbox_denial() {
        let stdout = "{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"command\":\"sh -lc probe\",\"aggregated_output\":\"socket: Operation not permitted\\n\",\"exit_code\":1}}\n";

        let err = validate_codex_child_network_probe_output(stdout, "", "OpenCode").unwrap_err();
        assert!(
            err.to_string()
                .contains("OpenCode sandbox/network capability denied outbound access"),
            "unexpected error: {err:#}"
        );
        assert!(
            !err.to_string().contains("Codex"),
            "should not contain Codex: {err:#}"
        );
    }

    #[test]
    fn prove_codex_child_network_access_runs_fake_codex_child_probe() {
        let (_dir, script) = write_fake_codex_script(&format!(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{{"type":"thread.started","thread_id":"probe-thread"}}'
printf '%s\n' '{{"type":"item.completed","item":{{"type":"command_execution","command":"sh -lc probe","aggregated_output":"{}\n","exit_code":0}}}}'
printf '%s\n' '{{"type":"turn.completed","usage":{{}}}}'
"#,
            CODEX_CHILD_NETWORK_PROBE_MARKER
        ));
        let env = std::collections::HashMap::new();

        prove_codex_child_network_access(
            &script,
            &["-s".to_string(), "danger-full-access".to_string()],
            &env,
            "codex",
            crate::agent::DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT,
        )
        .unwrap();
    }

    #[test]
    fn local_browser_cdp_permission_denied_matches_resume_capability_drift_signature() {
        assert!(looks_like_local_browser_cdp_permission_denied(
            "chromium-bridge check failed for 127.0.0.1:9222: Operation not permitted (os error 1)"
        ));
        assert!(!looks_like_local_browser_cdp_permission_denied(
            "chromium-bridge check failed for 127.0.0.1:9222: Connection refused"
        ));
    }

    #[test]
    fn required_ssh_capability_reports_alias_config_failure_when_direct_path_still_works() {
        let (_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
case "$*" in
  *monsterrodholders-server*)
    echo "ssh: Could not resolve hostname monsterrodholders-server: Name or service not known" >&2
    exit 255
    ;;
  *50.28.2.199*)
    exit 0
    ;;
esac
exit 0
"#,
        );
        let codex = Codex::new(None, None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let err = codex
            .prove_required_ssh_capability()
            .unwrap_err()
            .to_string();
        assert!(err.contains("monsterrodholders-server"), "got: {err}");
        assert!(err.contains("isolated direct host probe"), "got: {err}");
    }

    #[test]
    fn required_ssh_probes_disable_shared_control_socket_state() {
        let (dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$SSH_LOG"
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let log_path = dir.path().join("ssh-probe.log");
        fs::write(&log_path, "").unwrap();
        let codex = Codex::new(None, None)
            .with_env(vec![
                ("PATH".to_string(), Some(path_dir)),
                (
                    "SSH_LOG".to_string(),
                    Some(log_path.to_string_lossy().into_owned()),
                ),
            ])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        codex.prove_required_ssh_capability().unwrap();

        let log = fs::read_to_string(&log_path).unwrap();
        let connect_lines: Vec<_> = log
            .lines()
            .filter(|line| !line.starts_with("-G "))
            .collect();
        assert_eq!(connect_lines.len(), 2, "got log: {log}");
        for line in connect_lines {
            assert!(line.contains("-o BatchMode=yes"), "got: {line}");
            assert!(line.contains("-o ConnectTimeout=5"), "got: {line}");
            assert!(line.contains("-o ControlMaster=no"), "got: {line}");
            assert!(line.contains("-o ControlPath=none"), "got: {line}");
            assert!(line.contains("-o ClearAllForwardings=yes"), "got: {line}");
            assert!(line.contains("-o PermitLocalCommand=no"), "got: {line}");
        }
    }

    #[test]
    fn required_ssh_failure_detects_bare_socket_eperm_when_command_proves_ssh_context() {
        let line = r#"{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}"#;

        assert_eq!(
            transcript_has_required_ssh_failure(line, &["monsterrodholders-server".to_string()]),
            Some(
                "command `ssh monsterrodholders-server true`: socket: Operation not permitted"
                    .to_string()
            )
        );
    }

    #[test]
    fn required_ssh_failure_ignores_bare_socket_eperm_without_ssh_command_context() {
        let line = r#"{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"chromium-bridge list","aggregated_output":"socket: Operation not permitted","exit_code":1,"status":"completed"}}"#;

        assert_eq!(
            transcript_has_required_ssh_failure(line, &["monsterrodholders-server".to_string()]),
            None
        );
    }

    #[test]
    fn required_ssh_failure_ignores_historical_capture_grep_output() {
        let line = r#"{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"rg 'Operation not permitted' .agent-doc/captures","aggregated_output":".agent-doc/captures/old/cycle.json:16: \"response_body\": \"required SSH capability failed for target(s) monsterrodholders-server: socket: Operation not permitted\"","exit_code":0,"status":"completed"}}"#;

        assert_eq!(
            transcript_has_required_ssh_failure(line, &["monsterrodholders-server".to_string()]),
            None
        );
    }

    #[test]
    fn required_ssh_failure_detects_direct_ssh_diagnostic_without_command_field() {
        let line = r#"{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","aggregated_output":"ssh: connect to host 50.28.2.199 port 22: Operation not permitted","exit_code":255,"status":"completed"}}"#;

        assert_eq!(
            transcript_has_required_ssh_failure(
                line,
                &[
                    "monsterrodholders-server".to_string(),
                    "50.28.2.199".to_string()
                ]
            ),
            Some("ssh: connect to host 50.28.2.199 port 22: Operation not permitted".to_string())
        );
    }

    #[test]
    fn send_retries_fresh_exec_after_resume_capability_drift_signal() {
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","aggregated_output":"chromium-bridge check failed for 127.0.0.1:9222: Operation not permitted (os error 1)","exit_code":1,"status":"completed"}}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"stale resume response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None);

        let response = codex
            .send("prompt", Some("resume-123"), false, None)
            .unwrap();

        assert_eq!(response.text, "fresh response");
        assert_eq!(response.session_id.as_deref(), Some("fresh-thread"));
    }

    #[test]
    fn send_retries_fresh_exec_after_bare_socket_required_ssh_resume_drift_signal() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let response = codex
            .send("prompt", Some("resume-123"), false, None)
            .unwrap();

        assert_eq!(response.text, "fresh response");
        assert_eq!(response.session_id.as_deref(), Some("fresh-thread"));
    }

    #[test]
    fn send_retries_fresh_exec_after_required_ssh_resume_drift_signal() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","aggregated_output":"ssh: connect to host 50.28.2.199 port 22: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let response = codex
            .send("prompt", Some("resume-123"), false, None)
            .unwrap();

        assert_eq!(response.text, "fresh response");
        assert_eq!(response.session_id.as_deref(), Some("fresh-thread"));
    }

    #[test]
    fn streaming_retries_fresh_exec_before_yielding_stale_resume_response() {
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","aggregated_output":"chromium-bridge list failed for localhost:9222: Operation not permitted (os error 1)","exit_code":1,"status":"completed"}}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"stale resume response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None);

        let chunks: Vec<_> = codex
            .send_streaming("prompt", Some("resume-123"), false, None)
            .unwrap()
            .collect();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().unwrap().text, "fresh response");
        assert!(chunks[1].as_ref().unwrap().is_final);
        assert_eq!(
            chunks[1].as_ref().unwrap().session_id.as_deref(),
            Some("fresh-thread")
        );
    }

    #[test]
    fn streaming_retries_fresh_exec_after_required_ssh_resume_drift_signal() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","aggregated_output":"ssh: connect to host 50.28.2.199 port 22: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let chunks: Vec<_> = codex
            .send_streaming("prompt", Some("resume-123"), false, None)
            .unwrap()
            .collect();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().unwrap().text, "fresh response");
        assert!(chunks[1].as_ref().unwrap().is_final);
        assert_eq!(
            chunks[1].as_ref().unwrap().session_id.as_deref(),
            Some("fresh-thread")
        );
    }

    #[test]
    fn streaming_retries_fresh_exec_after_bare_socket_required_ssh_resume_drift_signal() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let chunks: Vec<_> = codex
            .send_streaming("prompt", Some("resume-123"), false, None)
            .unwrap()
            .collect();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().unwrap().text, "fresh response");
        assert!(chunks[1].as_ref().unwrap().is_final);
        assert_eq!(
            chunks[1].as_ref().unwrap().session_id.as_deref(),
            Some("fresh-thread")
        );
    }

    #[test]
    fn streaming_required_ssh_retry_discards_buffered_resumed_prelude_text() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"I am retrying the SSH step now.\n"}}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-2","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"","exit_code":0,"status":"completed"}}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let chunks: Vec<_> = codex
            .send_streaming("prompt", Some("resume-123"), false, None)
            .unwrap()
            .collect();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().unwrap().text, "fresh response");
        assert!(
            !chunks[0]
                .as_ref()
                .unwrap()
                .text
                .contains("I am retrying the SSH step now."),
            "stale resumed prelude should be discarded"
        );
        assert!(chunks[1].as_ref().unwrap().is_final);
        assert_eq!(
            chunks[1].as_ref().unwrap().session_id.as_deref(),
            Some("fresh-thread")
        );
    }

    #[test]
    fn streaming_required_ssh_success_releases_buffered_chunks() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
printf '%s\n' '{"type":"thread.started","thread_id":"resume-thread"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"I am checking SSH first.\n"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"","exit_code":0,"status":"completed"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"SSH worked."}}'
printf '%s\n' '{"type":"turn.completed","usage":{}}'
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let chunks: Vec<_> = codex
            .send_streaming("prompt", Some("resume-123"), false, None)
            .unwrap()
            .collect();

        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks[0].as_ref().unwrap().text,
            "I am checking SSH first.\n"
        );
        assert_eq!(chunks[1].as_ref().unwrap().text, "SSH worked.");
        assert!(chunks[2].as_ref().unwrap().is_final);
        assert_eq!(
            chunks[2].as_ref().unwrap().session_id.as_deref(),
            Some("resume-thread")
        );
    }
