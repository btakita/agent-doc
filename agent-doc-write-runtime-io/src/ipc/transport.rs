//! Write IPC transport adapter.

use super::*;
#[cfg(test)]
use agent_doc_document::write_normalization::cleanup_resolved_backlog_prompts_after_response;
#[cfg(test)]
use agent_doc_ipc_protocol::AlreadyAppliedSnapshotOutcome;
#[cfg(test)]
use agent_doc_write_converge_io::{
    AlreadyAppliedSocketSnapshotContext, cleanup_legacy_ipc_degraded,
    persist_already_applied_socket_content_ours_snapshot,
};

#[cfg(test)]
pub(crate) fn record_test_visible_write_receipt(
    file: &Path,
    patch_id: &str,
    content: &str,
    source: &str,
) {
    let file_key = file.to_string_lossy();
    let _ = agent_doc_debounce::record_live_buffer_synced_content_for_editor_with_capabilities(
        file_key.as_ref(),
        content,
        "test-editor",
        "test",
        "test",
        &[
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
        ],
    );
    let _ =
        agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
            file, patch_id, content, source,
        );
}

#[cfg(test)]
pub(crate) fn record_test_visible_write_receipt_with_relay(
    file: &Path,
    patch_id: &str,
    content: &str,
    source: &str,
) {
    agent_doc_test_support::publish_editor_text_via_crdt_relay(
        file,
        "visible-write-test-editor",
        content,
    );
    record_test_visible_write_receipt(file, patch_id, content, source);
}

/// Attempt to write via IPC (socket-first, file-based fallback).
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_ipc(
    file: &Path,
    patches: &[agent_doc_template::PatchBlock],
    unmatched: &str,
    frontmatter_yaml: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
    reuse_patch_id: Option<&str>,
) -> Result<agent_doc_write_ipc_io::IpcResult> {
    agent_doc_write_ipc_io::try_ipc_with_effects(
        &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
        file,
        patches,
        unmatched,
        frontmatter_yaml,
        baseline,
        content_ours,
        normalize_prefix_lines,
        reuse_patch_id,
    )
}

#[cfg(test)]
mod submodule_patch_routing_tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
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
        agent_doc_snapshot_io::save(&doc, &updated, agent_doc_ops_log_io::log_op).unwrap();

        let err = super::complete_required_closeout(&doc, false).unwrap_err();
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
            agent_doc_git_io::submodule::submodule_pointer_drift(&doc)
                .unwrap()
                .is_some(),
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
            let _ = agent_doc_ipc_io::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                let file_path = v.get("file").and_then(|f| f.as_str()).unwrap_or("");
                if !file_path.is_empty() {
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
                                    Some(agent_doc_template::PatchBlock::new(name, content))
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let unmatched = v
                        .get("unmatched")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    let after =
                        agent_doc_template_io::apply_patches(&before, &patches, unmatched, file)
                            .unwrap_or(before);
                    let _ = std::fs::write(file, &after);
                    crate::ipc::transport::record_test_visible_write_receipt(
                        file,
                        patch_id,
                        &after,
                        "test_socket_listener",
                    );
                }
                Some(
                    serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                        .to_string(),
                )
            });
        })
    }

    fn start_already_applied_listener(project_root: &Path) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root, |_msg| {
                Some(
                    serde_json::json!({
                        "type": "receipt",
                        "status": "applied",
                        "reason": "already_applied"
                    })
                    .to_string(),
                )
            });
        })
    }

    fn start_fixed_visible_write_listener(
        project_root: &Path,
        visible_write_content: String,
    ) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                if let Some(file_path) = v.get("file").and_then(|f| f.as_str()) {
                    let _ = std::fs::write(file_path, &visible_write_content);
                    crate::ipc::transport::record_test_visible_write_receipt_with_relay(
                        Path::new(file_path),
                        patch_id,
                        &visible_write_content,
                        "test_socket_listener",
                    );
                }
                Some(
                    serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                        .to_string(),
                )
            });
        })
    }

    fn start_visible_write_only_listener(
        project_root: &Path,
        visible_write_content: String,
    ) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                if let Some(file_path) = v.get("file").and_then(|f| f.as_str()) {
                    crate::ipc::transport::record_test_visible_write_receipt_with_relay(
                        Path::new(file_path),
                        patch_id,
                        &visible_write_content,
                        "test_socket_listener",
                    );
                }
                Some(
                    serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                        .to_string(),
                )
            });
        })
    }

    /// Helper: wait for the socket listener to become connectable (up to 1s).
    fn wait_for_listener(project_root: &Path) {
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(project_root) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("fake socket listener did not start within 1s");
    }

    fn seed_live_editor(doc: &Path) {
        agent_doc_test_support::seed_live_plugin_owner_lease_for_editor(
            doc.to_str().unwrap(),
            "visible-write-test-editor",
        );
    }

    #[test]
    fn try_ipc_routes_to_submodule_root_not_superproject() {
        // Verify that try_ipc routes patches to the SUBMODULE's own .agent-doc/
        // root, not the superproject. The submodule has its own .agent-doc/ so
        // the IDE plugin's resolveRootFor and Rust's find_project_root both
        // return the submodule root, keeping IPC state paths in sync.
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

        // Submodule has its own .agent-doc/ — mirrors the real sample-app layout.
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
        seed_live_editor(&doc);

        let patch = agent_doc_template::PatchBlock::new("exchange", "test response");

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
        // exercises the git_toplevel_at path in agent_doc_project_root_io.
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
        seed_live_editor(&doc);

        let patch = agent_doc_template::PatchBlock::new("exchange", "response");

        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "try_ipc should succeed via socket IPC routed to the git toplevel"
        );
    }

    #[test]
    fn try_ipc_already_applied_socket_refuses_unreceipted_disk_adoption() {
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
            "❯ Follow-up typed while closeout saved\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();
        fs::write(&doc, live_already_applied_with_user_edit).unwrap();

        let _listener = start_already_applied_listener(&root);
        wait_for_listener(&root);
        seed_live_editor(&doc);

        let patch = agent_doc_template::PatchBlock::new(
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
            !result.success,
            "an attached editor needs a visible-write receipt before closeout can succeed"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(baseline),
            "already_applied must not advance the snapshot from unreceipted disk content"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live_already_applied_with_user_edit,
            "the existing disk projection must remain untouched"
        );
        assert!(
            !agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .ipc_snapshot_adoption_blocked,
            "fail-closed receipt handling must not manufacture a snapshot-absorb block"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("invariant=already_applied_missing_visible_write_receipt")
                && log.contains("recovery=retry_without_file_ipc_or_disk_write")
                && !log.contains("ipc_socket_already_applied_snapshot")
                && !log.contains("file_ipc_fallback_skip_already_applied"),
            "unreceipted already_applied must retain the response without disk adoption:\n{log}"
        );
        // Without an authoritative receipt, disk divergence is not proof of user
        // typing. Leave both the buffer and its projection untouched.
        assert!(!log.contains("finalize_typing_during_write"));
    }

    #[test]
    fn try_ipc_already_applied_socket_adopts_visible_write_content_when_disk_lags() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in [
            "visible-write",
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
        let editor_visible_write_content = concat!(
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
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let editor_id = "jetbrains-test-editor";
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            editor_visible_write_content,
            editor_id,
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();
        assert!(
            agent_doc_debounce::editor_sync_statuses(&doc_str)
                .iter()
                .any(|status| status.in_flight),
            "fixture should start with an in-flight editor epoch"
        );

        let patch_id = "already-applied-visible-write";
        crate::ipc::transport::record_test_visible_write_receipt_with_relay(
            &doc,
            patch_id,
            editor_visible_write_content,
            "test_already_applied",
        );

        let _listener = start_already_applied_listener(&root);
        wait_for_listener(&root);
        seed_live_editor(&doc);

        let patch = agent_doc_template::PatchBlock::new(
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
            Some(patch_id),
        )
        .unwrap();

        assert!(
            result.success,
            "already_applied socket response with lazily visible-write receipt is a proven editor write"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(editor_visible_write_content),
            "already_applied must adopt fresh editor visible-write content when disk still lags"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            editor_visible_write_content,
            "proven visible-write content must be written through so stale disk cannot later overwrite the editor buffer"
        );
        assert!(
            agent_doc_debounce::editor_sync_statuses(&doc_str)
                .iter()
                .all(|status| !status.in_flight),
            "already_applied visible-write receipt should mark the targeted live-buffer epoch synced"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_already_applied_skip_file_fallback")
                && log.contains("ipc_socket_already_applied_snapshot")
                && log.contains("snap_source=lazily_visible_write_event")
                && log.contains("visible_write_live_buffer_sync_skipped")
                && log.contains("reason=sidecar_removed")
                && log.contains("visible_write_disk_write_through"),
            "already_applied visible-write adoption should be auditable:\n{log}"
        );
    }

    #[test]
    fn already_applied_visible_write_content_does_not_overwrite_newer_visible_operator_edit() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in [
            "visible-write",
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
        let newer_visible = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "Operator typed after visible-write was captured.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();

        let patch_id = "already-applied-stale-visible-write";
        crate::ipc::transport::record_test_visible_write_receipt(
            &doc,
            patch_id,
            content_ours,
            "test_already_applied",
        );
        fs::write(&doc, newer_visible).unwrap();

        let outcome = persist_already_applied_socket_content_ours_snapshot(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            AlreadyAppliedSocketSnapshotContext {
                file: &doc,
                patch_id,
                editor_id: Some("jetbrains-test-editor"),
                baseline: Some(baseline),
                content_ours: Some(content_ours),
                normalize_prefix_lines: None,
                expected_response: "### Re: Please reply — gpt-5\n\nAnswered.\n",
            },
        )
        .unwrap();

        assert_eq!(
            outcome,
            AlreadyAppliedSnapshotOutcome::NeedsAuthoritativeRetry,
            "a stale receipt must not authorize adoption of a newer disk projection"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(baseline),
            "a stale receipt must leave the durable snapshot unchanged"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            newer_visible,
            "stale visible-write content must not be written over a newer visible editor edit"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("invariant=visible_write_receipt_superseded_by_worktree")
                && log.contains("recovery=refresh_editor_cut_without_file_ipc_or_disk_write")
                && !log.contains("ipc_socket_already_applied_snapshot"),
            "stale visible-write receipt rejection should be auditable:\n{log}"
        );
    }

    #[test]
    fn already_applied_visible_write_content_does_not_overwrite_unsaved_live_editor_edit() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in [
            "visible-write",
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
        let stale_visible_write_content = concat!(
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
        let unsaved_live_editor = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "Operator typed after visible-write was captured.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();

        let patch_id = "already-applied-unsaved-live-editor";
        let editor_id = "jetbrains-test-editor";
        crate::ipc::transport::record_test_visible_write_receipt(
            &doc,
            patch_id,
            stale_visible_write_content,
            "test_already_applied",
        );
        agent_doc_test_support::publish_editor_text_via_crdt_relay(
            &doc,
            editor_id,
            unsaved_live_editor,
        );

        let outcome = persist_already_applied_socket_content_ours_snapshot(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            AlreadyAppliedSocketSnapshotContext {
                file: &doc,
                patch_id,
                editor_id: Some(editor_id),
                baseline: Some(baseline),
                content_ours: Some(stale_visible_write_content),
                normalize_prefix_lines: None,
                expected_response: "### Re: Please reply — gpt-5\n\nAnswered.\n",
            },
        )
        .unwrap();

        assert_eq!(
            outcome,
            AlreadyAppliedSnapshotOutcome::NeedsAuthoritativeRetry
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(baseline),
            "stale visible-write content must not replace the snapshot when the live editor buffer has moved on"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            baseline,
            "stale visible-write content must not be written over an unsaved live editor edit"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("invariant=visible_write_receipt_superseded_by_live_editor")
                && log.contains("recovery=refresh_editor_cut_without_file_ipc_or_disk_write")
                && !log.contains("ipc_socket_already_applied_snapshot"),
            "stale live-editor visible-write receipt should fail closed before snapshot adoption:\n{log}"
        );
    }

    #[test]
    fn try_ipc_socket_visible_write_content_writes_through_when_disk_lags() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in [
            "visible-write",
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
        let editor_visible_write_content = concat!(
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
        fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();

        let patch_id = "socket-visible-write-disk-lags";
        let _listener =
            start_visible_write_only_listener(&root, editor_visible_write_content.to_string());
        wait_for_listener(&root);
        seed_live_editor(&doc);

        let patch = agent_doc_template::PatchBlock::new(
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
            Some(patch_id),
        )
        .unwrap();

        assert!(
            result.success,
            "socket visible-write receipt should prove delivery"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(editor_visible_write_content),
            "socket visible-write content should remain the snapshot authority"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            editor_visible_write_content,
            "proven editor-visible content should write through so stale disk cannot overwrite the live buffer"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_visible_write")
                && log.contains("snap_source=lazily_visible_write_event")
                && log.contains("visible_write_disk_write_through file=")
                && !log.contains("visible_write_disk_write_through_blocked"),
            "socket visible-write CRDT adoption should prove its disk projection:\n{log}"
        );
    }

    #[test]
    fn try_ipc_already_applied_socket_leaves_unreceipted_duplicate_untouched() {
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
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();
        fs::write(&doc, duplicated_live_buffer).unwrap();

        let _listener = start_already_applied_listener(&root);
        wait_for_listener(&root);
        seed_live_editor(&doc);

        let patch = agent_doc_template::PatchBlock::new(
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

        assert!(
            !result.success,
            "already_applied duplicate recovery must remain pending without a visible-write receipt"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(baseline),
            "already_applied duplicate repair must not snapshot unproven dedupe"
        );
        let disk = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            disk.matches("### Re: Please reply — gpt-5").count(),
            2,
            "already_applied duplicate repair must leave the editor-visible duplicate state untouched: {disk}"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("invariant=already_applied_missing_visible_write_receipt")
                && log.contains("recovery=retry_without_file_ipc_or_disk_write")
                && !log.contains("file_ipc_fallback_skip_already_applied")
                && !log.contains("ipc_socket_already_applied_snapshot"),
            "unreceipted duplicate retry should be logged without document mutation:\n{log}"
        );
    }

    #[test]
    fn already_applied_socket_without_visible_receipt_never_repairs_disk_behind_editor() {
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
        fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        fs::write(&doc, stale_disk_with_live_prompt).unwrap();

        let outcome = persist_already_applied_socket_content_ours_snapshot(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            AlreadyAppliedSocketSnapshotContext {
                file: &doc,
                patch_id: "already-applied-missing",
                editor_id: None,
                baseline: Some(baseline),
                content_ours: Some(content_ours),
                normalize_prefix_lines: None,
                expected_response: "### Re: Please reply — gpt-5\n\nAnswered.\n",
            },
        )
        .unwrap();

        assert_eq!(
            outcome,
            AlreadyAppliedSnapshotOutcome::NeedsAuthoritativeRetry
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(baseline),
            "an unproven editor delivery must not advance the durable snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            stale_disk_with_live_prompt,
            "already_applied recovery must never write a reconstructed document behind the editor"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("invariant=already_applied_missing_visible_write_receipt")
                && log.contains("recovery=retry_without_file_ipc_or_disk_write")
                && !log.contains("ipc_socket_already_applied_missing_disk_response_repaired"),
            "missing-receipt already_applied must fail closed without disk repair:\n{log}"
        );
    }

    #[test]
    fn already_applied_receipt_missing_response_retries_cell_without_document_write() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in ["visible-write", "snapshots", "crdt", "logs", "state/cycles"] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }
        let doc = root.join("session.md");
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let receipt_current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ Prompt-shaped transport drift without a response receipt\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();

        let patch_id = "already-applied-receipt-missing-response";
        crate::ipc::transport::record_test_visible_write_receipt_with_relay(
            &doc,
            patch_id,
            receipt_current,
            "test_already_applied",
        );

        let outcome = persist_already_applied_socket_content_ours_snapshot(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            AlreadyAppliedSocketSnapshotContext {
                file: &doc,
                patch_id,
                editor_id: Some("jetbrains-test-editor"),
                baseline: Some(baseline),
                content_ours: Some(content_ours),
                normalize_prefix_lines: None,
                expected_response: "### Re: Please reply — gpt-5\n\nAnswered.\n",
            },
        )
        .unwrap();

        assert_eq!(
            outcome,
            AlreadyAppliedSnapshotOutcome::NeedsAuthoritativeRetry
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(baseline),
            "a receipt without the response must not advance the snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            baseline,
            "the response cell must be retried through CPC/CRDT, never by replacing the document"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("invariant=visible_write_receipt_missing_response")
                && log.contains("recovery=retry_response_cell_via_cpc_without_disk_write")
                && !log.contains("finalize_typing_during_write")
                && !log.contains("ipc_socket_already_applied_snapshot"),
            "receipt-backed response retry must be explicit and non-mutating:\n{log}"
        );
    }

    // #stale-already-applied — when the authoritative editor cut moves beyond an
    // `already_applied` receipt, the receipt is no longer proof for the current
    // document. Keep the response operation pending without touching disk.
    #[test]
    fn already_applied_socket_stale_receipt_leaves_response_pending_without_disk_write() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in ["visible-write", "snapshots", "crdt", "logs", "state/cycles"] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }
        let doc = root.join("session.md");
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        // Disk and the visible-write `current` both lack the response, but differ
        // from each other, so the visible-write guard re-reads disk, sees drift,
        // and defers.
        let disk_now = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ disk keystroke A\n",
            "<!-- /agent:exchange -->\n"
        );
        let ack_current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ buffer keystroke B\n",
            "<!-- /agent:exchange -->\n"
        );
        let newer_relay_current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ buffer keystroke B\n",
            "❯ operator typed after ack\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        fs::write(&doc, disk_now).unwrap();
        let patch_id = "already-applied-not-idle";
        crate::ipc::transport::record_test_visible_write_receipt_with_relay(
            &doc,
            patch_id,
            ack_current,
            "test_already_applied",
        );
        agent_doc_test_support::publish_editor_text_via_crdt_relay(
            &doc,
            "visible-write-test-editor",
            newer_relay_current,
        );

        let outcome = persist_already_applied_socket_content_ours_snapshot(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            AlreadyAppliedSocketSnapshotContext {
                file: &doc,
                patch_id,
                editor_id: None,
                baseline: Some(baseline),
                content_ours: Some(content_ours),
                normalize_prefix_lines: None,
                expected_response: "### Re: Please reply — gpt-5\n\nAnswered.\n",
            },
        )
        .unwrap();

        assert_eq!(
            outcome,
            AlreadyAppliedSnapshotOutcome::NeedsAuthoritativeRetry,
            "a stale already_applied receipt must keep the response pending"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("invariant=visible_write_receipt_missing_response")
                && log.contains("recovery=retry_response_cell_via_cpc_without_disk_write")
                && !log.contains("ipc_socket_already_applied_snapshot"),
            "deferred visible repair must remain pending without a disk write:\n{log}"
        );
        // The scrambling / partial write must NOT have landed: disk is untouched.
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            disk_now,
            "a deferred repair must not write a partial/scrambled document to disk"
        );
    }

    #[test]
    fn already_applied_without_response_in_content_ours_requires_authoritative_retry() {
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
        let content_ours = baseline;
        fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();

        let outcome = persist_already_applied_socket_content_ours_snapshot(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            AlreadyAppliedSocketSnapshotContext {
                file: &doc,
                patch_id: "already-applied-missing-response",
                editor_id: None,
                baseline: Some(baseline),
                content_ours: Some(content_ours),
                normalize_prefix_lines: None,
                expected_response: "### Re: Please reply — gpt-5\n\nAnswered.\n",
            },
        )
        .unwrap();

        assert_eq!(
            outcome,
            AlreadyAppliedSnapshotOutcome::NeedsAuthoritativeRetry
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(baseline),
            "response-less already_applied content_ours must not replace the snapshot"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("invariant=already_applied_missing_visible_write_receipt")
                && log.contains("recovery=retry_without_file_ipc_or_disk_write")
                && !log.contains("ipc_socket_already_applied_snapshot"),
            "response-less already_applied should fail proof without file fallback:\n{log}"
        );
    }

    #[test]
    fn already_applied_empty_response_probe_requires_authoritative_retry() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in ["snapshots", "crdt", "logs", "state/cycles"] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }
        let doc = root.join("session.md");
        let content_ours = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, content_ours).unwrap();
        agent_doc_snapshot_io::save(&doc, content_ours, agent_doc_ops_log_io::log_op).unwrap();

        let outcome = persist_already_applied_socket_content_ours_snapshot(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            AlreadyAppliedSocketSnapshotContext {
                file: &doc,
                patch_id: "already-applied-empty-response-probe",
                editor_id: None,
                baseline: Some(content_ours),
                content_ours: Some(content_ours),
                normalize_prefix_lines: None,
                expected_response: "",
            },
        )
        .unwrap();

        assert_eq!(
            outcome,
            AlreadyAppliedSnapshotOutcome::NeedsAuthoritativeRetry
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("invariant=already_applied_missing_visible_write_receipt")
                && log.contains("recovery=retry_without_file_ipc_or_disk_write")
                && !log.contains("ipc_socket_already_applied_snapshot"),
            "empty response probe should fail proof without file fallback:\n{log}"
        );
    }

    #[test]
    fn socket_visible_write_content_prompt_duplication_fails_closed_without_editor_repair() {
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
        let duplicated_visible_write_content = concat!(
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
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();

        let _listener =
            start_fixed_visible_write_listener(&root, duplicated_visible_write_content.to_string());
        wait_for_listener(&root);
        seed_live_editor(&doc);

        let patch = agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: Production key — gpt-5\n\nDone.",
        );
        let err = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("duplicated-visible-write"),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("refusing direct document write"),
            "duplicated visible-write repair must fail closed instead of repairing disk: {err}"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().as_deref(),
            Some(baseline),
            "duplicated visible-write must not replace the existing snapshot without editor proof"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            duplicated_visible_write_content,
            "duplicated visible-write content should remain editor-owned"
        );
        assert!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .ipc_snapshot_adoption_blocked,
            "later commit stages must not absorb the rejected duplicate sidecar"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=prompt_duplication_in_visible_write")
                && log.contains("duplicate_prompt_count=1")
                && log.contains("ipc_visible_repair_retry_required_no_disk_write"),
            "duplicate visible-write rejection and retry should be logged:\n{log}"
        );
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=prompt_duplication_in_visible_write")
                && log.contains("recovery=content_ours_snapshot_and_visible_repair")
                && log.contains("recovery=retry_without_disk_write"),
            "duplicate prompt visible-write receipt should name its failed invariant and recovery:\n{log}"
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

        let cleaned = cleanup_resolved_backlog_prompts_after_response(base, current, final_content)
            .unwrap()
            .expect("prompt target should be cleaned");

        assert!(cleaned.content.contains("### Re: backlog prompt — gpt-5"));
        assert!(
            cleaned
                .content
                .contains("- [x] [#keep1] Keep this tracked item")
        );
        assert!(!cleaned.content.contains("commit + push uncommitted files"));
    }

    #[test]
    fn cleanup_resolved_backlog_prompts_preserves_non_prompt_backlog_edits() {
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
            cleanup_resolved_backlog_prompts_after_response(base, current, current).unwrap();
        assert!(
            cleaned.is_none(),
            "ordinary tracked backlog additions are not prompt cleanup targets"
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
    fn normalize_final_template_content_strips_response_body_prompt_prefix_after_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ do #respfx. spec-test-build-install-commit-push
### Re: #respfx — gpt-5
<!-- /agent:exchange -->
";
        let merged = "\
<!-- agent:exchange patch=append -->
❯ do #respfx. spec-test-build-install-commit-push
### Re: #respfx — gpt-5

❯ Landed Phase 1 only this cycle.
<!-- /agent:exchange -->
";

        std::fs::write(&doc, merged).unwrap();
        let repaired =
            normalize_final_template_content(&doc, snapshot, Some(snapshot), None, merged, None)
                .unwrap();

        assert!(
            repaired.contains("\nLanded Phase 1 only this cycle.\n"),
            "post-merge guard should strip leaked response-body prompt prefix:\n{repaired}"
        );
        assert!(!repaired.contains("❯ Landed Phase 1"));
        assert!(repaired.contains("❯ do #respfx. spec-test-build-install-commit-push"));
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
}

#[cfg(test)]
#[cfg(test)]
mod late_fallback_patch_guard_tests {
    use super::try_ipc;
    use agent_doc_flow_io::closeout::{cleanup_fallback_patch_files, cycle_already_committed};
    use agent_doc_ipc_protocol::{
        EditorBadStateFingerprint, FullContentRepairRedelivery, IpcDiskRepairReason,
        IpcRepairDecision, IpcSnapshotSource,
    };
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    struct NoopRepairReplayWriteEffects;

    static NOOP_REPAIR_REPLAY_WRITE_EFFECTS: NoopRepairReplayWriteEffects =
        NoopRepairReplayWriteEffects;

    impl agent_doc_repair_io::RepairStrictReplayWriteEffects for NoopRepairReplayWriteEffects {
        fn run_strict_write_replay(
            &self,
            _file: &Path,
            _response: &str,
            _is_template: bool,
            _is_stream: bool,
            _force_disk: bool,
            _queue_completion_ids: &[String],
        ) -> anyhow::Result<()> {
            anyhow::bail!("unexpected strict replay in write IPC transport test")
        }
    }

    impl agent_doc_repair_io::RepairFallbackWriteEffects for NoopRepairReplayWriteEffects {
        fn apply_template_from_string(
            &self,
            _file: &Path,
            _response: &str,
            _force_disk: bool,
        ) -> anyhow::Result<()> {
            anyhow::bail!("unexpected template replay in write IPC transport test")
        }

        fn apply_append_from_string(&self, _file: &Path, _response: &str) -> anyhow::Result<()> {
            anyhow::bail!("unexpected append replay in write IPC transport test")
        }
    }

    impl agent_doc_repair_io::RepairRecoveredQueueHeadEffects for NoopRepairReplayWriteEffects {
        fn strike_recovered_free_text_queue_head(&self, _file: &Path) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn try_ipc_full_content(file: &Path, content: &str) -> anyhow::Result<bool> {
        agent_doc_write_converge_io::try_ipc_full_content(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            content,
        )
    }

    fn try_ipc_full_content_response_fallback_from_source(
        file: &Path,
        content: &str,
        source_content: &str,
    ) -> anyhow::Result<bool> {
        agent_doc_write_converge_io::try_ipc_full_content_response_fallback_from_source(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            content,
            source_content,
        )
    }

    fn try_ipc_full_content_operator_mutation_from_source(
        file: &Path,
        content: &str,
        source_content: &str,
    ) -> anyhow::Result<bool> {
        agent_doc_write_converge_io::try_ipc_full_content_operator_mutation_from_source(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            content,
            source_content,
        )
    }

    fn doc_in_agent_doc_project(tmp: &TempDir, content: &str) -> std::path::PathBuf {
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("state").join("cycles")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = tmp.path().join("doc.md");
        fs::write(&doc, content).unwrap();
        doc
    }

    struct TsiftDuplicateContentFixture {
        bad_state_before_live_typing: &'static str,
        repaired_snapshot: &'static str,
        live_buffer_after_typing: &'static str,
    }

    fn tsift_md_duplicate_content_corruption_fixture() -> TsiftDuplicateContentFixture {
        TsiftDuplicateContentFixture {
            bad_state_before_live_typing: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "<!-- agent:boundary:tsift-bad -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
            repaired_snapshot: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "<!-- agent:boundary:tsift-repaired -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
            live_buffer_after_typing: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "The duplicate content corrupt document bug happened on tsift.md as I was tying in a prompt. ",
                "What are #next-steps to ensure full-document IPC is not over-eager? #next-steps\n",
                "<!-- agent:boundary:tsift-live -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
        }
    }

    #[test]
    fn ipc_repair_decision_records_ack_prefix_repair_bad_state() {
        let decision = IpcRepairDecision::lazily_visible_write_prefix_repair(
            "fixed snapshot".to_string(),
            "bad editor state".to_string(),
            &["bad editor state".to_string()],
        );

        assert_eq!(decision.snapshot_content, "fixed snapshot");
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::LazilyVisibleWriteEvent
        );
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergence)
        );
        assert!(decision.redeliver_editor);
        let bad_state = decision
            .editor_bad_state
            .as_ref()
            .expect("prefix fallback should capture bad editor state");
        assert_eq!(bad_state.content(), "bad editor state");
        assert_eq!(bad_state.len, "bad editor state".len());
        assert_eq!(
            bad_state.hash,
            agent_doc_hash::content_hash("bad editor state")
        );
        assert_eq!(decision.normalize_prefix_lines, vec!["bad editor state"]);
    }

    #[test]
    fn ipc_repair_decision_preserves_original_bad_state_when_dedupe_follows_prefix_repair() {
        let decision = IpcRepairDecision::lazily_visible_write_prefix_repair(
            "prefix fallback with duplicate response".to_string(),
            "visible sidecar before fallback".to_string(),
            &["visible sidecar before fallback".to_string()],
        )
        .apply_ipc_dedupe(
            "deduped snapshot".to_string(),
            "prefix fallback with duplicate response".to_string(),
        );

        assert_eq!(decision.snapshot_content, "deduped snapshot");
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::LazilyVisibleWriteEvent
        );
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe)
        );
        assert!(decision.redeliver_editor);
        assert_eq!(
            decision
                .editor_bad_state
                .as_ref()
                .expect("combined repair should keep original bad editor proof")
                .content(),
            "visible sidecar before fallback"
        );
    }

    #[test]
    fn cycle_already_committed_returns_none_when_no_state() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("nonexistent.md");
        assert!(cycle_already_committed(&doc).is_none());
    }

    #[test]
    fn cycle_already_committed_returns_some_for_committed_cycle() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(&doc, "test", Some(content), Some(content))
            .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "test",
            Some(content),
            Some(content),
        )
        .unwrap();

        let result = cycle_already_committed(&doc);
        assert!(result.is_some(), "should return Some for committed cycle");
    }

    #[test]
    fn cycle_already_committed_prefers_lazily_projection_over_stale_sidecar() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        let opened =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(&doc, "test", Some(content), Some(content))
            .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "test",
            Some(content),
            Some(content),
        )
        .unwrap();

        let sidecar_path = agent_doc_fs::cycle_state_path_for(&doc)
            .unwrap()
            .expect("cycle sidecar path");
        fs::write(sidecar_path, serde_json::to_string_pretty(&opened).unwrap()).unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::PreflightStarted
        );

        assert_eq!(cycle_already_committed(&doc), Some(opened.cycle_id));
    }

    #[test]
    fn cycle_already_committed_returns_none_for_open_cycle() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        assert!(cycle_already_committed(&doc).is_none());
    }

    #[test]
    fn cleanup_fallback_patch_files_removes_patch_and_writes_sentinel() {
        let tmp = TempDir::new().unwrap();
        let doc =
            doc_in_agent_doc_project(&tmp, "---\nagent_doc_session: test\n---\n\n## Exchange\n");
        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        let patch_content = serde_json::json!({
            "patch_id": "test-patch-123",
            "type": "patch",
        });
        fs::write(
            &patch_path,
            serde_json::to_string_pretty(&patch_content).unwrap(),
        )
        .unwrap();
        assert!(patch_path.exists());

        cleanup_fallback_patch_files(&doc);

        assert!(
            !patch_path.exists(),
            "fallback patch file should be removed"
        );
        let sentinel = tmp
            .path()
            .join(".agent-doc/claimed-patches")
            .join("test-patch-123");
        assert!(sentinel.exists(), "claimed sentinel should be written");
    }

    #[test]
    fn cleanup_fallback_patch_files_noop_when_no_patch() {
        let tmp = TempDir::new().unwrap();
        let doc =
            doc_in_agent_doc_project(&tmp, "---\nagent_doc_session: test\n---\n\n## Exchange\n");
        cleanup_fallback_patch_files(&doc);
    }

    #[test]
    fn try_ipc_marks_committed_cycle_skip_as_not_consumed() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\nlate response\n";
        let doc = doc_in_agent_doc_project(&tmp, content);
        init_git_repo(tmp.path());
        git_commit_file(tmp.path(), "doc.md", content, "commit response");

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(&doc, "test", Some(content), Some(content))
            .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "test",
            Some(content),
            Some(content),
        )
        .unwrap();

        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let stale_patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        fs::write(
            &stale_patch_path,
            serde_json::json!({"patch_id": "late-patch-123"}).to_string(),
        )
        .unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "late response");
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            None,
            None,
            None,
            Some("current-patch-456"),
        )
        .unwrap();

        assert!(
            !result.success,
            "committed-cycle IPC skip must not look like a consumed write"
        );
        assert_eq!(result.patch_id, "current-patch-456");
        assert!(
            result.skipped_committed_cycle,
            "caller must be able to stop terminal fallback handling"
        );
        assert!(
            !stale_patch_path.exists(),
            "stale fallback patch should be removed"
        );
        assert!(
            tmp.path()
                .join(".agent-doc/claimed-patches/late-patch-123")
                .exists(),
            "removed stale patch should be claimed so watchers cannot replay it"
        );

        let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("late_fallback_patch_rejected"));
        assert!(ops_log.contains("patch_id=current-patch-456"));
        assert!(ops_log.contains(
            "flow=closeout stage=terminal_guard outcome=blocked reason=already_committed"
        ));
        assert!(
            !ops_log.contains("ipc_write_consumed"),
            "terminal skip must not be logged as an IPC consume"
        );
    }

    #[test]
    fn full_content_ipc_skips_committed_cycle_before_socket_or_file_fallback() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(&doc, "test", Some(content), Some(content))
            .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "test",
            Some(content),
            Some(content),
        )
        .unwrap();

        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let stale_patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        fs::write(
            &stale_patch_path,
            serde_json::json!({"patch_id": "full-content-stale"}).to_string(),
        )
        .unwrap();

        let result = try_ipc_full_content(&doc, "stale full-content repair").unwrap();

        assert!(!result, "committed-cycle full-content IPC must be skipped");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            content,
            "full-content IPC must not dirty an already committed cycle"
        );
        assert!(
            !stale_patch_path.exists(),
            "stale full-content fallback patch should be removed"
        );
        assert!(
            tmp.path()
                .join(".agent-doc/claimed-patches/full-content-stale")
                .exists(),
            "removed full-content fallback patch should be claimed"
        );

        let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("late_fallback_patch_rejected"));
        assert!(ops_log.contains("patch_id=full_content"));
        assert!(ops_log.contains(
            "flow=closeout stage=terminal_guard outcome=blocked reason=already_committed"
        ));
        assert!(
            !ops_log.contains("socket_full_content"),
            "full-content socket diagnostic must not be emitted after committed-cycle skip"
        );
    }

    #[test]
    fn full_content_operator_ipc_is_disabled_before_source_buffer_delivery() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let live = "before\nlive prompt\n";
        let target = "after\n";
        fs::write(&doc, live).unwrap();

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "operator full-content IPC must not be emitted when the disk buffer already contains live drift"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale full-content replacement must not overwrite live prompt drift"
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "failed full-content IPC must not save a snapshot"
        );
        let patch_count = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(
            patch_count, 0,
            "disabled full-content path must not hand a patch to file IPC"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_authority_rejected")
                && ops_log.contains("source=compact_exchange")
                && ops_log.contains("reason=stale_source_buffer"),
            "stale-source full-content rejection should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn full_content_operator_ipc_rejects_late_post_exchange_scratch_comment() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let prompt = "The full-document IPC scratch comment was typed below exchange after target computation. #spec-test-build-install-commit-push";
        let source = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "-->\n"
        );
        let live = source.replace("<!--\n-->", &format!("<!--\n{prompt}\n-->"));
        let target = source.replace(
            "### Re: previous — gpt-5\n\nDone.\n",
            "### Session Summary\n\nCompacted.\n",
        );
        fs::write(&doc, &live).unwrap();

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, &target, source).unwrap();

        assert!(
            !result,
            "operator full-content IPC must not be emitted after a late post-exchange scratch edit"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale full-content replacement must preserve the live scratch comment"
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "failed full-content IPC must not save a snapshot"
        );
        let patch_count = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(
            patch_count, 0,
            "scope/source guards must not hand a full-content patch to file IPC"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_scope_rejected")
                && ops_log.contains("source=compact_exchange"),
            "component-scope rejection should be logged before source-buffer proof:\n{ops_log}"
        );
    }

    #[test]
    fn response_fallback_full_content_is_disabled_before_socket_delivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let fallback = "before\n";
        let live = "before\nlive prompt typed after fallback was computed\n";
        fs::write(&doc, live).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            agent_doc_ipc_io::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let result = try_ipc_full_content(&doc, fallback).unwrap();

        assert!(
            !result,
            "stale response fallback full-content IPC must be skipped before socket delivery"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "socket listener must not receive stale response fallback full-content payloads"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale response fallback must not overwrite live prompt drift"
        );
        assert!(agent_doc_snapshot_io::load(&doc).unwrap().is_none());
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_authority_rejected")
                && ops_log.contains("source=response_fallback")
                && ops_log.contains("reason=stale_source_buffer"),
            "stale-source full-content rejection should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(agent_doc_ipc_io::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn ipc_dedupe_full_content_redelivery_is_disabled() {
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let bad_state = "before\n### Re: issue — gpt-5\nDone.\n### Re: issue — gpt-5\nDone.\n";
        let repaired = "before\n### Re: issue — gpt-5\nDone.\n";
        fs::write(&doc, bad_state).unwrap();

        let seen_payload = Arc::new(Mutex::new(None::<serde_json::Value>));
        let listener_seen = seen_payload.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                *listener_seen.lock().unwrap() = Some(payload.clone());
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            agent_doc_ipc_io::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let delivered = agent_doc_write_converge_io::redeliver_full_content_repair_to_editor(
            &doc,
            repaired,
            bad_state,
            FullContentRepairRedelivery::IpcDedupe,
            None,
            &mut |file, repaired_content, expected_bad_state| {
                try_ipc_full_content_response_fallback_from_source(
                    file,
                    repaired_content,
                    expected_bad_state,
                )
            },
        );

        assert!(!delivered, "full-content redelivery is disabled");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            bad_state,
            "disabled full-content redelivery must not mutate the editor-visible file"
        );
        assert!(
            seen_payload.lock().unwrap().is_none(),
            "listener should not receive a disabled full-content payload"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled")
                && ops_log.contains("source=response_fallback"),
            "disabled redelivery should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(agent_doc_ipc_io::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn ipc_dedupe_redelivery_skips_when_bad_state_is_stale() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let bad_state = "before\n### Re: issue — gpt-5\nDone.\n### Re: issue — gpt-5\nDone.\n";
        let live_state = "before\nlive prompt typed after repair planning\n";
        let repaired = "before\n### Re: issue — gpt-5\nDone.\n";
        fs::write(&doc, live_state).unwrap();

        let delivered = agent_doc_write_converge_io::redeliver_full_content_repair_to_editor(
            &doc,
            repaired,
            bad_state,
            FullContentRepairRedelivery::IpcDedupe,
            None,
            &mut |file, repaired_content, expected_bad_state| {
                try_ipc_full_content_response_fallback_from_source(
                    file,
                    repaired_content,
                    expected_bad_state,
                )
            },
        );

        assert!(
            !delivered,
            "redelivery must be skipped when the visible bad-state proof is stale"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live_state,
            "stale redelivery must not overwrite live content"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_dedupe_editor_redelivery_skipped")
                && ops_log.contains("skip=stale_bad_state"),
            "stale redelivery skip should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn template_ipc_dedupe_repair_requires_editor_delivery() {
        let tmp = TempDir::new().unwrap();
        let bad_state = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: issue — gpt-5\nDone.\n",
            "### Re: issue — gpt-5\nDone.\n",
            "<!-- /agent:exchange -->\n"
        );
        let repaired = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: issue — gpt-5\nDone.\n",
            "<!-- /agent:exchange -->\n"
        );
        let doc = doc_in_agent_doc_project(&tmp, bad_state);
        let agent_doc_dir = tmp.path().join(".agent-doc");

        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            agent_doc_ipc_io::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let decision = IpcRepairDecision::file_read(bad_state.to_string())
            .apply_ipc_dedupe(repaired.to_string(), bad_state.to_string());
        let err = agent_doc_write_converge_io::repair_ipc_decision_visible_state(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            &doc,
            &decision,
            Some("source-patch"),
            |file, repaired_content, expected_bad_state| {
                try_ipc_full_content_response_fallback_from_source(
                    file,
                    repaired_content,
                    expected_bad_state,
                )
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("refusing direct document write"),
            "template duplicate repair must fail closed without editor delivery: {err}"
        );

        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "component-scoped template repairs must not send socket fullContent payloads"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            bad_state,
            "template duplicate repair must leave the editor-visible bad state untouched"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_scope_rejected")
                && ops_log.contains("scope=template_frontmatter")
                && ops_log.contains("ipc_visible_repair_retry_required_no_disk_write"),
            "template fullContent rejection and retry should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(agent_doc_ipc_io::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn tsift_md_duplicate_content_fixture_skips_stale_full_document_redelivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let fixture = tsift_md_duplicate_content_corruption_fixture();
        let doc = tmp.path().join("tasks/software/tsift.md");
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, fixture.live_buffer_after_typing).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            agent_doc_ipc_io::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let delivered = agent_doc_write_converge_io::redeliver_full_content_repair_to_editor(
            &doc,
            fixture.repaired_snapshot,
            fixture.bad_state_before_live_typing,
            FullContentRepairRedelivery::IpcDedupe,
            None,
            &mut |file, repaired_content, expected_bad_state| {
                try_ipc_full_content_response_fallback_from_source(
                    file,
                    repaired_content,
                    expected_bad_state,
                )
            },
        );

        assert!(
            !delivered,
            "tsift.md fixture must skip full-document redelivery when the visible buffer changed after repair planning"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "stale tsift.md repair proof must be rejected before any socket fullContent payload"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fixture.live_buffer_after_typing,
            "live tsift.md prompt text typed after repair planning must remain untouched"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_dedupe_editor_redelivery_proof")
                && ops_log.contains("redeliver=false")
                && ops_log.contains("ipc_dedupe_editor_redelivery_skipped")
                && ops_log.contains("skip=stale_bad_state"),
            "stale tsift.md fixture should log proof and skip diagnostics:\n{ops_log}"
        );

        let _ = fs::remove_file(agent_doc_ipc_io::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn socket_full_content_is_disabled_before_payload_delivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let live = "before\nlive prompt typed during compact\n";
        let target = "after\n";
        fs::write(&doc, live).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            agent_doc_ipc_io::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "full-content path should reject before socket delivery"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "socket listener must not receive stale full-content payloads"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), live);
        assert!(agent_doc_snapshot_io::load(&doc).unwrap().is_none());
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_authority_rejected")
                && ops_log.contains("reason=stale_source_buffer"),
            "stale-source full-content rejection should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(agent_doc_ipc_io::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn socket_full_content_disabled_path_does_not_save_snapshot() {
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("visible-write")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let target = "after\n";
        fs::write(&doc, source).unwrap();

        let root = tmp.path().to_path_buf();
        let listener_root = root.clone();
        let server = std::thread::spawn(move || {
            agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = payload.get("patch_id")?.as_str()?;
                let file_path = payload.get("file")?.as_str()?;
                crate::ipc::transport::record_test_visible_write_receipt(
                    Path::new(file_path),
                    patch_id,
                    "wrong\n",
                    "test_socket_listener",
                );
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });

        std::thread::sleep(Duration::from_millis(100));
        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "socket full-content IPC must be disabled before payload delivery"
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "mismatched socket visible-write must not become the saved snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "socket mismatch rejection must leave disk content untouched"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled"),
            "disabled full-content path should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(agent_doc_ipc_io::socket_path(&root));
        drop(server);
    }

    fn init_git_repo(root: &Path) {
        use std::process::Command;
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "commit.gpgsign", "false"])
            .output()
            .unwrap();
    }

    fn git_commit_file(root: &Path, rel: &str, content: &str, msg: &str) {
        use std::process::Command;
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "--", rel])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", msg, "--no-verify"])
            .output()
            .unwrap();
    }

    fn head_count(root: &Path) -> usize {
        use std::process::Command;
        let out = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    #[test]
    fn recover_dedupe_only_drift_commits_when_file_matches_dedupe_of_head() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");

        // Simulate what `agent-doc dedupe` produced: file + snapshot both equal
        // the deduped form, HEAD still holds the duplicate.
        let deduped = agent_doc_turn::response_replay::dedupe_responses(duplicated);
        assert_ne!(
            deduped, duplicated,
            "test setup: duplicated content must actually dedupe"
        );
        fs::write(&doc, &deduped).unwrap();
        agent_doc_snapshot_io::save(&doc, &deduped, agent_doc_ops_log_io::log_op).unwrap();

        let head_before = head_count(root);
        let recovered = agent_doc_repair_runtime_io::recover_dedupe_only_drift(&doc)
            .expect("dedupe-only drift recovery should succeed");
        assert!(
            recovered,
            "file matching dedupe(HEAD) must be recognized as a dedupe-only drift"
        );

        // Commit landed through the binary path.
        let head_after = head_count(root);
        assert_eq!(
            head_after,
            head_before + 1,
            "dedupe-only recovery must produce exactly one new commit"
        );
        let head_content = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            head_content.matches("### Re: topic — opus-4-7").count(),
            1,
            "committed HEAD must hold the deduped response"
        );
        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            snapshot_after.matches("### Re: topic — opus-4-7").count(),
            1,
            "snapshot must hold the deduped response (boundary markers may differ from disk)"
        );
    }

    #[test]
    fn recover_dedupe_only_drift_skips_when_file_matches_head() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let clean = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", clean, "add clean");
        let doc = root.join("session.md");
        agent_doc_snapshot_io::save(&doc, clean, agent_doc_ops_log_io::log_op).unwrap();

        let recovered = agent_doc_repair_runtime_io::recover_dedupe_only_drift(&doc).unwrap();
        assert!(
            !recovered,
            "no drift between file and HEAD should not trigger dedupe-only recovery"
        );
    }

    #[test]
    fn recover_dedupe_only_drift_skips_when_drift_is_not_a_dedupe_outcome() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let original = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", original, "add original");
        let doc = root.join("session.md");

        // Working tree differs from HEAD by an arbitrary user edit, not by
        // dedupe. Recovery must refuse so we don't auto-commit unrelated drift.
        let user_edit = original.replace("Implemented.", "Implemented and tested.");
        fs::write(&doc, &user_edit).unwrap();
        agent_doc_snapshot_io::save(&doc, &user_edit, agent_doc_ops_log_io::log_op).unwrap();

        let recovered = agent_doc_repair_runtime_io::recover_dedupe_only_drift(&doc).unwrap();
        assert!(
            !recovered,
            "arbitrary working-tree drift must not be auto-committed as a dedupe recovery"
        );
    }

    // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
    // Phase 4 + Phase 5 regression coverage. Exercises the full
    // `agent-doc dedupe` → `agent-doc write --commit` (empty stdin) recovery
    // path through the strict-closeout entry point that the four `run` /
    // `stream` / `write` call sites use.
    #[test]
    fn recover_empty_response_for_strict_closeout_lands_dedupe_only_drift_through_binary_commit() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");

        let deduped = agent_doc_turn::response_replay::dedupe_responses(duplicated);
        fs::write(&doc, &deduped).unwrap();
        agent_doc_snapshot_io::save(&doc, &deduped, agent_doc_ops_log_io::log_op).unwrap();

        let head_before = head_count(root);
        let recovered = agent_doc_repair_io::recover_empty_response_for_strict_closeout(
            agent_doc_repair_runtime_io::repair_coordinator_effects(
                &NOOP_REPAIR_REPLAY_WRITE_EFFECTS,
            ),
            &doc,
            true,
            false,
            Some(false),
        )
        .expect("strict-closeout empty-stdin path should recognize dedupe-only drift");
        assert!(
            recovered,
            "empty stdin + strict closeout + dedupe-only drift must commit through the binary path"
        );
        assert_eq!(
            head_count(root),
            head_before + 1,
            "exactly one new commit should land via the dedupe recovery wrapper"
        );

        let head_after = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            head_after.matches("### Re: topic — opus-4-7").count(),
            1,
            "committed HEAD must hold the deduped response"
        );
    }

    #[test]
    fn recover_empty_response_for_strict_closeout_refuses_when_not_strict_closeout() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");
        let deduped = agent_doc_turn::response_replay::dedupe_responses(duplicated);
        fs::write(&doc, &deduped).unwrap();
        agent_doc_snapshot_io::save(&doc, &deduped, agent_doc_ops_log_io::log_op).unwrap();

        let head_before = head_count(root);
        let recovered = agent_doc_repair_io::recover_empty_response_for_strict_closeout(
            agent_doc_repair_runtime_io::repair_coordinator_effects(
                &NOOP_REPAIR_REPLAY_WRITE_EFFECTS,
            ),
            &doc,
            false,
            false,
            Some(false),
        )
        .unwrap();
        assert!(
            !recovered,
            "non-strict empty-stdin path must not silently auto-commit dedupe drift"
        );
        assert_eq!(
            head_count(root),
            head_before,
            "non-strict path should not produce a commit"
        );
    }

    /// #ipcvisredeliver-incycle: a `live_prompt_drift` repair whose disk-keyed
    /// redelivery is skipped as `stale_bad_state` (disk lags the live editor
    /// buffer) must now converge IN-CYCLE via the editor IPC listener instead of
    /// bailing with `retry_without_disk_write` and only recovering on a later
    /// commit-path cycle. A component-patch listener that publishes the
    /// lazily visible-write receipt proves the editor reached the repaired target.
    #[test]
    fn live_prompt_drift_repair_converges_in_cycle_via_editor_ipc() {
        // Disk lags the editor: the on-disk buffer equals neither the stale
        // candidate (`bad_state`) nor the repaired target, so the disk-keyed
        // redelivery is skipped and the new in-cycle convergence branch runs.
        let disk_lag = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "do the thing\n",
            "<!-- /agent:exchange -->\n"
        );
        let repaired = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "do the thing\n",
            "### Re: the thing — gpt-5\nDone.\n",
            "<!-- /agent:exchange -->\n"
        );
        let bad_state = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "stale candidate\n",
            "<!-- /agent:exchange -->\n"
        );
        let tmp = TempDir::new().unwrap();
        let doc = doc_in_agent_doc_project(&tmp, disk_lag);
        let agent_doc_dir = tmp.path().join(".agent-doc");

        // Editor listener: apply the component patch to the payload baseline and
        // publish the converged buffer as a lazily visible-write receipt, exactly
        // like the JetBrains plugin's Document-API convergence.
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = payload
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let mut content = payload.get("baseline")?.as_str()?.to_string();
                for patch in payload.get("patches")?.as_array()? {
                    let name = patch.get("component")?.as_str()?;
                    let replacement = patch.get("content")?.as_str()?;
                    let comps = agent_doc_element::element::parse(&content).ok()?;
                    let target = comps.iter().find(|component| component.name == name)?;
                    content = target.replace_content(&content, replacement);
                }
                if let Some(file_path) = payload.get("file").and_then(|value| value.as_str()) {
                    let _ = std::fs::write(file_path, &content);
                    crate::ipc::transport::record_test_visible_write_receipt(
                        Path::new(file_path),
                        &patch_id,
                        &content,
                        "test_socket_listener",
                    );
                }
                Some(
                    serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                        .to_string(),
                )
            });
        });
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            agent_doc_ipc_io::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        // The finalize path records the adoption block before repair; the
        // in-cycle auto-recovery is gated on that flag, so mirror it here (a
        // cycle must exist first, exactly as preflight seeds it in production).
        agent_doc_cycle_state_io::start_preflight(&doc, Some(repaired), Some(disk_lag)).unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let decision = IpcRepairDecision {
            snapshot_content: repaired.to_string(),
            snap_source: IpcSnapshotSource::ContentOurs,
            disk_repair_reason: Some(IpcDiskRepairReason::LivePromptDrift),
            editor_bad_state: Some(EditorBadStateFingerprint::new(bad_state.to_string())),
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: true,
            live_prompt_drift_state:
                agent_doc_ipc_protocol::IpcLivePromptDriftState::SnapshotReconciled,
        };

        agent_doc_write_converge_io::repair_ipc_decision_visible_state(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            &doc,
            &decision,
            Some("live-drift-1"),
            |file, repaired_content, expected_bad_state| {
                try_ipc_full_content_response_fallback_from_source(
                    file,
                    repaired_content,
                    expected_bad_state,
                )
            },
        )
        .expect("in-cycle editor-IPC convergence should prove visible state and succeed");

        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_visible_repair_incycle_editor_converged"),
            "in-cycle convergence marker should be logged:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("ipc_visible_repair_retry_required_no_disk_write"),
            "successful in-cycle convergence must not emit the retry_without_disk_write bail:\n{ops_log}"
        );

        let _ = fs::remove_file(agent_doc_ipc_io::socket_path(tmp.path()));
        drop(server);
    }

    /// The in-cycle convergence must stay fail-closed: with NO editor IPC
    /// listener active, a stale `live_prompt_drift` repair still refuses a direct
    /// disk write and bails with `retry_without_disk_write` (the no-disk-write
    /// invariant is preserved unchanged).
    #[test]
    fn live_prompt_drift_repair_without_listener_still_fails_closed() {
        let disk_lag = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "do the thing\n",
            "<!-- /agent:exchange -->\n"
        );
        let repaired = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "do the thing\n",
            "### Re: the thing — gpt-5\nDone.\n",
            "<!-- /agent:exchange -->\n"
        );
        let bad_state = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "stale candidate\n",
            "<!-- /agent:exchange -->\n"
        );
        let tmp = TempDir::new().unwrap();
        let doc = doc_in_agent_doc_project(&tmp, disk_lag);
        let agent_doc_dir = tmp.path().join(".agent-doc");

        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let decision = IpcRepairDecision {
            snapshot_content: repaired.to_string(),
            snap_source: IpcSnapshotSource::ContentOurs,
            disk_repair_reason: Some(IpcDiskRepairReason::LivePromptDrift),
            editor_bad_state: Some(EditorBadStateFingerprint::new(bad_state.to_string())),
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: true,
            live_prompt_drift_state:
                agent_doc_ipc_protocol::IpcLivePromptDriftState::SnapshotReconciled,
        };

        let err = agent_doc_write_converge_io::repair_ipc_decision_visible_state(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            &doc,
            &decision,
            Some("live-drift-2"),
            |file, repaired_content, expected_bad_state| {
                try_ipc_full_content_response_fallback_from_source(
                    file,
                    repaired_content,
                    expected_bad_state,
                )
            },
        )
        .expect_err("without a listener the stale live_prompt_drift repair must fail closed");
        assert!(
            err.to_string().contains("refusing direct document write"),
            "fail-closed bail expected: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            disk_lag,
            "fail-closed repair must not mutate the on-disk buffer"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_visible_repair_retry_required_no_disk_write")
                && !ops_log.contains("ipc_visible_repair_incycle_editor_converged"),
            "no-listener path must bail with retry_without_disk_write and no in-cycle marker:\n{ops_log}"
        );
    }
}
