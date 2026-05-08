use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

fn agent_doc() -> Command {
    cargo_bin_cmd!("agent-doc")
}

fn template_doc_content() -> String {
    "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n".to_string()
}

fn session_template_doc_content() -> String {
    "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n".to_string()
}

fn session_stream_doc_content() -> String {
    "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n".to_string()
}

fn setup_template_doc() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, template_doc_content()).unwrap();
    (tmp, doc)
}

fn setup_session_template_doc() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, session_template_doc_content()).unwrap();
    (tmp, doc)
}

fn setup_session_stream_doc() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, session_stream_doc_content()).unwrap();
    (tmp, doc)
}

fn init_git_repo(root: &Path, tracked: &Path) {
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
        .args(["add", tracked.file_name().unwrap().to_str().unwrap()])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["commit", "-m", "initial", "--no-verify"])
        .status()
        .unwrap();
}

fn write_baseline(root: &Path, content: &str) -> PathBuf {
    let baseline = root.join("baseline.md");
    fs::write(&baseline, content).unwrap();
    baseline
}

fn head_blob(root: &Path) -> String {
    let output = ProcessCommand::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn enable_strict_pending_capture(doc: &Path) {
    let current = fs::read_to_string(doc).unwrap();
    let updated = current.replace(
        "agent: codex\nmodel: gpt-5\n",
        "agent: codex\nmodel: gpt-5\npending_capture_guard: strict\n",
    );
    fs::write(doc, updated).unwrap();
}

fn insert_pending_item(doc: &Path, item: &str) {
    let current = fs::read_to_string(doc).unwrap();
    let updated = current.replace(
        "<!-- agent:pending -->\n<!-- /agent:pending -->\n",
        &format!("<!-- agent:pending -->\n{item}<!-- /agent:pending -->\n"),
    );
    fs::write(doc, updated).unwrap();
}

#[test]
fn finalize_requires_git_repo_before_mutating_document() {
    let (_tmp, doc) = setup_template_doc();
    let before = fs::read_to_string(&doc).unwrap();

    agent_doc()
        .args(["finalize", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: test — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "finalize requires a git repository",
        ));

    let after = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        before, after,
        "finalize should fail before mutating the file"
    );
}

#[test]
fn write_commit_requires_git_repo_before_mutating_session_document() {
    let (_tmp, doc) = setup_session_template_doc();
    let before = fs::read_to_string(&doc).unwrap();

    agent_doc()
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: repair — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "write --commit requires a git repository for session documents",
        ));

    let after = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        before, after,
        "write --commit should fail before mutating a git-less session document"
    );
}

#[test]
fn write_commit_writes_and_commits_session_response() {
    let (tmp, doc) = setup_session_template_doc();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: repair — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: repair — gpt-5"));

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&head_blob.stdout).contains("### Re: repair — gpt-5"),
        "HEAD blob should contain the write --commit response"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn bare_write_stream_on_session_doc_fails_closed_after_write_applied() {
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);

    let assert_result = agent_doc()
        .current_dir(tmp.path())
        .args(["write", doc.to_str().unwrap(), "--stream"])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: repair — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
    assert!(
        stderr.contains("outside the commit boundary"),
        "stderr should explain the open closeout boundary, got: {stderr}"
    );
    assert!(
        stderr.contains("write_applied"),
        "stderr should surface the open cycle phase, got: {stderr}"
    );
    assert!(
        stderr.contains("synthetic-"),
        "stderr should preserve the synthetic-cycle evidence, got: {stderr}"
    );

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: repair — gpt-5"));

    let cycles_dir = tmp.path().join(".agent-doc/state/cycles");
    let cycle_path = fs::read_dir(&cycles_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let cycle_json = fs::read_to_string(&cycle_path).unwrap();
    assert!(
        cycle_json.contains("\"phase\": \"write_applied\""),
        "cycle should remain open at write_applied after bare write, got: {cycle_json}"
    );
    assert!(
        cycle_json.contains("\"cycle_id\": \"synthetic-"),
        "cycle should record the synthetic provenance, got: {cycle_json}"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["commit", doc.to_str().unwrap()])
        .assert()
        .success();

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn write_commit_remains_best_effort_for_non_session_document() {
    let (_tmp, doc) = setup_template_doc();

    agent_doc()
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: note — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: note — gpt-5"));
}

#[test]
fn finalize_writes_and_commits_template_response() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--pending-add",
            "follow up task",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: test — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: test — gpt-5"));
    assert!(content.contains("follow up task"));

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&head_blob.stdout).contains("### Re: test — gpt-5"),
        "HEAD blob should contain the finalized response"
    );

    let log = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["log", "--oneline", "-1"])
        .output()
        .unwrap();
    let log_stdout = String::from_utf8_lossy(&log.stdout);
    assert!(
        log_stdout.contains("agent-doc(session):"),
        "expected finalize to create an agent-doc commit, got: {}",
        log_stdout
    );

    let cycles_dir = tmp.path().join(".agent-doc/state/cycles");
    let mut entries = fs::read_dir(&cycles_dir).unwrap();
    let cycle_path = entries.next().unwrap().unwrap().path();
    let cycle_json = fs::read_to_string(cycle_path).unwrap();
    assert!(
        cycle_json.contains("\"phase\": \"committed\""),
        "cycle should be committed, got: {}",
        cycle_json
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn finalize_rejects_status_only_response_for_imperative_directive() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);

    let original = fs::read_to_string(&doc).unwrap();
    let baseline = write_baseline(tmp.path(), &original);
    let edited = original.replace(
        "❯ Please reply\n",
        "❯ Please reply\n\ndo #6zyp. run tests. build + install. commit + push\n",
    );
    fs::write(&doc, edited).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: test — gpt-5\nIn progress. Continuing now.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "imperative document directive requires concrete execution evidence or a concrete blocker",
        ));

    let after = fs::read_to_string(&doc).unwrap();
    assert!(
        !after.contains("### Re: test — gpt-5"),
        "finalize should fail before patching a status-only response into the document"
    );
}

#[test]
fn finalize_rejects_status_only_response_for_natural_language_pending_task() {
    let (tmp, doc) = setup_template_doc();
    init_git_repo(tmp.path(), &doc);

    let original = fs::read_to_string(&doc).unwrap();
    let baseline = write_baseline(tmp.path(), &original);
    let edited = original.replace(
        "<!-- agent:pending -->\n<!-- /agent:pending -->\n",
        "<!-- agent:pending -->\n- [ ] [#n8q4] Fix the cross-repo `no-permissions-bypass` miss now dominating benchmark MAE\n<!-- /agent:pending -->\n",
    );
    fs::write(&doc, edited).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: test — gpt-5\nI’m starting #n8q4 now. First pass is underway.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "imperative document directive requires concrete execution evidence or a concrete blocker",
        ));

    let after = fs::read_to_string(&doc).unwrap();
    assert!(
        !after.contains("### Re: test — gpt-5"),
        "finalize should fail before patching a status-only response into the document"
    );
}

#[test]
fn finalize_fails_closed_when_internal_session_check_rejects_closeout() {
    let (tmp, doc) = setup_template_doc();
    enable_strict_pending_capture(&doc);
    init_git_repo(tmp.path(), &doc);

    // Pre-commit gate now catches uncaptured recommendations before commit
    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: recommendations — gpt-5\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains("[finalize] pre-write gate"))
        .stderr(predicates::str::contains("recommendation-like items"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("### Re: recommendations — gpt-5"),
        "strict closeout should fail before patching the response into the working tree"
    );

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&head_blob.stdout).contains("### Re: recommendations �� gpt-5"),
        "HEAD blob should NOT contain the response — pre-commit gate blocked commit"
    );
}

#[test]
fn finalize_blocks_session_closeout_when_completed_pending_lacks_pending_done() {
    let (tmp, doc) = setup_session_template_doc();
    insert_pending_item(&doc, "- [ ] [#4qja] Stream orchestrate patchback\n");
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: #4qja streaming orchestrate patchback — gpt-5\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains("[finalize] pre-write gate"))
        .stderr(predicates::str::contains("--pending-done 4qja"))
        .stderr(predicates::str::contains("agent-doc finalize"))
        .stderr(predicates::str::contains("re-run the same response"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("### Re: #4qja streaming orchestrate patchback — gpt-5"),
        "strict pending-done rejection should leave the response out of the working tree"
    );
    assert!(content.contains("- [ ] [#4qja] Stream orchestrate patchback"));

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    let head_text = String::from_utf8_lossy(&head_blob.stdout);
    assert!(
        !head_text.contains("### Re: #4qja streaming orchestrate patchback — gpt-5"),
        "HEAD blob should NOT contain the response — pending-done pre-commit gate blocked commit"
    );
    assert!(
        head_text.contains("- [ ] [#4qja] Stream orchestrate patchback"),
        "HEAD backlog should remain open when pre-commit pending-done gate blocks commit"
    );
}

#[test]
fn finalize_fails_before_write_when_completed_pending_line_is_malformed() {
    let (tmp, doc) = setup_session_template_doc();
    insert_pending_item(&doc, "_- [ ] [#pcops] Project controller ops\n");
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: #pcops — gpt-5\nImplemented #pcops.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains("[finalize] pre-write gate"))
        .stderr(predicates::str::contains(
            "malformed tracked checklist item",
        ))
        .stderr(predicates::str::contains("#pcops"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("### Re: #pcops"),
        "strict malformed-item rejection should leave the response out of the working tree"
    );
    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("### Re: #pcops"),
        "HEAD must not contain a response when the malformed-item pre-write gate blocks closeout"
    );
}

#[test]
fn finalize_reaps_completed_pending_items_in_same_closeout_commit() {
    let (tmp, doc) = setup_session_template_doc();
    insert_pending_item(
        &doc,
        "- [ ] [#done1] Close the loop\n- [ ] [#keep1] Keep tracking follow-up\n",
    );
    let current = fs::read_to_string(&doc).unwrap();
    let updated = current.replace(
        "<!-- /agent:pending -->\n",
        "<!-- /agent:pending -->\n\n<!-- agent:pending-done -->\n<!-- /agent:pending-done -->\n",
    );
    fs::write(&doc, &updated).unwrap();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--pending-done",
            "done1",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: #done1 close the loop — gpt-5\nImplemented and verified.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(!content.contains("- [x] [#done1] Close the loop"));
    assert!(content.contains("[#keep1] Keep tracking follow-up"));
    assert!(content.contains("### Re: #done1 close the loop — gpt-5"));
    assert!(content.contains("<!-- agent:pending-done -->"));
    assert!(content.contains("[#done1] Close the loop"));

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    let head_text = String::from_utf8_lossy(&head_blob.stdout);
    assert!(
        !head_text.contains("- [x] [#done1] Close the loop"),
        "HEAD backlog should not strand freshly completed items"
    );
    assert!(
        head_text.contains("- [ ] [#keep1] Keep tracking follow-up"),
        "HEAD backlog should retain remaining live work"
    );
    assert!(
        head_text.contains("<!-- agent:pending-done -->")
            && head_text.contains("[#done1] Close the loop"),
        "HEAD should archive reaped items when a pending-done component exists"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn finalize_accepts_hash_prefixed_pending_done_id() {
    let (tmp, doc) = setup_session_template_doc();
    insert_pending_item(&doc, "- [ ] [#done1] Close the loop\n");
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--pending-done",
            "#done1",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: #done1 close the loop — gpt-5\nImplemented and verified.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(!content.contains("- [ ] [#done1] Close the loop"));
    assert!(content.contains("### Re: #done1 close the loop — gpt-5"));
    assert!(content.contains("## Completed / Reaped"));
    assert!(content.contains("<!-- agent:pending-done -->"));
    assert!(content.contains("[#done1] Close the loop"));

    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("- [ ] [#done1] Close the loop"),
        "HEAD backlog should not keep the completed item open when --pending-done uses #id"
    );
    assert!(
        head_text.contains("<!-- agent:pending-done -->")
            && head_text.contains("[#done1] Close the loop"),
        "HEAD should create a completed/reaped archive when the session did not already have one"
    );
}

#[test]
fn finalize_stream_rejects_empty_exchange_shell_before_commit() {
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap(), "--stream"])
        .write_stdin("<!-- patch:exchange -->\n<!-- /patch:exchange -->\n")
        .assert()
        .failure()
        .stderr(predicates::str::contains("no real response-body write"));

    let after = fs::read_to_string(&doc).unwrap();
    assert!(
        !after.contains("### Re:"),
        "strict CRDT finalize must not write an assistant response when the response shell is empty"
    );
    assert!(
        after.contains("❯ Please reply"),
        "the original prompt should remain visible after the rejected closeout"
    );

    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("### Re:"),
        "HEAD must not contain a committed assistant response when strict CRDT finalize rejects the response"
    );
}

#[test]
fn write_commit_fails_closed_when_internal_session_check_rejects_closeout() {
    let (tmp, doc) = setup_template_doc();
    enable_strict_pending_capture(&doc);
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: recommendations — gpt-5\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains("[session-check] error:"))
        .stderr(predicates::str::contains("recommendation-like items"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: recommendations — gpt-5"));

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&head_blob.stdout).contains("### Re: recommendations — gpt-5"),
        "HEAD blob should still contain the committed response when internal session-check fails"
    );
}

#[test]
fn finalize_fails_closed_on_concurrent_prompt_added_after_baseline() {
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);
    let baseline_content = fs::read_to_string(&doc).unwrap();
    let baseline = write_baseline(tmp.path(), &baseline_content);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    let current_after_preflight = fs::read_to_string(&doc).unwrap();
    let concurrent = current_after_preflight.replace(
        "<!-- /agent:exchange -->",
        "❯ What remains after this response?\n<!-- /agent:exchange -->",
    );
    fs::write(&doc, concurrent).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
            "--stream",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: Please reply — gpt-5\nAnswered only the original prompt.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains("[session-check] INTERRUPTED"))
        .stderr(predicates::str::contains("prompt_target"))
        .stderr(predicates::str::contains("What remains after this response?"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: Please reply — gpt-5"));
    assert!(content.contains("❯ What remains after this response?"));

    let head = head_blob(tmp.path());
    assert!(head.contains("### Re: Please reply — gpt-5"));
    assert!(
        !head.contains("What remains after this response?"),
        "late prompt must remain outside the committed closeout snapshot"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicates::str::contains("prompt_target"))
        .stdout(predicates::str::contains(
            "What remains after this response?",
        ));
}

#[test]
fn finalize_preserves_late_comment_tail_edit_outside_exchange_uncommitted() {
    let (tmp, doc) = setup_session_stream_doc();
    let shaped = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:exchange -->\n\n<!-- agent:pending -->",
        "<!-- /agent:exchange -->\n###\n\n<!--\nold parked note\n-->\n\n<!-- agent:pending -->",
    );
    fs::write(&doc, shaped).unwrap();
    init_git_repo(tmp.path(), &doc);
    let baseline_content = fs::read_to_string(&doc).unwrap();
    let baseline = write_baseline(tmp.path(), &baseline_content);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    let current_after_preflight = fs::read_to_string(&doc).unwrap();
    let concurrent = current_after_preflight.replace("old parked note", "edited parked note");
    fs::write(&doc, concurrent).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
            "--stream",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: Please reply — gpt-5\nAnswered the original prompt.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: Please reply — gpt-5"));
    assert!(
        content.contains("edited parked note"),
        "late comment-tail edit must remain visible after closeout"
    );

    let head = head_blob(tmp.path());
    assert!(head.contains("### Re: Please reply — gpt-5"));
    assert!(
        !head.contains("edited parked note"),
        "late non-component edit must remain outside the assistant closeout commit"
    );
    assert!(
        head.contains("old parked note"),
        "committed closeout snapshot should keep the pre-response comment tail"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

// --- Phase 3: Queue consumption integration tests ---

fn queue_doc_content() -> String {
    "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do #fix1\n- do #fix2\n- run tests\n<!-- /agent:queue -->\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n".to_string()
}

#[test]
fn finalize_consumes_first_queue_prompt_after_commit() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(&doc, queue_doc_content()).unwrap();
    init_git_repo(tmp.path(), &doc);

    let baseline = write_baseline(tmp.path(), &queue_doc_content());

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: describe the project — gpt-5\nThe project is a CLI tool for interactive document sessions.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] consumed"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("- do #fix1"),
        "first prompt should be consumed"
    );
    assert!(
        content.contains("- do #fix2"),
        "second prompt should remain"
    );
    assert!(
        content.contains("- run tests"),
        "third prompt should remain"
    );
    assert!(
        content.contains("queue_active: true"),
        "queue_active should stay true when prompts remain"
    );
}

#[test]
fn finalize_drains_queue_and_clears_active_on_last_prompt() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let single_prompt = "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto -->\n- describe the project\n<!-- /agent:queue -->\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n";
    fs::write(&doc, single_prompt).unwrap();
    init_git_repo(tmp.path(), &doc);

    let baseline = write_baseline(tmp.path(), single_prompt);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: describe the project — gpt-5\nThe project is a CLI tool for interactive document sessions with AI agents.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] drained"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("- describe the project"),
        "prompt should be consumed"
    );
    assert!(
        content.contains("queue_active: false"),
        "queue_active should be false when drained"
    );
    assert!(
        !content.contains("auto"),
        "auto attribute should be stripped on drain"
    );
}

#[test]
fn finalize_does_not_consume_when_queue_inactive() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let inactive = "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do #fix1\n<!-- /agent:queue -->\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n";
    fs::write(&doc, inactive).unwrap();
    init_git_repo(tmp.path(), &doc);

    let baseline = write_baseline(tmp.path(), inactive);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: describe the project — gpt-5\nThe project is a CLI tool for interactive document sessions.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("- do #fix1"),
        "prompt should NOT be consumed when queue is inactive"
    );
}

#[test]
fn finalize_queue_consume_updates_snapshot_atomically() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = queue_doc_content();
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);

    // Write snapshot so consume_queue_prompt has something to update
    let snap_dir = tmp.path().join(".agent-doc/snapshots");
    // Find snapshot path by running agent-doc to create it via the baseline
    let baseline = write_baseline(tmp.path(), &content);
    // Also write snapshot manually to match the document
    // Use agent-doc snapshot path convention — just write the snapshot content
    // The binary will create the correct snapshot via finalize
    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: describe the project — gpt-5\nResponse text.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] consumed"));

    let file_content = fs::read_to_string(&doc).unwrap();
    assert!(
        !file_content.contains("- do #fix1"),
        "first prompt consumed from file"
    );

    // Verify snapshot was also updated — read all .md files in snapshots dir
    let mut snapshot_updated = false;
    for entry in fs::read_dir(&snap_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|e| e == "md") {
            let snap = fs::read_to_string(entry.path()).unwrap();
            if snap.contains("queue") {
                assert!(
                    !snap.contains("- do #fix1"),
                    "first prompt must also be consumed from snapshot: {}",
                    snap
                );
                snapshot_updated = true;
            }
        }
    }
    assert!(snapshot_updated, "snapshot should exist and be updated");
}

#[test]
fn finalize_fails_closed_when_active_queue_component_is_missing() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);

    let baseline = write_baseline(tmp.path(), content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: describe the project — gpt-5\nResponse text.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "queue consume: queue_active is true but document has no agent:queue component",
        ));

    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("### Re: describe the project — gpt-5\nResponse text."),
        "HEAD blob should remain unchanged when required queue closeout cannot prove consumption"
    );
}

#[test]
fn finalize_fails_closed_when_active_queue_is_malformed() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\nnot a queue entry\n<!-- /agent:queue -->\n\n<!-- agent:pending -->\n<!-- /agent:pending -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);

    let baseline = write_baseline(tmp.path(), content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: describe the project — gpt-5\nResponse text.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "queue consume: failed to parse document queue",
        ));

    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("### Re: describe the project — gpt-5\nResponse text."),
        "HEAD blob should remain unchanged when required queue closeout cannot prove queue consumption"
    );
}

#[test]
fn finalize_keeps_queue_head_when_later_strict_pending_gate_fails() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = queue_doc_content().replace(
        "agent: codex\nmodel: gpt-5\n",
        "agent: codex\nmodel: gpt-5\npending_capture_guard: strict\n",
    );
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);

    let baseline = write_baseline(tmp.path(), &content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--baseline-file",
            baseline.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: recommendations — gpt-5\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains("[finalize] pre-write gate"))
        .stderr(predicates::str::contains("recommendation-like items"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("- do #fix1"),
        "queue head should remain when a later strict closeout gate rejects the cycle"
    );
    assert!(
        !content.contains("### Re: recommendations — gpt-5"),
        "strict pre-write gates should leave the working tree untouched"
    );

    let snap_dir = tmp.path().join(".agent-doc/snapshots");
    let mut snapshot_checked = false;
    for entry in fs::read_dir(&snap_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|e| e == "md") {
            let snap = fs::read_to_string(entry.path()).unwrap();
            assert!(
                !snap.contains("### Re: recommendations — gpt-5"),
                "strict pre-write gates should leave snapshots untouched"
            );
            if snap.contains("- do #fix1") {
                snapshot_checked = true;
            }
        }
    }
    assert!(
        snapshot_checked,
        "expected a snapshot containing the captured response"
    );

    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("### Re: recommendations — gpt-5"),
        "HEAD should remain unchanged when strict pre-commit gates reject finalize"
    );
}
