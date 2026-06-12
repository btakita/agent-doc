use super::*;

// ----- #nm1x: scope-aware finalize-path drift gate -----

fn drift_gate_doc(queue_items: &str, backlog_items: &str) -> String {
    format!(
        "---\nagent_doc_format: template\n---\n\n\
             <!-- agent:exchange patch=append -->\n### Re: x\n<!-- /agent:exchange -->\n\n\
             <!-- agent:queue -->\n{queue_items}<!-- /agent:queue -->\n\n\
             <!-- agent:backlog -->\n{backlog_items}<!-- /agent:backlog -->\n"
    )
}

fn drift_gate_scope(content: &str, driver_id: &str) -> agent_doc_core::turn_scope::TurnScope {
    let nodes = agent_doc_markdown_ast::mutations::all_item_nodes(content);
    let node = nodes
        .iter()
        .find(|node| node.component == "queue" && node.item.id == driver_id)
        .expect("driver queue node present");
    let driver =
        agent_doc_core::turn_scope::Address::from_component_node_key("queue", &node.node_key);
    agent_doc_core::turn_scope::TurnScope::for_driver(Some(driver))
}

#[test]
fn scoped_drift_gate_ignores_independent_sibling_queue_insert() {
    // The motivating bug: a queue item inserted *beside* the running one is
    // Independent — it must integrate + persist without blocking finalize.
    let snapshot = drift_gate_doc("- do [#driver]\n", "- [ ] [#b1] task\n");
    let file = drift_gate_doc("- do [#driver]\n- do [#sibling]\n", "- [ ] [#b1] task\n");
    let scope = drift_gate_scope(&file, "driver");

    assert!(
        !has_non_exchange_component_drift_scoped(&snapshot, &file, Some(&scope)),
        "an independent sibling queue insert must not block finalize"
    );
    // Without a scope the coarse gate still blocks (the historical behavior).
    assert!(has_non_exchange_component_drift(&snapshot, &file));
}

#[test]
fn scoped_drift_gate_still_blocks_driver_edit() {
    // Editing the queue item the turn is answering is Input-affecting — it
    // must still gate the turn.
    let snapshot = drift_gate_doc("- do [#driver]\n", "- [ ] [#b1] task\n");
    let file = drift_gate_doc("- do [#driver] reworded\n", "- [ ] [#b1] task\n");
    let scope = drift_gate_scope(&file, "driver");

    assert!(has_non_exchange_component_drift_scoped(
        &snapshot,
        &file,
        Some(&scope)
    ));
}

#[test]
fn scoped_drift_gate_still_blocks_backlog_contention() {
    // The backlog is in the turn's write set, so a concurrent backlog change
    // is Output-contended and must still block the narrow absorb path.
    let snapshot = drift_gate_doc("- do [#driver]\n", "- [ ] [#b1] task\n");
    let file = drift_gate_doc("- do [#driver]\n", "- [ ] [#b1] task changed\n");
    let scope = drift_gate_scope(&file, "driver");

    assert!(has_non_exchange_component_drift_scoped(
        &snapshot,
        &file,
        Some(&scope)
    ));
}

#[test]
fn scoped_drift_gate_blocks_when_node_differ_cannot_explain_change() {
    // A non-exchange content change with no item-node explanation (component
    // count mismatch / prose churn) stays conservative and blocks even with a
    // scope present.
    let snapshot = drift_gate_doc("- do [#driver]\n", "- [ ] [#b1] task\n");
    // Drop the backlog component entirely → component count mismatch.
    let file = format!(
        "---\nagent_doc_format: template\n---\n\n\
             <!-- agent:exchange patch=append -->\n### Re: x\n<!-- /agent:exchange -->\n\n\
             <!-- agent:queue -->\n- do [#driver]\n<!-- /agent:queue -->\n"
    );
    let scope = drift_gate_scope(&snapshot, "driver");

    assert!(has_non_exchange_component_drift_scoped(
        &snapshot,
        &file,
        Some(&scope)
    ));
}

#[test]
fn normalize_for_replay_hash_neutralizes_queue_churn() {
    // #adoc-queue-ipc-buffer-divergence root cause #4: queue-maintenance
    // churn (auto strip + activation toggle + drain) must not change the
    // replay-hash normalization, because the response body lives in
    // `exchange`, not `queue`.
    let with_active_queue = concat!(
        "---\nagent_doc_format: template\nqueue_active: true\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: topic — gpt-5\nResponse body.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "preset #spec-test\n- do [#a]\n",
        "<!-- /agent:queue -->\n"
    );
    // Same response; queue halted/drained (the post-maintenance shape).
    let with_drained_queue = concat!(
        "---\nagent_doc_format: template\nqueue_active: false\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: topic — gpt-5\nResponse body.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n"
    );
    assert_eq!(
        normalize_for_replay_hash(with_active_queue),
        normalize_for_replay_hash(with_drained_queue),
        "queue-only churn must not change the replay normalization"
    );

    // A genuine response-body change still registers as different.
    let with_changed_response = with_active_queue.replace("Response body.", "Different body.");
    assert_ne!(
        normalize_for_replay_hash(with_active_queue),
        normalize_for_replay_hash(&with_changed_response),
        "a real response-body change must still change the replay normalization"
    );
}

fn init_repo(repo: &Path) {
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

fn commit_file(repo: &Path, rel: &str, content: &str, msg: &str) {
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

fn add_submodule(repo: &Path, origin: &Path, target: &str, msg: &str) {
    let url = format!("file://{}", origin.display());
    let output = Command::new("git")
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
        output.status.success(),
        "submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Command::new("git")
        .current_dir(repo)
        .args(["commit", "-m", msg, "--no-verify"])
        .output()
        .unwrap();
}

#[test]
fn strip_head_markers_from_headings() {
    let input = "# Title\n### Re: Foo (HEAD)\nSome text with (HEAD) in it\n### Re: Bar (HEAD)\n";
    let result = strip_head_markers(input);
    assert_eq!(
        result,
        "# Title\n### Re: Foo\nSome text with (HEAD) in it\n### Re: Bar\n"
    );
}

#[test]
fn strip_head_markers_preserves_non_heading_lines() {
    let input = "Normal line (HEAD)\n### Heading (HEAD)\n";
    let result = strip_head_markers(input);
    assert_eq!(result, "Normal line (HEAD)\n### Heading\n");
}

#[test]
fn strip_head_markers_bold_text() {
    let input = "**Re: Something** (HEAD)\nSome text.\n";
    let result = strip_head_markers(input);
    assert_eq!(result, "**Re: Something**\nSome text.\n");
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
    crate::snapshot::save(&doc, duplicated).unwrap();

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

    let head = show_head(&doc).unwrap().unwrap();
    let snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(initial), Some(initial)).unwrap();
    crate::cycle_state::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

    let did_commit = commit(&doc).expect("commit should stage content_ours snapshot");

    assert!(did_commit);
    let head = show_head(&doc).unwrap().unwrap();
    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
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
fn strip_head_markers_ignores_fenced_code_hash() {
    // strip_head_markers should not remove content inside fenced code blocks.
    // If somehow `# comment (HEAD)` ended up in a fence, it should be left alone.
    let input = "### Re: Answer (HEAD)\nResponse.\n```bash\n# comment (HEAD)\n```\n";
    let result = strip_head_markers(input);
    assert_eq!(
        result, "### Re: Answer\nResponse.\n```bash\n# comment (HEAD)\n```\n",
        "fenced (HEAD) must be preserved, got:\n{result}"
    );
}

#[test]
fn strip_guard_markers_removes_standalone_lines() {
    let input = "### Re: topic\nResponse text.\n<!-- no-pending-capture -->\nMore text.\n<!-- no-pending-done-guard -->\nEnd.\n";
    let result = strip_guard_markers(input);
    assert_eq!(
        result, "### Re: topic\nResponse text.\nMore text.\nEnd.\n",
        "standalone guard markers should be removed:\n{result}"
    );
}

#[test]
fn strip_guard_markers_strips_inline_content() {
    let input = "Text with <!-- no-pending-capture --> inline.\nNormal line.\n";
    let result = strip_guard_markers(input);
    assert_eq!(
        result, "Text with  inline.\nNormal line.\n",
        "inline guard markers should be stripped:\n{result}"
    );
}

#[test]
fn strip_guard_markers_strips_trailing_on_content_line() {
    let input = "**All 39 variable products now have defaults set.** <!-- no-pending-capture -->\nNext line.\n";
    let result = strip_guard_markers(input);
    assert_eq!(
        result, "**All 39 variable products now have defaults set.**\nNext line.\n",
        "trailing guard marker should be stripped with trailing whitespace trimmed:\n{result}"
    );
}

#[test]
fn reposition_boundary_to_end_basic() {
    let content = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:abc123 -->\nUser prompt.\n<!-- /agent:exchange -->\n";
    let result = crate::template::reposition_boundary_to_end(content);
    // Boundary should be after user prompt, before close tag
    assert!(result.contains("User prompt.\n<!-- agent:boundary:"));
    assert!(result.contains("-->\n<!-- /agent:exchange -->"));
    // Old boundary consumed
    assert!(!result.contains("abc123"));
}

#[test]
fn reposition_boundary_no_exchange() {
    let content = "# No exchange component\nJust text.\n";
    let result = crate::template::reposition_boundary_to_end(content);
    // Should return unchanged if no exchange
    assert_eq!(result.trim(), content.trim());
}

#[test]
fn reposition_boundary_preserves_user_edits() {
    let content = "<!-- agent:exchange patch=append -->\n### Re: Answer\nAgent response.\n<!-- agent:boundary:old-id -->\nUser's new prompt here.\nMore user text.\n<!-- /agent:exchange -->\n";
    let result = crate::template::reposition_boundary_to_end(content);
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
    let result = crate::template::reposition_boundary_to_end(content);
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

// --- Bug 2B regression tests ---
// Verify that commit does NOT overwrite the snapshot with user edits.
// The divergence detection was removed from commit because is_stale_baseline
// cannot distinguish "file has user edits" from "file has a missed agent response" —
// both look like "file has content snapshot doesn't have".

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
        !crate::write::is_stale_baseline(baseline_with_user_edits, snapshot),
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
        crate::write::is_stale_baseline(old_baseline, current_snapshot),
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
        is_in_git_repo(&doc),
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
        !is_in_git_repo(&doc),
        "file outside git repo should return false"
    );
}

#[test]
fn classify_safe_out_of_band_agent_doc_mutation_exchange_and_pending() {
    let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
    let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#c3d4] new pending\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";

    assert_eq!(
        classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
        Some("exchange+pending")
    );
}

#[test]
fn classify_safe_out_of_band_agent_doc_mutation_rejects_user_prompt_append() {
    let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
    let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";

    assert_eq!(
        classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
        None
    );
}

#[test]
fn classify_safe_out_of_band_agent_doc_mutation_status_and_exchange() {
    let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Older status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
    let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Newer status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";

    assert_eq!(
        classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
        Some("status+exchange")
    );
}

#[test]
fn classify_safe_out_of_band_agent_doc_mutation_rejects_status_prompt_preset_reference() {
    let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Compacted.\n\
            <!-- /agent:status -->\n";
    let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            #next-steps\n\
            <!-- /agent:status -->\n";

    assert_eq!(
        classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
        None
    );
}

#[test]
fn classify_safe_out_of_band_agent_doc_mutation_rejects_status_prompt_preset_reference_with_guidance()
 {
    let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Compacted.\n\
            <!-- /agent:status -->\n";
    let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            #next-steps for calibrating session benchmarks with expected scores\n\
            <!-- /agent:status -->\n";

    assert_eq!(
        classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
        None
    );
}

#[test]
fn is_safe_user_only_follow_up_after_committed_head_exchange_only() {
    let head = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n";
    let current = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:live -->\n\
            <!-- /agent:exchange -->\n";

    assert!(is_safe_user_only_follow_up_after_committed_head(
        head, current
    ));
}

#[test]
fn post_commit_drift_uses_prompt_classifier_for_queue_directive() {
    let head = "---\nagent_doc_session: test\n---\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: done\n\
            Completed.\n\
            <!-- /agent:exchange -->\n\n\
            ## Queue\n\n\
            <!-- agent:queue -->\n\
            <!-- /agent:queue -->\n\n\
            ## Backlog\n\n\
            <!-- agent:backlog -->\n\
            <!-- /agent:backlog -->\n";
    let current = "---\nagent_doc_session: test\n---\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: done\n\
            Completed.\n\
            <!-- /agent:exchange -->\n\n\
            ## Queue\n\n\
            <!-- agent:queue auto -->\n\
            preset #spec-test-build-install-commit-push\n\
            - do [#nexttop]\n\
            <!-- /agent:queue -->\n\n\
            ## Backlog\n\n\
            <!-- agent:backlog -->\n\
            - [ ] [#nexttop] Fix stale status.\n\
            <!-- /agent:backlog -->\n";

    assert_eq!(
        classify_post_commit_local_drift(head, current),
        Some(PostCommitLocalDriftKind::UserFollowUp)
    );
}

#[test]
fn post_commit_drift_keeps_inline_corrections_as_working_tree_edits() {
    let head = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: report\n\
            The service returned 401.\n\
            More analysis.\n\
            <!-- /agent:exchange -->\n";
    let current = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: report\n\
            The service returned 503.\n\
            More analysis.\n\
            <!-- /agent:exchange -->\n";

    assert_eq!(
        classify_post_commit_local_drift(head, current),
        Some(PostCommitLocalDriftKind::WorkingTreeEdits)
    );
}

#[test]
fn is_safe_historical_exchange_growth_allows_prompt_target_before_response() {
    let snapshot = "### Re: older\nold body\n";
    let head = "### Re: older\nold body\n\ndo #7mqc. spec-test-news-commit-push\n### Re: do `#7mqc` — codex\nCompleted.\n";

    assert!(is_safe_historical_exchange_insert_block(
        "do #7mqc. spec-test-news-commit-push\n### Re: do `#7mqc` — codex\nCompleted."
    ));
    assert!(is_safe_historical_exchange_growth(snapshot, head));
}

#[test]
fn classify_safe_committed_historical_agent_doc_mutation_exchange() {
    let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
    let file = "---\nagent_doc_session: test\n---\n\n\
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

    assert_eq!(
        classify_safe_committed_historical_agent_doc_mutation(snapshot, file),
        Some("exchange")
    );
    assert_eq!(
        classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
        None
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

    // Create a document at its pre-response state and commit it.
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

    let snap_path = crate::snapshot::path_for(&doc).unwrap();
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

// --- #73tv: repo-scoped commit serialization + full transaction retry ---

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
    crate::snapshot::save(&doc, updated).unwrap();

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
    let content = "---\nagent_doc_session: test\n---\n\n## Assistant\n\nResponse\n\n## User\n\n";
    fs::write(&doc, content).unwrap();
    let snap_path = crate::snapshot::path_for(&doc).unwrap();
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
    let snap_path = crate::snapshot::path_for(&doc).unwrap();
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

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
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
    let snap_path = crate::snapshot::path_for(&doc).unwrap();
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
    let snap_path = crate::snapshot::path_for(&doc).unwrap();
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

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
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
    let snap_path = crate::snapshot::path_for(&doc).unwrap();
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
fn canonicalize_answered_prompt_prefixes_uses_opt_in_prompt_start() {
    let exchange = "\
### Re: sync latency — gpt-5

The current tree has already started making this accountable.
### Re: closeout guard — gpt-5

No additional prompt-bearing change was present.
Please rerun the deploy check.
### Re: deploy check — gpt-5

Done.
";

    let normalized = canonicalize_answered_prompt_prefixes(exchange);

    assert!(
        normalized.contains("\nThe current tree has already started making this accountable.\n"),
        "plain assistant prose before the next response heading must stay bare:\n{normalized}"
    );
    assert!(
        !normalized.contains("\n❯ The current tree has already started making this accountable.\n"),
        "assistant prose must not become a prompt by default:\n{normalized}"
    );
    assert!(
        normalized.contains("\n❯ Please rerun the deploy check.\n"),
        "soft prompt requests before a response heading should still be canonicalized:\n{normalized}"
    );
}

#[test]
fn canonicalize_answered_prompt_prefixes_never_prefixes_duplicate_response_body() {
    // #finalize-retry-ipc-response-duplication: a multi-retry / late-IPC
    // reposition can leave a stale duplicate response block whose body
    // butts directly against the canonical `### Re: … (HEAD)` heading with
    // no blank-line separator. Those lines are agent response body, not a
    // user prelude, and must never receive the `❯ ` prompt prefix.
    let exchange = "\
❯ do [#fix-thing]
### Re: fix thing — opus-4-8
**Scope/honesty:** narrow.
**Commits:** abc123.
### Re: fix thing — opus-4-8 (HEAD)
**Scope/honesty:** narrow.
**Commits:** abc123.
";

    let normalized = canonicalize_answered_prompt_prefixes(exchange);

    assert!(
        !normalized.contains("❯ **Scope/honesty:**"),
        "duplicate response body must not be rewritten as a prompt:\n{normalized}"
    );
    assert!(
        !normalized.contains("❯ **Commits:**"),
        "duplicate response body must not be rewritten as a prompt:\n{normalized}"
    );
    // The only `❯` line is the genuine, already-marked user prompt.
    assert_eq!(
        normalized.matches('❯').count(),
        1,
        "exactly the existing user prompt keeps its marker:\n{normalized}"
    );
}

#[test]
fn canonicalize_answered_prompt_prefixes_preserves_markdown_lists() {
    let exchange = "\
Please compare these options:
- keep this bullet bare
  - keep this nested bullet bare
1. keep this ordered bullet bare
### Re: options — gpt-5

Done.
";

    let normalized = canonicalize_answered_prompt_prefixes(exchange);

    assert!(
            normalized.starts_with(
                "❯ Please compare these options:\n- keep this bullet bare\n  - keep this nested bullet bare\n1. keep this ordered bullet bare\n"
            ),
            "prompt prose should be prefixed without rewriting markdown list items:\n{normalized}"
        );
    assert!(
        !normalized.contains("\n❯ - keep this bullet bare")
            && !normalized.contains("\n❯   - keep this nested bullet bare")
            && !normalized.contains("\n❯ 1. keep this ordered bullet bare"),
        "markdown list items must not receive prompt prefixes:\n{normalized}"
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
    crate::snapshot::save(&doc, snapshot).unwrap();
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
    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, snapshot).unwrap();
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

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, scaffold).unwrap();
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

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, scaffold).unwrap();

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
    crate::snapshot::save(&doc, snapshot).unwrap();
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
    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, tracked).unwrap();
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

    crate::snapshot::save(&doc, tracked).unwrap();

    commit(&doc).expect("commit should repair the stale snapshot");

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
    assert!(
        snap.contains("### Re: historical\n"),
        "snapshot should repair to the committed historical response:\n{snap}"
    );
    assert!(
        snap.contains("#### #next-steps\n"),
        "h4 response sub-headings that look like prompt presets should not block repair:\n{snap}"
    );

    let committed = show_head(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();
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
    crate::snapshot::save(&doc, visible_snapshot).unwrap();

    let with_user_edit = format!("{visible_snapshot}\n❯ follow-up question\n");
    fs::write(&doc, &with_user_edit).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(visible_snapshot), Some(&with_user_edit))
        .unwrap();
    crate::cycle_state::mark_response_captured(
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

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
    assert_eq!(state.last_event, "commit_already_current");

    let capture = crate::capture::load_active(&doc).unwrap();
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
        log.contains("post_commit_local_drift file=") && log.contains("kind=working_tree_edits"),
        "out-of-component local edits should be classified as working-tree drift:\n{log}"
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
    crate::snapshot::save(&doc, committed).unwrap();
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

    crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
    let response = concat!(
        "<!-- patch:exchange -->\n",
        "### Re: missed patchback — gpt-5\n\n",
        "Recovered answer.\n",
        "<!-- /patch:exchange -->\n"
    );
    crate::capture::capture_response(&doc, response).unwrap();

    let head_before = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let err =
        commit(&doc).expect_err("HEAD-current snapshot must not close a missing captured response");
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

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::ResponseCaptured
    );
    let capture = crate::capture::load_active(&doc).unwrap().unwrap();
    assert_eq!(capture.state, crate::capture::CaptureState::Captured);

    let head = show_head(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();
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

    crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
    let response = concat!(
        "<!-- patch:exchange -->\n",
        "### Re: stale sidecar — gpt-5\n\n",
        "Recovered answer that must not be lost.\n",
        "<!-- /patch:exchange -->\n"
    );
    crate::capture::capture_response(&doc, response).unwrap();

    let stale_prompt_only = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Please answer the prompt\n",
        "<!-- agent:boundary:head -->\n",
        "❯ Later user follow-up while the response is missing\n",
        "<!-- /agent:exchange -->\n"
    );
    fs::write(&doc, stale_prompt_only).unwrap();
    crate::snapshot::save(&doc, stale_prompt_only).unwrap();

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
        !show_head(&doc)
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
fn commit_adopts_manual_escaped_tail_cleanup_after_head_current_snapshot() {
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
            The routed prompt escaped below the exchange block.\n\
            It should be cleaned up without being treated as later drift.\n\n\
            do #oobtaildel. spec-test-build-install-commit-push\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
    fs::write(&doc, committed).unwrap();
    crate::snapshot::save(&doc, committed).unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();

    let did_commit = commit(&doc).expect("escaped tail cleanup should commit");
    assert!(did_commit, "cleanup deletion should create a commit");

    let head = show_head(&doc).unwrap().unwrap();
    assert_eq!(
        normalize_transient_agent_doc_markers(&head),
        normalize_transient_agent_doc_markers(cleaned),
        "HEAD should contain the cleanup deletion"
    );
    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();

    let did_commit = commit(&doc).expect("mixed cleanup should close as no-op");
    assert!(
        !did_commit,
        "mixed cleanup plus prompt must not commit the fresh prompt"
    );

    let head = show_head(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, &snapshot).unwrap();
    fs::write(&doc, &working).unwrap();

    let did_commit = commit(&doc).expect("prompt duplicate drift should repair and commit");
    assert!(did_commit);

    let head_after = show_head(&doc).unwrap().unwrap();
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
    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, stale_snapshot).unwrap();
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

    crate::snapshot::save(&doc, stale_snapshot).unwrap();

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
    crate::cycle_state::start_preflight(&doc, Some(stale_snapshot), Some(working)).unwrap();
    crate::cycle_state::mark_response_captured(
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

    let committed = show_head(&doc).unwrap().unwrap();
    assert!(
        committed.contains("### Re: newer\n"),
        "HEAD should keep the newer committed response:\n{committed}"
    );
    assert!(
        !committed.contains("❯ follow-up question"),
        "HEAD should not absorb the user's follow-up prompt:\n{committed}"
    );

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
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

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
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
    crate::snapshot::save(&doc, committed).unwrap();
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

    let committed_state = crate::cycle_state::mark_committed(
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

    let state_after = crate::cycle_state::load(&doc).unwrap().unwrap();
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

    emit_postcommit_worktree_check(&doc);

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
fn postcommit_worktree_check_logs_match_false_for_real_corruption() {
    // The catch: a late IPC reposition / stale-patch replay deletes the
    // latest `### Re:` response from the visible file and splices its body
    // into an earlier block. HEAD stays correct; the working tree drifts in
    // a way the replay normalizer cannot explain → match=false for the
    // operator to file.
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

    // Corrupted working tree: the latest `### Re: second` block is deleted
    // and its body spliced into the earlier `### Re: first` block.
    let corrupted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
    fs::write(&doc, corrupted).unwrap();

    emit_postcommit_worktree_check(&doc);

    let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("postcommit_worktree_check file=") && log.contains("match=false"),
        "spliced/deleted-response working-tree corruption must log match=false:\n{log}"
    );
    // #pcwc: HEAD is authoritative and committed content (`### Re: second`) was
    // dropped with no new user work ⇒ the tree is auto-reconciled to HEAD,
    // replacing the old manual `git checkout HEAD -- FILE` recovery.
    assert!(
        log.contains("postcommit_worktree_auto_reconciled"),
        "lost-committed-content corruption must auto-reconcile:\n{log}"
    );
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        head_doc,
        "auto-reconcile must restore the working tree to the committed HEAD blob"
    );
}

#[test]
fn postcommit_worktree_auto_reconcile_refreshes_live_editor_buffer() {
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    init_repo(root);
    let _listener = start_fake_listener(root);
    wait_for_listener(root);

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

    let corrupted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
    fs::write(&doc, corrupted).unwrap();

    emit_postcommit_worktree_check(&doc);

    let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("postcommit_worktree_auto_reconciled"),
        "corruption must repair disk to HEAD before editor refresh:\n{log}"
    );
    assert!(
        log.contains("postcommit_editor_refresh_sent"),
        "auto-reconcile must push committed content back to the live editor buffer:\n{log}"
    );
    assert_eq!(
        fs::read_to_string(root.join(".agent-doc/ack-content/unknown.md")).unwrap(),
        head_doc,
        "fake listener should observe the repaired HEAD content"
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

    emit_postcommit_worktree_check(&doc);

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

    emit_postcommit_worktree_check(&doc);

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
    crate::snapshot::save(&doc, committed).unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();
    let stale_crdt = crate::crdt::CrdtDoc::from_text(transient).encode_state();
    crate::snapshot::save_crdt(&doc, &stale_crdt).unwrap();

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

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(
        snap, committed,
        "snapshot should also be restored to clean HEAD after transient cleanup"
    );

    let crdt = crate::snapshot::load_crdt(&doc)
        .unwrap()
        .expect("CRDT state should be preserved for CRDT docs");
    let crdt_text = crate::crdt::CrdtDoc::decode_state(&crdt).unwrap().to_text();
    assert_eq!(
        crdt_text, committed,
        "CRDT state should be refreshed to the same clean HEAD content after no-op cleanup"
    );

    assert!(
        root.join(".agent-doc/patches/vcs-refresh.signal").exists(),
        "no-op closeout cleanup should still signal the editor/VCS refresh path"
    );
}

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
            let ack_dir = root_clone.join(".agent-doc/ack-content");
            let _ = std::fs::create_dir_all(&ack_dir);
            let file_path = v.get("file").and_then(|f| f.as_str()).unwrap_or("");
            let content = if !file_path.is_empty() {
                std::fs::read_to_string(file_path).unwrap_or_default()
            } else {
                String::new()
            };
            let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &content);
            Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
        });
    })
}

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
    crate::snapshot::save(&doc, initial).unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();
    fs::write(&doc, transient).unwrap();
    let stale_crdt = crate::crdt::CrdtDoc::from_text(transient).encode_state();
    crate::snapshot::save_crdt(&doc, &stale_crdt).unwrap();

    let did_commit = commit(&doc).expect("real closeout commit should succeed");
    assert!(did_commit, "snapshot should produce a real git commit");

    let head = show_head(&doc)
        .unwrap()
        .expect("committed document should be readable from HEAD after commit");
    let working = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        working, head,
        "post-commit cleanup should restore the working tree to the committed HEAD blob"
    );

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(
        snap, head,
        "snapshot should stay aligned with the committed HEAD blob"
    );

    let crdt = crate::snapshot::load_crdt(&doc)
        .unwrap()
        .expect("CRDT state should be preserved for CRDT docs");
    let crdt_text = crate::crdt::CrdtDoc::decode_state(&crdt).unwrap().to_text();
    assert_eq!(
        crdt_text, head,
        "CRDT state should refresh to the committed HEAD blob after post-commit repair"
    );

    let status = tracked_modified_paths(&doc).unwrap();
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
    crate::snapshot::save(&doc, stale_snapshot).unwrap();
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
    crate::snapshot::save(&doc, stale_snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(stale_snapshot), Some(working)).unwrap();
    crate::cycle_state::mark_response_captured(
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
    let err = commit(&doc).expect_err("status-mutating historical patchback should fail closed");
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

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(
        snap, stale_snapshot,
        "snapshot must stay on the pre-repair baseline when the historical patchback is rejected"
    );

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::ResponseCaptured
    );
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
    crate::snapshot::save(&doc, committed).unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();

    let did_commit = commit(&doc).expect("heading attribution drift should self-heal");
    assert!(!did_commit, "repair should close as already committed");

    let working = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        working, committed,
        "working tree should be restored to the committed response heading and boundary"
    );

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(
        snap, committed,
        "snapshot should also return to committed HEAD"
    );
}

#[test]
fn commit_already_current_repairs_stale_agent_response_collapse_preserving_queue_follow_up() {
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
    crate::snapshot::save(&doc, committed).unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();
    let stale_crdt = crate::crdt::CrdtDoc::from_text(drifted).encode_state();
    crate::snapshot::save_crdt(&doc, &stale_crdt).unwrap();

    let did_commit = commit(&doc).expect("stale response collapse should self-heal");
    assert!(
        !did_commit,
        "repair should close as already committed and leave queue follow-up local"
    );

    let expected_working = committed.replace(
            "<!-- agent:queue -->\n<!-- /agent:queue -->",
            "<!-- agent:queue -->\n- do [#submitdiag] Add diagnostics for JB Run Agent Doc submit misses?\n<!-- /agent:queue -->",
        );
    let working = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        working, expected_working,
        "only the stale exchange collapse should be restored; queue follow-up drift must remain visible"
    );

    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(
        snap, committed,
        "snapshot must stay on clean HEAD so the queue follow-up is not committed"
    );

    let crdt = crate::snapshot::load_crdt(&doc)
        .unwrap()
        .expect("CRDT state should be refreshed for the repaired visible document");
    let crdt_text = crate::crdt::CrdtDoc::decode_state(&crdt).unwrap().to_text();
    assert_eq!(
        crdt_text, expected_working,
        "CRDT state should match the repaired visible worktree, including preserved queue drift"
    );

    let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("stale_agent_response_collapse_cleanup file=")
            && log.contains("preserved_local_drift=true"),
        "repair should leave durable evidence that only the exchange collapse was cleaned:\n{log}"
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
    crate::snapshot::save(&doc, committed).unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();

    let did_commit = commit(&doc).expect("HEAD-current local edits should close as no-op");
    assert!(
        !did_commit,
        "later local edits on top of HEAD must stay uncommitted"
    );

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
    assert_eq!(state.last_event, "commit_already_current");

    let working_after = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        working_after, working,
        "commit should not overwrite later local edits when HEAD is already current"
    );

    let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("post_commit_local_drift file=") && log.contains("kind=working_tree_edits"),
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
    crate::snapshot::save(&doc, cleaned).unwrap();
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

    crate::cycle_state::start_preflight(&doc, Some(cleaned), Some(cleaned)).unwrap();
    crate::cycle_state::record_reaped_pending_ids(&doc, &["gone1".to_string()])
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
    crate::snapshot::save(&doc, committed).unwrap();
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
    crate::snapshot::save(&doc, committed).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(committed), Some(bypassed)).unwrap();
    crate::cycle_state::mark_response_captured(
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

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::ResponseCaptured
    );
    assert_eq!(state.last_event, "response_captured");

    let head_doc = show_head(&doc).unwrap().unwrap();
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
    crate::snapshot::save(&doc, snapshot).unwrap();
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

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(committed)).unwrap();
    crate::cycle_state::mark_write_applied(&doc, "write_template", Some(snapshot), Some(committed))
        .unwrap();

    let err = commit(&doc).expect_err("status-mutating historical patchback should fail closed");
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

#[test]
fn commit_allows_current_snapshot_to_replace_committed_historical_patchback() {
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
    crate::snapshot::save(&doc, clean).unwrap();
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
    crate::snapshot::save(&doc, compacted).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(compacted), Some(compacted)).unwrap();
    crate::cycle_state::mark_write_applied(
        &doc,
        "write_template",
        Some(compacted),
        Some(compacted),
    )
    .unwrap();

    let did_commit =
        commit(&doc).expect("current snapshot/file should replace the historical patchback");
    assert!(did_commit, "replacement commit should be created");

    let head_doc = show_head(&doc).unwrap().unwrap();
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

// --- Fix 1: snapshot saved before process::exit(75) (structural test) ---
// The actual exit path in write::run_stream calls snapshot::save before process::exit(75).
// We verify this by checking that snapshot::save is callable at that point.
// Full integration testing requires IPC infrastructure; unit coverage is in write.rs.

// --- Submodule-aware commit routing ---

#[test]
fn commit_in_submodule_routes_through_submodule_repo() {
    use std::fs;
    let outer_dir = tempfile::TempDir::new().unwrap();
    let outer = outer_dir.path();

    // Initialize a "submodule" repo inside a temp dir
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
    // Allow file:// transport inside this test invocation
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

    // Initialize the outer repo
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

    // Add the submodule
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
    // Configure the checked-out submodule for committing
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

    // Sanity: narrow_to_submodule returns the submodule path, not the outer
    let doc = submodule_path.join("session.md");
    let content = "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## User\n\n";
    fs::write(&doc, content).unwrap();
    let (narrowed, in_sub) = narrow_to_submodule(outer, &doc);
    assert!(in_sub, "doc inside src/sub should be detected as submodule");
    assert_eq!(
        narrowed, submodule_path,
        "narrowed root should be the submodule toplevel"
    );

    // Stage + commit the file inside the submodule so it's tracked
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

    // Modify the file (simulate an agent response landing) and create snapshot
    let new_content = "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## Assistant\n\nupdated\n\n## User\n\n";
    fs::write(&doc, new_content).unwrap();
    let snap_rel = crate::snapshot::path_for(&doc).unwrap();
    // The snapshot path is computed against the project root (walks for .agent-doc).
    // For this test, ensure the .agent-doc dir exists at the outer root and write the snapshot there.
    let project_root = crate::snapshot::find_project_root(&doc.canonicalize().unwrap())
        .unwrap_or_else(|| outer.to_path_buf());
    let snap_abs = project_root.join(&snap_rel);
    fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
    fs::write(&snap_abs, new_content).unwrap();

    // Run commit() — should route through the submodule, succeed, and update parent pointer
    let result = commit(&doc);
    assert!(
        result.is_ok(),
        "commit should succeed for submodule file: {:?}",
        result.err()
    );

    // Verify the submodule has a new agent-doc commit
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

    // Verify the parent has a submodule-pointer commit
    let outer_log = Command::new("git")
        .current_dir(outer)
        .args(["log", "--oneline", "-5"])
        .output()
        .unwrap();
    let outer_log_str = String::from_utf8_lossy(&outer_log.stdout);
    assert!(
        outer_log_str.contains("(submodule pointer)"),
        "parent git log should contain pointer-update commit, got:\n{outer_log_str}"
    );
}

#[test]
fn external_git_dirs_for_submodule_include_submodule_and_parent_gitdirs() {
    use std::fs;
    let outer_dir = tempfile::TempDir::new().unwrap();
    let outer = outer_dir.path();

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

    let doc = outer.join("src/sub/session.md");
    fs::write(&doc, "test\n").unwrap();

    let dirs = external_git_dirs_for_doc(&doc);
    assert!(
        dirs.contains(&outer.join(".git/modules/src/sub")),
        "submodule gitdir should be exposed to workspace-write harnesses: {dirs:?}"
    );
    assert!(
        dirs.contains(&outer.join(".git")),
        "superproject gitdir should be exposed for pointer updates: {dirs:?}"
    );
}

#[test]
fn external_git_dirs_for_submodule_include_nested_submodule_gitdirs() {
    let outer_dir = tempfile::TempDir::new().unwrap();
    let outer = outer_dir.path();
    init_repo(outer);
    commit_file(outer, "README.md", "# outer\n", "init outer");

    let sub_origin_dir = tempfile::TempDir::new().unwrap();
    let sub_origin = sub_origin_dir.path();
    init_repo(sub_origin);
    commit_file(sub_origin, "README.md", "# sub\n", "init sub");

    let nested_origin_dir = tempfile::TempDir::new().unwrap();
    let nested_origin = nested_origin_dir.path();
    init_repo(nested_origin);
    commit_file(nested_origin, "README.md", "# nested\n", "init nested");

    add_submodule(outer, sub_origin, "src/sub", "add submodule");

    let submodule_root = outer.join("src/sub");
    add_submodule(
        &submodule_root,
        nested_origin,
        "src/nested",
        "add nested submodule",
    );

    let doc = submodule_root.join("tasks/session.md");
    fs::create_dir_all(doc.parent().unwrap()).unwrap();
    fs::write(&doc, "test\n").unwrap();

    let dirs = external_git_dirs_for_doc(&doc);
    assert!(
        dirs.contains(&outer.join(".git/modules/src/sub")),
        "submodule gitdir should be exposed: {dirs:?}"
    );
    assert!(
        dirs.contains(&outer.join(".git/modules/src/sub/modules/src/nested")),
        "nested submodule gitdir should be exposed: {dirs:?}"
    );
    assert!(
        dirs.contains(&outer.join(".git")),
        "superproject gitdir should still be exposed: {dirs:?}"
    );
}

#[test]
fn workspace_access_dirs_for_submodule_include_superproject_root_and_gitdirs() {
    use std::fs;
    let outer_dir = tempfile::TempDir::new().unwrap();
    let outer = outer_dir.path();

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

    let doc = outer.join("src/sub/session.md");
    fs::write(&doc, "test\n").unwrap();

    let dirs = workspace_access_dirs_for_doc(&doc);
    assert!(
        dirs.contains(&outer.to_path_buf()),
        "superproject working tree should be writable for parent-repo patchback targets: {dirs:?}"
    );
    assert!(
        dirs.contains(&outer.join(".git/modules/src/sub")),
        "submodule gitdir should still be exposed: {dirs:?}"
    );
    assert!(
        dirs.contains(&outer.join(".git")),
        "superproject gitdir should still be exposed: {dirs:?}"
    );
}

#[test]
fn narrow_to_submodule_returns_super_root_for_non_submodule_file() {
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    Command::new("git")
        .current_dir(root)
        .args(["init"])
        .output()
        .unwrap();
    let doc = root.join("session.md");
    fs::write(&doc, "x").unwrap();
    let (narrowed, in_sub) = narrow_to_submodule(root, &doc);
    assert!(
        !in_sub,
        "non-submodule file should not be detected as in-submodule"
    );
    assert_eq!(narrowed, root);
}

// --- relative_to path normalization ---

#[test]
fn relative_to_strips_prefix_for_normal_paths() {
    let root = Path::new("/home/user/project");
    let file = Path::new("/home/user/project/src/main.rs");
    let rel = relative_to(file, root);
    assert_eq!(rel, PathBuf::from("src/main.rs"));
}

#[test]
fn relative_to_returns_original_when_no_common_prefix() {
    let root = Path::new("/home/user/project");
    let file = Path::new("/other/path/file.rs");
    let rel = relative_to(file, root);
    assert_eq!(rel, PathBuf::from("/other/path/file.rs"));
}

#[test]
fn relative_to_handles_symlinked_path() {
    use std::fs;
    let real_dir = tempfile::TempDir::new().unwrap();
    let link_dir = tempfile::TempDir::new().unwrap();
    let real_root = real_dir.path();
    let link_path = link_dir.path().join("link");

    // Create a real file
    let subdir = real_root.join("tasks");
    fs::create_dir_all(&subdir).unwrap();
    fs::write(subdir.join("doc.md"), "content").unwrap();

    // Create symlink: link -> real_root
    std::os::unix::fs::symlink(real_root, &link_path).unwrap();

    // Access the file through the symlink
    let file_via_symlink = link_path.join("tasks/doc.md");
    assert!(file_via_symlink.exists());

    // relative_to should resolve symlinks and produce the correct relative path
    let rel = relative_to(&file_via_symlink, real_root);
    assert_eq!(
        rel,
        PathBuf::from("tasks/doc.md"),
        "should produce submodule-relative path even when accessed via symlink"
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
    let content = "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## User\n\n";
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
    let project_root = crate::snapshot::find_project_root(&doc_real.canonicalize().unwrap())
        .unwrap_or_else(|| outer.to_path_buf());
    let snap_rel = crate::snapshot::path_for(&doc_real).unwrap();
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

// --- #8jzg: resolve_pane_cwd tests ---

#[test]
fn resolve_pane_cwd_returns_git_root_for_file_in_repo() {
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    Command::new("git")
        .current_dir(root)
        .args(["init"])
        .output()
        .unwrap();
    let doc = root.join("plan.md");
    fs::write(&doc, "# Plan\n").unwrap();

    // resolve_pane_cwd should return the git root (not the file's parent)
    let cwd = resolve_pane_cwd(&doc);
    assert_eq!(
        cwd, root,
        "cwd should be the git root for a file inside a plain repo"
    );
}

#[test]
fn resolve_pane_cwd_falls_back_to_process_cwd_for_non_git_path() {
    // A file in a temp dir with no git repo — should fall back to process cwd
    let dir = tempfile::TempDir::new().unwrap();
    let non_git_file = dir.path().join("notes.md");
    std::fs::write(&non_git_file, "notes\n").unwrap();

    // resolve_pane_cwd should not panic and should return a valid path
    let cwd = resolve_pane_cwd(&non_git_file);
    assert!(
        cwd.exists() || cwd == std::env::current_dir().unwrap_or_default(),
        "fallback cwd should be the process cwd or an existing path"
    );
}

#[test]
fn resolve_relative_path_prefers_existing_submodule_file_over_superproject_shadow() {
    use std::fs;
    let outer_dir = tempfile::TempDir::new().unwrap();
    let outer = outer_dir.path();

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

    let shadow_dir = outer.join("tasks");
    fs::create_dir_all(&shadow_dir).unwrap();
    fs::write(shadow_dir.join("monsterrodholders.md"), "outer shadow\n").unwrap();
    fs::write(outer.join("README.md"), "# outer\n").unwrap();
    Command::new("git")
        .current_dir(outer)
        .args(["add", "."])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(outer)
        .args(["commit", "-m", "init outer", "--no-verify"])
        .output()
        .unwrap();

    let sub_origin_dir = tempfile::TempDir::new().unwrap();
    let sub_origin = sub_origin_dir.path();
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
    fs::create_dir_all(sub_origin.join("tasks")).unwrap();
    fs::write(
        sub_origin.join("tasks/monsterrodholders.md"),
        "submodule doc\n",
    )
    .unwrap();
    Command::new("git")
        .current_dir(sub_origin)
        .args(["add", "."])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(sub_origin)
        .args(["commit", "-m", "init sub", "--no-verify"])
        .output()
        .unwrap();

    let sub_url = format!("file://{}", sub_origin.display());
    let sub_add = Command::new("git")
        .current_dir(outer)
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            &sub_url,
            "src/boost-client",
        ])
        .output()
        .unwrap();
    assert!(
        sub_add.status.success(),
        "submodule add failed: {}",
        String::from_utf8_lossy(&sub_add.stderr)
    );
    Command::new("git")
        .current_dir(outer)
        .args(["commit", "-m", "add submodule", "--no-verify"])
        .output()
        .unwrap();

    let submodule_root = outer.join("src/boost-client");
    let (super_root, resolved) =
        resolve_relative_to_git_root_from(&submodule_root, Path::new("tasks/monsterrodholders.md"))
            .unwrap();

    assert_eq!(
        super_root, outer,
        "superproject root should still be returned for IPC/project-root coordination"
    );
    assert_eq!(
        resolved,
        submodule_root
            .join("tasks/monsterrodholders.md")
            .canonicalize()
            .unwrap(),
        "relative path should resolve to the existing submodule file, not the outer shadow file"
    );
}

#[test]
fn resolve_absolute_file_path_returns_absolute_for_existing_relative() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let doc = tasks.join("plan.md");
    std::fs::write(&doc, "# Plan\n").unwrap();

    let _cwd = crate::test_support::ScopedCurrentDir::set(&root);

    let resolved = resolve_absolute_file_path(Path::new("tasks/plan.md"));
    assert!(resolved.is_absolute(), "resolved path must be absolute");
    assert_eq!(resolved, doc, "must resolve to the CWD-relative file");
}

#[test]
fn resolve_absolute_file_path_preserves_absolute_input() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let doc = root.join("test.md");
    std::fs::write(&doc, "test\n").unwrap();

    let resolved = resolve_absolute_file_path(&doc);
    assert_eq!(resolved, doc, "absolute paths must be returned as-is");
}

#[test]
fn resolve_absolute_file_path_returns_relative_when_not_found() {
    let rel = Path::new("nonexistent/path.md");
    let resolved = resolve_absolute_file_path(rel);
    assert_eq!(
        resolved, rel,
        "missing files should return the original path"
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
        !crate::write::is_stale_baseline(baseline, snapshot),
        "user edits in replace + append components should NOT be stale"
    );
}

#[test]
fn reposition_skips_working_tree_when_ipc_listener_active() {
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
            <!-- /agent:exchange -->\n";
    let doc = root.join("plan.md");
    fs::write(&doc, doc_content).unwrap();

    // Create snapshot
    let snap_dir = root.join(".agent-doc/snapshots");
    fs::create_dir_all(&snap_dir).unwrap();
    crate::snapshot::save(&doc, doc_content).unwrap();

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
        crate::ipc_socket::start_listener(&root_clone, |_msg| {
            Some(serde_json::json!({"type": "ack"}).to_string())
        })
        .ok();
    });
    thread::sleep(Duration::from_millis(100));

    // Run reposition — should skip working tree because the listener is active.
    let changed = reposition_boundary_in_snapshot(&doc);

    // Snapshot should be repositioned
    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
    assert!(
        !snap.contains("oldid123"),
        "snapshot boundary should be repositioned"
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

    // Working tree should NOT be modified (listener owns the update)
    let working = fs::read_to_string(&doc).unwrap();
    assert!(
        working.contains("oldid123"),
        "working tree should keep old boundary when listener is active"
    );
    assert!(
        working.contains("### Re: test — opus-4-6 (HEAD)\n"),
        "working tree should stay untouched before plugin reposition"
    );
    assert_eq!(
        working.matches("(HEAD)").count(),
        1,
        "working tree should retain exactly one visible head marker"
    );

    assert!(changed, "snapshot change should report changed=true");

    let _ = std::fs::remove_file(crate::ipc_socket::socket_path(root));
    drop(server);
}

#[test]
fn reposition_queues_file_ipc_when_only_patches_dir_exists() {
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
            <!-- /agent:exchange -->\n";
    let doc = root.join("plan.md");
    fs::write(&doc, doc_content).unwrap();

    // Create snapshot
    let snap_dir = root.join(".agent-doc/snapshots");
    fs::create_dir_all(&snap_dir).unwrap();
    crate::snapshot::save(&doc, doc_content).unwrap();

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

    // File-watch IPC is editor-owned even without a live socket listener.
    // Queue a patch instead of rewriting the open markdown file directly.
    fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

    // Run reposition
    reposition_boundary_in_snapshot(&doc);

    // Snapshot is repositioned for commit staging.
    let snap = crate::snapshot::load(&doc).unwrap().unwrap();
    assert!(
        !snap.contains("oldid456"),
        "snapshot boundary should be repositioned"
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

    // Working tree stays untouched; the queued file IPC patch lets the IDE
    // apply the visible cleanup through its Document API.
    let working = fs::read_to_string(&doc).unwrap();
    assert!(
        working.contains("oldid456"),
        "working tree should not be rewritten while file IPC is available"
    );
    assert!(
        working.contains("### Re: test — opus-4-6 (HEAD)\n"),
        "working tree must preserve the active editor buffer; got:\n{working}"
    );
    assert_eq!(
        working.matches("(HEAD)").count(),
        1,
        "working tree should retain exactly one (HEAD) marker; got:\n{working}"
    );

    let patch_file = root
        .join(".agent-doc/patches")
        .join(format!("{}.json", crate::snapshot::doc_hash(&doc).unwrap()));
    assert!(
        patch_file.exists(),
        "reposition should be queued for file IPC"
    );
    let payload: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&patch_file).unwrap()).unwrap();
    assert_eq!(payload["reposition_boundary"], true);
    assert_eq!(payload["preserve_head"], true);
    let queued_boundary = payload["reposition_boundary_id"].as_str().unwrap();
    assert_ne!(queued_boundary, "oldid456");
    assert!(
        snap.contains(&format!("<!-- agent:boundary:{queued_boundary} -->")),
        "queued patch should reuse committed snapshot boundary id"
    );
    assert_eq!(payload["patches"].as_array().unwrap().len(), 0);
    assert_eq!(payload["unmatched"], "");
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
            <!-- /agent:exchange -->\n";
    let doc = root.join("plan.md");
    fs::write(&doc, doc_content).unwrap();

    let snap_dir = root.join(".agent-doc/snapshots");
    fs::create_dir_all(&snap_dir).unwrap();
    crate::snapshot::save(&doc, doc_content).unwrap();

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

    reposition_boundary_in_snapshot(&doc);

    let working = fs::read_to_string(&doc).unwrap();
    assert!(
        !working.contains("oldid789"),
        "working tree should be rewritten when no editor IPC is available"
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
    crate::snapshot::save(&doc, snapshot_content).unwrap();

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

    reposition_boundary_in_snapshot(&doc);

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

#[test]
fn commit_serializes_closeout_per_git_root() {
    use std::fs;
    use std::sync::mpsc;
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

    let doc_a = root.join("plan-a.md");
    let doc_b = root.join("plan-b.md");
    let initial = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
    fs::write(&doc_a, initial).unwrap();
    fs::write(&doc_b, initial).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "plan-a.md", "plan-b.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "initial", "--no-verify"])
        .output()
        .unwrap();

    let updated_a =
        "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nA\n\n";
    let updated_b =
        "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nB\n\n";
    fs::write(&doc_a, updated_a).unwrap();
    fs::write(&doc_b, updated_b).unwrap();
    let snap_dir = root.join(".agent-doc/snapshots");
    fs::create_dir_all(&snap_dir).unwrap();
    crate::snapshot::save(&doc_a, updated_a).unwrap();
    crate::snapshot::save(&doc_b, updated_b).unwrap();

    let lock_path = commit_lock_path_for_git_root(root).unwrap();
    fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    let held = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    held.lock_exclusive().unwrap();

    let (tx, rx) = mpsc::channel();
    let mut handles = Vec::new();
    for doc in [doc_a.clone(), doc_b.clone()] {
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            let result = commit(&doc);
            tx.send((doc, result)).unwrap();
        }));
    }
    drop(tx);

    assert!(
        rx.recv_timeout(Duration::from_millis(150)).is_err(),
        "both commit threads should be waiting on the shared repo lock"
    );

    held.unlock().unwrap();

    let results = vec![
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
    ];
    for handle in handles {
        handle.join().unwrap();
    }

    for (doc, result) in results {
        let did_commit =
            result.unwrap_or_else(|e| panic!("commit should succeed for {}: {e}", doc.display()));
        assert!(did_commit, "{} should create a git commit", doc.display());
    }

    let log = Command::new("git")
        .current_dir(root)
        .args(["log", "--oneline", "-4"])
        .output()
        .unwrap();
    let log_str = String::from_utf8_lossy(&log.stdout);
    assert!(
        log_str.contains("agent-doc(plan-a):"),
        "git log should contain the plan-a closeout, got:\n{log_str}"
    );
    assert!(
        log_str.contains("agent-doc(plan-b):"),
        "git log should contain the plan-b closeout, got:\n{log_str}"
    );
}

/// #ipc-drift-writeback-serialize: two supervisors writing back to the same
/// superproject must serialize on one repo-scoped lock, so a submodule doc's
/// parent-pointer commit cannot interleave with a concurrent superproject-root
/// commit. Both must land cleanly (no interleaved partial commits, no
/// stranded response) once the shared lock is released.
#[test]
fn superproject_writeback_serializes_pointer_update_and_root_commit() {
    use std::fs;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn git(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // Submodule origin repo.
    let sub_dir = tempfile::TempDir::new().unwrap();
    let sub_origin = sub_dir.path();
    git(sub_origin, &["init"]);
    git(sub_origin, &["config", "user.email", "test@test.com"]);
    git(sub_origin, &["config", "user.name", "Test"]);
    git(sub_origin, &["config", "protocol.file.allow", "always"]);
    fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
    git(sub_origin, &["add", "README.md"]);
    git(sub_origin, &["commit", "-m", "init sub", "--no-verify"]);

    // Superproject repo with the submodule wired in.
    let outer_dir = tempfile::TempDir::new().unwrap();
    let outer = outer_dir.path();
    git(outer, &["init"]);
    git(outer, &["config", "user.email", "test@test.com"]);
    git(outer, &["config", "user.name", "Test"]);
    git(outer, &["config", "protocol.file.allow", "always"]);
    fs::write(outer.join("README.md"), "# outer\n").unwrap();
    git(outer, &["add", "README.md"]);
    git(outer, &["commit", "-m", "init outer", "--no-verify"]);
    let sub_url = format!("file://{}", sub_origin.display());
    git(
        outer,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            &sub_url,
            "src/sub",
        ],
    );
    git(outer, &["commit", "-m", "add submodule", "--no-verify"]);

    let submodule_path = outer.join("src/sub");
    git(&submodule_path, &["config", "user.email", "test@test.com"]);
    git(&submodule_path, &["config", "user.name", "Test"]);

    // A submodule-owned session doc and a superproject-root session doc.
    let initial = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
    let sub_doc = submodule_path.join("session.md");
    let root_doc = outer.join("root-doc.md");
    fs::write(&sub_doc, initial).unwrap();
    fs::write(&root_doc, initial).unwrap();
    git(&submodule_path, &["add", "session.md"]);
    git(
        &submodule_path,
        &["commit", "-m", "add sub doc", "--no-verify"],
    );
    git(outer, &["add", "root-doc.md"]);
    git(outer, &["commit", "-m", "add root doc", "--no-verify"]);

    // Agent responses land in both docs; snapshots stage the committed image.
    let sub_updated =
        "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nSUB\n\n";
    let root_updated =
        "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nROOT\n\n";
    fs::write(&sub_doc, sub_updated).unwrap();
    fs::write(&root_doc, root_updated).unwrap();
    fs::create_dir_all(outer.join(".agent-doc/snapshots")).unwrap();
    crate::snapshot::save(&sub_doc, sub_updated).unwrap();
    crate::snapshot::save(&root_doc, root_updated).unwrap();

    // Externally hold the superproject commit lock so both write-back paths
    // (the submodule pointer update and the root commit) must wait on it.
    let super_lock_path = commit_lock_path_for_git_root(outer).unwrap();
    fs::create_dir_all(super_lock_path.parent().unwrap()).unwrap();
    let held = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&super_lock_path)
        .unwrap();
    held.lock_exclusive().unwrap();

    let (tx, rx) = mpsc::channel();
    let mut handles = Vec::new();
    for doc in [sub_doc.clone(), root_doc.clone()] {
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            let result = commit(&doc);
            tx.send((doc, result)).unwrap();
        }));
    }
    drop(tx);

    // Neither write-back may finish while the superproject lock is held: the
    // root commit blocks at lock acquisition and the submodule pointer update
    // blocks before touching the parent index.
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "neither superproject write-back should complete while the shared lock is held"
    );

    held.unlock().unwrap();

    let results = vec![
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
    ];
    for handle in handles {
        handle.join().unwrap();
    }
    for (doc, result) in results {
        let did_commit =
            result.unwrap_or_else(|e| panic!("commit should succeed for {}: {e}", doc.display()));
        assert!(did_commit, "{} should create a git commit", doc.display());
    }

    // The superproject HEAD chain holds both write-backs without interleave:
    // the submodule pointer update and the root-doc closeout each landed.
    let outer_log = Command::new("git")
        .current_dir(outer)
        .args(["log", "--oneline", "-5"])
        .output()
        .unwrap();
    let outer_log_str = String::from_utf8_lossy(&outer_log.stdout);
    assert!(
        outer_log_str.contains("(submodule pointer)"),
        "superproject log should contain the submodule pointer update, got:\n{outer_log_str}"
    );
    assert!(
        outer_log_str.contains("agent-doc(root-doc):"),
        "superproject log should contain the root-doc closeout, got:\n{outer_log_str}"
    );

    // The captured response landed in each repo's HEAD (no stuck_captured_cycle).
    let sub_head = Command::new("git")
        .current_dir(&submodule_path)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&sub_head.stdout).contains("SUB"),
        "submodule HEAD should carry the captured response"
    );
    let root_head = Command::new("git")
        .current_dir(outer)
        .args(["show", "HEAD:root-doc.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&root_head.stdout).contains("ROOT"),
        "superproject HEAD should carry the captured response"
    );
}

#[test]
fn redact_component_contents_handles_nested_components() {
    let body = r#"## Status

<!-- agent:status patch=replace -->
Status content here.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Some exchange content.
Add <!-- agent:queue -->...<!-- /agent:queue --> to the template.
More content.
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] task
<!-- /agent:pending -->
"#;
    let result = redact_component_contents_for_absorb(body);
    assert!(result.is_some(), "should not panic on nested components");
    let redacted = result.unwrap();
    assert!(
        redacted.contains("<!-- agent:status patch=replace -->"),
        "should contain status open marker"
    );
    assert!(
        redacted.contains("<!-- /agent:status -->"),
        "should contain status close marker"
    );
    assert!(
        !redacted.contains("Status content here."),
        "should redact status content"
    );
    assert!(
        !redacted.contains("Some exchange content."),
        "should redact exchange content (including nested markers)"
    );
}

#[test]
fn verify_snapshot_committed_returns_committed_when_matching() {
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

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
    let content = "# Hello\n\nbody\n";
    fs::write(&doc, content).unwrap();
    crate::snapshot::save(&doc, content).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "doc.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    assert_eq!(
        verify_snapshot_committed(&doc).unwrap(),
        SnapshotCommitStatus::Committed,
    );
}

#[test]
fn verify_snapshot_committed_returns_differs_when_snapshot_ahead() {
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

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
    let old_content = "# Hello\n\nold body\n";
    fs::write(&doc, old_content).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "doc.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let new_content = "# Hello\n\nnew response body\n";
    crate::snapshot::save(&doc, new_content).unwrap();

    match verify_snapshot_committed(&doc).unwrap() {
        SnapshotCommitStatus::SnapshotDiffersFromHead { .. } => {}
        other => panic!("expected SnapshotDiffersFromHead, got {:?}", other),
    }
}

#[test]
fn verify_snapshot_committed_no_snapshot() {
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

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
    fs::write(&doc, "body\n").unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "doc.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    assert_eq!(
        verify_snapshot_committed(&doc).unwrap(),
        SnapshotCommitStatus::NoSnapshot,
    );
}

#[test]
fn verify_snapshot_committed_no_head() {
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

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
    fs::write(&doc, "body\n").unwrap();
    crate::snapshot::save(&doc, "body\n").unwrap();

    assert_eq!(
        verify_snapshot_committed(&doc).unwrap(),
        SnapshotCommitStatus::NoHead,
    );
}

#[test]
fn safe_exchange_user_prompt_insert_basic() {
    let snapshot = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
    let file = "### Re: prev — model\nprev response\nUSER PROMPT\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
    assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
}

#[test]
fn safe_exchange_user_prompt_insert_rejects_after_response() {
    let snapshot = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
    let file = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response\nEXTRA TEXT";
    assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
}

#[test]
fn safe_exchange_user_prompt_insert_rejects_deletions() {
    let snapshot = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
    let file =
        "### Re: prev — model\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
    assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
}

#[test]
fn safe_exchange_user_prompt_insert_rejects_agent_markers() {
    let snapshot = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
    let file = "### Re: prev — model\nprev response\n### Re: injected — model\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
    assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
}

#[test]
fn safe_exchange_user_prompt_insert_no_boundary() {
    let snapshot = "### Re: new — model\nnew response";
    let file = "USER PROMPT\n### Re: new — model\nnew response";
    assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
}

#[test]
fn safe_exchange_user_prompt_insert_identical() {
    let snapshot = "### Re: prev — model\nprev response\n### Re: new — model\nnew response";
    assert!(!is_safe_exchange_user_prompt_insert(snapshot, snapshot));
}

#[test]
fn safe_exchange_user_prompt_insert_multiline_prompts() {
    let snapshot = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
    let file = "### Re: prev — model\nprev response\nline one\nline two\nline three\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
    assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
}

#[test]
fn safe_exchange_user_prompt_insert_classify_integration() {
    let snapshot_doc = "---\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: prev — model\nprev response\n\
            <!-- agent:boundary:abc -->\n\
            ### Re: new — model\nnew response\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] item\n\
            <!-- /agent:backlog -->\n";

    let file_doc = "---\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: prev — model\nprev response\n\
            USER PROMPT\n\
            <!-- agent:boundary:abc -->\n\
            ### Re: new — model\nnew response\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] item\n\
            <!-- /agent:backlog -->\n";

    assert_eq!(
        classify_safe_out_of_band_agent_doc_mutation(snapshot_doc, file_doc),
        Some("exchange")
    );
}
