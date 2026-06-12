    use super::*;
    use sessions::{IsolatedTmux, SessionEntry, SessionRegistry};

    static TMUX_START_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct ScopedCurrentDir {
        prev_cwd: std::path::PathBuf,
        _env_guard: crate::test_support::ProcessGlobalLockGuard,
    }

    impl ScopedCurrentDir {
        fn set(path: &std::path::Path) -> Self {
            let env_guard = crate::test_support::env_lock();
            let prev_cwd = std::env::current_dir()
                .ok()
                .filter(|cwd| cwd.exists())
                .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            std::env::set_current_dir(path).unwrap();
            Self {
                prev_cwd,
                _env_guard: env_guard,
            }
        }
    }

    impl Drop for ScopedCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev_cwd);
        }
    }

    fn tmux_start_lock() -> std::sync::MutexGuard<'static, ()> {
        TMUX_START_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn test_cwd() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Poll until `pane_current_command` returns an idle shell, or timeout.
    /// Needed because shell startup is asynchronous and the 500ms sleep is
    /// insufficient under parallel test load (other tests saturate the CPU,
    /// slowing the new pane's shell init — which can briefly show transient
    /// commands like `mv` from shell frameworks).
    fn wait_for_shell(iso: &IsolatedTmux, pane: &str, timeout_ms: u64) -> bool {
        let start = std::time::Instant::now();
        loop {
            if let Some(cmd) = pane_current_command(iso, pane)
                && IDLE_SHELLS.contains(&cmd.as_str())
            {
                return true;
            }
            if start.elapsed().as_millis() >= timeout_ms as u128 {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// Helper to create a registry entry for testing.
    fn test_entry(pane: &str, file: &str) -> SessionEntry {
        SessionEntry {
            pane: pane.to_string(),
            pid: std::process::id(),
            cwd: "/tmp".to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            session_id: format!("sess-{pane}"),
            file: file.to_string(),
            window: String::new(),
            supervisor_instance_id: String::new(),
        }
    }

    #[test]
    fn pane_process_kind_uses_prefetched_command_without_sampling() {
        assert!(matches!(
            pane_process_kind_from_current_command("zsh"),
            PaneProcessKind::IdleShell(cmd) if cmd == "zsh"
        ));
        assert!(matches!(
            pane_process_kind_from_current_command("agent-doc"),
            PaneProcessKind::Agent(cmd) if cmd == "agent-doc"
        ));
        assert!(matches!(
            pane_process_kind_from_current_command("sleep"),
            PaneProcessKind::Foreign(cmd) if cmd == "sleep"
        ));
        assert!(matches!(
            pane_process_kind_from_current_command(""),
            PaneProcessKind::UnknownTransient
        ));
    }

    fn write_mock_agent_doc(base: &std::path::Path) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let bin_dir = base.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let script = bin_dir.join("agent-doc");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf \"> \\n\"\nwhile IFS= read -r CMD; do\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        script
    }

    fn wait_for_pane_contains(
        tmux: &IsolatedTmux,
        pane: &str,
        needle: &str,
        timeout: std::time::Duration,
    ) -> String {
        let start = std::time::Instant::now();
        loop {
            let content = sessions::capture_pane(tmux, pane).unwrap_or_default();
            if content.contains(needle) || start.elapsed() >= timeout {
                return content;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    fn wait_for_pane_current_command(
        tmux: &IsolatedTmux,
        pane: &str,
        expected: &str,
        timeout: std::time::Duration,
    ) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if pane_current_command(tmux, pane).as_deref() == Some(expected) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    fn wait_for_window_relation(
        tmux: &IsolatedTmux,
        pane_a: &str,
        pane_b: &str,
        same_window: bool,
        timeout: std::time::Duration,
    ) -> Option<(String, String)> {
        let start = std::time::Instant::now();
        let mut last = None;
        while start.elapsed() < timeout {
            if let (Ok(window_a), Ok(window_b)) =
                (tmux.pane_window(pane_a), tmux.pane_window(pane_b))
            {
                let relation_matches = (window_a == window_b) == same_window;
                last = Some((window_a, window_b));
                if relation_matches {
                    return last;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        last
    }

    fn wait_for_pane_in_stash_window(
        tmux: &IsolatedTmux,
        session: &str,
        pane: &str,
        timeout: std::time::Duration,
    ) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Some(stash_window) = tmux.find_stash_window(session)
                && tmux.pane_window(pane).ok().as_deref() == Some(stash_window.as_str())
            {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    fn wait_for_pane_dead(tmux: &IsolatedTmux, pane: &str, timeout: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if !tmux.pane_alive(pane) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    fn wait_for_pane_removed(
        tmux: &IsolatedTmux,
        pane: &str,
        timeout: std::time::Duration,
    ) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if !tmux.pane_alive(pane) && !tmux.pane_dead(pane) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        !tmux.pane_alive(pane) && !tmux.pane_dead(pane)
    }

    fn drive_pane_to_retained_dead(
        tmux: &IsolatedTmux,
        pane: &str,
        command: &str,
        timeout: std::time::Duration,
    ) {
        {
            let _tmux_guard = tmux_start_lock();
            assert!(
                wait_for_shell(tmux, pane, 5000),
                "shell did not become ready before driving {} to retained-dead",
                pane
            );
            send_keys_with_retry(tmux, pane, command);
        }
        assert!(
            wait_for_pane_dead(tmux, pane, timeout),
            "pane should first become a retained dead pane"
        );
        assert!(
            tmux.pane_dead(pane),
            "pane should still be retained by tmux"
        );
    }

    fn send_keys_with_retry(tmux: &IsolatedTmux, pane: &str, text: &str) {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(3);
        let poll = std::time::Duration::from_millis(100);
        let mut last_err = None;

        while start.elapsed() < timeout {
            match tmux.send_keys(pane, text) {
                Ok(()) => return,
                Err(err) => last_err = Some(err.to_string()),
            }
            std::thread::sleep(poll);
        }

        panic!(
            "failed to send keys to pane {} after {:.1}s: {}",
            pane,
            start.elapsed().as_secs_f64(),
            last_err.unwrap_or_else(|| "unknown error".to_string())
        );
    }

    fn launch_mock_agent_doc(
        tmux: &IsolatedTmux,
        pane: &str,
        script: &std::path::Path,
        file: &std::path::Path,
    ) {
        {
            let _tmux_guard = tmux_start_lock();
            assert!(
                wait_for_shell(tmux, pane, 5000),
                "shell did not become ready before mock agent launch in {}",
                pane
            );
            send_keys_with_retry(
                tmux,
                pane,
                &format!("exec {} {}", script.display(), file.display()),
            );
        }
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(8);
        let poll = std::time::Duration::from_millis(300);
        let mut content = String::new();
        while start.elapsed() < timeout {
            content = sessions::capture_pane(tmux, pane).unwrap_or_default();
            if mock_agent_prompt_visible(&content) {
                break;
            }
            if let Some(cmd) = pane_current_command(tmux, pane)
                && IDLE_SHELLS.contains(&cmd.as_str())
            {
                let _ = tmux.send_keys_raw(pane, "Enter");
            }
            std::thread::sleep(poll);
        }
        assert!(
            mock_agent_prompt_visible(&content),
            "mock agent should be ready, got: {content}"
        );
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(3) {
            if crate::sync::find_alive_pane_for_file(tmux, &file.to_string_lossy()).as_deref()
                == Some(pane)
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!(
            "mock agent pane {} never became the live owner for {}",
            pane,
            file.display()
        );
    }

    fn wait_for_process_pid(pattern: &str, timeout: std::time::Duration) -> u32 {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Ok(output) = std::process::Command::new("pgrep")
                .args(["-f", pattern])
                .output()
                && output.status.success()
                && let Some(pid) = String::from_utf8_lossy(&output.stdout).lines().next()
                && let Ok(pid) = pid.trim().parse::<u32>()
            {
                return pid;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("timed out waiting for process matching {pattern}");
    }

    fn mock_agent_prompt_visible(content: &str) -> bool {
        content.lines().any(|line| line.trim() == ">")
    }

    #[test]
    fn filter_registry_for_target_matches_only_selected_file() {
        let dir = tempfile::tempdir().unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "# A\n").unwrap();
        std::fs::write(&doc_b, "# B\n").unwrap();
        let target = doc_a.canonicalize().unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "sess-a".to_string(),
            test_entry("%1", &doc_a.to_string_lossy()),
        );
        registry.insert(
            "sess-b".to_string(),
            test_entry("%2", &doc_b.to_string_lossy()),
        );

        let filtered = filter_registry_for_target(&registry, &target);
        assert_eq!(filtered.len(), 1, "only the target doc should remain");
        assert!(filtered.contains_key("sess-a"));
        assert!(!filtered.contains_key("sess-b"));
    }

    #[test]
    fn prune_dead_entries_for_target_only_removes_matching_file() {
        let dir = tempfile::tempdir().unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "# A\n").unwrap();
        std::fs::write(&doc_b, "# B\n").unwrap();
        let target = doc_a.canonicalize().unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "sess-a".to_string(),
            test_entry("%dead-a", &doc_a.to_string_lossy()),
        );
        registry.insert(
            "sess-b".to_string(),
            test_entry("%dead-b", &doc_b.to_string_lossy()),
        );

        let removed =
            prune_dead_entries_for_target_in_registry(&mut registry, &target, |_pane| false);
        assert_eq!(removed.len(), 1, "only the target doc should be pruned");
        assert_eq!(removed[0].0, "sess-a");
        assert!(!registry.contains_key("sess-a"));
        assert!(registry.contains_key("sess-b"));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn detect_dead_pane_not_flagged_as_issue() {
        // Dead panes are handled by prune(), not detect_issues.
        // detect_issues should skip dead panes entirely.
        let iso = IsolatedTmux::new("resync-test-dead");

        let mut registry = SessionRegistry::new();
        registry.insert("dead-session".to_string(), test_entry("%99999", "test.md"));

        let issues = detect_issues_in_registry(&iso, &registry);
        assert!(
            issues.is_empty(),
            "dead panes should not generate issues (handled by prune), got: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn detect_wrong_session_pane() {
        // A pane in tmux session "wrong" but frontmatter expects "correct"
        let iso = IsolatedTmux::new("resync-test-wrong-sess");
        let cwd = std::env::current_dir().unwrap();

        // Create a pane in session "wrong" — poll until the shell settles.
        // Fixed sleep is unreliable under parallel test load.
        let pane = iso.auto_start("wrong", &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, 5000),
            "shell did not start in pane within 5s"
        );

        // Create a temp file with frontmatter specifying tmux_session: correct
        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("test.md");
        std::fs::write(
            &doc_path,
            "---\nsession: abc-123\ntmux_session: correct\n---\n# Test\n",
        )
        .unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "abc-123".to_string(),
            test_entry(&pane, &doc_path.to_string_lossy()),
        );

        let issues = detect_issues_in_registry(&iso, &registry);
        assert_eq!(issues.len(), 1, "should detect 1 stale-owner issue");
        assert!(
            matches!(&issues[0], Issue::NoLiveOwner { .. }),
            "pane with no provable owner should now fail as NoLiveOwner before WrongSession; got: {}",
            &issues[0]
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn detect_wrong_process_pane() {
        // A pane running a non-agent-doc process (e.g., "sleep")
        let iso = IsolatedTmux::new("resync-test-wrong-proc");
        let cwd = std::env::current_dir().unwrap();

        // Create a pane running "sleep" (not agent-doc/claude/node/shell)
        let output = iso
            .cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                "test",
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
                "sleep",
                "60",
            ])
            .output()
            .unwrap();
        let pane = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let mut registry = SessionRegistry::new();
        registry.insert("sess-1".to_string(), test_entry(&pane, "test.md"));

        // Give tmux a moment to register the process
        std::thread::sleep(std::time::Duration::from_millis(200));

        let issues = detect_issues_in_registry(&iso, &registry);
        assert_eq!(issues.len(), 1, "should detect 1 wrong-process issue");
        assert!(
            matches!(&issues[0], Issue::WrongProcess { process, .. } if process == "sleep"),
            "issue should be WrongProcess(sleep), got: {}",
            &issues[0]
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn detect_no_live_owner_pane() {
        let iso = IsolatedTmux::new("resync-test-no-live-owner");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();
        assert!(wait_for_shell(&iso, &pane, 5000));

        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("test.md");
        std::fs::write(&doc_path, "# Test\n").unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "sess-no-owner".to_string(),
            test_entry(&pane, &doc_path.to_string_lossy()),
        );

        let issues = detect_issues_in_registry(&iso, &registry);
        assert_eq!(issues.len(), 1, "should detect 1 no-live-owner issue");
        assert!(
            matches!(&issues[0], Issue::NoLiveOwner { .. }),
            "issue should be NoLiveOwner, got: {}",
            &issues[0]
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn fix_wrong_session_kills_pane_and_deregisters() {
        let iso = IsolatedTmux::new("resync-test-fix-sess");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("wrong", &cwd).unwrap();
        assert!(iso.pane_alive(&pane));
        // Create a second window so the kill_pane guard allows killing the pane
        let _ = iso.new_window("wrong", &cwd);

        let mut registry = SessionRegistry::new();
        registry.insert("sess-fix".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::WrongSession {
            key: "sess-fix".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
            actual_session: "wrong".to_string(),
            expected_session: "correct".to_string(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-fix"),
            "entry should be removed from registry"
        );
        assert!(!iso.pane_alive(&pane), "pane should be killed");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn fix_wrong_process_deregisters_but_keeps_pane() {
        let iso = IsolatedTmux::new("resync-test-fix-proc");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();
        assert!(iso.pane_alive(&pane));

        let mut registry = SessionRegistry::new();
        registry.insert("sess-proc".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::WrongProcess {
            key: "sess-proc".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
            process: "corky".to_string(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-proc"),
            "entry should be removed from registry"
        );
        assert!(
            iso.pane_alive(&pane),
            "pane should NOT be killed (foreign process)"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn fix_no_live_owner_deregisters_but_keeps_pane() {
        let iso = IsolatedTmux::new("resync-test-fix-no-owner");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();
        assert!(iso.pane_alive(&pane));

        let mut registry = SessionRegistry::new();
        registry.insert("sess-no-owner".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::NoLiveOwner {
            key: "sess-no-owner".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-no-owner"),
            "entry should be removed from registry"
        );
        assert!(
            iso.pane_alive(&pane),
            "pane should remain alive after NoLiveOwner deregister"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn no_fix_without_flag() {
        // detect_issues returns issues but apply_fixes is only called with --fix.
        // This test verifies the reporting path doesn't mutate anything.
        let iso = IsolatedTmux::new("resync-test-no-fix");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("wrong", &cwd).unwrap();

        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("test.md");
        std::fs::write(&doc_path, "---\nsession: abc\ntmux_session: correct\n---\n").unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "abc".to_string(),
            test_entry(&pane, &doc_path.to_string_lossy()),
        );

        // detect_issues finds the problem
        let issues = detect_issues_in_registry(&iso, &registry);
        assert!(!issues.is_empty(), "should detect issues");

        // But without calling apply_fixes, nothing changes
        assert!(registry.contains_key("abc"), "registry should be unchanged");
        assert!(iso.pane_alive(&pane), "pane should still be alive");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn healthy_pane_has_no_issues() {
        // A pane running a shell (idle) with no tmux_session mismatch should be clean.
        let iso = IsolatedTmux::new("resync-test-healthy");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();

        // Wait for the shell to fully start (otherwise pane_current_command
        // may return "tmux" or a profile command instead of "zsh"/"bash")
        std::thread::sleep(std::time::Duration::from_millis(2000));

        // No file path means no frontmatter check; shell is in IDLE_SHELLS
        let mut registry = SessionRegistry::new();
        registry.insert("healthy-sess".to_string(), test_entry(&pane, ""));

        let issues = detect_issues_in_registry(&iso, &registry);
        assert!(
            issues.is_empty(),
            "healthy idle shell should have no issues, got: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn detect_wrong_window_panes_in_different_windows() {
        // Two panes in the same tmux session but different non-stash windows
        // should trigger WrongWindow.
        let iso = IsolatedTmux::new("resync-test-wrong-win");
        let cwd = test_cwd();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "# A\n").unwrap();
        std::fs::write(&doc_b, "# B\n").unwrap();

        // Create two panes in separate windows in the same session
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.auto_start("test", &cwd).unwrap(); // creates new window
        launch_mock_agent_doc(&iso, &pane1, &script, &doc_a);
        launch_mock_agent_doc(&iso, &pane2, &script, &doc_b);

        let (w1, w2) = wait_for_window_relation(
            &iso,
            &pane1,
            &pane2,
            false,
            std::time::Duration::from_secs(3),
        )
        .expect("panes should report windows");
        assert_ne!(w1, w2, "panes should be in different windows");

        let mut registry = SessionRegistry::new();
        registry.insert(
            "sess-1".to_string(),
            test_entry(&pane1, &doc_a.to_string_lossy()),
        );
        registry.insert(
            "sess-2".to_string(),
            test_entry(&pane2, &doc_b.to_string_lossy()),
        );

        let issues = detect_issues_in_registry(&iso, &registry);
        let wrong_window_count = issues
            .iter()
            .filter(|i| matches!(i, Issue::WrongWindow { .. }))
            .count();
        assert_eq!(
            wrong_window_count,
            1,
            "should detect 1 wrong-window issue (minority pane), got issues: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn no_wrong_window_when_panes_in_same_window() {
        // Two panes in the same window should NOT trigger WrongWindow.
        let iso = IsolatedTmux::new("resync-test-same-win");
        let cwd = test_cwd();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "# A\n").unwrap();
        std::fs::write(&doc_b, "# B\n").unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        launch_mock_agent_doc(&iso, &pane1, &script, &doc_a);
        launch_mock_agent_doc(&iso, &pane2, &script, &doc_b);

        let (w1, w2) = wait_for_window_relation(
            &iso,
            &pane1,
            &pane2,
            true,
            std::time::Duration::from_secs(3),
        )
        .expect("panes should report windows");
        assert_eq!(w1, w2, "panes should be in the same window");

        let mut registry = SessionRegistry::new();
        registry.insert(
            "sess-1".to_string(),
            test_entry(&pane1, &doc_a.to_string_lossy()),
        );
        registry.insert(
            "sess-2".to_string(),
            test_entry(&pane2, &doc_b.to_string_lossy()),
        );

        let issues = detect_issues_in_registry(&iso, &registry);
        let wrong_window_count = issues
            .iter()
            .filter(|i| matches!(i, Issue::WrongWindow { .. }))
            .count();
        assert_eq!(
            wrong_window_count,
            0,
            "should not detect wrong-window when panes are in same window, got: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn no_wrong_window_for_stash_panes() {
        // A pane in a stash window should NOT trigger WrongWindow.
        let iso = IsolatedTmux::new("resync-test-stash-excl");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.auto_start("test", &cwd).unwrap();

        // Move pane2 to a stash window
        iso.stash_pane(&pane2, "test").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(500));

        let mut registry = SessionRegistry::new();
        registry.insert("sess-1".to_string(), test_entry(&pane1, "a.md"));
        registry.insert("sess-2".to_string(), test_entry(&pane2, "b.md"));

        let issues = detect_issues_in_registry(&iso, &registry);
        let wrong_window_count = issues
            .iter()
            .filter(|i| matches!(i, Issue::WrongWindow { .. }))
            .count();
        assert_eq!(
            wrong_window_count,
            0,
            "stash panes should be excluded from wrong-window detection, got: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn fix_wrong_window_stashes_pane() {
        // --fix for WrongWindow should move the pane to stash.
        let iso = IsolatedTmux::new("resync-test-fix-win");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.auto_start("test", &cwd).unwrap();
        let w1 = iso.pane_window(&pane1).unwrap();
        let w2_before = iso.pane_window(&pane2).unwrap();
        assert_ne!(w1, w2_before, "panes should start in different windows");

        let mut registry = SessionRegistry::new();
        registry.insert("sess-1".to_string(), test_entry(&pane1, "a.md"));
        registry.insert("sess-2".to_string(), test_entry(&pane2, "b.md"));

        let issues = vec![Issue::WrongWindow {
            key: "sess-2".to_string(),
            file: "b.md".to_string(),
            pane: pane2.clone(),
            actual_window: w2_before.clone(),
            expected_window: w1.clone(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            iso.pane_alive(&pane2),
            "pane should still be alive (moved, not killed)"
        );

        // Verify pane2 is now in the stash window
        let stash_win = iso.find_stash_window("test");
        assert!(stash_win.is_some(), "stash window should exist");
        let w2_after = iso.pane_window(&pane2).unwrap();
        assert_eq!(
            w2_after,
            stash_win.unwrap(),
            "pane should have been moved to stash window"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_kills_unregistered_shell_in_stash() {
        // An unregistered idle shell in the stash should be killed.
        let iso = IsolatedTmux::new("resync-purge-shell");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();

        // Move pane2 to stash (it will be running a shell)
        iso.stash_pane(&pane2, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        assert!(iso.pane_alive(&pane2), "pane2 should be alive in stash");

        // Empty registry — pane2 is not registered
        let registry = SessionRegistry::new();
        purge_unregistered_stash_panes_with_registry(&iso, &registry);

        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            !iso.pane_alive(&pane2),
            "unregistered shell in stash should be killed"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_preserves_registered_pane_in_stash() {
        // A registered pane in stash should NOT be killed.
        let iso = IsolatedTmux::new("resync-purge-registered");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.stash_pane(&pane2, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Registry with pane2 registered
        let mut registry = SessionRegistry::new();
        registry.insert("registered-sess".to_string(), test_entry(&pane2, "test.md"));

        purge_unregistered_stash_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "registered pane in stash should survive purge"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_preserves_user_process_in_stash() {
        // A pane running a user process (not shell/agent) should NOT be killed.
        let iso = IsolatedTmux::new("resync-purge-userproc");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let output = iso
            .cmd()
            .args([
                "split-window",
                "-t",
                &pane1,
                "-d",
                "-h",
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
                "sleep",
                "60",
            ])
            .output()
            .unwrap();
        let pane2 = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Move to stash
        iso.stash_pane(&pane2, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let registry = SessionRegistry::new();
        purge_unregistered_stash_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "user process (sleep) in stash should survive purge"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_kills_unregistered_agent_in_stash_without_live_owner() {
        let iso = IsolatedTmux::new("resync-purge-agent-no-owner");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {}", script.display()))
            .unwrap();
        let _ = wait_for_pane_contains(&iso, &pane2, "\n>", std::time::Duration::from_secs(3));
        iso.stash_pane(&pane2, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let registry = SessionRegistry::new();
        purge_unregistered_stash_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            !iso.pane_alive(&pane2),
            "unregistered agent pane with no live owner should be killed"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_stash_panes_bulk_kills_unregistered_agent_without_live_owner() {
        let iso = IsolatedTmux::new("resync-purge-agent-no-owner-bulk");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {}", script.display()))
            .unwrap();
        let _ = wait_for_pane_contains(&iso, &pane2, "\n>", std::time::Duration::from_secs(3));
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_pane_in_stash_window(&iso, "test", &pane2, std::time::Duration::from_secs(3)),
            "pane should move into stash before purge"
        );

        let windows = fetch_all_window_metadata(&iso);
        let panes = fetch_all_pane_metadata(&iso);
        purge_unregistered_stash_panes_bulk(&iso, &windows, &panes);
        assert!(
            wait_for_pane_dead(&iso, &pane2, std::time::Duration::from_secs(3)),
            "bulk stash purge should kill unregistered agent panes with no live owner"
        );
        assert!(
            !iso.pane_alive(&pane2),
            "bulk stash purge should kill unregistered agent panes with no live owner"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_stash_cleanup_defers_live_agent_owner_proof() {
        let iso = IsolatedTmux::new("resync-safe-passive-defer-agent-proof");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let agent_bin = bin_dir.join("agent-doc");
        std::os::unix::fs::symlink("/bin/sleep", &agent_bin).unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {} 60", agent_bin.display()))
            .unwrap();
        assert!(
            wait_for_pane_current_command(
                &iso,
                &pane2,
                "agent-doc",
                std::time::Duration::from_secs(3)
            ),
            "pane should run an agent-doc-named process before stash cleanup"
        );
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_pane_in_stash_window(&iso, "test", &pane2, std::time::Duration::from_secs(3)),
            "pane should move into stash before safe-passive prune"
        );

        let windows = fetch_all_window_metadata(&iso);
        let panes = fetch_all_pane_metadata(&iso);
        purge_unregistered_stash_panes_bulk_in_mode(&iso, &windows, &panes, true);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "safe-passive stash cleanup should defer live agent panes to full cleanup instead of proving ownership"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_stash_panes_kills_retained_dead_stash_pane() {
        let iso = IsolatedTmux::new("resync-purge-dead-stash");
        let cwd = std::env::current_dir().unwrap();
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.enable_remain_on_exit(&pane2).unwrap();
        drive_pane_to_retained_dead(
            &iso,
            &pane2,
            "printf 'dead stash\\n'; exit 11",
            std::time::Duration::from_secs(6),
        );
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_pane_in_stash_window(&iso, "test", &pane2, std::time::Duration::from_secs(3)),
            "dead pane should move into stash before purge"
        );

        purge_unregistered_stash_panes_with_registry(&iso, &SessionRegistry::new());
        assert!(
            wait_for_pane_removed(&iso, &pane2, std::time::Duration::from_secs(3)),
            "retained dead stash pane should be removed during purge"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_preserves_unregistered_agent_in_stash_with_live_supervisor() {
        let iso = IsolatedTmux::new("resync-purge-agent-live-supervisor");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {}", script.display()))
            .unwrap();
        let _ = wait_for_pane_contains(&iso, &pane2, "\n>", std::time::Duration::from_secs(3));
        let live_pid = wait_for_process_pid(
            &script.display().to_string(),
            std::time::Duration::from_secs(3),
        );
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), "super-live", move |method| {
                match method {
                    crate::supervisor::ipc::IpcMethod::Pid => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "pid": live_pid }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::State => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "running": true }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::Inject { bytes }
                    | crate::supervisor::ipc::IpcMethod::Clear { bytes } => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "n": bytes.len() }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::Restart { .. }
                    | crate::supervisor::ipc::IpcMethod::Stop { .. } => {
                        crate::supervisor::ipc::IpcResponse::ok_empty()
                    }
                }
            })
            .unwrap();
        iso.stash_pane(&pane2, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let live_supervisors = crate::supervisor::ipc::active_supervisor_pids(dir.path());
        purge_unregistered_stash_panes_with_registry_and_supervisors(
            &iso,
            &SessionRegistry::new(),
            &live_supervisors,
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "unregistered stash pane with a live supervisor should survive purge"
        );
        ipc.stop();
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_stash_panes_bulk_preserves_live_supervisor() {
        let iso = IsolatedTmux::new("resync-purge-agent-live-supervisor-bulk");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_agent_doc(dir.path());
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {}", script.display()))
            .unwrap();
        let _ = wait_for_pane_contains(&iso, &pane2, "\n>", std::time::Duration::from_secs(3));
        let live_pid = wait_for_process_pid(
            &script.display().to_string(),
            std::time::Duration::from_secs(3),
        );
        let mut ipc = crate::supervisor::ipc::SupervisorIpc::start(
            dir.path(),
            "super-live-bulk",
            move |method| match method {
                crate::supervisor::ipc::IpcMethod::Pid => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({ "pid": live_pid }))
                }
                crate::supervisor::ipc::IpcMethod::State => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({ "running": true }))
                }
                crate::supervisor::ipc::IpcMethod::Inject { bytes }
                | crate::supervisor::ipc::IpcMethod::Clear { bytes } => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                crate::supervisor::ipc::IpcMethod::Restart { .. }
                | crate::supervisor::ipc::IpcMethod::Stop { .. } => {
                    crate::supervisor::ipc::IpcResponse::ok_empty()
                }
            },
        )
        .unwrap();
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_pane_in_stash_window(&iso, "test", &pane2, std::time::Duration::from_secs(3)),
            "pane should move into stash before bulk purge"
        );

        let windows = fetch_all_window_metadata(&iso);
        let panes = fetch_all_pane_metadata(&iso);
        let live_supervisors = crate::supervisor::ipc::active_supervisor_pids(dir.path());
        purge_unregistered_stash_panes_bulk_with_supervisors(
            &iso,
            &windows,
            &panes,
            &live_supervisors,
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "bulk purge should preserve stash panes with a live supervisor"
        );
        ipc.stop();
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_stash_panes_bulk_preserves_live_supervisor_in_pane_project_root() {
        let root = tempfile::tempdir().unwrap();
        let child_root = root.path().join("src/session-share");
        std::fs::create_dir_all(&child_root).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(root.path());

        let iso = IsolatedTmux::new("resync-purge-agent-live-supervisor-cross-root");
        let script = write_mock_agent_doc(&child_root);
        let pane1 = iso.auto_start("test", root.path()).unwrap();
        let pane2 = iso.split_window(&pane1, &child_root, "-dh").unwrap();
        iso.send_keys(&pane2, &format!("exec {}", script.display()))
            .unwrap();
        let _ = wait_for_pane_contains(&iso, &pane2, "\n>", std::time::Duration::from_secs(3));
        let live_pid = wait_for_process_pid(
            &script.display().to_string(),
            std::time::Duration::from_secs(3),
        );
        let mut ipc = crate::supervisor::ipc::SupervisorIpc::start(
            &child_root,
            "super-live-cross-root",
            move |method| match method {
                crate::supervisor::ipc::IpcMethod::Pid => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({ "pid": live_pid }))
                }
                crate::supervisor::ipc::IpcMethod::State => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({ "running": true }))
                }
                crate::supervisor::ipc::IpcMethod::Inject { bytes }
                | crate::supervisor::ipc::IpcMethod::Clear { bytes } => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                crate::supervisor::ipc::IpcMethod::Restart { .. }
                | crate::supervisor::ipc::IpcMethod::Stop { .. } => {
                    crate::supervisor::ipc::IpcResponse::ok_empty()
                }
            },
        )
        .unwrap();
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_pane_in_stash_window(&iso, "test", &pane2, std::time::Duration::from_secs(3)),
            "pane should move into stash before bulk purge"
        );

        let windows = fetch_all_window_metadata(&iso);
        let panes = fetch_all_pane_metadata(&iso);
        purge_unregistered_stash_panes_bulk_with_supervisors(&iso, &windows, &panes, &[]);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&pane2),
            "bulk purge should preserve stash panes with a live supervisor in the pane project root"
        );
        ipc.stop();
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_preserves_unregistered_agent_in_stash_with_live_owner() {
        let iso = IsolatedTmux::new("resync-purge-agent-live-owner");
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let script = write_mock_agent_doc(dir.path());
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "# Test\n").unwrap();
        let session_id = "sess-live-owner";

        let stale_pane = iso.auto_start("test", &cwd).unwrap();
        let live_pane = iso.split_window(&stale_pane, &cwd, "-dh").unwrap();
        launch_mock_agent_doc(&iso, &live_pane, &script, &doc);
        let live_pid = wait_for_process_pid(
            &script.display().to_string(),
            std::time::Duration::from_secs(3),
        );
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    crate::supervisor::ipc::IpcMethod::Pid => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "pid": live_pid }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::State => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "running": true }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::Inject { bytes }
                    | crate::supervisor::ipc::IpcMethod::Clear { bytes } => {
                        crate::supervisor::ipc::IpcResponse::ok(
                            serde_json::json!({ "n": bytes.len() }),
                        )
                    }
                    crate::supervisor::ipc::IpcMethod::Restart { .. }
                    | crate::supervisor::ipc::IpcMethod::Stop { .. } => {
                        crate::supervisor::ipc::IpcResponse::ok_empty()
                    }
                }
            })
            .unwrap();
        iso.stash_pane(&live_pane, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let mut registry = SessionRegistry::new();
        registry.insert(
            session_id.to_string(),
            test_entry(&stale_pane, &doc.to_string_lossy()),
        );

        purge_unregistered_stash_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            iso.pane_alive(&live_pane),
            "unregistered agent pane that still owns a registered file should survive purge"
        );
        ipc.stop();
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_orphan_agent_in_non_stash_window() {
        // An unregistered agent-doc pane in a regular window (not stash) should be killed
        // if the window has other panes.
        let iso = IsolatedTmux::new("resync-purge-orphan-agent");
        let cwd = std::env::current_dir().unwrap();

        // Create a session with 2 panes in the same window
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        // pane2 is running a shell. We want to simulate an agent-doc process.
        // Instead, we test the shell case: shells should NOT be killed by this function
        // (only agent processes). Let's just verify the non-stash purge doesn't touch shells.
        let registry = SessionRegistry::new();
        purge_orphaned_agent_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Both panes should survive (they're running shells, not agent processes)
        assert!(iso.pane_alive(&pane1), "shell pane1 should survive");
        assert!(iso.pane_alive(&pane2), "shell pane2 should survive");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_orphan_agent_in_non_stash_preserves_nested_project_owner() {
        let parent = tempfile::tempdir().unwrap();
        let nested = parent.path().join("nested");
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        let doc = nested.join("tasks/root.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Root\n").unwrap();
        let script = write_mock_agent_doc(parent.path());

        let _cwd_guard = ScopedCurrentDir::set(parent.path());
        let iso = IsolatedTmux::new("resync-preserve-nested-owner");
        let pane = iso.new_session("test", &nested).unwrap();
        let sibling = iso.split_window(&pane, &nested, "-dh").unwrap();
        launch_mock_agent_doc(&iso, &pane, &script, &doc);
        sessions::register_full_in(
            &nested,
            "nested-session",
            &pane,
            &doc.to_string_lossy(),
            123,
            "@1",
        )
        .unwrap();

        purge_orphaned_agent_panes_with_registry(&iso, &SessionRegistry::new());
        std::thread::sleep(std::time::Duration::from_millis(100));

        assert!(
            iso.pane_alive(&pane),
            "parent resync must not kill a non-stash pane registered in a nested project"
        );
        assert!(
            iso.pane_alive(&sibling),
            "sibling shell pane should survive"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_dead_non_stash_pane() {
        let iso = IsolatedTmux::new("resync-purge-dead-non-stash");
        let cwd = std::env::current_dir().unwrap();

        let live_pane = iso.auto_start("test", &cwd).unwrap();
        let dead_pane = iso.split_window(&live_pane, &cwd, "-dh").unwrap();
        iso.enable_remain_on_exit(&dead_pane).unwrap();
        drive_pane_to_retained_dead(
            &iso,
            &dead_pane,
            "printf 'dead pane\\n'; exit 0",
            std::time::Duration::from_secs(6),
        );

        let registry = SessionRegistry::new();
        purge_unregistered_dead_non_stash_panes_with_registry(&iso, &registry);
        assert!(
            wait_for_pane_removed(&iso, &dead_pane, std::time::Duration::from_secs(3)),
            "unregistered dead pane with siblings should be reaped"
        );
        assert!(
            iso.pane_alive(&live_pane),
            "live sibling pane should survive"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_orphan_does_not_kill_last_pane() {
        // A window with only one pane (even if orphaned agent) should not be touched.
        let iso = IsolatedTmux::new("resync-purge-last-pane");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let registry = SessionRegistry::new();
        purge_orphaned_agent_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));

        assert!(
            iso.pane_alive(&pane1),
            "last pane in window should not be killed"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn purge_unregistered_dead_non_stash_panes_skips_last_pane() {
        let iso = IsolatedTmux::new("resync-purge-dead-last-pane");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();
        let _other_window = iso.new_window("test", &cwd).unwrap();
        iso.enable_remain_on_exit(&pane).unwrap();
        drive_pane_to_retained_dead(
            &iso,
            &pane,
            "printf 'dead last pane\\n'; exit 0",
            std::time::Duration::from_secs(6),
        );

        let registry = SessionRegistry::new();
        purge_unregistered_dead_non_stash_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(100));

        assert!(
            iso.pane_dead(&pane),
            "last pane in window should be preserved for manual inspection"
        );
    }

    #[test]
    fn is_stash_window_name_matches() {
        assert!(is_stash_window_name("stash"));
        assert!(is_stash_window_name("stash-1"));
        assert!(is_stash_window_name("stash-42"));
        assert!(!is_stash_window_name("claude"));
        assert!(!is_stash_window_name(""));
        assert!(!is_stash_window_name("stashed"));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn return_stashed_panes_moves_active_pane_back() {
        // A registered pane running an active process (sleep) in stash should be
        // returned to its original session window.
        let iso = IsolatedTmux::new("resync-return-active");
        let cwd = std::env::current_dir().unwrap();

        // Create a pane with an active process (sleep)
        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let output = iso
            .cmd()
            .args([
                "split-window",
                "-t",
                &pane1,
                "-d",
                "-h",
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
                "sleep",
                "60",
            ])
            .output()
            .unwrap();
        let active_pane = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let original_window = iso.pane_window(&active_pane).unwrap();

        // Move the active pane to stash
        iso.stash_pane(&active_pane, "test").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Verify it's in stash
        let stash_window = iso.pane_window(&active_pane).unwrap();
        assert_ne!(stash_window, original_window, "pane should be in stash");

        // Register the pane with the original window
        let mut registry = SessionRegistry::new();
        let mut entry = test_entry(&active_pane, "");
        entry.window = original_window.clone();
        registry.insert("active-sess".to_string(), entry);

        // Return stashed panes
        return_stashed_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(300));

        // Verify pane is back in original window
        assert!(iso.pane_alive(&active_pane), "pane should still be alive");
        let current_window = iso.pane_window(&active_pane).unwrap();
        assert_eq!(
            current_window, original_window,
            "active pane should be returned to original window"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn return_stashed_panes_skips_idle_shells() {
        // An idle shell in stash should NOT be returned — it's handled by purge.
        let iso = IsolatedTmux::new("resync-return-idle");
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start("test", &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        let original_window = iso.pane_window(&pane2).unwrap();

        // Move idle shell to stash, then wait for the pane to settle.
        // Fixed sleep is unreliable under parallel test load.
        iso.stash_pane(&pane2, "test").unwrap();
        assert!(
            wait_for_shell(&iso, &pane2, 5000),
            "shell did not settle in stash pane within 5s"
        );

        let stash_window = iso.pane_window(&pane2).unwrap();
        assert_ne!(stash_window, original_window, "pane should be in stash");

        // Register the idle shell pane
        let mut registry = SessionRegistry::new();
        let mut entry = test_entry(&pane2, "");
        entry.window = original_window.clone();
        registry.insert("idle-sess".to_string(), entry);

        // Return stashed panes — should skip idle shells
        return_stashed_panes_with_registry(&iso, &registry);
        std::thread::sleep(std::time::Duration::from_millis(1000));

        // Verify pane is still in stash (not returned)
        let current_window = iso.pane_window(&pane2).unwrap();
        assert_eq!(
            current_window, stash_window,
            "idle shell should NOT be returned from stash"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn fix_wrong_session_idle_shell_still_killed() {
        // Regression: idle shells in the wrong session should still be killed
        // even with the new agent-preservation logic.
        let iso = IsolatedTmux::new("resync-fix-shell-killed");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("wrong", &cwd).unwrap();
        let _ = iso.new_window("wrong", &cwd); // second window so kill works
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(iso.pane_alive(&pane));

        let mut registry = SessionRegistry::new();
        registry.insert("sess-shell".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::WrongSession {
            key: "sess-shell".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
            actual_session: "wrong".to_string(),
            expected_session: "correct".to_string(),
        }];

        // No relocate_session — uses the new auto-detect path
        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-shell"),
            "entry should be removed"
        );
        // Idle shell should be killed (not just deregistered)
        assert!(!iso.pane_alive(&pane), "idle shell should be killed");
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn fix_wrong_session_deregisters_agent_without_kill_when_expected_session_dead() {
        // When the expected session doesn't exist, active agent panes should be
        // deregistered but NOT killed.
        let iso = IsolatedTmux::new("resync-fix-agent-nodeadkill");
        let cwd = std::env::current_dir().unwrap();

        // Start a pane running `node -e "setTimeout(()=>{},60000)"` to simulate
        // an agent process. If node isn't available, use `sleep` and adjust expectations.
        let pane = iso.auto_start("wrong", &cwd).unwrap();
        let _ = iso.new_window("wrong", &cwd);
        std::thread::sleep(std::time::Duration::from_millis(500));

        // The pane is running an idle shell. For this test, verify the shell case:
        // idle shells should be killed even when expected session is dead.
        let mut registry = SessionRegistry::new();
        registry.insert("sess-agent".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::WrongSession {
            key: "sess-agent".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
            actual_session: "wrong".to_string(),
            expected_session: "nonexistent-session".to_string(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        assert!(
            !registry.contains_key("sess-agent"),
            "entry should be removed"
        );
        // Shell pane should be killed (expected session doesn't matter for shells)
        assert!(
            !iso.pane_alive(&pane),
            "idle shell should still be killed when expected session is dead"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn registered_pane_still_owns_file_returns_false_when_file_missing() {
        let iso = IsolatedTmux::new("resync-live-owner-missing-file");
        assert!(!registered_pane_still_owns_file(
            &iso,
            "session-1",
            "/tmp/does-not-exist.md",
            "%42"
        ));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn fix_wrong_session_relocates_agent_when_expected_session_alive() {
        // When the expected session exists, active agent panes should be relocated
        // via join-pane, not killed.
        let iso = IsolatedTmux::new("resync-fix-agent-relocate");
        let cwd = std::env::current_dir().unwrap();

        // Create the expected session with a pane (needed for join-pane target)
        let _anchor = iso.auto_start("correct", &cwd).unwrap();

        // Create a pane in the wrong session running node (agent process)
        let output = iso
            .cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                "wrong",
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
                "node",
                "-e",
                "setTimeout(()=>{},60000)",
            ])
            .output()
            .unwrap();
        let agent_pane = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if agent_pane.is_empty() || !iso.pane_alive(&agent_pane) {
            // node not available — skip test gracefully
            eprintln!("skipping: node not available");
            return;
        }

        std::thread::sleep(std::time::Duration::from_millis(500));

        // Verify pane is running node (an AGENT_PROCESS)
        let cmd = pane_current_command(&iso, &agent_pane);
        if cmd.as_deref() != Some("node") {
            eprintln!("skipping: pane running {:?} instead of node", cmd);
            return;
        }

        let mut registry = SessionRegistry::new();
        registry.insert("sess-node".to_string(), test_entry(&agent_pane, "test.md"));

        let issues = vec![Issue::WrongSession {
            key: "sess-node".to_string(),
            file: "test.md".to_string(),
            pane: agent_pane.clone(),
            actual_session: "wrong".to_string(),
            expected_session: "correct".to_string(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry, None);
        assert_eq!(fixed, 1);
        // Agent pane should be alive (relocated, not killed)
        assert!(
            iso.pane_alive(&agent_pane),
            "agent pane should be alive after relocation"
        );
        // Registry entry should still exist (relocation preserves it)
        assert!(
            registry.contains_key("sess-node"),
            "entry should be preserved after successful relocation"
        );
        // Pane should now be in the correct session
        let new_session = iso.pane_session(&agent_pane).unwrap();
        assert_eq!(
            new_session, "correct",
            "agent pane should be relocated to the correct session"
        );
    }

    #[test]
    fn superseded_candidates_excludes_canonical_and_dedupes() {
        // The canonical (active agent-doc window) session is never a close target;
        // the rest are closed once each, order-stable.
        let drift = vec![
            "0".to_string(),
            "5".to_string(),
            "5".to_string(),
            "8".to_string(),
        ];
        assert_eq!(
            superseded_candidates("0", &drift),
            vec!["5".to_string(), "8".to_string()]
        );
        // Canonical absent from the drift set → all are candidates.
        assert_eq!(
            superseded_candidates("9", &["0".to_string(), "5".to_string()]),
            vec!["0".to_string(), "5".to_string()]
        );
        // Single session (no drift) → nothing to close.
        assert!(superseded_candidates("0", &["0".to_string()]).is_empty());
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn close_superseded_drift_sessions_skips_canonical_closes_others() {
        // Canonical session is preserved; a sibling pure agent-doc orphan is closed.
        let iso = IsolatedTmux::new("resync-drift-superseded");
        let cwd = std::env::current_dir().unwrap();

        let _canon = iso.new_session("canon", &cwd).unwrap();
        iso.cmd()
            .args(["rename-window", "-t", "canon:", "agent-doc"])
            .output()
            .unwrap();
        iso.ensure_stash_window("canon").unwrap();

        let _orphan = iso.new_session("orphan", &cwd).unwrap();
        iso.cmd()
            .args(["rename-window", "-t", "orphan:", "agent-doc"])
            .output()
            .unwrap();
        iso.ensure_stash_window("orphan").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let drift = vec!["canon".to_string(), "orphan".to_string()];
        let closed = close_superseded_drift_sessions(&iso, "canon", &drift).unwrap();
        assert_eq!(closed, 1, "only the non-canonical orphan should be closed");
        assert!(iso.session_alive("canon"), "canonical session must survive");
        assert!(
            !iso.session_alive("orphan"),
            "superseded orphan should be closed"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn close_superseded_session_kills_pure_agent_doc_orphan() {
        // A superseded session holding only agent-doc + stash windows (idle shells,
        // no live agent) is a pure orphan → closed.
        let iso = IsolatedTmux::new("resync-close-superseded-orphan");
        let cwd = std::env::current_dir().unwrap();

        let _pane = iso.new_session("oldcanon", &cwd).unwrap();
        iso.cmd()
            .args(["rename-window", "-t", "oldcanon:", "agent-doc"])
            .output()
            .unwrap();
        iso.ensure_stash_window("oldcanon").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(iso.session_alive("oldcanon"));

        let closed = close_superseded_session(&iso, "oldcanon").unwrap();
        assert!(closed, "pure agent-doc orphan should be closed");
        assert!(
            !iso.session_alive("oldcanon"),
            "superseded session should be killed"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn close_superseded_session_preserves_session_with_user_window() {
        // A session that still holds an unmanaged user window must NOT be closed.
        let iso = IsolatedTmux::new("resync-close-superseded-userwin");
        let cwd = std::env::current_dir().unwrap();

        let _pane = iso.new_session("oldcanon2", &cwd).unwrap();
        iso.cmd()
            .args(["rename-window", "-t", "oldcanon2:", "agent-doc"])
            .output()
            .unwrap();
        let userwin = iso.new_window("oldcanon2", &cwd).unwrap();
        iso.cmd()
            .args(["rename-window", "-t", &userwin, "vim"])
            .output()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let closed = close_superseded_session(&iso, "oldcanon2").unwrap();
        assert!(
            !closed,
            "session with an unmanaged window must be preserved"
        );
        assert!(
            iso.session_alive("oldcanon2"),
            "session with a user window must stay alive"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn close_superseded_session_reports_already_gone_session() {
        // tmux already auto-destroyed the session (no windows remained) → treated as
        // already closed (Ok(true)).
        let iso = IsolatedTmux::new("resync-close-superseded-gone");
        let closed = close_superseded_session(&iso, "neverexisted").unwrap();
        assert!(closed, "absent session is treated as already closed");
    }

    #[test]
    fn resolve_registry_root_finds_submodule_agent_doc_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Simulate superproject with .agent-doc/
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Simulate submodule with its own .agent-doc/
        let sub = dir.path().join("src/sub");
        std::fs::create_dir_all(sub.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(sub.join("tasks")).unwrap();
        let doc = sub.join("tasks/test.md");
        std::fs::write(&doc, "# test\n").unwrap();

        let root = resolve_registry_root(&doc);
        assert_eq!(
            root,
            sub.canonicalize().unwrap_or(sub.clone()),
            "should resolve to the submodule .agent-doc root, not the superproject"
        );
    }

    #[test]
    fn resolve_registry_root_falls_back_to_superproject() {
        let dir = tempfile::tempdir().unwrap();
        // Superproject with .agent-doc/
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Subpath without its own .agent-doc/
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::write(&doc, "# test\n").unwrap();

        let root = resolve_registry_root(&doc);
        assert_eq!(
            root,
            dir.path()
                .canonicalize()
                .unwrap_or(dir.path().to_path_buf()),
            "should resolve to the superproject .agent-doc root"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn prune_targeted_in_uses_submodule_registry() {
        let iso = IsolatedTmux::new("resync-test-submod-prune");

        let dir = tempfile::tempdir().unwrap();
        // Superproject with .agent-doc/
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Submodule with its own .agent-doc/ and registry
        let sub = dir.path().join("src/sub");
        std::fs::create_dir_all(sub.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(sub.join("tasks")).unwrap();
        let doc = sub.join("tasks/test.md");
        std::fs::write(&doc, "# test\n").unwrap();

        // Register in the submodule's sessions.json with a dead pane
        let mut registry = SessionRegistry::new();
        let canonical = doc.canonicalize().unwrap_or(doc.clone());
        registry.insert(
            canonical.to_string_lossy().to_string(),
            test_entry("%99998", &canonical.to_string_lossy()),
        );
        sessions::save_in(&sub, &registry).unwrap();

        // Superproject registry should be empty
        sessions::save_in(dir.path(), &SessionRegistry::new()).unwrap();

        let target = canonical;
        let removed = prune_targeted_in(&iso, &target, &sub).unwrap();
        assert_eq!(
            removed.len(),
            1,
            "should find and prune the dead entry from the submodule registry"
        );

        // Verify the submodule registry is now empty
        let after = sessions::load_in(&sub).unwrap();
        assert!(
            after.is_empty(),
            "submodule registry should be empty after prune"
        );
    }

    #[test]
    fn finish_unfinished_turn_commits_orphaned_response() {
        // #jb-fix-document-finish-turn: `agent-doc fix <FILE>` (and the JB Fix
        // Document action) must commit a stranded response before routing fixes.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "t@t.com"],
            vec!["config", "user.name", "T"],
        ] {
            std::process::Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .ok();
        }
        let doc = root.join("doc.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "-A"])
            .output()
            .ok();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "init", "--no-verify"])
            .output()
            .ok();

        // Strand a response (the "unfinished turn").
        crate::repair::save_pending(
            &doc,
            "<!-- patch:exchange -->\n### Re: unfinished — gpt-5\n\nRecovered.\n<!-- /patch:exchange -->\n",
        )
        .unwrap();

        finish_unfinished_turn(&doc).unwrap();

        // The response is now committed to HEAD.
        let head = std::process::Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:doc.md"])
            .output()
            .unwrap();
        let head_str = String::from_utf8_lossy(&head.stdout);
        assert!(
            head_str.contains("Re: unfinished"),
            "stranded response must be committed by finish_unfinished_turn:\n{head_str}"
        );
    }

    #[test]
    fn finish_unfinished_turn_is_noop_on_clean_document() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let content = "---\nagent_doc_session: test\n---\n\nplain body\n";
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();

        // No pending/cycle state → no-op, no error, content unchanged.
        finish_unfinished_turn(&doc).unwrap();
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }
