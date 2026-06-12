    use super::*;
    use std::ffi::CString;
    use tempfile::TempDir;

    #[test]
    fn test_write_ack_content_creates_file() {
        let tmp = TempDir::new().unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-id-123").unwrap();
        let content = CString::new("hello world").unwrap();

        let result = unsafe {
            agent_doc_write_ack_content(project_root.as_ptr(), patch_id.as_ptr(), content.as_ptr())
        };
        assert_eq!(result, 1, "should return 1 on success");

        let sidecar = tmp
            .path()
            .join(".agent-doc/ack-content/test-patch-id-123.md");
        assert!(
            sidecar.exists(),
            "sidecar file should exist at {:?}",
            sidecar
        );
        assert_eq!(std::fs::read_to_string(&sidecar).unwrap(), "hello world");
    }

    #[test]
    fn test_is_claimed_by_force_disk_present() {
        let tmp = TempDir::new().unwrap();
        let claimed_dir = tmp.path().join(".agent-doc/claimed-patches");
        std::fs::create_dir_all(&claimed_dir).unwrap();
        std::fs::write(claimed_dir.join("test-patch-456"), "").unwrap();

        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-456").unwrap();

        let claimed =
            unsafe { agent_doc_is_claimed_by_force_disk(project_root.as_ptr(), patch_id.as_ptr()) };
        assert_eq!(claimed, 1, "should return 1 when sentinel exists");
        assert!(
            claimed_dir.join("test-patch-456").exists(),
            "sentinel should remain so repeated watcher passes skip the patch"
        );
        let claimed_again =
            unsafe { agent_doc_is_claimed_by_force_disk(project_root.as_ptr(), patch_id.as_ptr()) };
        assert_eq!(claimed_again, 1, "claimed sentinel should be durable");
    }

    #[test]
    fn plugin_watch_readonly_always_demotes_post_cutover() {
        // 08b cutover complete: the plugin WatchService file-apply path is
        // unconditionally read-only — the controller-owned watcher + socket IPC
        // are the sole writer (#dsqa / #pcp7).
        let tmp = TempDir::new().unwrap();
        let file = CString::new(tmp.path().join("plan.md").to_str().unwrap()).unwrap();
        let readonly = unsafe { agent_doc_plugin_watch_readonly(file.as_ptr()) };
        assert_eq!(
            readonly, 1,
            "post-cutover the plugin must never apply file-IPC patches"
        );
    }

    #[test]
    fn test_is_claimed_by_force_disk_absent() {
        let tmp = TempDir::new().unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("nonexistent-patch").unwrap();

        let claimed =
            unsafe { agent_doc_is_claimed_by_force_disk(project_root.as_ptr(), patch_id.as_ptr()) };
        assert_eq!(claimed, 0, "should return 0 when sentinel absent");
    }

    #[test]
    fn patch_content_already_committed_requires_disk_to_match_head() {
        use std::process::Command;
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        macro_rules! git {
            ($($arg:expr),+) => {
                Command::new("git")
                    .current_dir(root)
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_INDEX_FILE")
                    .env_remove("GIT_WORK_TREE")
                    .args([$($arg),+])
                    .output()
                    .unwrap()
            };
        }

        git!["init"];
        git!["config", "user.email", "test@test.com"];
        git!["config", "user.name", "Test"];

        let doc = root.join("doc.md");
        let committed = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: committed — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, committed).unwrap();
        git!["add", "doc.md"];
        git!["commit", "-m", "committed response", "--no-verify"];

        let file_path = CString::new(doc.to_string_lossy().as_ref()).unwrap();
        let patch_content = CString::new("### Re: committed — gpt-5\n\nDone.\n").unwrap();
        assert_eq!(
            unsafe {
                agent_doc_patch_content_already_committed(
                    file_path.as_ptr(),
                    patch_content.as_ptr(),
                )
            },
            1,
            "committed disk content should prove the patch is already present"
        );

        std::fs::write(
            &doc,
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        assert_eq!(
            unsafe {
                agent_doc_patch_content_already_committed(
                    file_path.as_ptr(),
                    patch_content.as_ptr(),
                )
            },
            0,
            "HEAD alone must not be enough when disk drifted away from HEAD"
        );
    }

    // --- Fix 4: agent_doc_commit FFI export ---

    #[test]
    fn agent_doc_commit_returns_false_for_null() {
        let result = unsafe { agent_doc_commit(std::ptr::null()) };
        assert_eq!(result, 0, "null path should return 0");
    }

    #[test]
    fn ffi_git_commit_commits_staged_file() {
        use std::process::Command;
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Helper: run git command isolated from any parent git hook env vars.
        // Pre-commit hooks set GIT_DIR/GIT_INDEX_FILE which would confuse
        // git commands targeting the temp repo.
        macro_rules! git {
            ($($arg:expr),+) => {
                Command::new("git")
                    .current_dir(root)
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_INDEX_FILE")
                    .env_remove("GIT_WORK_TREE")
                    .args([$($arg),+])
                    .output()
                    .unwrap()
            };
        }

        // Set up minimal git repo
        git!["init"];
        git!["config", "user.email", "test@test.com"];
        git!["config", "user.name", "Test"];

        // Commit initial file so HEAD exists
        let readme = root.join("README.md");
        std::fs::write(&readme, "# test\n").unwrap();
        git!["add", "README.md"];
        git!["commit", "-m", "initial", "--no-verify"];

        // Create a document file (not yet committed)
        let doc = root.join("session.md");
        std::fs::write(&doc, "# content\n").unwrap();

        // ffi_git_commit should stage + commit the doc
        let ok = ffi_git_commit(&doc);
        assert!(ok, "ffi_git_commit should succeed for a valid git repo");

        // Verify git log contains the commit
        let log = git!["log", "--oneline", "-2"];
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(session):"),
            "git log should contain agent-doc commit, got:\n{log_str}"
        );
    }
