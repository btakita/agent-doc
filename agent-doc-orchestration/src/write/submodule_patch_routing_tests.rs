    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    /// Helper: run a git command in `dir` with isolated user.name/email so the
    /// command works in CI environments that lack global git config. Asserts
    /// the command succeeds and prints stderr on failure.
    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "protocol.file.allow=always",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("git command failed to spawn");
        assert!(
            out.status.success(),
            "git {:?} failed: stderr={}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn resolve_ipc_project_root_uses_nearest_agent_doc_for_submodule_file() {
        // Build a parent+submodule layout. Verify that a document inside the
        // submodule resolves to the SUBMODULE's .agent-doc/ root, not the
        // superproject. This matches the IDE plugin's resolveRootFor logic so
        // ack-content paths agree between Rust and Kotlin.
        let parent_dir = TempDir::new().unwrap();
        let sub_src_dir = TempDir::new().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        // Bootstrap a "remote" submodule repo with one committed file.
        git(&sub_src, &["init"]);
        std::fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init"]);

        // Bootstrap parent repo and add the submodule under src/submodule.
        git(&parent, &["init"]);
        std::fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );

        // Submodule has its own .agent-doc — the IDE plugin registers it as a root.
        let submodule_root = parent.join("src/submodule");
        std::fs::create_dir_all(submodule_root.join(".agent-doc/patches")).unwrap();

        // Place a document inside the submodule.
        let doc = submodule_root.join("test.md");
        std::fs::write(
            &doc,
            "---\n---\n\n<!-- agent:exchange -->c<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let canonical = doc.canonicalize().unwrap();
        let project_root = resolve_ipc_project_root(&canonical);

        assert_eq!(
            project_root, submodule_root,
            "submodule file must resolve to submodule root (nearest .agent-doc/) to match IDE plugin routing"
        );

        // The superproject must NOT be returned — ack-content would diverge.
        assert_ne!(
            project_root, parent,
            "must not return the superproject — ack-content written at submodule root would not be found"
        );
    }

    #[test]
    fn resolve_ipc_project_root_ignores_agent_doc_outside_git_toplevel() {
        let outer_dir = TempDir::new().unwrap();
        let outer = outer_dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(outer.join(".agent-doc/patches")).unwrap();

        let nested = outer.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        git(&nested, &["init"]);
        let doc = nested.join("session.md");
        std::fs::write(
            &doc,
            "---\n---\n\n<!-- agent:exchange -->c<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let canonical = doc.canonicalize().unwrap();
        let project_root = resolve_ipc_project_root(&canonical);

        assert_eq!(
            project_root, nested,
            "a parent .agent-doc outside the current git toplevel must not capture IPC routing"
        );
    }

    #[test]
    fn required_closeout_fails_when_parent_submodule_pointer_commit_fails() {
        let parent_dir = TempDir::new().unwrap();
        let sub_src_dir = TempDir::new().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        git(&sub_src, &["init"]);
        std::fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init"]);

        git(&parent, &["init"]);
        std::fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );
        git(&parent, &["commit", "-m", "add submodule"]);

        let submodule_root = parent.join("src/submodule");
        git(
            &submodule_root,
            &["config", "user.email", "test@example.com"],
        );
        git(&submodule_root, &["config", "user.name", "Test"]);
        std::fs::create_dir_all(submodule_root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(submodule_root.join(".agent-doc/state/cycles")).unwrap();

        let doc = submodule_root.join("session.md");
        let initial = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, initial).unwrap();
        git(&submodule_root, &["add", "session.md"]);
        git(&submodule_root, &["commit", "-m", "add doc"]);
        git(&parent, &["add", "src/submodule"]);
        git(&parent, &["commit", "-m", "record doc commit"]);

        let parent_git_dir = Command::new("git")
            .current_dir(&parent)
            .args(["rev-parse", "--absolute-git-dir"])
            .output()
            .unwrap();
        assert!(parent_git_dir.status.success());
        let parent_git_dir = PathBuf::from(String::from_utf8_lossy(&parent_git_dir.stdout).trim());
        std::fs::write(parent_git_dir.join("index.lock"), "held by test").unwrap();

        let updated = initial.replace(
            "<!-- /agent:exchange -->\n",
            "### Re: reply — gpt-5\nbody\n<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, &updated).unwrap();
        crate::snapshot::save(&doc, &updated).unwrap();

        let err = super::complete_required_closeout(&doc).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("parent submodule pointer is not committed"),
            "strict closeout should name the missing parent layer, got: {message}"
        );
        assert!(
            message.contains("agent-doc commit"),
            "strict closeout should prescribe the idempotent commit recovery, got: {message}"
        );
        assert!(
            crate::git::submodule_pointer_drift(&doc).unwrap().is_some(),
            "parent gitlink should remain stale when parent commit fails"
        );
    }

    // Note: a "not in git repo" fallback test is intentionally omitted because
    // /tmp tempdirs are typically nested inside the developer's checkout (the
    // agent-doc workspace itself is a git repo), so `git rev-parse
    // --show-toplevel` from `/tmp/...` walks up into the source tree. The
    // fallback path is exercised in production by non-git workspaces.

    /// Helper: start a fake socket listener that ACKs every message.
    /// Returns a handle that keeps the listener alive until dropped.
    fn start_fake_listener(project_root: &Path) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let root_clone = root.clone();
            let _ = crate::ipc_socket::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                // Write ack-content sidecar so poll_ack_content_sidecar succeeds
                let ack_dir = root_clone.join(".agent-doc/ack-content");
                let _ = std::fs::create_dir_all(&ack_dir);
                let file_path = v.get("file").and_then(|f| f.as_str()).unwrap_or("");
                let content = if !file_path.is_empty() {
                    let file = Path::new(file_path);
                    let before = std::fs::read_to_string(file).unwrap_or_default();
                    let patches = v
                        .get("patches")
                        .and_then(|value| value.as_array())
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| {
                                    let name = item
                                        .get("component")
                                        .or_else(|| item.get("name"))
                                        .and_then(|value| value.as_str())?;
                                    let content =
                                        item.get("content").and_then(|value| value.as_str())?;
                                    Some(crate::template::PatchBlock::new(name, content))
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let unmatched = v
                        .get("unmatched")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    let after = crate::template::apply_patches(&before, &patches, unmatched, file)
                        .unwrap_or(before);
                    let _ = std::fs::write(file, &after);
                    after
                } else {
                    String::new()
                };
                let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &content);
                Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
            });
        })
    }

    fn start_already_applied_listener(project_root: &Path) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let _ = crate::ipc_socket::start_listener(&root, |_msg| {
                Some(
                    serde_json::json!({
                        "type": "ack",
                        "status": "error",
                        "reason": "already_applied"
                    })
                    .to_string(),
                )
            });
        })
    }

    fn start_fixed_ack_content_listener(
        project_root: &Path,
        ack_content: String,
    ) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let root_clone = root.clone();
            let _ = crate::ipc_socket::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                let ack_dir = root_clone.join(".agent-doc/ack-content");
                let _ = std::fs::create_dir_all(&ack_dir);
                if let Some(file_path) = v.get("file").and_then(|f| f.as_str()) {
                    let _ = std::fs::write(file_path, &ack_content);
                }
                let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &ack_content);
                Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
            });
        })
    }

    /// Helper: wait for the socket listener to become connectable (up to 1s).
    fn wait_for_listener(project_root: &Path) {
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(project_root) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("fake socket listener did not start within 1s");
    }

    #[test]
    fn try_ipc_routes_to_submodule_root_not_superproject() {
        // Verify that try_ipc routes patches to the SUBMODULE's own .agent-doc/
        // root, not the superproject. The submodule has its own .agent-doc/ so
        // the IDE plugin's resolveRootFor and Rust's find_project_root both
        // return the submodule root, keeping ack-content paths in sync.
        let parent_dir = TempDir::new().unwrap();
        let sub_src_dir = TempDir::new().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        // Bootstrap "remote" submodule repo with one commit.
        git(&sub_src, &["init"]);
        std::fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init"]);

        // Bootstrap parent repo and add the submodule.
        git(&parent, &["init"]);
        std::fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );

        // Submodule has its own .agent-doc/ — mirrors the real boost-client layout.
        let submodule_root = parent.join("src/submodule");
        std::fs::create_dir_all(submodule_root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(submodule_root.join(".agent-doc/crdt")).unwrap();

        // Start a fake socket listener on the SUBMODULE root (not the parent).
        let _listener = start_fake_listener(&submodule_root);
        wait_for_listener(&submodule_root);

        // Place a document inside the submodule.
        let doc = submodule_root.join("test.md");
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->content<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "test response");

        // try_ipc should route to the submodule's socket listener and succeed.
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "try_ipc should succeed via socket IPC routed to the submodule root"
        );

        // Verify the parent did NOT get the patch file.
        let parent_patches = parent.join(".agent-doc/patches");
        assert!(
            !parent_patches.exists(),
            "parent should NOT receive patch files — submodule routes to its own .agent-doc/"
        );
    }

    #[test]
    fn try_ipc_routes_to_git_toplevel_for_non_submodule() {
        // Verify that try_ipc routes patches to the git toplevel (not a
        // superproject) when the document lives in a plain git repo. This
        // exercises the git_toplevel_at path (step 2 in resolve_ipc_project_root).
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();

        // Initialize a plain git repo (not a submodule of anything).
        git(&root, &["init"]);
        std::fs::write(root.join("README.md"), "root").unwrap();
        git(&root, &["add", "README.md"]);
        git(&root, &["commit", "-m", "init"]);

        // Create .agent-doc structure.
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/crdt")).unwrap();

        // Start a fake socket listener.
        let _listener = start_fake_listener(&root);
        wait_for_listener(&root);

        // Create a document in a subdirectory.
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let doc = root.join("tasks/test.md");
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->content<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "response");

        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "try_ipc should succeed via socket IPC routed to the git toplevel"
        );
    }

    #[test]
    fn try_ipc_already_applied_socket_adopts_disk_when_response_is_present() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in [
            "patches",
            "snapshots",
            "crdt",
            "logs",
            "state/cycles",
            "claimed-patches",
        ] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }

        let doc = root.join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let live_already_applied_with_user_edit = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "User typed the next prompt while finalize was running.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();
        fs::write(&doc, live_already_applied_with_user_edit).unwrap();

        let _listener = start_already_applied_listener(&root);
        wait_for_listener(&root);

        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: Please reply — gpt-5\n\nAnswered.",
        );
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("already-applied-patch"),
        )
        .unwrap();

        assert!(
            result.success,
            "already_applied socket ack is a consumed editor write"
        );
        assert_eq!(
            crate::snapshot::load(&doc).unwrap().as_deref(),
            Some(live_already_applied_with_user_edit),
            "already_applied must adopt disk content when it contains the response plus live user edits"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live_already_applied_with_user_edit,
            "live editor content should remain the committed snapshot candidate"
        );
        assert!(
            !crate::cycle_state::load(&doc)
                .unwrap()
                .unwrap()
                .ipc_snapshot_adoption_blocked,
            "safe disk adoption must not leave a later snapshot-absorb block"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_already_applied_skip_file_fallback")
                && log.contains("ipc_socket_already_applied_live_buffer_diverged")
                && log.contains("ipc_socket_already_applied_snapshot")
                && log.contains("snap_source=file_read"),
            "already_applied disk adoption should be auditable:\n{log}"
        );
        // #6cmx/#wy0y: this scenario IS typing-during-finalize (live buffer has a
        // user edit beyond our content), so it must emit the explicit verification
        // marker with the response intact — one greppable line proving completion.
        assert!(
            log.contains("prompt_drift=true"),
            "user-edit divergence is a prompt-drift case:\n{log}"
        );
        assert!(
            log.contains("finalize_typing_during_write") && log.contains("response_present=true"),
            "typing-during-finalize must log finalize_typing_during_write with response_present:\n{log}"
        );
    }

    #[test]
    fn try_ipc_already_applied_socket_dedupes_duplicate_response_before_snapshot() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in [
            "patches",
            "snapshots",
            "crdt",
            "logs",
            "state/cycles",
            "claimed-patches",
        ] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }

        let doc = root.join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let duplicated_live_buffer = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();
        fs::write(&doc, duplicated_live_buffer).unwrap();

        let _listener = start_already_applied_listener(&root);
        wait_for_listener(&root);

        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: Please reply — gpt-5\n\nAnswered.",
        );
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("already-applied-duplicate"),
        )
        .unwrap();

        assert!(result.success);
        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap.matches("### Re: Please reply — gpt-5").count(),
            1,
            "already_applied snapshot must dedupe duplicate response headings: {snap}"
        );
        let disk = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            disk.matches("### Re: Please reply — gpt-5").count(),
            1,
            "already_applied disk repair must converge with deduped snapshot: {disk}"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_dedupe_repaired_working_tree")
                && log.contains("ipc_socket_already_applied_snapshot"),
            "dedupe repair should be logged:\n{log}"
        );
    }

    #[test]
    fn already_applied_socket_missing_disk_response_repairs_visible_without_file_fallback() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in ["snapshots", "crdt", "logs", "state/cycles"] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }
        let doc = root.join("session.md");
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let stale_disk_with_live_prompt = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ Follow-up typed while closeout saved\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let repaired_visible = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ Follow-up typed while closeout saved\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        fs::write(&doc, stale_disk_with_live_prompt).unwrap();

        let outcome = persist_already_applied_socket_content_ours_snapshot(
            &doc,
            "already-applied-missing",
            Some(baseline),
            Some(content_ours),
            None,
            "### Re: Please reply — gpt-5\n\nAnswered.\n",
        )
        .unwrap();

        assert_eq!(outcome, AlreadyAppliedSnapshotOutcome::Persisted);
        assert_eq!(
            crate::snapshot::load(&doc).unwrap().as_deref(),
            Some(content_ours),
            "missing disk response must keep the committed snapshot at agent-owned content_ours"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            repaired_visible,
            "visible repair must add the response without deleting the live follow-up prompt"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_already_applied_missing_disk_response_repaired")
                && log.contains("recovery=content_ours_snapshot_visible_response_repair")
                && !log.contains("ipc_socket_already_applied_fallback_to_file_ipc"),
            "missing-response already_applied must not reapply through file IPC:\n{log}"
        );
    }

    #[test]
    fn socket_ack_content_prompt_duplication_uses_content_ours_and_repairs_visible_buffer() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let agent_doc_dir = root.join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("state").join("cycles")).unwrap();

        let doc = root.join("doc.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:before -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please set the production RESEND_API_KEY\n",
            "### Re: Production key — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:ours -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let duplicated_ack_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please set the production RESEND_API_KEY\n",
            "❯ Please set the production RESEND_API_KEY\n",
            "### Re: Production key — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:bad -->\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();

        let _listener = start_fixed_ack_content_listener(&root, duplicated_ack_content.to_string());
        wait_for_listener(&root);

        let patch =
            crate::template::PatchBlock::new("exchange", "### Re: Production key — gpt-5\n\nDone.");
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("duplicated-ack-content"),
        )
        .unwrap();

        assert!(
            result.success,
            "IPC delivery should remain successful while snapshot adoption falls back"
        );
        assert_eq!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(content_ours),
            "duplicated ack-content must not become the committed snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            content_ours,
            "visible duplicated ack-content should be repaired from the guarded response image"
        );
        assert!(
            crate::cycle_state::load(&doc)
                .unwrap()
                .unwrap()
                .ipc_snapshot_adoption_blocked,
            "later commit stages must not absorb the rejected duplicate sidecar"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=prompt_duplication_in_ack_content")
                && log.contains("duplicate_prompt_count=1")
                && log.contains("ipc_dedupe_repaired_working_tree"),
            "duplicate sidecar rejection and visible repair should be logged:\n{log}"
        );
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=prompt_duplication_in_ack_content")
                && log.contains("recovery=content_ours_snapshot_and_visible_repair"),
            "duplicate prompt ACK should name its failed invariant and recovery:\n{log}"
        );
    }

    #[test]
    fn cleanup_legacy_ipc_degraded_removes_marker() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let marker = root.join(".agent-doc/ipc-degraded");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::write(&marker, "").unwrap();
        assert!(marker.exists());
        cleanup_legacy_ipc_degraded(root);
        assert!(!marker.exists(), "legacy marker should be removed");
    }

    #[test]
    fn cleanup_resolved_backlog_prompts_removes_new_prompt_target_only() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep this tracked item\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep this tracked item\n",
            "commit + push uncommitted files\n",
            "<!-- /agent:backlog -->\n",
        );
        let final_content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "### Re: backlog prompt — gpt-5\n\n",
            "Committed and pushed.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#keep1] Keep this tracked item\n",
            "commit + push uncommitted files\n",
            "<!-- /agent:backlog -->\n",
        );

        let cleaned =
            cleanup_resolved_backlog_prompts_after_response(&doc, base, current, final_content)
                .unwrap()
                .expect("prompt target should be cleaned");

        assert!(cleaned.contains("### Re: backlog prompt — gpt-5"));
        assert!(cleaned.contains("- [x] [#keep1] Keep this tracked item"));
        assert!(!cleaned.contains("commit + push uncommitted files"));
    }

    #[test]
    fn cleanup_resolved_backlog_prompts_preserves_non_prompt_backlog_edits() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Existing item\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Existing item\n",
            "- [ ] [#new1] Added tracked item\n",
            "<!-- /agent:backlog -->\n",
        );

        let cleaned =
            cleanup_resolved_backlog_prompts_after_response(&doc, base, current, current).unwrap();
        assert!(
            cleaned.is_none(),
            "ordinary tracked backlog additions are not prompt cleanup targets"
        );
    }

    #[test]
    fn response_already_in_current_detects_plugin_applied() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        // Plugin applied the response AND user added an edit
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
User added this line.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        assert!(
            response_already_in_current(base, content_ours, content_current),
            "should detect plugin-applied response"
        );
    }

    #[test]
    fn response_already_in_current_rejects_partial_line_overlap() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — gpt-5
Done.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
Done.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "a shared response body line is not proof that the response delta was applied"
        );
    }

    #[test]
    fn response_already_in_current_accepts_normalized_delta_with_bare_prompt() {
        let base = "\
<!-- agent:exchange patch=append -->
do #ipcd
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #ipcd
### Re: #ipcd — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
do #ipcd
while typing next prompt
### Re: #ipcd — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        assert!(
            response_already_in_current(base, content_ours, content_current),
            "the response hunk should be detected even when prompt-prefix normalization differs"
        );
    }

    #[test]
    fn response_already_in_current_false_when_not_applied() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        // Plugin did NOT apply — only user edits present
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
User typed something new.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "should not detect when plugin hasn't applied"
        );
    }

    #[test]
    fn response_already_in_current_false_when_no_exchange() {
        let base = "No components here.";
        let content_ours = "No components here either.";
        let content_current = "Still no components.";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "should return false when no exchange components"
        );
    }

    #[test]
    fn response_already_in_current_false_when_no_changes() {
        let base = "\
<!-- agent:exchange patch=append -->
Same content.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, base, base),
            "should return false when ours equals base"
        );
    }

    #[test]
    fn adopt_current_response_without_duplication_rejects_partial_line_overlap() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: timeout fallback — gpt-5
Done.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
Done.
<!-- /agent:exchange -->
";

        std::fs::write(&doc, content_current).unwrap();
        let adopted = adopt_current_response_without_duplication(
            &doc,
            base,
            content_ours,
            content_current,
            None,
            "### Re: timeout fallback — gpt-5\nDone.\n",
        )
        .unwrap();

        assert!(
            adopted.is_none(),
            "socket-timeout fallback must not adopt current content from a partial line overlap"
        );
    }

    #[test]
    fn adopt_current_response_without_duplication_repairs_bare_prompt_prefix() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #scpd. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
❯ do #scpd. spec-test-build-install-commit-push
### Re: #scpd retry — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #scpd. spec-test-build-install-commit-push
### Re: #scpd retry — gpt-5

Implemented.
<!-- /agent:exchange -->
";

        std::fs::write(&doc, content_current).unwrap();
        let repaired = adopt_current_response_without_duplication(
            &doc,
            base,
            content_ours,
            content_current,
            Some(snapshot),
            "### Re: #scpd retry — gpt-5\n\nImplemented.\n",
        )
        .unwrap()
        .expect("response should be adopted from current");

        assert!(repaired.contains("❯ do #scpd. spec-test-build-install-commit-push"));
        assert!(!repaired.contains("\ndo #scpd. spec-test-build-install-commit-push\n"));
        assert_eq!(repaired.matches("### Re: #scpd retry — gpt-5").count(), 1);
    }

    #[test]
    fn normalize_final_template_content_repairs_bare_prompt_prefix_after_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #dupfx. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let merged = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #dupfx. spec-test-build-install-commit-push
### Re: #dupfx — gpt-5

Implemented.
<!-- /agent:exchange -->
";

        std::fs::write(&doc, merged).unwrap();
        let repaired =
            normalize_final_template_content(&doc, base, Some(snapshot), None, merged, None)
                .unwrap();

        assert!(repaired.contains("❯ do #dupfx. spec-test-build-install-commit-push"));
        assert!(!repaired.contains("\ndo #dupfx. spec-test-build-install-commit-push\n"));
        assert_eq!(repaired.matches("### Re: #dupfx — gpt-5").count(), 1);
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_strips_leaked_marker() {
        // CRDT merge corruption: first non-empty line of the response body
        // got a leading `❯ `. The repair must strip it without touching real
        // user prompts elsewhere in the exchange.
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #respfx. spec-test-build-install-commit-push
### Re: #respfx — opus-4-7

❯ Landed Phase 1 only this cycle. Item stays open.

#### Details

`agent-doc <FILE>` now accepts `--wait-for-ready <SECONDS>`.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leaked ❯ on response body first line must be stripped");
        assert!(
            repaired.contains("\nLanded Phase 1 only this cycle. Item stays open.\n"),
            "stripped response body should start with the original prose, got:\n{repaired}"
        );
        assert!(
            !repaired.contains("❯ Landed"),
            "leaked ❯ must be removed, got:\n{repaired}"
        );
        // User prompt before the response heading is preserved.
        assert!(repaired.contains("❯ do #respfx. spec-test-build-install-commit-push"));
        // Heading and subsequent body lines are untouched.
        assert!(repaired.contains("### Re: #respfx — opus-4-7"));
        assert!(repaired.contains("#### Details"));
        assert!(repaired.contains("`agent-doc <FILE>` now accepts `--wait-for-ready <SECONDS>`."));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_strips_leading_run() {
        // Repair adoption can see every response paragraph prefixed when the
        // stale snapshot already had the response heading but not the body.
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #leading-run. spec-test-build-install-commit-push
### Re: #leading-run — gpt-5

❯ First response paragraph.

❯ Second response paragraph.
❯ - Proof line.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leading response-body prompt markers must be stripped");

        assert!(repaired.contains("\nFirst response paragraph.\n"));
        assert!(repaired.contains("\nSecond response paragraph.\n- Proof line.\n"));
        assert!(!repaired.contains("❯ First response paragraph."));
        assert!(!repaired.contains("❯ Second response paragraph."));
        assert!(!repaired.contains("❯ - Proof line."));
        assert!(repaired.contains("❯ do #leading-run. spec-test-build-install-commit-push"));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_skips_when_clean() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #clean. spec-test-build-install-commit-push
### Re: #clean — opus-4-7

Landed cleanly.
<!-- /agent:exchange -->
";
        let result = strip_prompt_prefix_from_response_body_first_lines(content);
        assert!(
            result.is_none(),
            "clean document must not trigger the strip path"
        );
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_preserves_inner_prompt_like_lines() {
        // A `❯ ` appearing AFTER the first body line — e.g. quoted user input
        // inside the response prose — must be preserved. Only the leaked
        // first-line marker is stripped.
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #inner. spec-test-build-install-commit-push
### Re: #inner — opus-4-7

❯ first line gets stripped

The user said:
❯ this quoted line stays
because it is not the first body line.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leaked first-line ❯ must be stripped");
        assert!(repaired.contains("\nfirst line gets stripped\n"));
        assert!(!repaired.contains("❯ first line gets stripped"));
        // Inner `❯ ` is preserved — it is part of the response body text.
        assert!(repaired.contains("❯ this quoted line stays"));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_handles_multiple_re_blocks() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #a
### Re: #a — opus-4-7

❯ first response

❯ do #b
### Re: #b — opus-4-7

❯ second response
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("multiple leaks must be stripped");
        assert!(repaired.contains("\nfirst response\n"));
        assert!(repaired.contains("\nsecond response\n"));
        assert!(!repaired.contains("❯ first response"));
        assert!(!repaired.contains("❯ second response"));
        // User prompts between blocks preserved.
        assert!(repaired.contains("❯ do #a"));
        assert!(repaired.contains("❯ do #b"));
    }

    #[test]
    fn normalize_final_template_content_removes_adjacent_duplicate_response_blocks() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
❯ do #duppb. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let duplicated = "\
<!-- agent:exchange patch=append -->
❯ do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.

Verification:
- `cargo test`
### Re: #duppb — gpt-5

Implemented.

Verification:
- `cargo test`
<!-- /agent:exchange -->
";

        let repaired = normalize_final_template_content(
            &doc,
            baseline,
            Some(baseline),
            None,
            duplicated,
            None,
        )
        .expect("duplicate response repair should succeed");

        assert_eq!(
            repaired.matches("### Re: #duppb — gpt-5").count(),
            1,
            "closeout normalization must remove adjacent duplicate response blocks: {repaired}"
        );
        assert!(repaired.contains("Verification:\n- `cargo test`"));
    }

    #[test]
    fn normalize_final_template_content_scrubs_duplicate_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. Was this an issue because I didn't restart agent-doc on this document? #spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let merged = base
            .replace(
                "<!-- /agent:exchange -->",
                "### Re: duplicate prompt cleanup — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
            )
            .replace(
                "<!-- agent:backlog -->",
                "<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push\n-->\n\n<!-- agent:backlog -->",
            );

        let repaired =
            normalize_final_template_content(&doc, base, Some(snapshot), None, &merged, None)
                .unwrap();

        assert!(
            repaired.contains("❯ The duplicate content corrupting document"),
            "live prompt should remain in exchange and be normalized:\n{repaired}"
        );
        assert!(
            !repaired.contains(
                "\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again."
            ),
            "duplicate post-exchange prompt text should be scrubbed from comments:\n{repaired}"
        );
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "duplicate prompt cleanup must preserve the ordinary HTML comment shell:\n{repaired}"
        );
        assert!(
            repaired.contains("<!-- agent:backlog -->\n- [ ] keep me"),
            "backlog scaffold should remain intact:\n{repaired}"
        );
    }

    #[test]
    fn normalize_final_template_content_preserves_baseline_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
        let base = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        let merged = base.replace(
            "<!-- /agent:exchange -->",
            "### Re: sqlitedb graph performance next steps — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
        );

        let repaired =
            normalize_final_template_content(&doc, &base, Some(&base), None, &merged, None)
                .unwrap();

        assert!(
            repaired.contains(&format!("<!--\n{prompt}\n-->")),
            "baseline-owned post-exchange scratch text must not be scrubbed:\n{repaired}"
        );
    }

    #[test]
    fn normalize_final_template_content_preserves_current_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "The html comment below this document's agent:exchange close tag had content that I put into it. This should not happen. #spec-test-build-install-commit-push";
        let base = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        let before_current = base.replace("<!--\n-->", &format!("<!--\n{prompt}\n-->"));
        let merged = before_current.replace(
            "<!-- /agent:exchange -->",
            "### Re: scratch comment preservation — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
        );

        let repaired = normalize_final_template_content(
            &doc,
            &base,
            Some(&base),
            Some(&before_current),
            &merged,
            None,
        )
        .unwrap();

        assert!(
            repaired.contains(&format!("<!--\n{prompt}\n-->")),
            "current visible post-exchange scratch text must not be scrubbed:\n{repaired}"
        );
    }

    #[test]
    fn normalize_template_structure_preserves_unique_post_exchange_html_comment_tail() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "do #visible. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "Keep this unrelated scratch note hidden.\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert!(
            repaired.contains("Keep this unrelated scratch note hidden."),
            "unique scratch comments must stay outside exchange:\n{repaired}"
        );
    }

    #[test]
    fn normalize_template_structure_scrubs_answered_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "❯ The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. Was this an issue because I didn't restart agent-doc on this document? #spec-test-build-install-commit-push\n",
            "### Re: backlog update and duplicate prompt corruption — gpt-5\n",
            "Implemented.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert!(
            repaired.contains("### Re: backlog update and duplicate prompt corruption"),
            "answered exchange turn should remain:\n{repaired}"
        );
        assert!(
            !repaired.contains(
                "\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again."
            ),
            "answered duplicate prompt text should be scrubbed from the HTML comment:\n{repaired}"
        );
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "answered duplicate prompt cleanup must preserve the ordinary HTML comment shell:\n{repaired}"
        );
    }

    #[test]
    fn normalize_final_template_content_repairs_duplicate_exchange_close_after_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let merged = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->

### Re: #xguard — gpt-5

Implemented.
<!-- /agent:exchange -->

<!-- agent:backlog -->
- [ ] keep me
<!-- /agent:backlog -->
";

        std::fs::write(&doc, merged).unwrap();
        let repaired =
            normalize_final_template_content(&doc, base, Some(snapshot), None, merged, None)
                .unwrap();

        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let response = repaired.find("### Re: #xguard — gpt-5").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            response < exchange_close,
            "response should be restored inside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
        assert_eq!(repaired.matches("<!-- /agent:exchange -->").count(), 1);
    }

    #[test]
    fn normalize_final_template_content_repairs_response_before_prompt_tail() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Please handle the timeout fallback.
<!-- agent:boundary:old -->
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Please handle the timeout fallback.
Can you preserve the second paragraph too?
<!-- agent:boundary:old -->
<!-- /agent:exchange -->
";
        let merged = "\
<!-- agent:exchange patch=append -->
❯ Please handle the timeout fallback.
### Re: timeout fallback — gpt-5

Done.
<!-- agent:boundary:new -->
Can you preserve the second paragraph too?
<!-- /agent:exchange -->
";
        let response = "### Re: timeout fallback — gpt-5\n\nDone.\n";

        let repaired = normalize_final_template_content(
            &doc,
            base,
            Some(snapshot),
            None,
            merged,
            Some(response),
        )
        .unwrap();

        let prompt_tail = repaired
            .find("Can you preserve the second paragraph too?")
            .unwrap();
        let response_heading = repaired.find("### Re: timeout fallback").unwrap();
        let boundary = repaired.find("<!-- agent:boundary:").unwrap();
        let close = repaired.find("<!-- /agent:exchange -->").unwrap();
        assert!(
            prompt_tail < response_heading,
            "prompt tail must move before response:\n{repaired}"
        );
        assert!(
            response_heading < boundary && boundary < close,
            "boundary must close the repaired response turn:\n{repaired}"
        );
    }

    #[test]
    fn normalize_template_structure_repairs_duplicate_scaffold_close() {
        let dir = TempDir::new().unwrap();
        let doc_path = dir.path().join("doc.md");
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "❯ keep this prompt\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc_path)
            .expect("pure duplicated scaffold should be repaired");

        assert_eq!(repaired.matches("<!-- /agent:exchange -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:queue -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:backlog -->").count(), 1);
        assert!(repaired.contains("❯ keep this prompt"));
    }

    #[test]
    fn normalize_template_structure_rejects_duplicate_scaffold_with_user_text() {
        let dir = TempDir::new().unwrap();
        let doc_path = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "c The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "corky.md The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        fs::write(&doc_path, content).unwrap();

        let err = normalize_template_structure_or_fail(content, &doc_path).unwrap_err();

        assert!(
            err.to_string().contains("mixed duplicate scaffold"),
            "unexpected error: {err}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=mixed_duplicate_scaffold_tail"));
    }
