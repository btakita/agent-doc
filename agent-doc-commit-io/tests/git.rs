use std::fs;
use std::path::Path;
use std::process::Command;

use agent_doc_commit_io::{
    commit, commit_proven_response_replay_canonicalization, commit_with_authoritative_compaction,
};
use agent_doc_document::transient_markers::normalize_transient_agent_doc_markers;

#[cfg(test)]
mod th {
    use super::*;
    pub(crate) fn init_repo(repo: &Path) {
        Command::new("git")
            .current_dir(repo)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
    }
    pub(crate) fn commit_file(repo: &Path, rel: &str, content: &str, msg: &str) {
        let path = repo.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["add", "--", rel])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", msg, "--no-verify"])
            .output()
            .unwrap();
    }
    // --- Bug 2B regression tests ---
    // Verify that commit does NOT overwrite the snapshot with user edits.
    // The divergence detection was removed from commit because is_stale_baseline
    // cannot distinguish "file has user edits" from "file has a missed agent response" —
    // both look like "file has content snapshot doesn't have".
    // --- #73tv: repo-scoped commit serialization + full transaction retry ---
    fn start_fake_listener_with_ack_status(
        project_root: &Path,
        ack_status: Option<&'static str>,
    ) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let result = agent_doc_ipc_io::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                if let Some(status) = ack_status {
                    let receipt_status = match status {
                        "error" | "rejected" => "rejected",
                        _ => "applied",
                    };
                    return Some(
                        serde_json::json!({
                            "type": "receipt",
                            "id": patch_id,
                            "status": receipt_status,
                            "reason": "test_refresh_failed"
                        })
                        .to_string(),
                    );
                }
                // Model the JB plugin's behavior: refresh_content messages carry
                // the new content in the message body (the IDE applies it to its
                // in-memory buffer without reading disk). Other message types
                // fall back to disk (patch files, etc.).
                let file_path = v.get("file").and_then(|f| f.as_str()).unwrap_or("");
                let content = if v.get("type").and_then(|t| t.as_str()) == Some("refresh_content") {
                    v.get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string()
                } else if !file_path.is_empty() {
                    std::fs::read_to_string(file_path).unwrap_or_default()
                } else {
                    String::new()
                };
                if !file_path.is_empty() {
                    let file = Path::new(file_path);
                    let _ =
                        agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
                            file,
                            patch_id,
                            &content,
                            "test_socket_listener",
                        );
                }
                Some(
                    serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                        .to_string(),
                )
            });
            if let Err(err) = result {
                eprintln!("[test] fake listener stopped: {err:#}");
            }
        })
    }
    pub(crate) fn start_fake_listener(project_root: &Path) -> std::thread::JoinHandle<()> {
        start_fake_listener_with_ack_status(project_root, None)
    }
    pub(crate) fn wait_for_listener(project_root: &Path) {
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(project_root) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("fake socket listener did not start within 1s");
    }
    // --- Retired: run_stream unproven-IPC direct write ---
    // Unproven IPC now fails closed without saving snapshots or writing the document.
    // --- Submodule-aware commit routing ---
    // --- relative_to path normalization ---
}
#[cfg(test)]
pub(crate) use th::{commit_file, init_repo, start_fake_listener, wait_for_listener};

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;

    #[test]
    fn commit_adopts_manual_escaped_tail_cleanup_after_head_current_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n\n\
            The routed prompt escaped below the exchange block.\n\
            It should be cleaned up without being treated as later drift.\n\n\
            do #oobtaildel. spec-test-build-install-commit-push\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let cleaned = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
        fs::write(&doc, cleaned).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let did_commit = commit(&doc).expect("escaped tail cleanup should commit");
        assert!(did_commit, "cleanup deletion should create a commit");

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&head),
            normalize_transient_agent_doc_markers(cleaned),
            "HEAD should contain the cleanup deletion"
        );
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&snap),
            normalize_transient_agent_doc_markers(cleaned),
            "snapshot should advance to the cleaned file"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_escaped_tail_cleanup file="),
            "cleanup should get a specific ops-log marker:\n{log}"
        );
        assert!(
            !log.contains("post_commit_local_drift file="),
            "cleanup-only deletion must not be classified as local drift:\n{log}"
        );
    }

    #[test]
    fn commit_allows_current_snapshot_to_replace_committed_historical_patchback() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let clean = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "clean exchange\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#tmv7] Release workflow\n",
            "<!-- /agent:icebox -->\n",
        );
        fs::write(&doc, clean).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            clean,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let historical_head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "clean exchange\n\n",
            "#code-review\n",
            "### Re: code review — gpt-5\n\n",
            "Historical patchback.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#tmv7] Release workflow\n",
            "<!-- /agent:icebox -->\n",
        );
        fs::write(&doc, historical_head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        let compacted = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted.*\n\n",
            "❯ #code-review\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [x] [#tmv7] Release workflow\n",
            "<!-- /agent:icebox -->\n",
        );
        fs::write(&doc, compacted).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            compacted,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(compacted), Some(compacted)).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(compacted),
            Some(compacted),
        )
        .unwrap();

        let did_commit =
            commit(&doc).expect("current snapshot/file should replace the historical patchback");
        assert!(did_commit, "replacement commit should be created");

        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&head_doc),
            normalize_transient_agent_doc_markers(compacted),
            "HEAD should advance to the compacted document:\n{head_doc}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("commit_blocked_committed_historical_patchback file="),
            "historical patchback should not block replacement commit:\n{log}"
        );
    }

    #[test]
    fn commit_dedupes_duplicate_response_snapshot_before_staging() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let initial = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do #pbdupchurn
<!-- /agent:exchange -->
";
        commit_file(root, "session.md", initial, "add session");

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do #pbdupchurn
### Re: #pbdupchurn — gpt-5

Implemented.
### Re: #pbdupchurn — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let doc = root.join("session.md");
        fs::write(&doc, duplicated).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            duplicated,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let before = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        let before_count: usize = String::from_utf8_lossy(&before.stdout)
            .trim()
            .parse()
            .unwrap();

        let did_commit = commit(&doc).expect("deduped closeout should commit");
        assert!(did_commit);

        let after = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        let after_count: usize = String::from_utf8_lossy(&after.stdout)
            .trim()
            .parse()
            .unwrap();
        assert_eq!(
            after_count,
            before_count + 1,
            "dedupe must happen before the first closeout commit, not in a second cleanup commit"
        );

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(head.matches("### Re: #pbdupchurn — gpt-5").count(), 1);
        assert_eq!(snapshot.matches("### Re: #pbdupchurn — gpt-5").count(), 1);
        assert_eq!(working.matches("### Re: #pbdupchurn — gpt-5").count(), 1);
    }
    #[test]
    fn commit_blocks_snapshot_absorb_after_ipc_snapshot_adoption_blocked() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/state/cycles")).unwrap();

        let initial = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do #snapabsorb
<!-- /agent:exchange -->
";
        commit_file(root, "session.md", initial, "add session");

        let snapshot = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do #snapabsorb
### Re: #snapabsorb — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let live = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do #snapabsorb
### Re: #snapabsorb — gpt-5

Implemented.
### Re: late socket replay — gpt-5

Duplicate replay should stay live.
<!-- /agent:exchange -->
";
        let doc = root.join("session.md");
        fs::write(&doc, live).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(initial), Some(initial)).unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let did_commit = commit(&doc).expect("commit should stage content_ours snapshot");

        assert!(did_commit);
        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        let working = fs::read_to_string(&doc).unwrap();
        assert!(head.contains("### Re: #snapabsorb — gpt-5"));
        assert!(!head.contains("late socket replay"));
        assert!(!snapshot_after.contains("late socket replay"));
        assert!(
            working.contains("late socket replay"),
            "live divergent body should stay in the working tree for the next cycle"
        );
        let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("snapshot_absorb_blocked_after_ipc_snapshot_adoption"),
            "blocked absorb should be logged:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("snapshot_absorb file="),
            "commit must not silently absorb the divergent disk body after IPC adoption was blocked:\n{ops_log}"
        );
    }

    #[test]
    fn commit_excludes_and_preserves_unrelated_staged_work() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        th::init_repo(root);
        th::commit_file(root, "session.md", "initial session\n", "add session");
        th::commit_file(root, "foreign.txt", "foreign baseline\n", "add foreign");

        fs::write(root.join("foreign.txt"), "foreign staged work\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "--", "foreign.txt"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        fs::write(&doc, "updated session\n").unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "updated session\n",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        assert!(commit(&doc).expect("session document should commit"));

        let committed_paths = Command::new("git")
            .current_dir(root)
            .args(["show", "--pretty=format:", "--name-only", "HEAD"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&committed_paths.stdout).trim(),
            "session.md",
            "agent-doc must commit only its owned session document"
        );
        let staged_paths = Command::new("git")
            .current_dir(root)
            .args(["diff", "--cached", "--name-only"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&staged_paths.stdout).trim(),
            "foreign.txt",
            "unrelated staged work must remain staged"
        );
        let foreign_head = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:foreign.txt"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&foreign_head.stdout),
            "foreign baseline\n"
        );
        let foreign_index = Command::new("git")
            .current_dir(root)
            .args(["show", ":foreign.txt"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&foreign_index.stdout),
            "foreign staged work\n"
        );
    }

    #[test]
    fn replay_canonicalization_commit_is_exact_idempotent_and_preserves_unrelated_staging() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        th::init_repo(root);
        let duplicated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ operator prompt\n\n",
            "### Re: retained topic — gpt-5\n\nRetained response.\n\n",
            "### Re: intervening topic — gpt-5\n\nIntervening response.\n\n",
            "### Re: retained topic — gpt-5\n\n",
            "### Re: latest topic — gpt-5\n\nLatest response.\n",
            "<!-- agent:boundary:latest -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let repaired =
            agent_doc_document_realtime_io::normalize_recoverable_response_replay_duplication(
                duplicated,
            )
            .expect("fixture must be a losslessly repairable replay");
        th::commit_file(root, "session.md", duplicated, "add replayed session");
        th::commit_file(root, "foreign.txt", "foreign baseline\n", "add foreign");

        fs::write(root.join("foreign.txt"), "foreign staged work\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "--", "foreign.txt"])
            .output()
            .unwrap();
        let doc = root.join("session.md");
        fs::write(&doc, &repaired).unwrap();

        assert!(
            commit_proven_response_replay_canonicalization(&doc)
                .expect("proven replay repair should commit")
        );

        let session_head = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&session_head.stdout), repaired);
        let foreign_head = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:foreign.txt"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&foreign_head.stdout),
            "foreign baseline\n"
        );
        let foreign_index = Command::new("git")
            .current_dir(root)
            .args(["show", ":foreign.txt"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&foreign_index.stdout),
            "foreign staged work\n"
        );
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .as_deref(),
            Some(repaired.as_str())
        );

        let commit_count_before = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        assert!(
            !commit_proven_response_replay_canonicalization(&doc)
                .expect("retry should settle without another commit")
        );
        let commit_count_after = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        assert_eq!(commit_count_before.stdout, commit_count_after.stdout);
    }

    #[test]
    fn reposition_boundary_to_end_basic() {
        let content = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:abc123 -->\nUser prompt.\n<!-- /agent:exchange -->\n";
        let result = agent_doc_template::reposition_boundary_to_end(content);
        // Boundary should be after user prompt, before close tag
        assert!(result.contains("User prompt.\n<!-- agent:boundary:"));
        assert!(result.contains("-->\n<!-- /agent:exchange -->"));
        // Old boundary consumed
        assert!(!result.contains("abc123"));
    }
    #[test]
    fn reposition_boundary_no_exchange() {
        let content = "# No exchange component\nJust text.\n";
        let result = agent_doc_template::reposition_boundary_to_end(content);
        // Should return unchanged if no exchange
        assert_eq!(result.trim(), content.trim());
    }
    #[test]
    fn reposition_boundary_preserves_user_edits() {
        let content = "<!-- agent:exchange patch=append -->\n### Re: Answer\nAgent response.\n<!-- agent:boundary:old-id -->\nUser's new prompt here.\nMore user text.\n<!-- /agent:exchange -->\n";
        let result = agent_doc_template::reposition_boundary_to_end(content);
        assert!(
            result.contains("User's new prompt here."),
            "user edit must be preserved"
        );
        assert!(
            result.contains("More user text."),
            "user edit must be preserved"
        );
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let user_pos = result.find("User's new prompt here.").unwrap();
        assert!(boundary_pos > user_pos, "boundary must be after user text");
    }
    #[test]
    fn reposition_boundary_cleans_multiple_stale() {
        // Simulate a document with multiple stale boundary markers
        let content = "<!-- agent:exchange patch=append -->\n\
            First response.\n\
            <!-- agent:boundary:aaa111 -->\n\
            Second response.\n\
            <!-- agent:boundary:bbb222 -->\n\
            User prompt.\n\
            <!-- /agent:exchange -->\n";
        let result = agent_doc_template::reposition_boundary_to_end(content);
        // All old boundaries should be removed
        assert!(
            !result.contains("aaa111"),
            "first stale boundary must be removed"
        );
        assert!(
            !result.contains("bbb222"),
            "second stale boundary must be removed"
        );
        // Exactly one fresh boundary should exist
        let boundary_count = result.matches("<!-- agent:boundary:").count();
        assert_eq!(
            boundary_count, 1,
            "exactly one boundary marker should remain"
        );
        // The single boundary should be after user prompt
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let user_pos = result.find("User prompt.").unwrap();
        assert!(boundary_pos > user_pos, "boundary must be after user text");
    }
    #[test]
    fn is_stale_baseline_write_path_user_edits_in_baseline_not_stale() {
        // Write path: baseline has user edits appended, snapshot is the committed state.
        // is_stale_baseline(baseline_with_edits, snapshot) should be FALSE
        // because the baseline's exchange CONTAINS the snapshot's exchange content.
        let snapshot = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response\n\
            Agent response text.\n\
            <!-- /agent:exchange -->\n";
        let baseline_with_user_edits = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response\n\
            Agent response text.\n\
            Implement agent-kit changes.\n\
            Implement updates to agent-doc.\n\
            <!-- /agent:exchange -->\n";

        assert!(
            !agent_doc_template::stale_baseline::is_stale_baseline(
                baseline_with_user_edits,
                snapshot
            ),
            "baseline with user edits should NOT be stale (it contains snapshot content)"
        );
    }
    #[test]
    fn is_stale_baseline_write_path_stale_baseline_detected() {
        // Write path: baseline is from before the last agent response.
        // is_stale_baseline(old_baseline, current_snapshot) should be TRUE.
        let current_snapshot = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response 1\n\
            First response.\n\
            ### Re: Response 2\n\
            Second response.\n\
            <!-- /agent:exchange -->\n";
        let old_baseline = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response 1\n\
            First response.\n\
            <!-- /agent:exchange -->\n";

        assert!(
            agent_doc_template::stale_baseline::is_stale_baseline(old_baseline, current_snapshot),
            "baseline missing committed response should be stale"
        );
    }
    #[test]
    fn is_in_git_repo_true_inside_repo() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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

        let doc = root.join("doc.md");
        fs::write(&doc, "# test\n").unwrap();

        assert!(
            agent_doc_git_io::status::is_in_git_repo(&doc),
            "file inside git repo should return true"
        );
    }
    #[test]
    fn is_in_git_repo_false_outside_repo() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "# test\n").unwrap();

        assert!(
            !agent_doc_git_io::status::is_in_git_repo(&doc),
            "file outside git repo should return false"
        );
    }
    #[test]
    fn write_commit_lifecycle() {
        // Full lifecycle: git repo + snapshot + commit → verify commit in log.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        // Set up git repo
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

        // Create and commit an initial file so HEAD exists
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // Create a document at its state before response capture and commit it.
        let doc = root.join("session.md");
        let initial_content = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
        fs::write(&doc, initial_content).unwrap();

        // Stage + initial commit so the file is tracked
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Simulate a write cycle landing a new response: update both the
        // working tree and the snapshot with the post-response content so
        // commit staging has something to commit.
        let post_response = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, post_response).unwrap();

        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, post_response).unwrap();

        // Now call commit (simulating what --commit does after write)
        commit(&doc).expect("commit should succeed");

        // Verify a new commit exists with the agent-doc message
        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-3"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(session):"),
            "git log should contain agent-doc commit, got:\n{log_str}"
        );
    }
    #[test]
    fn commit_retries_full_transaction_when_stage_hits_index_lock() {
        use std::fs;
        use std::thread;
        use std::time::Duration;

        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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

        let doc = root.join("session.md");
        let initial = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
        fs::write(&doc, initial).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let updated =
            "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nFixed.\n\n";
        fs::write(&doc, updated).unwrap();
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            updated,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let index_lock = root.join(".git/index.lock");
        fs::write(&index_lock, "held").unwrap();

        let remover = thread::spawn({
            let index_lock = index_lock.clone();
            move || {
                thread::sleep(Duration::from_millis(200));
                fs::remove_file(index_lock).unwrap();
            }
        });

        let did_commit = commit(&doc).expect("commit should retry until index.lock clears");
        remover.join().unwrap();

        assert!(
            did_commit,
            "commit should create a git commit after retrying"
        );
        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-2"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(session):"),
            "git log should contain the retried agent-doc commit, got:\n{log_str}"
        );
    }
    #[test]
    fn commit_succeeds_when_no_lock_contention() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let content =
            "---\nagent_doc_session: test\n---\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // No lock present — commit should succeed on first try
        let result = commit(&doc);
        assert!(
            result.is_ok(),
            "commit without lock should succeed: {:?}",
            result.err()
        );
    }
    #[test]
    fn commit_staged_blob_has_no_head_markers() {
        // Regression for bug #dsng: (HEAD) is a working-tree-only marker and
        // must never appear in the committed blob. If it does, the next
        // cycle's reposition produces a phantom "strip (HEAD)" diff on
        // prior-cycle headings the user is editing.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // Initial doc + snapshot, tracked cleanly (no HEAD markers yet).
        let doc = root.join("session.md");
        let initial = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        fs::write(&doc, initial).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, initial).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Simulate a write cycle: snapshot has a new response whose heading
        // still carries a transient `(HEAD)` marker.
        let cycle1 = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: older\nold body\n\n### Re: newer (HEAD)\nnew body\n<!-- /agent:exchange -->\n";
        fs::write(&doc, cycle1).unwrap();
        fs::write(&snap_abs, cycle1).unwrap();

        commit(&doc).expect("commit should succeed");

        // Assert the committed blob has ZERO `(HEAD)` occurrences.
        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let blob = String::from_utf8_lossy(&show.stdout);
        assert!(
            !blob.contains("(HEAD)"),
            "committed blob must not contain (HEAD); got:\n{blob}"
        );
        assert!(
            blob.contains("### Re: newer\n"),
            "committed blob should contain the clean new heading; got:\n{blob}"
        );
        assert!(
            blob.contains("### Re: older\n"),
            "committed blob should still contain the older heading; got:\n{blob}"
        );

        // Post-commit cleanup now converges the working tree back to committed
        // HEAD when the only remaining drift is agent-owned transient churn.
        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("### Re: newer\n"),
            "working tree should keep the clean newest heading after closeout; got:\n{working}"
        );
        assert_eq!(
            working.matches("(HEAD)").count(),
            0,
            "working tree should not retain transient head markers after closeout; got:\n{working}"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("### Re: newer\n"),
            "snapshot should keep the clean heading; got:\n{snap}"
        );
        assert!(
            snap.matches("(HEAD)").count() == 0,
            "snapshot should not retain transient head markers; got:\n{snap}"
        );
    }
    #[test]
    fn reposition_collapses_snapshot_boundaries_even_during_active_run() {
        // Regression for #boundaryaccum1: a wedged finalize leaves a
        // retained response intent in the ledger, and the response lands via a direct
        // commit. The active-run guard must scope ONLY the working-tree rewrite
        // — the binary-owned snapshot collapse must still run, so the
        // staged/committed blob always carries exactly one boundary and a
        // boundary can no longer accrete per wedged cycle.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        init_repo(root);

        let doc = root.join("session.md");
        // Snapshot carries THREE scattered stale boundaries, as a wedged
        // multi-cycle drain would leave them.
        let multi = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            ### Re: one\nbody one\n\
            <!-- agent:boundary:aaa111 -->\n\
            ### Re: two\nbody two\n\
            <!-- agent:boundary:bbb222 -->\n\
            User prompt.\n\
            <!-- agent:boundary:ccc333 -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, multi).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            multi,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // Simulate an active run through its retained response intent.
        agent_doc_cycle_state_io::start_preflight(&doc, Some(multi), Some(multi)).unwrap();
        agent_doc_repair_io::pending::save_pending(&doc, "in-flight").unwrap();

        agent_doc_git_io::boundary_reposition::reposition_boundary_in_snapshot(
            &agent_doc_commit_io::BOUNDARY_REPOSITION_EFFECTS,
            &doc,
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        let count = snap
            .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
            .count();
        assert_eq!(
            count, 1,
            "snapshot must collapse to exactly one boundary even during an active run; got {count}:\n{snap}"
        );
    }
    #[test]
    fn commit_skips_ignored_untracked_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        commit_file(
            root,
            ".gitignore",
            "scratch/\n.agent-doc/\n",
            "ignore scratch",
        );

        let doc = root.join("scratch/session.md");
        let content = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: ignored\nbody\n<!-- /agent:exchange -->\n";
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, content).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();

        let did_commit = commit(&doc).expect("ignored path should be skipped without panicking");
        assert!(
            !did_commit,
            "ignored untracked document must not create an agent-doc commit"
        );

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:scratch/session.md"])
            .output()
            .unwrap();
        assert!(
            !show.status.success(),
            "ignored untracked document must not be present in HEAD"
        );

        let listed = Command::new("git")
            .current_dir(root)
            .args(["ls-files", "--", "scratch/session.md"])
            .output()
            .unwrap();
        assert!(
            listed.stdout.is_empty(),
            "ignored untracked document must not be staged/tracked"
        );
    }
    #[test]
    fn commit_staged_blob_restores_answered_prompt_prefixes() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let initial = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        fs::write(&doc, initial).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, initial).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let cycle = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: older\nold body\n\nPlease restart Codex and deploy the 503 fixes again.\n### Re: retry production deploy — gpt-5\nNo state change.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, cycle).unwrap();
        fs::write(&snap_abs, cycle).unwrap();

        commit(&doc).expect("commit should canonicalize answered prompt prefixes");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let blob = String::from_utf8_lossy(&show.stdout);
        assert!(
            blob.contains("❯ Please restart Codex and deploy the 503 fixes again.\n"),
            "committed blob should preserve the user prompt prefix:\n{blob}"
        );
        assert!(
            !blob.contains("\nPlease restart Codex and deploy the 503 fixes again.\n"),
            "committed blob must not keep the bare prompt line:\n{blob}"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ Please restart Codex and deploy the 503 fixes again.\n"),
            "working tree should preserve the user prompt prefix after closeout:\n{working}"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("❯ Please restart Codex and deploy the 503 fixes again.\n"),
            "snapshot should preserve the user prompt prefix after closeout:\n{snap}"
        );
    }
    #[test]
    fn commit_does_not_prefix_prior_response_tail_before_answered_prompt() {
        use std::fs;
        use std::process::Command;

        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".agent-doc/\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", ".gitignore"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let cycle = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nCommit / push:\n- `src/agent-doc`: `abc1234` pushed to `origin/main`\n\nI did not create a superproject gitlink commit because the workspace root already had unrelated dirty changes outside this fix.\n\nThere were no actionable follow-up items to capture.\ndo [#tailpatch]. spec-test-build-install-commit-push\n### Re: `#tailpatch` closeout-gap plan — gpt-5\n\nPlan refreshed.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, cycle).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, cycle).unwrap();

        commit(&doc).expect("commit should keep prior response tail unprefixed");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let blob = String::from_utf8_lossy(&show.stdout);
        assert!(
            blob.contains(
                "\nThere were no actionable follow-up items to capture.\n❯ do [#tailpatch]. spec-test-build-install-commit-push\n"
            ),
            "assistant tail must stay bare while the real prompt is prefixed:\n{blob}"
        );
        assert!(
            !blob.contains("\n❯ There were no actionable follow-up items to capture.\n"),
            "assistant tail must not be rewritten as a prompt:\n{blob}"
        );
    }
    #[test]
    fn commit_blocks_out_of_band_exchange_and_pending_mutation() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#c3d4] new pending\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, file).unwrap();

        let err = commit(&doc).expect_err("typed pending mutations should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("direct response patchback without agent-doc cycle"),
            "error should explain the blocked bypassed patchback:\n{message}"
        );
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snap, snapshot, "snapshot must remain unchanged on failure");
    }
    #[test]
    fn commit_does_not_absorb_out_of_band_user_prompt() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, file).unwrap();

        commit(&doc).expect("commit should succeed even when there's nothing new to stage");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(
            !committed.contains("follow-up question"),
            "user prompt should remain uncommitted:\n{committed}"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !snap.contains("follow-up question"),
            "snapshot should stay at the older committed state:\n{snap}"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ follow-up question"),
            "working tree should retain the user prompt:\n{working}"
        );
    }
    #[test]
    fn commit_blocks_extreme_drift_resync_for_tracked_user_prompt() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let scaffold = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, scaffold).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            scaffold,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add scaffold", "--no-verify"])
            .output()
            .unwrap();

        let live = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ user question that still needs an answer\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, live).unwrap();

        commit(&doc).expect("commit should succeed without absorbing the prompt");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(
            !committed.contains("user question that still needs an answer"),
            "tracked extreme drift must not absorb unanswered prompt:\n{committed}"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !snap.contains("user question that still needs an answer"),
            "snapshot should remain selective for tracked docs:\n{snap}"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ user question that still needs an answer"),
            "working tree should retain the unanswered prompt:\n{working}"
        );
    }
    #[test]
    fn commit_resyncs_extreme_drift_for_untracked_scaffold_doc() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let scaffold = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, scaffold).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            scaffold,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let live = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            Ready\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: imported\n\
           body from moved file\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] imported\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, live).unwrap();

        commit(&doc).expect("commit should resync bootstrap scaffold snapshot");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(
            committed.contains("### Re: imported\n"),
            "bootstrap resync should stage the real file content:\n{committed}"
        );
        assert!(
            committed.contains("[#a1b2] imported"),
            "bootstrap resync should carry pending content too:\n{committed}"
        );
    }
    #[test]
    fn commit_blocks_out_of_band_status_and_exchange_mutation() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Older status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Newer status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, file).unwrap();

        let err = commit(&doc).expect_err("typed status mutations should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("direct response patchback without agent-doc cycle"),
            "error should explain the blocked bypassed patchback:\n{message}"
        );
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snap, snapshot, "snapshot must remain unchanged on failure");
    }
    #[test]
    fn commit_repairs_committed_historical_snapshot_drift() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let tracked = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, tracked).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            tracked,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let historical = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: historical\n\
            repaired body\n\
            #### #next-steps\n\
            Follow up.\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, historical).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual repair", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            tracked,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        commit(&doc).expect("commit should repair the stale snapshot");

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("### Re: historical\n"),
            "snapshot should repair to the committed historical response:\n{snap}"
        );
        assert!(
            snap.contains("#### #next-steps\n"),
            "h4 response sub-headings that look like prompt presets should not block repair:\n{snap}"
        );

        let committed = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            committed.contains("### Re: historical\n"),
            "committed blob should keep the historical response after repair:\n{committed}"
        );
    }
    #[test]
    fn commit_closes_cycle_when_staged_snapshot_already_matches_head() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let visible_snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            visible_snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let with_user_edit = format!("{visible_snapshot}\n❯ follow-up question\n");
        fs::write(&doc, &with_user_edit).unwrap();
        agent_doc_cycle_state_io::start_preflight(
            &doc,
            Some(visible_snapshot),
            Some(&with_user_edit),
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(visible_snapshot),
            Some(&with_user_edit),
            "sha256",
            None,
        )
        .unwrap();

        let did_commit = commit(&doc).expect("commit should treat HEAD-current snapshot as no-op");
        assert!(
            !did_commit,
            "HEAD-current closeout should not create a duplicate git commit"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let capture = agent_doc_capture_io::load_active(&doc).unwrap();
        assert!(
            capture.is_none(),
            "already-committed no-op closeout should clear active capture state"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_already_current file="),
            "ops log should record the dedicated no-op closeout:\n{log}"
        );
        assert!(
            !log.contains("commit_failed"),
            "already-committed no-op must not be logged as commit_failed:\n{log}"
        );
        assert!(
            log.contains("post_commit_local_drift file=")
                && log.contains("kind=working_tree_edits"),
            "out-of-component local edits should be classified as working-tree drift:\n{log}"
        );
    }

    #[test]
    fn commit_absorbs_exact_retained_pending_only_target_when_snapshot_matches_head() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#old]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#old] finish old work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=\"session.done.md\" -->\n",
            "<!-- completed work archived in session.done.md -->\n",
            "<!-- /agent:done -->\n"
        );
        commit_file(root, "session.md", committed, "add doc");
        commit_file(
            root,
            "session.done.md",
            "# Agent Doc Completed Work\n\n- old archive entry\n",
            "add done archive",
        );
        commit_file(root, "unrelated.md", "operator baseline\n", "add unrelated");
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let retained_target = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#next] follow-up work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=\"session.done.md\" -->\n",
            "<!-- completed work archived in session.done.md -->\n",
            "<!-- /agent:done -->\n"
        );
        fs::write(&doc, retained_target).unwrap();
        fs::write(
            root.join("session.done.md"),
            "# Agent Doc Completed Work\n\n- old archive entry\n- retained completion\n",
        )
        .unwrap();
        fs::write(root.join("unrelated.md"), "operator staged edit\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "unrelated.md"])
            .output()
            .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(retained_target))
            .unwrap();
        let target_hash = agent_doc_hash::content_hash(retained_target);
        agent_doc_cycle_state_io::record_pending_only_commit_target(&doc, &target_hash).unwrap();

        let did_commit = commit(&doc).expect("exact retained target should commit");
        assert!(did_commit);
        let committed_target = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            committed_target.contains("- [ ] [#next] follow-up work")
                && !committed_target.contains("- [ ] [#old] finish old work"),
            "committed blob should contain the retained tracked-work target:\n{committed_target}"
        );
        let committed_archive =
            agent_doc_git_io::revision::show_head(&root.join("session.done.md"))
                .unwrap()
                .unwrap();
        assert!(
            committed_archive.contains("- retained completion"),
            "the binary-owned archive must land in the same closeout commit:\n{committed_archive}"
        );
        let committed_paths = Command::new("git")
            .current_dir(root)
            .args(["show", "--pretty=format:", "--name-only", "HEAD"])
            .output()
            .unwrap();
        let committed_paths = String::from_utf8_lossy(&committed_paths.stdout);
        assert!(committed_paths.lines().any(|line| line == "session.md"));
        assert!(
            committed_paths
                .lines()
                .any(|line| line == "session.done.md")
        );
        assert!(
            !committed_paths.lines().any(|line| line == "unrelated.md"),
            "unrelated staged work must stay outside the private closeout commit:\n{committed_paths}"
        );
        let staged_unrelated = Command::new("git")
            .current_dir(root)
            .args(["diff", "--cached", "--", "unrelated.md"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&staged_unrelated.stdout).contains("operator staged edit"),
            "the operator's unrelated staged edit must remain staged"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=pending_only_exact_target"),
            "commit should record the exact retained-target authority:\n{log}"
        );
        assert!(
            log.contains("commit_binary_owned_side_effects") && log.contains("session.done.md"),
            "commit should record the typed archive side effect:\n{log}"
        );
    }

    #[test]
    fn commit_rejects_unproved_typed_component_drift_when_snapshot_matches_head() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#old]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#old] finish old work\n",
            "<!-- /agent:backlog -->\n"
        );
        commit_file(root, "session.md", committed, "add doc");
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let unproved = committed.replace(
            "- [ ] [#old] finish old work",
            "- [ ] [#old] silently changed work",
        );
        fs::write(&doc, unproved).unwrap();

        let err = commit(&doc).expect_err("unproved typed-component drift must fail closed");
        assert!(
            err.to_string()
                .contains("without an exact binary-owned retained-target proof"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn commit_blocks_head_current_noop_with_uncommitted_response_body_append() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: pending closeout\n",
            "<!-- /agent:exchange -->\n"
        );
        commit_file(root, "session.md", committed, "add doc");
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let live = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: pending closeout\n",
            "implemented the response body after the snapshot was already HEAD-current\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, live).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(live)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(committed),
            Some(live),
            "sha256",
            None,
        )
        .unwrap();

        let err = commit(&doc).expect_err("uncommitted response body drift must fail closed");
        let message = err.to_string();
        assert!(
            message.contains("response-bearing exchange edits that are not committed"),
            "error should explain the uncommitted response patchback:\n{message}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::ResponseCaptured);
        assert_eq!(state.last_event, "response_captured");

        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !head_doc.contains("implemented the response body"),
            "HEAD must not appear to contain the uncommitted response body:\n{head_doc}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_response_patchback_uncommitted file="),
            "ops log should record the failed-closed response patchback guard:\n{log}"
        );
        assert!(
            !log.contains("commit_already_current file="),
            "guard must fire before the already-current no-op marks the cycle committed:\n{log}"
        );
    }

    #[test]
    fn commit_blocks_head_current_noop_when_active_capture_response_missing() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please answer the prompt\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: missed patchback — gpt-5\n\n",
            "Recovered answer.\n",
            "<!-- /patch:exchange -->\n"
        );
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        assert!(!capture.capture_id.is_empty());

        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let err = commit(&doc)
            .expect_err("HEAD-current snapshot must not close a missing captured response");
        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        assert!(
            err.to_string()
                .contains("captured response body is not present"),
            "error should name the missing captured response body:\n{err}"
        );
        assert_eq!(
            String::from_utf8_lossy(&head_before.stdout),
            String::from_utf8_lossy(&head_after.stdout),
            "blocked no-op closeout must not advance HEAD"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::ResponseCaptured);

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !head.contains("Recovered answer."),
            "HEAD should remain prompt-only when response materialization is missing:\n{head}"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_missing_captured_response file="),
            "blocked missing materialization should be logged:\n{log}"
        );
        assert!(
            !log.contains("commit_already_current file="),
            "missing response materialization must not be recorded as already-current closeout:\n{log}"
        );
    }
    #[test]
    fn commit_blocks_stale_snapshot_commit_when_active_capture_response_missing() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please answer the prompt\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: stale sidecar — gpt-5\n\n",
            "Recovered answer that must not be lost.\n",
            "<!-- /patch:exchange -->\n"
        );
        agent_doc_capture_io::capture_response(&doc, response).unwrap();

        let stale_prompt_only = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please answer the prompt\n",
            "<!-- agent:boundary:head -->\n",
            "❯ Later user follow-up while the response is missing\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, stale_prompt_only).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            stale_prompt_only,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let err = commit(&doc)
            .expect_err("stale prompt-only snapshot must not commit over captured response");
        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        assert!(
            err.to_string()
                .contains("captured response body is not present"),
            "error should name the missing captured response body:\n{err}"
        );
        assert_eq!(
            String::from_utf8_lossy(&head_before.stdout),
            String::from_utf8_lossy(&head_after.stdout),
            "blocked stale snapshot commit must not advance HEAD"
        );
        assert!(
            !agent_doc_git_io::revision::show_head(&doc)
                .unwrap()
                .unwrap()
                .contains("Later user follow-up"),
            "stale prompt-only snapshot must not be committed"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_missing_captured_response file=")
                && log.contains("basis=staged"),
            "blocked staged commit should be logged with staged basis:\n{log}"
        );
    }
    #[test]
    fn commit_preserves_fresh_prompt_when_escaped_tail_cleanup_is_mixed() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n\n\
            do #oobtaildel. spec-test-build-install-commit-push\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let mixed = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ❯ fresh follow-up prompt\n\
            <!-- agent:boundary:live -->\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
        fs::write(&doc, mixed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let did_commit = commit(&doc).expect("mixed cleanup should close as no-op");
        assert!(
            !did_commit,
            "mixed cleanup plus prompt must not commit the fresh prompt"
        );

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            head, committed,
            "HEAD should remain unchanged when fresh prompt drift is present"
        );
        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ fresh follow-up prompt"),
            "fresh prompt must remain visible for the next cycle:\n{working}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_local_drift file=") && log.contains("kind=user_follow_up"),
            "mixed cleanup should be diagnosed as preserved user follow-up drift:\n{log}"
        );
        assert!(
            log.contains("post_commit_user_follow_up file="),
            "mixed cleanup should use the benign user-follow-up marker:\n{log}"
        );
        assert!(
            log.contains("commit_noop file=") && log.contains("drift_kind=user_follow_up"),
            "mixed cleanup noop should record the benign drift kind for ops summary:\n{log}"
        );
        assert!(
            !log.contains("prior_patchback_without_response_body file="),
            "fresh follow-up prompts must not be mislabeled as missing response-body repair:\n{log}"
        );
        assert!(
            !log.contains("out_of_band_write file="),
            "classified follow-up prompt drift must not be mislabeled as out-of-band write:\n{log}"
        );
        assert!(
            !log.contains("post_commit_escaped_tail_cleanup file="),
            "mixed cleanup must not be auto-adopted:\n{log}"
        );
    }
    #[test]
    fn commit_repairs_prompt_prefix_duplicate_drift_before_staging() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n"
        );
        commit_file(root, "session.md", head, "add doc");

        let prompt = "lucas-huang may not have the necessary packages to use the runbooks. Please add development dependencies so any programmer can use the runbooks.";
        let snapshot = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\n",
                "Done.\n",
                "{prompt}\n",
                "#spec-test-commit-push\n",
                "<!-- agent:boundary:edf37a04 -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let working = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\n",
                "Done.\n",
                "❯ {prompt}\n",
                "{prompt}\n",
                "#spec-test-commit-push\n",
                "<!-- agent:boundary:edf37a04 -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        fs::write(&doc, &working).unwrap();

        let did_commit = commit(&doc).expect("prompt duplicate drift should repair and commit");
        assert!(did_commit);

        let head_after = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_after.contains(&format!("❯ {prompt}\n#spec-test-commit-push")),
            "committed prompt should keep one normalized line:\n{head_after}"
        );
        assert!(
            !head_after.contains(&format!("❯ {prompt}\n{prompt}")),
            "duplicate prompt must not be committed:\n{head_after}"
        );
        let working_after = fs::read_to_string(&doc).unwrap();
        assert!(
            !working_after.contains(&format!("❯ {prompt}\n{prompt}")),
            "working tree must be repaired before closeout:\n{working_after}"
        );
        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !snapshot_after.contains(&format!("❯ {prompt}\n{prompt}")),
            "snapshot must be repaired before closeout:\n{snapshot_after}"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_pre_stage_prompt_duplicate_repaired file=")
                && log.contains("snapshot_updated=true"),
            "commit pre-stage prompt repair should be logged:\n{log}"
        );
        assert!(
            !log.contains("out_of_band_write file="),
            "repaired prefix duplicate drift must not be left as out-of-band drift:\n{log}"
        );
    }
    #[test]
    fn commit_repairs_committed_head_before_user_follow_up_noop() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let stale_snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:old -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, stale_snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            stale_snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let committed_head = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed_head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            stale_snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let working = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:live -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, working).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(stale_snapshot), Some(working))
            .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(stale_snapshot),
            Some(working),
            "sha256",
            None,
        )
        .unwrap();

        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let did_commit = commit(&doc).expect("commit should not rewind a stale snapshot");
        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        assert!(
            !did_commit,
            "repairing the snapshot up to committed HEAD should close as a no-op"
        );
        assert_eq!(
            String::from_utf8_lossy(&head_before.stdout),
            String::from_utf8_lossy(&head_after.stdout),
            "HEAD should stay on the already-committed response instead of creating a rewind commit"
        );

        let committed = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            committed.contains("### Re: newer\n"),
            "HEAD should keep the newer committed response:\n{committed}"
        );
        assert!(
            !committed.contains("❯ follow-up question"),
            "HEAD should not absorb the user's follow-up prompt:\n{committed}"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("### Re: newer\n"),
            "snapshot should repair up to the already-committed response:\n{snap}"
        );
        assert!(
            !snap.contains("❯ follow-up question"),
            "snapshot repair must stop at HEAD, not absorb the follow-up prompt:\n{snap}"
        );

        let working_after = fs::read_to_string(&doc).unwrap();
        assert!(
            working_after.contains("❯ follow-up question"),
            "working tree should keep the user's follow-up prompt uncommitted:\n{working_after}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_local_drift file=") && log.contains("kind=user_follow_up"),
            "follow-up noop closeout should classify post-commit local drift:\n{log}"
        );
        assert!(
            log.contains("post_commit_user_follow_up file="),
            "follow-up noop closeout should record the benign follow-up diagnostic:\n{log}"
        );
        assert!(
            log.contains("commit_noop file=") && log.contains("drift_kind=user_follow_up"),
            "follow-up noop closeout should record the benign drift kind for ops summary:\n{log}"
        );
        assert!(
            !log.contains("prior_patchback_without_response_body file="),
            "follow-up noop closeout must not reopen missed-response repair semantics:\n{log}"
        );
        assert!(
            !log.contains("out_of_band_write file="),
            "classified follow-up prompt drift must not be mislabeled as out-of-band write:\n{log}"
        );
    }
    #[test]
    fn commit_skips_terminal_user_follow_up_noop_closeout() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: previous\n\
            previous body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let committed_state = agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        let with_user_follow_up = format!(
            "{}❯ follow-up question\n",
            committed.replace("<!-- /agent:exchange -->\n", "")
        ) + "<!-- /agent:exchange -->\n";
        fs::write(&doc, &with_user_follow_up).unwrap();

        let did_commit =
            commit(&doc).expect("terminal user-follow-up drift should remain a prompt handoff");
        assert!(!did_commit, "no new commit should be created");

        let state_after = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state_after, committed_state,
            "terminal user follow-up drift must not rewrite committed cycle state"
        );

        let working_after = fs::read_to_string(&doc).unwrap();
        assert!(
            working_after.contains("❯ follow-up question"),
            "working tree should preserve the user's follow-up prompt:\n{working_after}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_user_follow_up file="),
            "prompt handoff should still be diagnosed:\n{log}"
        );
        assert!(
            log.contains("commit_prompt_handoff_noop file="),
            "prompt handoff should have a non-closeout noop marker:\n{log}"
        );
        assert!(
            !log.contains("commit_noop file=") && !log.contains("commit_already_current file="),
            "terminal prompt handoff must not emit closeout lifecycle noop markers:\n{log}"
        );
    }
    #[test]
    fn postcommit_worktree_check_logs_match_true_for_transient_only_drift() {
        // `#postcommit-ipc-worktree-corruption`: a clean closeout whose working
        // tree differs from HEAD only by the legitimate transient `(HEAD)` /
        // boundary markers must log match=true — the visible document is
        // structurally equal to HEAD, so this is NOT the corruption class.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic\n\
            response body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");

        // Working tree keeps the transient `(HEAD)` annotation + repositioned
        // boundary the user sees post-commit — stripped by the replay normalizer.
        let working = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic (HEAD)\n\
            response body\n\
            <!-- agent:boundary:abc123 -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, working).unwrap();

        agent_doc_git_io::post_commit_cleanup::emit_postcommit_worktree_check(
            &agent_doc_commit_io::POST_COMMIT_CLEANUP_EFFECTS,
            &doc,
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("postcommit_worktree_check file=") && log.contains("match=true"),
            "transient-only working-tree drift must log a worktree==HEAD proof (match=true):\n{log}"
        );
        assert!(
            !log.contains("match=false"),
            "transient-only drift must not be flagged as corruption:\n{log}"
        );
    }
    #[test]
    fn postcommit_worktree_preserves_carry_forward_superset() {
        // A concurrent user edit carried forward UNCOMMITTED makes the working tree a
        // superset of HEAD (every committed line present, plus a new line). HEAD
        // content is NOT lost, so #pcwc must preserve the tree, never clobber the
        // carried-forward edit.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic\n\
            response body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");

        // Superset: all of HEAD plus a new uncommitted user note after the boundary.
        let superset = format!("{head_doc}\na new uncommitted user note line\n");
        fs::write(&doc, &superset).unwrap();

        agent_doc_git_io::post_commit_cleanup::emit_postcommit_worktree_check(
            &agent_doc_commit_io::POST_COMMIT_CLEANUP_EFFECTS,
            &doc,
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("match=false"),
            "superset differs from HEAD:\n{log}"
        );
        assert!(
            !log.contains("postcommit_worktree_auto_reconciled"),
            "a carry-forward superset must NOT be auto-reconciled:\n{log}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            superset,
            "the carried-forward user edit must be preserved untouched"
        );
    }
    #[test]
    fn postcommit_worktree_match_does_not_flush_editor() {
        // When the working tree already equals HEAD there is no drift to clear, so
        // the post-commit check must NOT send a save_document (avoid persisting a
        // possibly-stale editor buffer over an already-correct disk).
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        let _listener = start_fake_listener(root);
        wait_for_listener(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic\n\
            response body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");
        // Working tree already equals HEAD (no edit).

        agent_doc_git_io::post_commit_cleanup::emit_postcommit_worktree_check(
            &agent_doc_commit_io::POST_COMMIT_CLEANUP_EFFECTS,
            &doc,
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("postcommit_editor_save_flushed"),
            "a clean match=true working tree must not flush the editor:\n{log}"
        );

        let _ = fs::remove_file(agent_doc_ipc_io::socket_path(root));
    }
    #[test]
    fn postcommit_worktree_preserves_when_content_lost_but_user_work_added() {
        // The tree dropped a committed line BUT also added a carry-forward signal (a
        // `#tag` directive = real next-cycle user work). The ambiguous case fails
        // safe toward PRESERVING the user edit rather than clobbering it to HEAD.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\n\
            ### Re: second\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");

        // `### Re: second` dropped (content loss) AND a new `#tag` directive added.
        let drifted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n\
            follow up on #newtask\n";
        fs::write(&doc, drifted).unwrap();

        agent_doc_git_io::post_commit_cleanup::emit_postcommit_worktree_check(
            &agent_doc_commit_io::POST_COMMIT_CLEANUP_EFFECTS,
            &doc,
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("postcommit_worktree_auto_reconciled"),
            "content loss WITH new user work must not be auto-reconciled:\n{log}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            drifted,
            "ambiguous drift with new user work must be preserved"
        );
    }

    #[test]
    fn postcommit_worktree_observe_only_never_reverts_lost_content() {
        // #realtimecutover: the legacy revert tower used to write HEAD back over a
        // working tree that LOST committed content (pure corruption, no new user
        // work). That revert is RIPPED OUT — the realtime replica owns disk
        // reconciliation — so the post-commit check now only LOGS match=false and
        // leaves the working tree exactly as-is (it must never clobber it).
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\n\
            ### Re: second\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");

        // Pure corruption: `### Re: second` dropped, NOTHING added (the exact shape
        // the old code auto-reconciled back to HEAD).
        let corrupted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        fs::write(&doc, corrupted).unwrap();

        agent_doc_git_io::post_commit_cleanup::emit_postcommit_worktree_check(
            &agent_doc_commit_io::POST_COMMIT_CLEANUP_EFFECTS,
            &doc,
        );

        // Observe-only: drift is logged, but NOTHING is reverted.
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("postcommit_worktree_check") && log.contains("match=false"),
            "drift must be logged for observability:\n{log}"
        );
        assert!(
            !log.contains("postcommit_worktree_auto_reconciled"),
            "the legacy revert must be gone — no auto-reconcile:\n{log}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            corrupted,
            "the working tree must be left untouched (realtime replica owns reconciliation)"
        );
    }

    #[test]
    fn postcommit_worktree_preserves_when_content_lost_but_queue_work_added() {
        // A markdown queue mirror is real next-cycle work too. If the editor
        // buffer lost committed content and gained a pinned queue item, the
        // post-commit check must preserve the buffer instead of reconciling it
        // back to HEAD and deleting the queue addition.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\n\
            ### Re: second\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:queue priority go -->\n\
            - do [#existing]\n\
            <!-- /agent:queue -->\n\
            <!-- agent:backlog priority queue -->\n\
            - [ ] [#existing] existing task\n\
            <!-- /agent:backlog -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");

        let drifted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:queue priority go -->\n\
            - do [#existing]\n\
            - :pushpin: do [#advance-review]\n\
            <!-- /agent:queue -->\n\
            <!-- agent:backlog priority queue -->\n\
            - [ ] [#existing] existing task\n\
            <!-- /agent:backlog -->\n\
            <!-- agent:boundary:abc123 -->\n";
        fs::write(&doc, drifted).unwrap();

        agent_doc_git_io::post_commit_cleanup::emit_postcommit_worktree_check(
            &agent_doc_commit_io::POST_COMMIT_CLEANUP_EFFECTS,
            &doc,
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("postcommit_worktree_auto_reconciled"),
            "content loss WITH pinned queue work must not be auto-reconciled:\n{log}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            drifted,
            "ambiguous drift with pinned queue work must be preserved"
        );
    }

    #[test]
    fn commit_already_current_commits_preserved_queue_additions_neutralized_by_replay() {
        // #editorbufwin P2: queue-only drift is neutralized by replay hashing, so
        // an already-current snapshot used to close as a no-op and leave the
        // operator's queue addition local. The preserved queue prompt must become
        // durable in a follow-up commit while staying live for the next drain.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\nqueue_active: true\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: prior\n\
            response body\n\
            <!-- agent:boundary:abc123 -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:queue priority go -->\n\
            - do [#existing]\n\
            <!-- /agent:queue -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: prior response");
        let doc = root.join("session.md");
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            head_doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head_doc), Some(head_doc)).unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc)
            .unwrap()
            .expect("cycle state should be present");

        let visible_with_queue_add = head_doc.replace(
            "- do [#existing]\n",
            "- do [#existing]\n- :pushpin: do [#advance-review]\n",
        );
        fs::write(&doc, &visible_with_queue_add).unwrap();

        let did_commit = commit(&doc).expect("queue-only drift should commit");
        assert!(
            did_commit,
            "queue-only preserved editor drift must create a follow-up commit"
        );

        let head_after = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_after.contains("- :pushpin: do [#advance-review]\n"),
            "HEAD must include the preserved queue addition:\n{head_after}"
        );
        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snapshot_after.contains("- :pushpin: do [#advance-review]\n"),
            "snapshot must make the queue addition durable for session-check:\n{snapshot_after}"
        );
        let working_after = fs::read_to_string(&doc).unwrap();
        assert!(
            working_after.contains("- :pushpin: do [#advance-review]\n"),
            "visible document must keep the queue addition live:\n{working_after}"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=preserved_queue_addition_replay_neutralized"),
            "commit should log the replay-neutralized queue recovery:\n{log}"
        );
        assert!(
            !log.contains("commit_already_current file="),
            "the queue addition must not be closed as an already-current no-op:\n{log}"
        );
    }

    #[test]
    fn commit_already_current_repairs_transient_working_tree_churn() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

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

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: newer\n\
            body\n\
            <!-- agent:boundary:head-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let transient = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: newer (HEAD)\n\
            body\n\
            <!-- agent:boundary:fresh-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, transient).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let did_commit = commit(&doc).expect("HEAD-current closeout should succeed");
        assert!(
            !did_commit,
            "transient-only churn should close as already committed"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working, committed,
            "working tree should be restored to clean HEAD when only transient churn differed"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snap, committed,
            "snapshot should also be restored to clean HEAD after transient cleanup"
        );

        assert!(
            !root.join(".agent-doc/patches/vcs-refresh.signal").exists(),
            "no-op closeout must not resurrect the removed filesystem VCS-refresh sidecar"
        );
    }
    #[test]
    fn commit_success_repairs_transient_working_tree_churn_after_real_commit() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

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

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let initial = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ Initial prompt\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, initial).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            initial,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let _listener = start_fake_listener(root);
        wait_for_listener(root);

        let committed = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ Initial prompt\n\
            ### Re: closeout follow-up — gpt-5\n\
            body\n\
            <!-- agent:boundary:committed-boundary -->\n\
            <!-- /agent:exchange -->\n";
        let transient = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ Initial prompt\n\
            ### Re: closeout follow-up — gpt-5 (HEAD)\n\
            body\n\
            <!-- agent:boundary:fresh-boundary -->\n\
            <!-- /agent:exchange -->\n";
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        fs::write(&doc, transient).unwrap();

        let did_commit = commit(&doc).expect("real closeout commit should succeed");
        assert!(did_commit, "snapshot should produce a real git commit");

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .expect("committed document should be readable from HEAD after commit");
        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working, head,
            "post-commit cleanup should restore the working tree to the committed HEAD blob"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snap, head,
            "snapshot should stay aligned with the committed HEAD blob"
        );

        let status = agent_doc_git_io::status::tracked_modified_paths(&doc).unwrap();
        assert!(
            status.is_empty(),
            "post-commit cleanup should leave no tracked worktree dirtiness for the document: {status:?}"
        );
    }
    #[test]
    fn commit_fails_closed_when_committed_historical_response_mutates_status() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let stale_snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Before.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, stale_snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            stale_snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #done. spec-test-commit-push\n",
            "### Re: do `#done` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual repair", "--no-verify"])
            .output()
            .unwrap();

        let working = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After. Tuned manually.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #done. spec-test-commit-push\n",
            "### Re: do `#done` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:live -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, working).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            stale_snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(stale_snapshot), Some(working))
            .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(stale_snapshot),
            Some(working),
            "sha256",
            None,
        )
        .unwrap();

        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let err =
            commit(&doc).expect_err("status-mutating historical patchback should fail closed");
        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        assert!(
            err.to_string()
                .contains("committed historical response patchback"),
            "error should explain the blocked historical patchback:\n{err}"
        );
        assert_eq!(
            String::from_utf8_lossy(&head_before.stdout),
            String::from_utf8_lossy(&head_after.stdout),
            "HEAD should stay on the already-committed response instead of creating a rewind commit"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snap, stale_snapshot,
            "snapshot must stay on the pre-repair baseline when the historical patchback is rejected"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::ResponseCaptured);
        assert_eq!(state.last_event, "response_captured");

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_committed_historical_patchback file="),
            "blocked historical patchback should be recorded in ops.log:\n{log}"
        );
        assert!(
            !log.contains("snapshot_repair file="),
            "rejected historical patchback must not rewrite the snapshot:\n{log}"
        );
    }
    #[test]
    fn commit_already_current_repairs_response_heading_attribution_drift() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

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

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic — gpt-5\n\
            body\n\
            <!-- agent:boundary:committed-id -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let drifted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic — codex (HEAD)\n\
            body\n\
            <!-- agent:boundary:stale-id -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, drifted).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let did_commit = commit(&doc).expect("heading attribution drift should self-heal");
        assert!(!did_commit, "repair should close as already committed");

        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working, committed,
            "working tree should be restored to the committed response heading and boundary"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snap, committed,
            "snapshot should also return to committed HEAD"
        );
    }
    #[test]
    fn commit_already_current_repairs_stale_agent_response_collapse_and_commits_queue_follow_up() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

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

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: #vbc1 next backlog — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> [#jblive160]\n\n",
            "Backlog complete.\n\n",
            "Proof:\n",
            "- Confidence: high.\n",
            "- Escalation: none.\n\n",
            "### Re: #queueeditloss — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> [#queueeditloss]\n\n",
            "Implemented queue fix.\n\n",
            "Proof:\n",
            "- Changed paths: `write.rs`.\n",
            "- Verification: `make check`.\n",
            "- Confidence: high.\n",
            "- Escalation: none.\n",
            "<!-- agent:boundary:head-boundary -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let drifted = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: #vbc1 next backlog — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> [#queueeditloss]\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> [#jblive160]\n\n",
            "Backlog complete.\n\n",
            "Proof:\n",
            "- Confidence: high.\n",
            "- Escalation: none.\n",
            "Implemented queue fix.\n\n",
            "Proof:\n",
            "- Verification: `make check`.\n",
            "- Changed paths: `write.rs`.\n",
            "- Confidence: high.\n",
            "- Escalation: none.\n",
            "<!-- agent:boundary:live-boundary -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do [#submitdiag] Add diagnostics for JB Run Agent Doc submit misses?\n",
            "<!-- /agent:queue -->\n"
        );
        fs::write(&doc, drifted).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let did_commit = commit(&doc).expect("stale response collapse should self-heal");
        assert!(
            did_commit,
            "repair should commit the preserved queue follow-up after cleaning the exchange"
        );

        let head_after = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_after.contains("### Re: #vbc1 next backlog"),
            "committed exchange response must be restored:\n{head_after}"
        );
        assert!(
            head_after.contains("### Re: #queueeditloss"),
            "second committed exchange response must be restored:\n{head_after}"
        );
        assert!(
            !head_after.contains("<!-- agent:boundary:live-boundary -->"),
            "stale live boundary must not survive in HEAD:\n{head_after}"
        );
        assert!(
            head_after.contains(
                "- do [#submitdiag] Add diagnostics for JB Run Agent Doc submit misses?\n"
            ),
            "preserved queue follow-up must be durable in HEAD:\n{head_after}"
        );
        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains(
                "- do [#submitdiag] Add diagnostics for JB Run Agent Doc submit misses?\n"
            ),
            "queue follow-up must remain visible:\n{working}"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains(
                "- do [#submitdiag] Add diagnostics for JB Run Agent Doc submit misses?\n"
            ),
            "snapshot must include the committed queue follow-up:\n{snap}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("stale_agent_response_collapse_cleanup file=")
                && log.contains("preserved_local_drift=true"),
            "repair should leave durable evidence that only the exchange collapse was cleaned:\n{log}"
        );
        assert!(
            log.contains("reason=preserved_queue_addition_replay_neutralized"),
            "queue follow-up commit should log the replay-neutralized recovery:\n{log}"
        );
    }
    #[test]
    fn commit_identifies_post_commit_local_working_tree_edits() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response\n\
            <!-- agent:boundary:head-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let working = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response plus later local edit\n\
            <!-- agent:boundary:live-boundary -->\n\
            <!-- /agent:exchange -->\n\n\
            <!-- later local note -->\n";
        fs::write(&doc, working).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let did_commit = commit(&doc).expect("HEAD-current local edits should close as no-op");
        assert!(
            !did_commit,
            "later local edits on top of HEAD must stay uncommitted"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let working_after = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working_after, working,
            "commit should not overwrite later local edits when HEAD is already current"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_local_drift file=")
                && log.contains("kind=working_tree_edits"),
            "working-tree edits should be classified as post-commit local drift:\n{log}"
        );
        assert!(
            log.contains("commit_noop file=") && log.contains("drift_kind=working_tree_edits"),
            "working-tree noop should record its anomalous drift kind for ops summary:\n{log}"
        );
        assert!(
            !log.contains("out_of_band_write file="),
            "classified post-commit local drift should not be mislabeled as out-of-band write:\n{log}"
        );
        assert!(
            !log.contains("drift_warning file="),
            "post-commit local drift should not be mislabeled as a generic out-of-band write:\n{log}"
        );
    }
    #[test]
    fn commit_fails_closed_when_reaped_backlog_ids_reappear_before_closeout() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let cleaned = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, cleaned).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            cleaned,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(cleaned), Some(cleaned)).unwrap();
        agent_doc_cycle_state_io::record_reaped_pending_ids(&doc, &["gone1".to_string()])
            .unwrap()
            .unwrap();

        let resurrected = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [/] [#gone1] Resurrected by stale editor state\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, resurrected).unwrap();

        let err = commit(&doc).expect_err("reintroduced reaped ids must fail closed");
        let message = err.to_string();
        assert!(message.contains("#gone1"), "unexpected error: {message}");
        assert!(
            message.contains("reappeared in the live file"),
            "unexpected error: {message}"
        );

        let head = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(head.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&head.stdout);
        assert!(
            !committed.contains("[#gone1]"),
            "HEAD must stay at the cleaned backlog state:\n{committed}"
        );
    }
    #[test]
    fn commit_blocks_bypassed_response_patchback_on_head_current() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response\n\
            <!-- agent:boundary:head-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let bypassed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response\n\
            \n\
            do #later. spec-test-build-install-commit-push\n\
            \n\
            ### Re: bypassed\n\
            landed outside agent-doc\n\
            <!-- agent:boundary:live-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, bypassed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(bypassed)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(committed),
            Some(bypassed),
            "sha256",
            None,
        )
        .unwrap();

        let err = commit(&doc).expect_err("bypassed response patchback should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("direct response patchback without agent-doc cycle"),
            "error should explain the bypassed patchback:\n{message}"
        );
        assert!(
            message.contains("### Re: bypassed"),
            "error should surface the offending heading:\n{message}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::ResponseCaptured);
        assert_eq!(state.last_event, "response_captured");

        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !head_doc.contains("### Re: bypassed"),
            "HEAD must stay on the last binary-owned patchback:\n{head_doc}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_bypassed_patchback file="),
            "ops log should record the blocked bypassed patchback:\n{log}"
        );
    }
    #[test]
    fn commit_blocks_committed_historical_patchback_that_mutates_status() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Before.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: state\n",
            "clean committed response\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: state\n",
            "clean committed response\n\n",
            "do #patchbypass. spec-test-build-install-commit-push\n",
            "### Re: #patchbypass — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(snapshot), Some(committed)).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(snapshot),
            Some(committed),
        )
        .unwrap();

        let err =
            commit(&doc).expect_err("status-mutating historical patchback should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("committed historical response patchback"),
            "error should explain the committed historical patchback:\n{message}"
        );
        assert!(
            message.contains("typed_component_drift")
                || message.contains("status+exchange")
                || message.contains("status"),
            "error should surface the out-of-band mutation kind:\n{message}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_committed_historical_patchback file="),
            "ops log should record the blocked historical patchback:\n{log}"
        );
    }
    // #compactdrift — a clean exchange-only compaction (responses archived, every
    // NON-exchange component preserved) must NOT trip the committed-historical
    // `typed_component_drift` / out-of-band patchback guard. HEAD legitimately still
    // holds the last finalized `### Re:` response(s); the post-compact snapshot/file
    // archived them. With no non-exchange drift this is the benign steady state, so
    // `commit` must adopt the compacted document instead of failing closed with
    // "refusing to auto-adopt committed historical response patchback".
    #[test]
    fn commit_allows_clean_exchange_only_compaction() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        // HEAD: pre-compact committed state with a finalized response carrying the
        // exact `### Re: do [#rtwbcast]` marker the live repro reported, plus a stable
        // status + backlog (non-exchange components).
        let pre_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "rtwbcast landed.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Earlier work.\n\n",
            "do #rtwbcast. spec-test-build-install-commit-push\n",
            "### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8\n\n",
            "Implemented the broadcast rung.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#follow] keep an eye on convergence\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, pre_compact).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            pre_compact,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "finalized rtwbcast", "--no-verify"])
            .output()
            .unwrap();

        // Post-compact document: exchange archived to a Session Summary, status +
        // backlog preserved exactly. Snapshot + working tree both hold this (the
        // normal post-archival state after compact refreshes the snapshot).
        let post_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "rtwbcast landed.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Archived 2 response topic(s) to .agent-doc/archives/session-20260613.md\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#follow] keep an eye on convergence\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, post_compact).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            post_compact,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        commit(&doc).expect("clean exchange-only compaction must not fail closed");

        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_doc.contains("### Session Summary"),
            "HEAD should hold the compacted document after commit:\n{head_doc}"
        );
        assert!(
            !head_doc.contains("### Re: do [#rtwbcast]"),
            "the archived response must not remain in HEAD after compaction:\n{head_doc}"
        );
    }

    // `#jb-compact-commit-historical-patchback-guard`: the live "Compact Exchange
    // left the summary uncommitted" defect. When a compaction's authoritative
    // (post-compact) snapshot diverges from a pre-compact HEAD in a NON-exchange
    // component too (e.g. a concurrent queue/status reconciliation), the commit
    // classifies HEAD's `### Re:` turns as `typed_component_drift` committed
    // historical patchback and fails closed — so `agent-doc compact --commit`
    // silently leaves HEAD pre-compact. The compaction-aware commit entry
    // (`commit_with_authoritative_compaction`) must adopt the compacted document:
    // the dropped turns were archived first and the caller verifies HEAD landed.
    // Plain `commit` must STILL block (the guard is intact for non-compaction).
    #[test]
    fn authoritative_compaction_commits_past_historical_patchback_guard() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let doc = root.join("session.md");
        // Post-compact authoritative state: exchange archived to a Session Summary
        // AND a non-exchange status change (the divergence that trips
        // `typed_component_drift`). Snapshot + working tree both hold it.
        let post_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "compacted.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Archived 2 response topic(s) to .agent-doc/archives/session-20260712.md\n",
            "<!-- /agent:exchange -->\n",
        );
        // Pre-compact HEAD: the two `### Re:` turns still present, and a DIFFERENT
        // status (non-exchange drift) so the committed-historical guard classifies
        // this as `typed_component_drift`, not a clean exchange-only compaction.
        let pre_compact_head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "before compaction.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — gpt-5\n\n",
            "First answer.\n\n",
            "### Re: second — opus-4-8\n\n",
            "Second answer.\n",
            "<!-- /agent:exchange -->\n",
        );
        commit_file(root, "session.md", pre_compact_head, "pre-compact HEAD");
        // `commit_compacted_authoritative` re-asserts the authoritative compacted
        // SNAPSHOT before committing, while the working-tree file still lags
        // pre-compact (the editor-IPC-async window `#jb-compact-commit-editor-ipc-async`):
        // snapshot = compacted, disk = pre-compact. This is the split that makes
        // `snapshot_matches_current_file` false and trips the historical-patchback
        // guard.
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            post_compact,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        fs::write(&doc, pre_compact_head).unwrap();

        // Plain commit must still fail closed — the guard is intact for
        // non-compaction callers.
        let err =
            commit(&doc).expect_err("plain commit must still block the historical patchback drift");
        assert!(
            err.to_string()
                .contains("committed historical response patchback"),
            "plain commit should block with the historical patchback error:\n{err}"
        );

        // The compaction-aware entry adopts the compacted document.
        commit_with_authoritative_compaction(&doc)
            .expect("authoritative compaction must commit past the guard");
        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_doc.contains("### Session Summary"),
            "HEAD must hold the compacted document after the compaction commit:\n{head_doc}"
        );
        assert!(
            !head_doc.contains("### Re: first") && !head_doc.contains("### Re: second"),
            "the archived turns must not remain in HEAD after compaction:\n{head_doc}"
        );
    }

    // #jb-compact-commit-left-uncommitted: a CLEAN exchange-only compaction (no
    // non-exchange component divergence) does NOT trip the `typed_component_drift`
    // patchback guard covered by the test above. Instead
    // `repair_committed_historical_snapshot_drift` classifies the compacted
    // snapshot as stale exchange drift and reverts it back to the pre-compact HEAD,
    // so the commit no-ops ("staged snapshot already matches HEAD") and the
    // compaction closeout's `verify_compact_head_landed` reports uncommitted
    // compaction drift — the live `sampleportal.md` incident. The
    // authoritative-compaction scope must suppress that repair so the compacted
    // snapshot lands in HEAD.
    #[test]
    fn authoritative_compaction_commits_past_historical_snapshot_repair() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let doc = root.join("session.md");
        // Clean exchange-only compaction: only the Exchange component changes
        // (the archived `### Re:` turns collapse to a Session Summary). No status
        // divergence, so this exercises the historical-snapshot REPAIR path, not
        // the typed-component-drift guard.
        let post_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Archived 2 response topic(s) to .agent-doc/archives/session-20260712.md\n",
            "<!-- /agent:exchange -->\n",
        );
        let pre_compact_head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — gpt-5\n\n",
            "First answer.\n\n",
            "### Re: second — opus-4-8\n\n",
            "Second answer.\n",
            "<!-- /agent:exchange -->\n",
        );
        commit_file(root, "session.md", pre_compact_head, "pre-compact HEAD");
        // snapshot = compacted (authoritative, re-asserted by the compaction
        // closeout inside the Git-native commit transaction); working tree / realtime
        // resolution = pre-compact (editor-IPC-async lag or a frozen reliable-sync
        // canonical). This is the split that, without the scope guard, lets the
        // historical-snapshot repair revert the compacted snapshot to HEAD.
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            post_compact,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        fs::write(&doc, pre_compact_head).unwrap();

        commit_with_authoritative_compaction(&doc)
            .expect("authoritative compaction must commit the compacted snapshot");
        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_doc.contains("### Session Summary"),
            "HEAD must hold the compacted document after the compaction commit:\n{head_doc}"
        );
        assert!(
            !head_doc.contains("### Re: first") && !head_doc.contains("### Re: second"),
            "archived turns must not remain in HEAD after compaction:\n{head_doc}"
        );
    }

    #[test]
    fn repair_historical_snapshot_drift_accepts_committed_capture_with_queue_mutation() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let doc = root.join("session.md");
        let stale_snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- [ ] [#old] old work\n",
            "<!-- /agent:queue -->\n",
        );
        commit_file(root, "session.md", stale_snapshot, "initial session");
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            stale_snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: go-mode backlog - gpt-5\n\n",
            "The response is already committed.\n",
            "<!-- /patch:exchange -->\n",
        );
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\n",
            "Done.\n",
            "### Re: go-mode backlog - gpt-5 (HEAD)\n\n",
            "The response is already committed.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- [x] [#old] old work\n",
            "- [ ] [#next] next work\n",
            "<!-- /agent:queue -->\n",
        );
        commit_file(
            root,
            "session.md",
            head,
            "committed response with queue mutation",
        );

        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            stale_snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(stale_snapshot),
            Some(head),
        )
        .unwrap();
        agent_doc_capture_io::mark_committed(&doc).unwrap();

        let repaired = agent_doc_repair_io::repair_committed_historical_snapshot_drift(&doc)
            .expect("committed capture proof should repair stale snapshot");

        assert_eq!(repaired, Some("committed_capture"));
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc).unwrap(),
            Some(head.to_string())
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("snapshot_repair file=")
                && log.contains("reason=committed_capture")
                && log.contains("basis=head"),
            "repair should be audited as committed-capture snapshot refresh:\n{log}"
        );
    }

    #[test]
    fn commit_allows_clean_exchange_only_compaction_with_head_marker_worktree() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let pre_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "stable status.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older - gpt-5\n\n",
            "Earlier work.\n\n",
            "do #compactdrift. spec-test-build-install-commit-push\n",
            "### Re: #compactdrift-agent - gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do [#compactdrift-agent]\n",
            "- do [#next]\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, pre_compact).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            pre_compact,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "finalized compactdrift", "--no-verify"])
            .output()
            .unwrap();

        let post_compact_snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "stable status.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Archived compactdrift responses.\n\n",
            "### Re: #compactdrift-agent - gpt-5\n\n",
            "Verified compact drift.\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do [#next]\n",
            "<!-- /agent:queue -->\n",
        );
        let post_compact_worktree = post_compact_snapshot.replace(
            "### Re: #compactdrift-agent - gpt-5",
            "### Re: #compactdrift-agent - gpt-5 (HEAD)",
        );
        fs::write(&doc, &post_compact_worktree).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            post_compact_snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let result = commit(&doc);
        assert!(
            result.is_ok(),
            "transient (HEAD) marker drift must not trip the committed-historical guard: {:?}",
            result.err().map(|e| e.to_string())
        );

        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_doc.contains("### Session Summary"),
            "HEAD should hold the compacted document after commit:\n{head_doc}"
        );
        assert!(
            !head_doc.contains("do #compactdrift. spec-test-build-install-commit-push"),
            "the archived historical response prompt must not remain in HEAD:\n{head_doc}"
        );
    }
    // #compactdrift — the recovery shape: compact archived the exchange and refreshed
    // the working tree, but the snapshot was left STALE at the pre-compact size (the
    // reported "snapshot stale at pre-compact size vs the compacted visible file").
    // With no concurrent wedged write and no non-exchange drift, `agent-doc commit`
    // recovery must adopt the compacted file rather than fail closed on the historical
    // `### Re:` marker.
    #[test]
    fn commit_recovers_stale_pre_compact_snapshot_without_wedge() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let pre_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "rtwbcast landed.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Earlier work.\n\n",
            "do #rtwbcast. spec-test-build-install-commit-push\n",
            "### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8\n\n",
            "Implemented the broadcast rung.\n",
            "<!-- /agent:exchange -->\n",
        );
        // HEAD is still the pre-compact committed state (compact's own commit failed).
        fs::write(&doc, pre_compact).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            pre_compact,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "finalized rtwbcast", "--no-verify"])
            .output()
            .unwrap();

        // Working tree = compacted; snapshot left STALE at the pre-compact bytes.
        let post_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "rtwbcast landed.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Archived 2 response topic(s) to .agent-doc/archives/session-20260613.md\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, post_compact).unwrap();
        // snapshot intentionally NOT refreshed — still pre_compact.

        let result = commit(&doc);
        assert!(
            result.is_ok(),
            "stale-pre-compact-snapshot recovery must not fail closed: {:?}",
            result.err().map(|e| e.to_string())
        );
    }
    #[test]
    fn commit_in_submodule_with_symlinked_absolute_path() {
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();
        let link_dir = tempfile::TempDir::new().unwrap();
        let link_path = link_dir.path().join("workspace");

        // Create symlink: workspace -> outer
        std::os::unix::fs::symlink(outer, &link_path).unwrap();

        // Initialize a "submodule" origin repo
        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        // Initialize the outer repo (via real path, as git would)
        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        // Add submodule
        let sub_url = format!("file://{}", sub_origin.display());
        let sub_status = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ])
            .output()
            .unwrap();
        assert!(
            sub_status.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_status.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

        let submodule_path = outer.join("src/sub");
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        // Create and track the document inside the submodule
        let doc_real = submodule_path.join("session.md");
        let content =
            "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## User\n\n";
        fs::write(&doc_real, content).unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Modify the file and create snapshot
        let new_content = "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## Assistant\n\nupdated\n\n## User\n\n";
        fs::write(&doc_real, new_content).unwrap();
        let project_root =
            agent_doc_project_root_io::project_root_containing(&doc_real.canonicalize().unwrap())
                .unwrap_or_else(|| outer.to_path_buf());
        let snap_rel = agent_doc_fs::snapshot_path_for(&doc_real).unwrap();
        let snap_abs = project_root.join(&snap_rel);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, new_content).unwrap();

        // Access the file via the SYMLINK path — this is the bug scenario
        let doc_via_symlink = link_path.join("src/sub/session.md");
        assert!(doc_via_symlink.exists(), "symlinked path should exist");

        // commit() should succeed even with the symlinked absolute path
        let result = commit(&doc_via_symlink);
        assert!(
            result.is_ok(),
            "commit should succeed for submodule file accessed via symlink: {:?}",
            result.err()
        );

        // Verify the submodule has the agent-doc commit
        let sub_log = Command::new("git")
            .current_dir(&submodule_path)
            .args(["log", "--oneline", "-5"])
            .output()
            .unwrap();
        let sub_log_str = String::from_utf8_lossy(&sub_log.stdout);
        assert!(
            sub_log_str.contains("agent-doc(session)"),
            "submodule git log should contain agent-doc commit, got:\n{sub_log_str}"
        );
    }
    #[test]
    fn is_stale_baseline_write_path_replace_edits_ignored() {
        // Write path: user edited a replace-mode component in the baseline.
        // Only append-mode components are checked. Replace edits are fine.
        let snapshot = "<!-- agent:status patch=replace -->\nOriginal\n<!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:status patch=replace -->\nUser changed\n<!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\nResponse.\nUser question\n<!-- /agent:exchange -->\n";
        assert!(
            !agent_doc_template::stale_baseline::is_stale_baseline(baseline, snapshot),
            "user edits in replace + append components should NOT be stale"
        );
    }
    #[test]
    fn reposition_ignores_legacy_socket_listener_as_editor_authority() {
        use std::fs;
        use std::thread;
        use std::time::Duration;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
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

        let doc_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: test — opus-4-6 (HEAD)\nResponse.\n\
            <!-- agent:boundary:oldid123 -->\n\
            later prompt\n\
            <!-- /agent:exchange -->\n";
        let doc = root.join("plan.md");
        fs::write(&doc, doc_content).unwrap();

        // Create snapshot
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            doc_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // Initial commit
        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // Start a live IPC listener to simulate an active editor plugin.
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let root_clone = root.to_path_buf();
        let server = thread::spawn(move || {
            agent_doc_ipc_io::start_listener(&root_clone, |_msg| {
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });
        thread::sleep(Duration::from_millis(100));

        // A legacy control socket is not live-document authority. Lazily editor
        // attachment is the only reason to defer the visible projection.
        let changed = agent_doc_git_io::boundary_reposition::reposition_boundary_in_snapshot(
            &agent_doc_commit_io::BOUNDARY_REPOSITION_EFFECTS,
            &doc,
        );

        // Snapshot should be repositioned
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snap.matches("<!-- agent:boundary:oldid123 -->").count(),
            1,
            "snapshot reposition should preserve the projected boundary identity"
        );
        assert!(
            snap.contains(
                "later prompt\n<!-- agent:boundary:oldid123 -->\n<!-- /agent:exchange -->"
            )
        );
        assert!(
            snap.contains("### Re: test — opus-4-6\n"),
            "snapshot should be normalized to the clean heading"
        );
        assert_eq!(
            snap.matches("(HEAD)").count(),
            0,
            "snapshot should not retain transient head markers"
        );

        // Working tree is normalized because this test has no Lazily editor.
        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working.matches("<!-- agent:boundary:oldid123 -->").count(),
            1,
            "legacy socket listener must not replace the projected boundary identity"
        );
        assert!(
            working.contains(
                "later prompt\n<!-- agent:boundary:oldid123 -->\n<!-- /agent:exchange -->"
            )
        );
        assert!(
            working.contains("### Re: test — opus-4-6 (HEAD)\n"),
            "boundary reposition should preserve the response heading"
        );
        assert_eq!(
            working.matches("(HEAD)").count(),
            1,
            "boundary reposition alone does not own head-marker normalization"
        );

        assert!(changed, "snapshot change should report changed=true");

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(root));
        drop(server);
    }
    #[test]
    fn reposition_ignores_legacy_patches_directory_as_editor_authority() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
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

        let doc_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: test — opus-4-6 (HEAD)\nResponse.\n\
            <!-- agent:boundary:oldid456 -->\n\
            later prompt\n\
            <!-- /agent:exchange -->\n";
        let doc = root.join("plan.md");
        fs::write(&doc, doc_content).unwrap();

        // Create snapshot
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            doc_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // Initial commit
        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // A leftover patches directory is not an authority signal and must not
        // resurrect the retired file-watch hot path.
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

        // Run reposition
        agent_doc_git_io::boundary_reposition::reposition_boundary_in_snapshot(
            &agent_doc_commit_io::BOUNDARY_REPOSITION_EFFECTS,
            &doc,
        );

        // Snapshot is repositioned for commit staging.
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snap.matches("<!-- agent:boundary:oldid456 -->").count(),
            1,
            "snapshot reposition should preserve the projected boundary identity"
        );
        assert!(
            snap.contains(
                "later prompt\n<!-- agent:boundary:oldid456 -->\n<!-- /agent:exchange -->"
            )
        );
        assert!(
            snap.contains("### Re: test — opus-4-6\n"),
            "snapshot should be normalized to the clean heading"
        );
        assert_eq!(
            snap.matches("(HEAD)").count(),
            0,
            "snapshot should not retain transient head markers"
        );

        // With no Lazily editor attached, the working tree is the detached
        // projection and is normalized directly.
        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working.matches("<!-- agent:boundary:oldid456 -->").count(),
            1,
            "legacy patches directory must not replace the projected boundary identity"
        );
        assert!(
            working.contains(
                "later prompt\n<!-- agent:boundary:oldid456 -->\n<!-- /agent:exchange -->"
            )
        );
        assert!(
            working.contains("### Re: test — opus-4-6 (HEAD)\n"),
            "boundary reposition should preserve the response heading; got:\n{working}"
        );
        assert_eq!(
            working.matches("(HEAD)").count(),
            1,
            "boundary reposition alone does not own head-marker normalization; got:\n{working}"
        );

        let patch_file = root.join(".agent-doc/patches").join(format!(
            "{}.json",
            agent_doc_fs::document_state_hash(&doc).unwrap()
        ));
        assert!(
            !patch_file.exists(),
            "reposition must not recreate the retired file IPC sidecar"
        );
    }
    #[test]
    fn reposition_updates_working_tree_when_no_editor_ipc_available() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
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

        let doc_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: test — opus-4-6 (HEAD)\nResponse.\n\
            <!-- agent:boundary:oldid789 -->\n\
            later prompt\n\
            <!-- /agent:exchange -->\n";
        let doc = root.join("plan.md");
        fs::write(&doc, doc_content).unwrap();

        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            doc_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_git_io::boundary_reposition::reposition_boundary_in_snapshot(
            &agent_doc_commit_io::BOUNDARY_REPOSITION_EFFECTS,
            &doc,
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working.matches("<!-- agent:boundary:oldid789 -->").count(),
            1,
            "direct reposition should preserve the projected boundary identity"
        );
        assert!(
            working.contains(
                "later prompt\n<!-- agent:boundary:oldid789 -->\n<!-- /agent:exchange -->"
            )
        );
        assert!(
            working.contains("### Re: test — opus-4-6 (HEAD)"),
            "direct fallback must preserve (HEAD) annotations; got:\n{working}"
        );
    }
    #[test]
    fn reposition_repairs_missing_working_tree_prompt_prefix_without_listener() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
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

        let snapshot_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ do #spfxnorm. spec-test-build-install-commit-push\n\
            ### Re: #spfxnorm — opus-4-6\n\
            Implemented.\n\
            <!-- agent:boundary:clean789 -->\n\
            <!-- /agent:exchange -->\n";
        let working_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            do #spfxnorm. spec-test-build-install-commit-push\n\
            ### Re: #spfxnorm — opus-4-6 (HEAD)\n\
            Implemented.\n\
            <!-- agent:boundary:dirty789 -->\n\
            <!-- /agent:exchange -->\n";
        let doc = root.join("plan.md");
        fs::write(&doc, working_content).unwrap();

        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_git_io::boundary_reposition::reposition_boundary_in_snapshot(
            &agent_doc_commit_io::BOUNDARY_REPOSITION_EFFECTS,
            &doc,
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ do #spfxnorm. spec-test-build-install-commit-push"),
            "working tree should regain the missing prompt prefix:\n{working}"
        );
        assert!(
            !working.contains("<!-- agent:boundary:dirty789 -->"),
            "working tree boundary should also be repositioned:\n{working}"
        );
    }
}
