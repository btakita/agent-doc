use agent_doc_hash::content_hash;
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
    "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
}

fn session_template_doc_content() -> String {
    "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
}

fn session_stream_doc_content() -> String {
    "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
}

fn session_stream_auto_queue_doc_content() -> String {
    "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ #next-steps\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto -->\n- do #fix1\n- do #fix2\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
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

fn checkpoint_baseline(root: &Path, content: &str) {
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &root.join("session.md"),
        content,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
}

fn crdt_path(root: &Path, doc: &Path) -> PathBuf {
    root.join(".agent-doc/crdt")
        .join(format!("{}.yrs", doc_hash(doc)))
}

fn seed_reliable_sync_open(doc: &Path, tag: &str) {
    let project_root = agent_doc_fs::find_project_root(doc).expect("test project root");
    let document_hash = agent_doc_hash::document_id_for_path(doc);
    let ops = vec![agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
        document_hash: document_hash.clone(),
        pid: std::process::id().into(),
        tag: tag.to_string(),
    }];
    agent_doc_sqlite::reliable_sync_inbox::record_remote_frame(
        &agent_doc_sqlite::state_store::state_db_path(&project_root),
        &document_hash,
        1,
        Some(&serde_json::to_string(&ops).unwrap()),
    )
    .expect("seed durable reliable-sync Open fact");
}

fn snapshot_path(root: &Path, doc: &Path) -> PathBuf {
    root.join(".agent-doc/snapshots")
        .join(format!("{}.md", doc_hash(doc)))
}

/// Recreate the precondition the empty-stdin `write --commit` adoption path
/// repairs: an agent response visible in the working tree that HEAD and the
/// snapshot do not yet have (an IPC/editor partial patchback written straight to
/// disk), with no cycle_state.
///
/// Bare `agent-doc write` now fails before response/capture/document mutation
/// (#final-response-transaction), so this setup places the response on disk
/// directly to model a legacy/manual patchback. `committed` is the content at
/// HEAD; the response is inserted at the exchange tail while the snapshot omits
/// it (drift).
fn place_uncommitted_visible_response(
    root: &Path,
    doc: &Path,
    committed: &str,
    response_block: &str,
) {
    let with_response = committed.replacen(
        "<!-- /agent:exchange -->",
        &format!("{response_block}\n<!-- /agent:exchange -->"),
        1,
    );
    fs::write(doc, &with_response).unwrap();
    let snap = snapshot_path(root, doc);
    fs::create_dir_all(snap.parent().unwrap()).unwrap();
    fs::write(&snap, committed).unwrap();
}

fn doc_hash(doc: &Path) -> String {
    let canonical = doc.canonicalize().unwrap();
    content_hash(canonical.to_string_lossy().as_ref())
}

fn read_cycle_phase(_root: &Path, doc: &Path) -> Option<String> {
    agent_doc_cycle_state_io::load_with_closeout_projection(doc)
        .ok()
        .flatten()
        .map(|state| state.phase.as_str().to_string())
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
        "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        &format!("<!-- agent:backlog -->\n{item}<!-- /agent:backlog -->\n"),
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
fn finalize_skips_ignored_untracked_session_doc() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("scratch")).unwrap();
    fs::write(tmp.path().join(".gitignore"), "scratch/\n.agent-doc/\n").unwrap();
    let doc = tmp.path().join("scratch/session.md");
    fs::write(&doc, session_template_doc_content()).unwrap();

    ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["init"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["config", "user.name", "Test User"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["add", ".gitignore"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["commit", "-m", "initial", "--no-verify"])
        .status()
        .unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: ignored doc — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "skipped ignored untracked path scratch/session.md",
        ));

    let show = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:scratch/session.md"])
        .output()
        .unwrap();
    assert!(
        !show.status.success(),
        "ignored untracked session doc must not be committed"
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
fn finalize_stale_snapshot_does_not_block_response_or_pending_flags() {
    // Snapshots are durable recovery evidence, not hot-path authority. A stale
    // sidecar must not prevent the response and its same-cycle pending mutation
    // from landing atomically.
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);
    let current = fs::read_to_string(&doc).unwrap();
    let stale_exchange = (0..40)
        .map(|idx| {
            format!(
                "### Re: archived {idx} — gpt-5\n\n{}\n",
                "body\n".repeat(20)
            )
        })
        .collect::<String>();
    let stale_snapshot = current.replace(
        "❯ Please reply\n<!-- agent:boundary:1234abcd -->",
        &format!("{stale_exchange}❯ Please reply\n<!-- agent:boundary:1234abcd -->"),
    );
    fs::write(snapshot_path(tmp.path(), &doc), stale_snapshot).unwrap();
    checkpoint_baseline(tmp.path(), &current);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--backlog-add-back",
            "id=partial Pending item that must land with the response",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: stale snapshot — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let after = fs::read_to_string(&doc).unwrap();
    assert!(
        after.contains("#partial"),
        "pending add must land with the response despite stale recovery snapshot:\n{after}"
    );
    assert!(
        after.contains("### Re: stale snapshot — gpt-5"),
        "exchange response must land despite stale recovery snapshot:\n{after}"
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
fn write_commit_empty_stdin_does_not_commit_live_prompt_drift() {
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);
    let committed = head_blob(tmp.path());
    let drifted = committed.replace(
        "<!-- /agent:exchange -->",
        "follow-up typed while repair is attempted\n<!-- /agent:exchange -->",
    );
    fs::write(&doc, drifted).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicates::str::contains("empty response"));

    let head = head_blob(tmp.path());
    assert_eq!(
        committed, head,
        "empty write --commit must fail before committing live prompt drift"
    );
    assert!(
        fs::read_to_string(&doc)
            .unwrap()
            .contains("follow-up typed while repair is attempted"),
        "failed empty repair should leave live user drift in the working tree for the next cycle"
    );
}

#[test]
fn finalize_stream_editor_absent_skips_ipc_and_writes_directly() {
    let (tmp, doc) = setup_session_stream_doc();
    fs::create_dir_all(tmp.path().join(".agent-doc/patches")).unwrap();
    init_git_repo(tmp.path(), &doc);
    let initial_head = head_blob(tmp.path());
    checkpoint_baseline(tmp.path(), &session_stream_doc_content());

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: ipc timeout — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "reliable-sync reports the editor absent",
        ));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("### Re: ipc timeout — gpt-5"),
        "editor-absent direct write should materialize the response:\n{content}"
    );
    assert_ne!(
        initial_head,
        head_blob(tmp.path()),
        "editor-absent direct write should commit the response"
    );

    let patch_jsons = fs::read_dir(tmp.path().join(".agent-doc/patches"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    assert!(
        patch_jsons.is_empty(),
        "editor-absent direct write must not queue a file IPC patch"
    );
}

#[test]
fn finalize_editor_absent_replays_operator_deleted_struck_queue_rows() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let base = concat!(
        "---\n",
        "agent_doc_session: test-session\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "agent: codex\n",
        "model: gpt-5\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Resume after the capacity pause\n",
        "<!-- agent:boundary:1234abcd -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- ~~do [#old-a]~~\n",
        "- ~~do [#old-b]~~\n",
        "<!-- /agent:queue -->\n",
    );
    fs::write(&doc, base).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), base);
    let deleted = "- ~~do [#old-a]~~\n- ~~do [#old-b]~~\n";
    let offset = base.find(deleted).unwrap();
    agent_doc_op_capture_io::record_editor_op(
        &doc,
        &content_hash(base),
        agent_doc_merge::crdt::EditorOp::Delete {
            offset,
            len: deleted.len(),
        },
    )
    .unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap(), "--stream"])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: capacity pause — gpt-5\n\nResumed safely.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    let head = head_blob(tmp.path());
    for materialized in [&content, &head] {
        assert!(materialized.contains("Resumed safely."));
        assert!(!materialized.contains("#old-a"));
        assert!(!materialized.contains("#old-b"));
    }
    assert!(!agent_doc_op_capture_io::has_pending_editor_ops(&doc));
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(ops.contains("run_stream_pending_editor_cut_replayed"));
    assert!(ops.contains("run_stream_pending_editor_cut_consumed"));
}

#[test]
fn finalize_editor_absent_skips_ipc_and_applies_done_directly() {
    let (tmp, doc) = setup_session_stream_doc();
    insert_pending_item(&doc, "- [ ] [#done1] Close the loop\n");
    fs::create_dir_all(tmp.path().join(".agent-doc/patches")).unwrap();
    init_git_repo(tmp.path(), &doc);
    let initial_head = head_blob(tmp.path());

    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap(), "--done", "done1"])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: #done1 close the loop — gpt-5\nImplemented and verified.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "reliable-sync reports the editor absent",
        ));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("### Re: #done1 close the loop — gpt-5"),
        "editor-absent direct write should materialize the response:\n{content}"
    );
    assert!(
        !content.contains("- [ ] [#done1] Close the loop"),
        "--done should not remain open after the response materializes:\n{content}"
    );
    assert!(
        content.contains("<!-- agent:done -->")
            && content.contains("[#done1] Close the loop")
            && content.contains("## Completed / Reaped"),
        "--done should be reaped only after the direct write succeeds:\n{content}"
    );
    assert_ne!(
        initial_head,
        head_blob(tmp.path()),
        "editor-absent direct write should commit the response and tracked-work mutation"
    );
}

#[test]
fn attached_model_missing_does_not_merge_from_stale_recovery_projection() {
    let tmp = TempDir::new().unwrap();
    for subdir in ["snapshots", "crdt", "locks", "logs", "pending"] {
        fs::create_dir_all(tmp.path().join(".agent-doc").join(subdir)).unwrap();
    }
    let doc = tmp.path().join("session.md");
    let base = concat!(
        "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Existing prompt\n",
        "<!-- agent:boundary:ba5e1234 -->\n",
        "<!-- /agent:exchange -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#keep] Keep this backlog item\n",
        "<!-- /agent:backlog -->\n",
    );
    let current = base.replace(
        "<!-- agent:boundary:ba5e1234 -->",
        "while typing note\n<!-- agent:boundary:ba5e1234 -->",
    );
    let stale = concat!(
        "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Existing prompt\n",
        "<!-- /agent:exchange -->\n",
    );
    fs::write(&doc, base).unwrap();
    agent_doc_crdt_relay_io::seed_embedded_relay_for_file(&doc).unwrap();
    init_git_repo(tmp.path(), &doc);
    let initial_head = head_blob(tmp.path());
    fs::write(&doc, &current).unwrap();
    checkpoint_baseline(tmp.path(), base);
    let stale_doc = agent_doc_merge::crdt::CrdtDoc::from_text(stale);
    fs::write(crdt_path(tmp.path(), &doc), stale_doc.encode_state()).unwrap();

    // This scenario models an attached editor whose registration exists but whose
    // Lazily document model is unavailable. Disk and recovery projections are not
    // eligible substitutes for the missing current authority.
    seed_reliable_sync_open(&doc, "jetbrains-test-owner-ipc");

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            doc.to_str().unwrap(),
            "--ipc",
            "--commit",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: ipc stale crdt — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        // The read-authority guard now refuses before the write/endpoint guard:
        // an attached missing replica cannot make stale disk current even as an
        // intermediate read. The substantive assertions below (no merge,
        // operator text intact, HEAD unmoved, no filesystem inbox) pin the same
        // anti-clobber behavior at the earlier authority boundary.
        .stderr(predicates::str::contains(
            "disk read authority is refused",
        ));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("### Re: ipc stale crdt — gpt-5"),
        "IPC timeout must not merge a response through stale CRDT state"
    );
    assert!(
        content.contains("while typing note"),
        "live editor typing should remain untouched for retry"
    );
    assert_eq!(
        content.matches("[#keep] Keep this backlog item").count(),
        1,
        "IPC timeout retry must not replay shared document structure from stale CRDT state"
    );
    assert_eq!(
        initial_head,
        head_blob(tmp.path()),
        "IPC timeout retry must fail before committing"
    );
    assert!(
        !tmp.path().join(".agent-doc/patches").exists(),
        "fail-closed recovery must not create a filesystem delivery inbox"
    );
}

#[test]
fn explicit_ipc_on_detached_session_fails_without_creating_an_inbox() {
    // An explicit attached-editor transport request cannot silently elect disk
    // or manufacture a filesystem patch transport when no editor is registered.
    let tmp = TempDir::new().unwrap();
    for subdir in ["snapshots", "crdt", "locks", "logs", "pending"] {
        fs::create_dir_all(tmp.path().join(".agent-doc").join(subdir)).unwrap();
    }
    let doc = tmp.path().join("session.md");
    let base = concat!(
        "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Existing prompt\n",
        "<!-- agent:boundary:ba5e1234 -->\n",
        "<!-- /agent:exchange -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#keep] Keep this backlog item\n",
        "<!-- /agent:backlog -->\n",
    );
    fs::write(&doc, base).unwrap();
    init_git_repo(tmp.path(), &doc);
    let initial_head = head_blob(tmp.path());
    checkpoint_baseline(tmp.path(), base);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            doc.to_str().unwrap(),
            "--ipc",
            "--commit",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: editorless disk fallback — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("### Re: editorless disk fallback — gpt-5"),
        "run_ipc no-ACK must not materialize the response through disk:\n{content}"
    );
    assert_eq!(
        content.matches("[#keep] Keep this backlog item").count(),
        1,
        "retry path must preserve shared document structure exactly once"
    );
    assert_eq!(
        initial_head,
        head_blob(tmp.path()),
        "run_ipc no-ACK must fail before committing"
    );
    let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        !ops_log.contains("run_ipc_editorless_disk_fallback")
            && !ops_log.contains("transport=disk_fallback"),
        "ordinary run_ipc no-ACK must not emit a disk fallback:\n{ops_log}"
    );
    assert!(
        !tmp.path().join(".agent-doc/patches").exists(),
        "run_ipc no-ACK must not create a filesystem retry transport"
    );
}

#[test]
fn malformed_patchback_is_rejected_instead_of_appended_as_unmatched() {
    let tmp = TempDir::new().unwrap();
    for subdir in ["snapshots", "crdt", "locks", "pending"] {
        fs::create_dir_all(tmp.path().join(".agent-doc").join(subdir)).unwrap();
    }
    let doc = tmp.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Please reply\n",
        "<!-- agent:boundary:ba5e1234 -->\n",
        "<!-- /agent:exchange -->\n",
    );
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            doc.to_str().unwrap(),
            "--template",
            "--commit",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: malformed — gpt-5\nbody without closing patch\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "malformed template patchback: found patch/replace markers but no closed patch blocks parsed",
        ));

    let after = fs::read_to_string(&doc).unwrap();
    assert!(!after.contains("### Re: malformed"));
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(ops.contains("template_patchback_malformed_rejected"));
    assert!(ops.contains("reason=patch_markers_without_closed_blocks"));
    assert!(ops.contains("flow_event file="));
    assert!(ops.contains(
        "flow=document_mutation stage=patchback_parse outcome=failed_closed reason=malformed_patch"
    ));
}

#[test]
fn template_flag_on_crdt_doc_routes_to_stream_merge_instead_of_diff3() {
    let tmp = TempDir::new().unwrap();
    for subdir in ["snapshots", "crdt", "locks", "pending"] {
        fs::create_dir_all(tmp.path().join(".agent-doc").join(subdir)).unwrap();
    }
    let doc = tmp.path().join("session.md");
    let base = concat!(
        "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Please reply\n",
        "<!-- agent:boundary:ba5e1234 -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n",
    );
    let current = base.replace(
        "<!-- agent:boundary:ba5e1234 -->",
        "while I was typing the next queue item\n<!-- agent:boundary:ba5e1234 -->",
    );
    fs::write(&doc, base).unwrap();
    init_git_repo(tmp.path(), &doc);
    fs::write(&doc, current).unwrap();
    checkpoint_baseline(tmp.path(), base);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            doc.to_str().unwrap(),
            "--template",
            "--commit",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: crdt template route - gpt-5\n\nDone.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let after = fs::read_to_string(&doc).unwrap();
    assert!(after.contains("### Re: crdt template route - gpt-5"));
    assert!(after.contains("while I was typing the next queue item"));
    assert!(
        !after.contains("<<<<<<<") && !after.contains(">>>>>>>"),
        "CRDT template route must not write diff3 conflict markers:\n{after}"
    );
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(ops.contains("template_flag_crdt_routed_to_stream"));
    assert!(ops.contains("recovery=retry_crdt_instead"));
}

#[test]
fn bare_write_stream_on_session_doc_fails_before_mutating_the_document() {
    // #final-response-transaction: a response prefix must never become
    // authoritative. Session response writes require an explicit final commit
    // boundary and fail before capture/document/queue mutation otherwise.
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);
    let before = fs::read_to_string(&doc).unwrap();
    let head_before = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap()
        .stdout;

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
        stderr.contains("partial or non-committing response writes are disabled")
            && stderr.contains("agent-doc finalize"),
        "stderr should explain the final-only response boundary, got: {stderr}"
    );
    assert_eq!(fs::read_to_string(&doc).unwrap(), before);
    let head_after = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap()
        .stdout;
    assert_eq!(head_after, head_before);
    assert!(!tmp.path().join(".agent-doc/active-response").exists());
}

#[test]
fn write_commit_empty_stdin_rejects_untracked_visible_patchback() {
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);
    let original = fs::read_to_string(&doc).unwrap();

    place_uncommitted_visible_response(tmp.path(), &doc, &original, "### Re: repair — gpt-5\nbody");

    let visible_before = fs::read_to_string(&doc).unwrap();
    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "empty response — nothing to write",
        ));

    assert_eq!(fs::read_to_string(&doc).unwrap(), visible_before);
    assert!(!head_blob(tmp.path()).contains("### Re: repair — gpt-5"));
}

#[test]
fn write_commit_empty_stdin_rejection_preserves_post_exchange_scratch_comment() {
    let (tmp, doc) = setup_session_stream_doc();
    let prompt = "The repair write --commit scratch comment should not be deleted. #spec-test-build-install-commit-push";
    let original = session_stream_doc_content()
        .replace("❯ Please reply", &format!("❯ {prompt}"))
        .replace(
            "<!-- /agent:exchange -->\n\n<!-- agent:backlog -->",
            &format!(
                "<!-- /agent:exchange -->\n###\n\n<!--\n{prompt}\n#spec-test-build-install-commit-push\n---\nKeep repair scratch notes visible.\n-->\n\n<!-- agent:backlog -->"
            ),
        );
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);

    place_uncommitted_visible_response(
        tmp.path(),
        &doc,
        &original,
        "### Re: repair comment ownership — gpt-5\nbody",
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "empty response — nothing to write",
        ));

    let expected_comment = format!(
        "<!--\n{prompt}\n#spec-test-build-install-commit-push\n---\nKeep repair scratch notes visible.\n-->"
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: repair comment ownership — gpt-5"));
    assert!(
        content.contains(&expected_comment),
        "repair write --commit must preserve owned post-exchange scratch comments:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert!(
        head.contains(&expected_comment),
        "repair closeout commit must preserve owned scratch comments:\n{head}"
    );
}

#[test]
fn write_commit_empty_stdin_rejection_preserves_active_auto_queue_without_done() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let original = session_stream_auto_queue_doc_content();
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);

    place_uncommitted_visible_response(
        tmp.path(),
        &doc,
        &original,
        "### Re: #next-steps — gpt-5\nbody",
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "empty response — nothing to write",
        ));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("### Re: #next-steps — gpt-5"),
        "repair should materialize the visible response:\n{content}"
    );
    assert!(
        content.contains("queue_active: true") && content.contains("<!-- agent:queue auto -->"),
        "repair closeout must preserve active auto queue state:\n{content}"
    );
    assert!(
        content.contains("- do #fix1") && !content.contains("- ~~do #fix1~~"),
        "repair closeout must not consume the queue head without explicit done proof:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert!(
        !head.contains("### Re: #next-steps — gpt-5") && head.contains("- do #fix1"),
        "empty write must not commit the untracked response or change the queue:\n{head}"
    );
}

#[test]
fn write_commit_empty_stdin_with_pending_add_commits_pending_only_change() {
    let (tmp, doc) = setup_session_stream_doc();
    fs::write(
        &doc,
        "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n### Re: already handled — gpt-5\nDone.\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
    )
    .unwrap();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            "--commit",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "pending-only repair item",
        ])
        .write_stdin("")
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("pending-only repair item"),
        "pending-only write should update the live document:\n{content}"
    );
    assert!(
        content.matches("### Re:").count() == 1,
        "empty pending-only write should not synthesize an assistant response:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert!(
        head.contains("pending-only repair item"),
        "pending-only write --commit should commit the pending mutation:\n{head}"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn write_commit_visible_response_retry_preserves_pending_add() {
    let (tmp, doc) = setup_session_stream_doc();
    let original = session_stream_doc_content();
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);
    place_uncommitted_visible_response(
        tmp.path(),
        &doc,
        &original,
        "### Re: capture backlog — gpt-5\nFiled the retry follow-up as #retry.",
    );

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            "--commit",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "[#retry] Retry-visible response must keep backlog mutation",
        ])
        .write_stdin("")
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("[#retry] Retry-visible response must keep backlog mutation"),
        "retry closeout must keep the pending-add mutation in the live document:\n{content}"
    );
    assert!(
        content.contains("### Re: capture backlog"),
        "retry closeout must preserve the already-visible response:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert!(
        head.contains("[#retry] Retry-visible response must keep backlog mutation"),
        "retry closeout must commit the pending-add mutation:\n{head}"
    );
    assert!(
        head.contains("### Re: capture backlog"),
        "retry closeout must commit the already-visible response:\n{head}"
    );
}

#[test]
fn write_commit_empty_stdin_with_done_commits_pending_only_reap() {
    let (tmp, doc) = setup_session_stream_doc();
    fs::write(
        &doc,
        "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n### Re: already handled — gpt-5\nDone.\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n- [ ] [#done1] Close the loop\n<!-- /agent:backlog -->\n\n<!-- agent:queue auto -->\n- do [#done1]\n<!-- /agent:queue -->\n",
    )
    .unwrap();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            "--commit",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "done1",
        ])
        .write_stdin("")
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("- [ ] [#done1] Close the loop"),
        "empty write --commit with --done should reap the completed item:\n{content}"
    );
    assert!(
        content.matches("### Re:").count() == 1,
        "empty done-only write should not synthesize an assistant response:\n{content}"
    );
    assert!(
        content.contains("<!-- agent:done -->") && content.contains("[#done1] Close the loop"),
        "empty write --commit with --done should archive the reaped item:\n{content}"
    );
    assert!(
        !content.contains("- do [#done1]"),
        "late --done must project the matching queue head as completed in the same closeout:\n{content}"
    );
    let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("queue_consume_state_event_recorded")
            && ops_log.contains("stage=AfterMutation"),
        "pending-only --done must emit the reactive queue completion fact:\n{ops_log}"
    );

    let head = head_blob(tmp.path());
    assert!(
        !head.contains("- [ ] [#done1] Close the loop"),
        "pending-only --done closeout should commit the reap:\n{head}"
    );
    assert!(
        head.contains("<!-- agent:done -->") && head.contains("[#done1] Close the loop"),
        "pending-only --done closeout should commit the completed archive:\n{head}"
    );

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
            "--force-disk",
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

    assert_eq!(
        read_cycle_phase(tmp.path(), &doc).as_deref(),
        Some("committed")
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn finalize_stream_auto_reopens_committed_cycle_for_new_response() {
    // #finalize-stale-baseline-reopen-friction: a genuinely new response supplied
    // after a committed cycle (even with the stale baseline file) must auto-reopen
    // a fresh cycle from HEAD and commit, instead of forcing a manual preflight.
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);

    let original = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &original);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: first — gpt-5\n\nFirst response.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let after_first = head_blob(tmp.path());
    assert!(
        after_first.contains("### Re: first — gpt-5"),
        "first finalize should commit the first response:\n{after_first}"
    );

    // Second finalize reuses the STALE baseline (pre-"first"). The new response is
    // not yet in HEAD, so the gate auto-reopens from HEAD and commits it.
    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: second — gpt-5\n\nSecond response.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let head = head_blob(tmp.path());
    assert!(
        head.contains("### Re: first — gpt-5"),
        "HEAD should keep the first response after auto-reopen:\n{head}"
    );
    assert!(
        head.contains("### Re: second — gpt-5"),
        "auto-reopened finalize should commit the genuinely new response:\n{head}"
    );
    assert_eq!(
        read_cycle_phase(tmp.path(), &doc).as_deref(),
        Some("committed")
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn finalize_stream_rejects_true_duplicate_replay_after_committed_cycle() {
    // The auto-reopen does NOT weaken duplicate protection: a true replay (the
    // incoming response is already materialized in HEAD) must still fail closed so
    // the committed response is not re-applied as a duplicate block.
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);

    let original = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &original);

    let first_response = "<!-- patch:exchange -->\n### Re: first — gpt-5\n\nFirst response.\n<!-- /patch:exchange -->\n";

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
        ])
        .write_stdin(first_response)
        .assert()
        .success();

    // Replay the SAME response after commit — already in HEAD, must be rejected.
    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
        ])
        .write_stdin(first_response)
        .assert()
        .failure()
        .stderr(predicates::str::contains("already `committed`"))
        .stderr(predicates::str::contains("agent-doc preflight"));

    let head = head_blob(tmp.path());
    assert_eq!(
        head.matches("### Re: first — gpt-5").count(),
        1,
        "a true duplicate replay must not append a second copy of the response:\n{head}"
    );

    let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("baseline_replay_rejected")
            && ops_log.contains("reason=response_already_in_head"),
        "true replay should log the in-HEAD rejection reason:\n{ops_log}"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

/// `#committedwedge`: recording tracked-work bookkeeping after a committed cycle
/// must not be rejected as a response replay. A pending-only write carries no
/// response body, so the duplicate-block risk the gate guards against cannot
/// apply — and rejecting it left a loop with no operator escape: `write --commit`
/// said "run preflight", and preflight reported `no_changes` and pointed back at
/// `write --commit`.
#[test]
fn write_commit_pending_only_update_reopens_committed_cycle() {
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);

    let original = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &original);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
        ])
        .write_stdin("<!-- patch:exchange -->\n### Re: first — gpt-5\n\nFirst response.\n<!-- /patch:exchange -->\n")
        .assert()
        .success();

    // The cycle is now `committed`. Recording a follow-up item is legitimate
    // bookkeeping, not a replay.
    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            "--commit",
            doc.to_str().unwrap(),
            "--backlog-add",
            "id=followup recorded after the cycle committed",
        ])
        .write_stdin("")
        .assert()
        .success();

    let head = head_blob(tmp.path());
    assert!(
        head.contains("recorded after the cycle committed"),
        "the pending-only update must reach HEAD:\n{head}"
    );
    assert_eq!(
        head.matches("### Re: first — gpt-5").count(),
        1,
        "reopening for bookkeeping must not duplicate the committed response:\n{head}"
    );

    let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("baseline_replay_pending_only_reopened"),
        "the pending-only reopen should be recorded:\n{ops_log}"
    );
}

/// The `#committedwedge` escape is scoped to tracked-work mutations: a bare
/// empty-stdin replay with no mutations is still a no-op and still fails closed.
#[test]
fn write_commit_without_mutations_still_rejects_replay_after_committed_cycle() {
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);

    let original = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &original);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
        ])
        .write_stdin("<!-- patch:exchange -->\n### Re: only — gpt-5\n\nOnly response.\n<!-- /patch:exchange -->\n")
        .assert()
        .success();

    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicates::str::contains("already `committed`"));
}

#[test]
fn finalize_stream_rebases_stale_exchange_baseline_to_head_after_new_preflight() {
    let (tmp, doc) = setup_session_stream_doc();
    init_git_repo(tmp.path(), &doc);

    let original = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &original);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: first — gpt-5\n\nFirst response.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let after_first = head_blob(tmp.path());
    assert!(
        after_first.contains("### Re: first — gpt-5"),
        "first finalize should commit the first response:\n{after_first}"
    );

    let after_first_doc = fs::read_to_string(&doc).unwrap();
    let with_follow_up = after_first_doc.replace(
        "<!-- agent:boundary:",
        "❯ Follow-up prompt\n<!-- agent:boundary:",
    );
    assert_ne!(
        after_first_doc, with_follow_up,
        "test fixture should contain a boundary before inserting the follow-up prompt"
    );
    fs::write(&doc, with_follow_up).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: Follow-up prompt — gpt-5\n\nSecond response.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let head = head_blob(tmp.path());
    let first = head
        .find("### Re: first — gpt-5")
        .expect("HEAD should keep the first response");
    let second = head
        .find("### Re: Follow-up prompt — gpt-5")
        .expect("HEAD should append the follow-up response");
    assert!(
        first < second,
        "stale-baseline finalize must append after the prior response:\n{head}"
    );
    assert!(
        head.contains("❯ Follow-up prompt"),
        "stale-baseline rebase must preserve the live follow-up prompt:\n{head}"
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
    checkpoint_baseline(tmp.path(), &original);
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
            "--force-disk",
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
    checkpoint_baseline(tmp.path(), &original);
    let edited = original.replace(
        "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        "<!-- agent:backlog -->\n- [ ] [#n8q4] Fix the cross-repo `no-permissions-bypass` miss now dominating benchmark MAE\n<!-- /agent:backlog -->\n",
    );
    fs::write(&doc, edited).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
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
fn finalize_no_followups_records_closeout_intent_without_visible_guard_marker() {
    let (tmp, doc) = setup_template_doc();
    enable_strict_pending_capture(&doc);
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--no-followups",
            "--force-disk",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: complete — gpt-5\nThe requested work is complete.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: complete — gpt-5"));
    assert!(!content.contains("no-pending-capture"));
    let head = head_blob(tmp.path());
    assert!(head.contains("### Re: complete — gpt-5"));
    assert!(!head.contains("no-pending-capture"));
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(ops.contains("pending_capture_intent"));
    assert!(ops.contains("outcome=declared_none"));
}

#[test]
fn finalize_prewrite_guard_failure_leaves_cycle_open_for_retry() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let committed = session_template_doc_content()
        .replace(
            "agent: codex\nmodel: gpt-5\n",
            "agent: codex\nmodel: gpt-5\npending_capture_guard: strict\n",
        )
        .replace("❯ Please reply\n", "");
    fs::write(&doc, &committed).unwrap();
    init_git_repo(tmp.path(), &doc);
    fs::write(snapshot_path(tmp.path(), &doc), &committed).unwrap();

    // A QUESTION, not an imperative directive: this test covers the
    // pending-capture pre-write gate, and an imperative prompt would instead
    // trip the (stricter, earlier) execution-evidence guard — see
    // `#politeimperative`, which made "Please update the backlog." imperative.
    let current = committed.replace(
        "<!-- agent:boundary:1234abcd -->",
        "❯ What do you recommend for the backlog?\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, &current).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    assert_eq!(
        read_cycle_phase(tmp.path(), &doc).as_deref(),
        Some("preflight_started")
    );

    let response = "<!-- patch:exchange -->\n### Re: recommendations — gpt-5\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n<!-- /patch:exchange -->\n";

    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(response)
        .assert()
        .failure()
        .stderr(predicates::str::contains("[finalize] pre-write gate"));

    assert_eq!(
        read_cycle_phase(tmp.path(), &doc).as_deref(),
        Some("preflight_started"),
        "a pre-write guard failure must not close the cycle"
    );
    assert!(
        !fs::read_to_string(&doc)
            .unwrap()
            .contains("### Re: recommendations — gpt-5"),
        "pre-write guard failure must leave the response out of the document"
    );
    assert!(
        !head_blob(tmp.path()).contains("### Re: recommendations — gpt-5"),
        "pre-write guard failure must not commit the response"
    );
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops.contains(
            "flow=closeout stage=pre_write_guard outcome=blocked reason=pending_capture_required"
        ),
        "pre-write closeout guard should be mirrored as a typed FlowCore event:\n{ops}"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "id=rec1 Regression coverage follow-up",
        ])
        .write_stdin(response)
        .assert()
        .success();

    assert_eq!(
        read_cycle_phase(tmp.path(), &doc).as_deref(),
        Some("committed")
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: recommendations — gpt-5"));
    assert!(content.contains("[#rec1] Regression coverage follow-up"));
    let head = head_blob(tmp.path());
    assert!(head.contains("### Re: recommendations — gpt-5"));
    assert!(head.contains("[#rec1] Regression coverage follow-up"));
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(ops.contains(
        "flow=document_mutation stage=patchback_parse outcome=completed reason=valid_patch"
    ));
    assert!(
        ops.contains("flow=closeout stage=commit outcome=completed reason=commit_success"),
        "successful retry should cross the typed closeout commit boundary:\n{ops}"
    );
}

#[test]
fn finalize_pending_add_multiple_flags_keep_cli_order_at_top() {
    let (tmp, doc) = setup_template_doc();
    insert_pending_item(&doc, "- [ ] [#old1] existing task\n");
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "id=first first task",
            "--pending-add",
            "id=second second task",
            "--pending-add",
            "id=third third task",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: ordered pending adds — gpt-5\nDone.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    let pending = content
        .split("<!-- agent:backlog -->\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<!-- /agent:backlog -->").next())
        .unwrap();
    let first = pending.find("[#first] first task").unwrap();
    let second = pending.find("[#second] second task").unwrap();
    let third = pending.find("[#third] third task").unwrap();
    let existing = pending.find("[#old1] existing task").unwrap();
    assert!(
        first < second && second < third && third < existing,
        "expected pending-add flags to keep CLI order above existing backlog, got:\n{}",
        pending
    );

    let head = head_blob(tmp.path());
    assert!(
        head.contains("[#first] first task")
            && head.contains("[#second] second task")
            && head.contains("[#third] third task")
    );
}

#[test]
fn finalize_next_steps_pending_adds_keep_priority_order_and_status_top() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        concat!(
            "---\n",
            "agent_doc_session: test-session\n",
            "agent_doc_format: template\n",
            "agent: codex\n",
            "model: gpt-5\n",
            "prompt_presets:\n",
            "  '#next-steps': Any follow-up items to place in the backlog?\n",
            "---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Waiting.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "❯ #next-steps\n",
            "<!-- agent:boundary:1234abcd -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#old1] existing task\n",
            "<!-- /agent:backlog -->\n"
        ),
    )
    .unwrap();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-add",
            "id=first first prioritized next step",
            "--pending-add",
            "id=second second prioritized next step",
            "--status",
            "Added #next-steps follow-ups. Top backlog item: #first.",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: #next-steps — gpt-5\n\nCaptured follow-ups in priority order.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    let backlog = content
        .split("<!-- agent:backlog -->\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<!-- /agent:backlog -->").next())
        .unwrap();
    let first = backlog
        .find("[#first] first prioritized next step")
        .unwrap();
    let second = backlog
        .find("[#second] second prioritized next step")
        .unwrap();
    let existing = backlog.find("[#old1] existing task").unwrap();
    assert!(
        first < second && second < existing,
        "expected #next-steps pending-adds to keep priority order above existing backlog, got:\n{}",
        backlog
    );
    assert!(
        content.contains("Added #next-steps follow-ups. Top backlog item: #first."),
        "status should print the first inserted backlog id:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert!(head.contains("[#first] first prioritized next step"));
    assert!(head.contains("Top backlog item: #first."));
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
        .stderr(predicates::str::contains("--done 4qja"))
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
        "<!-- /agent:backlog -->\n",
        "<!-- /agent:backlog -->\n\n<!-- agent:done -->\n<!-- /agent:done -->\n",
    );
    fs::write(&doc, &updated).unwrap();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
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
    assert!(content.contains("<!-- agent:done -->"));
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
        head_text.contains("<!-- agent:done -->") && head_text.contains("[#done1] Close the loop"),
        "HEAD should archive reaped items when a done component exists"
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
            "--force-disk",
            "--done",
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
    assert!(content.contains("<!-- agent:done -->"));
    assert!(content.contains("[#done1] Close the loop"));

    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("- [ ] [#done1] Close the loop"),
        "HEAD backlog should not keep the completed item open when --done uses #id"
    );
    assert!(
        head_text.contains("<!-- agent:done -->") && head_text.contains("[#done1] Close the loop"),
        "HEAD should create a completed/reaped archive when the session did not already have one"
    );
}

#[test]
fn finalize_rejects_removed_done_aliases() {
    for removed_alias in ["--pending-done", "--backlog-done"] {
        let (tmp, doc) = setup_session_template_doc();
        insert_pending_item(&doc, "- [ ] [#done1] Close the loop\n");
        init_git_repo(tmp.path(), &doc);

        let assert_result = agent_doc()
            .current_dir(tmp.path())
            .args([
                "finalize",
                doc.to_str().unwrap(),
                "--force-disk",
                removed_alias,
                "done1",
            ])
            .write_stdin(
                "<!-- patch:exchange -->\n### Re: #done1 close the loop — gpt-5\nImplemented and verified.\n<!-- /patch:exchange -->\n",
            )
            .assert()
            .failure();

        let stderr = String::from_utf8_lossy(&assert_result.get_output().stderr);
        assert!(
            stderr.contains(removed_alias),
            "expected stderr to name {removed_alias}, got: {stderr}"
        );
        assert!(
            stderr.contains("unexpected argument"),
            "expected {removed_alias} to be rejected by clap, got: {stderr}"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            content.contains("- [ ] [#done1] Close the loop"),
            "{removed_alias} must not reap tracked work:\n{content}"
        );
        assert!(
            !content.contains("### Re: #done1 close the loop"),
            "{removed_alias} must fail before writing the response:\n{content}"
        );
    }
}

#[test]
fn finalize_pending_done_is_noop_when_item_was_already_reaped() {
    let (tmp, doc) = setup_session_template_doc();
    let current = fs::read_to_string(&doc).unwrap();
    let updated = current.replace(
        "<!-- /agent:backlog -->\n",
        "<!-- /agent:backlog -->\n\n<!-- agent:done -->\n- 2026-05-09 [#done1] Close the loop\n<!-- /agent:done -->\n",
    );
    fs::write(&doc, updated).unwrap();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "done1",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: #done1 close the loop — gpt-5\nImplemented and verified.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("### Re: #done1 close the loop — gpt-5"));
    assert!(content.contains("- 2026-05-09 [#done1] Close the loop"));

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
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
    checkpoint_baseline(tmp.path(), &baseline_content);
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
            "--force-disk",
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
fn finalize_forward_merges_late_comment_tail_edit_outside_exchange() {
    let (tmp, doc) = setup_session_stream_doc();
    let shaped = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:exchange -->\n\n<!-- agent:backlog -->",
        "<!-- /agent:exchange -->\n###\n\n<!--\nold parked note\n-->\n\n<!-- agent:backlog -->",
    );
    fs::write(&doc, shaped).unwrap();
    init_git_repo(tmp.path(), &doc);
    let baseline_content = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);
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
            "--force-disk",
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

    // #fintol2: a plain, disjoint comment-tail edit (no prompt/directive) is now
    // FORWARD-MERGED into the response commit instead of carried forward — the
    // response lands AND the user's edit is preserved in the same commit.
    let head = head_blob(tmp.path());
    assert!(head.contains("### Re: Please reply — gpt-5"));
    assert!(
        head.contains("edited parked note"),
        "the plain comment-tail edit must be forward-merged into the closeout commit"
    );
    assert!(
        !head.contains("old parked note"),
        "the forward-merge replaces the pre-edit comment tail with the user's edit"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn finalize_preserves_current_duplicate_prompt_html_comment_body() {
    let (tmp, doc) = setup_session_stream_doc();
    let prompt = "The post-exchange HTML comment block should survive finalize. #spec-test-build-install-commit-push";
    let baseline_shaped = fs::read_to_string(&doc)
        .unwrap()
        .replace("❯ Please reply", &format!("❯ {prompt}"));
    fs::write(&doc, baseline_shaped).unwrap();
    init_git_repo(tmp.path(), &doc);
    let baseline_content = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    let current_with_duplicate = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:exchange -->\n\n<!-- agent:backlog -->",
        &format!("<!-- /agent:exchange -->\n###\n\n<!--\n{prompt}\n-->\n\n<!-- agent:backlog -->"),
    );
    fs::write(&doc, current_with_duplicate).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--stream",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: comment cleanup — gpt-5\nPreserved the post-exchange scratch comment.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let expected_comment = format!("<!--\n{prompt}\n-->");
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains(&expected_comment),
        "finalize must preserve visible ordinary HTML scratch comments in the working tree:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert!(
        !head.contains(&expected_comment),
        "concurrent non-component scratch comments should stay outside the assistant closeout commit:\n{head}"
    );
}

#[test]
fn finalize_ignores_concurrent_duplicate_prompt_comment_for_session_check() {
    let (tmp, doc) = setup_session_stream_doc();
    let prompt = "As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened. #spec-test-build-install-commit-push";
    let baseline_shaped = fs::read_to_string(&doc)
        .unwrap()
        .replace("❯ Please reply", &format!("❯ {prompt}"));
    fs::write(&doc, baseline_shaped).unwrap();
    init_git_repo(tmp.path(), &doc);
    let baseline_content = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    let scratch_comment = format!("<!--\n{prompt}\n#spec-test-build-install-commit-push\n-->");
    let current_with_duplicate = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:exchange -->\n\n<!-- agent:backlog -->",
        &format!("<!-- /agent:exchange -->\n###\n\n{scratch_comment}\n\n<!-- agent:backlog -->"),
    );
    fs::write(&doc, current_with_duplicate).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--stream",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: IPC comment typing — gpt-5\nPreserved the scratch comment without replaying it into the exchange.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        content.matches(&scratch_comment).count(),
        1,
        "visible scratch comment should survive exactly once:\n{content}"
    );
    assert_eq!(
        content
            .matches("### Re: IPC comment typing — gpt-5")
            .count(),
        1,
        "response heading should not be duplicated by closeout repair:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert!(head.contains("### Re: IPC comment typing — gpt-5"));
    assert!(
        !head.contains(&scratch_comment),
        "concurrent duplicate-prompt scratch comment must stay outside the closeout commit:\n{head}"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn finalize_preserves_compacted_exchange_ipc_scratch_comment() {
    let (tmp, doc) = setup_session_stream_doc();
    let prompt = "As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened.";
    let preset = "#spec-test-build-install-commit-push";
    let compacted_exchange = format!(
        "### Session Summary\n\n\
         *Compacted. Content archived to `.agent-doc/archives/session.md`*\n\n\
         Compacted content:\n\
         - Archived 4 response topic(s): Stale queue / IPC corruption note; Closeout repair\n\
         - Prior summary/context: previous compacted exchange content\n\
         <!-- agent:boundary:a94c53cc -->\n\
         {prompt}\n\
         {preset}",
    );
    let baseline_shaped = fs::read_to_string(&doc).unwrap().replace(
        "❯ Please reply\n<!-- agent:boundary:1234abcd -->",
        &compacted_exchange,
    );
    fs::write(&doc, baseline_shaped).unwrap();
    init_git_repo(tmp.path(), &doc);
    let baseline_content = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    let scratch_comment = format!(
        "<!--\n\
         {prompt} Then the duplicate line happened, then the full-document IPC corruption happened again.\n\
         {preset}\n\
         -->",
    );
    let current_with_scratch = fs::read_to_string(&doc).unwrap().replace(
        "<!-- /agent:exchange -->\n\n<!-- agent:backlog -->",
        &format!("<!-- /agent:exchange -->\n###\n\n{scratch_comment}\n\n<!-- agent:backlog -->"),
    );
    fs::write(&doc, current_with_scratch).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--stream",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: compact IPC scratch — gpt-5\nPreserved the compacted-session scratch comment without whole-document replay.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        content.matches(&scratch_comment).count(),
        1,
        "visible compacted-session scratch comment should survive exactly once:\n{content}"
    );
    assert_eq!(
        content
            .matches("### Re: compact IPC scratch — gpt-5")
            .count(),
        1,
        "response heading should not be duplicated by closeout repair:\n{content}"
    );
    assert!(
        content.contains("### Session Summary"),
        "compacted exchange summary should remain intact:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert!(head.contains("### Re: compact IPC scratch — gpt-5"));
    assert!(
        !head.contains(&scratch_comment),
        "concurrent compacted-session scratch comment must stay outside the closeout commit:\n{head}"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn finalize_preserves_baseline_prompt_html_comment_body() {
    let (tmp, doc) = setup_session_stream_doc();
    let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
    let shaped = fs::read_to_string(&doc)
        .unwrap()
        .replace("❯ Please reply", &format!("❯ {prompt}"))
        .replace(
            "<!-- /agent:exchange -->\n\n<!-- agent:backlog -->",
            &format!(
                "<!-- /agent:exchange -->\n###\n\n<!--\n{prompt}\n-->\n\n<!-- agent:backlog -->"
            ),
        );
    fs::write(&doc, shaped).unwrap();
    init_git_repo(tmp.path(), &doc);
    let baseline_content = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--stream",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: sqlitedb graph performance next steps — gpt-5\nAnswered without deleting the parked scratch comment.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let expected_comment = format!("<!--\n{prompt}\n-->");
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains(&expected_comment),
        "pre-existing post-exchange scratch comments must remain visible after closeout:\n{content}"
    );

    let head = head_blob(tmp.path());
    assert!(
        head.contains(&expected_comment),
        "pre-existing post-exchange scratch comments must remain in the closeout commit:\n{head}"
    );
}

// --- Phase 3: Queue consumption integration tests ---

fn queue_doc_content() -> String {
    "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do #fix1\n- do #fix2\n- run tests\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
}

fn queue_doc_content_with_dispatch() -> String {
    "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\ndispatch #spec-test-build-install-commit-push\n- do [#has9]\n- do [#5pr6]\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n".to_string()
}

#[test]
fn finalize_pending_add_prepends_to_active_go_backlog_queue_after_consuming_head() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\nqueue: start\n---\n\n<!-- agent:exchange -->\n❯ do [#head]\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue priority go -->\n- do [#head]\n- do [#tail]\n<!-- /agent:queue -->\n\n<!-- agent:backlog priority queue -->\n- [ ] [#head] current queue head\n- [ ] [#tail] next queue head\n<!-- /agent:backlog -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    let baseline_content = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "head",
            "--pending-add",
            "[#fresh] same-cycle follow-up",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: do [#head] — gpt-5\nChanged paths: tests/finalize_integration.rs and agent-doc-orchestration/src/write.rs.\nCommands: cargo test finalize_pending_add_appends_to_active_go_backlog_queue_after_consuming_head.\nVerification: passed.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "[write] queue: prepended 1 same-cycle actionable backlog id(s)",
        ));

    let committed = head_blob(tmp.path());
    assert!(
        committed.contains("[#fresh] same-cycle follow-up"),
        "pending add must be committed into backlog:\n{committed}"
    );
    let tail = committed.find("do [#tail]").unwrap();
    let fresh = committed.find("do [#fresh]").unwrap();
    // `#queueatcreate`: the follow-up this turn filed becomes the NEXT head, not
    // work parked behind the existing tail where it never surfaces.
    assert!(
        fresh < tail,
        "same-cycle pending add must land at the queue head:\n{committed}"
    );
    assert!(
        committed.contains("~~do [#head]~~"),
        "the consumed head must stay struck — prepending must not resurrect it:\n{committed}"
    );
}

#[test]
fn finalize_pending_add_multiple_flags_keep_cli_order_at_active_go_queue_head() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\nqueue: start\n---\n\n<!-- agent:exchange -->\n❯ do [#head]\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue priority go -->\n- do [#head]\n- do [#tail]\n<!-- /agent:queue -->\n\n<!-- agent:backlog priority queue -->\n- [ ] [#head] current queue head\n- [ ] [#tail] next queue head\n<!-- /agent:backlog -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    let baseline_content = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "head",
            "--pending-add",
            "[#first] first follow-up",
            "--pending-add",
            "[#second] second follow-up",
            "--pending-add",
            "[#third] third follow-up",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: do [#head] — gpt-5\nDone.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "[write] queue: prepended 3 same-cycle actionable backlog id(s)",
        ));

    let committed = head_blob(tmp.path());
    let backlog = committed
        .split("<!-- agent:backlog priority queue -->\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<!-- /agent:backlog -->").next())
        .unwrap();
    let backlog_first = backlog.find("[#first] first follow-up").unwrap();
    let backlog_second = backlog.find("[#second] second follow-up").unwrap();
    let backlog_third = backlog.find("[#third] third follow-up").unwrap();
    let backlog_tail = backlog.find("[#tail] next queue head").unwrap();
    assert!(
        backlog_first < backlog_second
            && backlog_second < backlog_third
            && backlog_third < backlog_tail,
        "pending-add flags must keep CLI order at the backlog front:\n{backlog}"
    );

    let queue = committed
        .split("<!-- agent:queue priority go -->\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<!-- /agent:queue -->").next())
        .unwrap();
    let tail = queue.find("do [#tail]").unwrap();
    let first = queue.find("do [#first]").unwrap();
    let second = queue.find("do [#second]").unwrap();
    let third = queue.find("do [#third]").unwrap();
    // `#queueatcreate`: CLI order is preserved, now at the HEAD — the follow-ups
    // this turn filed are the next work, not work queued behind everything.
    assert!(
        first < second && second < third && third < tail,
        "same-cycle queue mirror must preserve pending-add CLI order at the queue head:\n{queue}"
    );
}

#[test]
fn finalize_pending_add_back_appends_to_active_go_queue() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\nqueue: start\n---\n\n<!-- agent:exchange -->\n❯ do [#head]\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue priority go -->\n- do [#head]\n- do [#tail]\n<!-- /agent:queue -->\n\n<!-- agent:backlog priority queue -->\n- [ ] [#head] current queue head\n- [ ] [#tail] next queue head\n<!-- /agent:backlog -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    let baseline_content = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "head",
            "--backlog-add-back",
            "id=agentsignals realtime agent:signals follow-up",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: do [#head] — gpt-5\nCaptured the signals follow-up.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "[write] queue: appended 1 same-cycle actionable backlog id(s)",
        ));

    let committed = head_blob(tmp.path());
    assert!(
        committed.contains("[#agentsignals] realtime agent:signals follow-up"),
        "backlog-add-back must be committed into backlog:\n{committed}"
    );
    let queue = committed
        .split("<!-- agent:queue priority go -->\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<!-- /agent:queue -->").next())
        .unwrap();
    let tail = queue.find("do [#tail]").unwrap();
    let signals = queue.find("do [#agentsignals]").unwrap();
    assert!(
        tail < signals,
        "backlog-add-back must append behind the existing live queue tail:\n{queue}"
    );
}

/// `#queueatcreate`: a `--backlog-only` write must enqueue what it creates.
///
/// This path returned before the same-cycle queue sync, so tracked-work-only
/// writes grew `agent:backlog` while `agent:queue` silently stayed put — items
/// filed this way were never picked up by any drain.
#[test]
fn backlog_only_write_enqueues_created_items_at_go_queue_head() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\nqueue: start\n---\n\n<!-- agent:exchange -->\n❯ earlier prompt\n\n### Re: earlier prompt — gpt-5\n\nDone.\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue priority go -->\n- do [#tail]\n<!-- /agent:queue -->\n\n<!-- agent:backlog priority queue -->\n- [ ] [#tail] existing queue head\n<!-- /agent:backlog -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    let baseline_content = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            doc.to_str().unwrap(),
            "--commit",
            "--force-disk",
            "--backlog-only",
            "--backlog-add",
            "id=freshonly follow-up filed by a backlog-only write",
        ])
        .assert()
        .success();

    let updated = fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("[#freshonly] follow-up filed by a backlog-only write"),
        "backlog-only write must create the backlog item:\n{updated}"
    );
    let queue = updated
        .split("<!-- agent:queue priority go -->\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<!-- /agent:queue -->").next())
        .unwrap();
    assert!(
        queue.contains("do [#freshonly]"),
        "a backlog-only write must also enqueue what it created:\n{queue}"
    );
    let fresh = queue.find("do [#freshonly]").unwrap();
    let tail = queue.find("do [#tail]").unwrap();
    assert!(
        fresh < tail,
        "the follow-up belongs at the queue head:\n{queue}"
    );
}

#[test]
fn finalize_icebox_add_back_does_not_append_to_active_go_queue() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\nqueue: start\n---\n\n<!-- agent:exchange -->\n❯ do [#head]\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue priority go -->\n- do [#head]\n- do [#tail]\n<!-- /agent:queue -->\n\n<!-- agent:backlog priority queue -->\n- [ ] [#head] current queue head\n- [ ] [#tail] next queue head\n<!-- /agent:backlog -->\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);
    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    let baseline_content = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "head",
            "--icebox-add-back",
            "id=agentsignals parked realtime agent:signals follow-up",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: do [#head] — gpt-5\nParked the signals follow-up.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let committed = head_blob(tmp.path());
    let icebox = committed
        .split("<!-- agent:icebox -->\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<!-- /agent:icebox -->").next())
        .unwrap();
    assert!(
        icebox.contains("[#agentsignals] parked realtime agent:signals follow-up"),
        "icebox-add-back must be committed into icebox:\n{icebox}"
    );
    let queue = committed
        .split("<!-- agent:queue priority go -->\n")
        .nth(1)
        .and_then(|rest| rest.split("\n<!-- /agent:queue -->").next())
        .unwrap();
    assert!(
        !queue.contains("do [#agentsignals]"),
        "icebox additions are parked work and must not mirror into the runnable queue:\n{queue}"
    );
}

#[test]
fn finalize_consumes_first_queue_prompt_after_commit() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let baseline_content = queue_doc_content();
    fs::write(&doc, &baseline_content).unwrap();
    init_git_repo(tmp.path(), &doc);

    let current = baseline_content.replace(
        "<!-- agent:boundary:1234abcd -->",
        "❯ do #fix1\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, current).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: do #fix1 — gpt-5\nImplemented and verified.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] consumed"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("- ~~do #fix1~~"),
        "first prompt should be marked complete"
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
fn finalize_skips_queue_consumption_when_unrelated_prompt_is_already_in_baseline() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let baseline_content = queue_doc_content().replace(
        "❯ describe the project\n<!-- agent:boundary:1234abcd -->",
        "❯ #next-steps\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, &baseline_content).unwrap();
    init_git_repo(tmp.path(), &doc);

    checkpoint_baseline(tmp.path(), &baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: #next-steps — gpt-5\nTop backlog item remains unchanged.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "`--pending-gate fix1`",
        ));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("- do #fix1"),
        "queue head should remain open when the prompt was unrelated and already in the baseline"
    );
    assert!(
        !content.contains("- ~~do #fix1~~"),
        "queue head must not be marked complete without exact prompt or done proof"
    );
    assert!(content.contains("queue_active: true"));
}

#[test]
fn finalize_consumes_synthetic_queue_prompt_when_response_topic_targets_head_id() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let queue_head = "JB `Run Agent Doc` on tsift.md add the prompt into agent:queue but does not complete sending the `agent-doc .../tsift.md` in the Codex session. Please include a mermaid in the response explaining what happened.\n#spec-test-build-install-commit-push";
    let content = format!(
        "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n### Re: older\nOld response.\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto -->\n---\n{queue_head}\n---\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n"
    );
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), &content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: #spec-test-build-install-commit-push — gpt-5\nImplemented and verified.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] consumed"));

    let content = fs::read_to_string(&doc).unwrap();
    let queue_section = content
        .split_once("<!-- agent:queue")
        .and_then(|(_, rest)| rest.split_once("<!-- /agent:queue -->"))
        .map(|(body, _)| body.to_string())
        .unwrap_or_default();
    assert!(
        !queue_section.contains("JB `Run Agent Doc` on tsift.md add the prompt into agent:queue"),
        "queue head should drain from the queue when the response topic targets its preset id:\n{content}"
    );
    assert!(
        content.contains("queue: stop"),
        "drained queue should clear active state:\n{content}"
    );
    assert!(
        content.contains("### Re: #spec-test-build-install-commit-push"),
        "response should still be written:\n{content}"
    );
    // #queue-prompt-echo-in-response: the consumed synthetic queue prompt is
    // embedded into the response block so the turn records what it answered.
    assert!(
        content.contains("> **Queue prompt:**"),
        "response should echo the consumed queue prompt:\n{content}"
    );
    assert!(
        content.contains("> JB `Run Agent Doc` on tsift.md add the prompt into agent:queue"),
        "response echo should quote the originating queue prompt text:\n{content}"
    );
}

#[test]
fn finalize_echoes_consumed_free_text_queue_prompt_into_response() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let queue_head =
        "Make queue responses copy the originating prompt.\nThis line documents the request.";
    let content = format!(
        "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n### Re: older\nOld response.\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto -->\n---\n{queue_head}\n---\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n"
    );
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), &content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: queue prompt copy — gpt-5\n\n> **Queue prompt:**\n>\n> Make queue responses copy the originating prompt.\n> This line documents the request.\n\nImplemented and verified.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] consumed"));

    let content = fs::read_to_string(&doc).unwrap();
    // The free-text head drains from the queue once answered.
    assert!(
        content.contains("queue: stop"),
        "drained free-text queue should clear active state:\n{content}"
    );
    // The consumed prompt is embedded into THIS cycle's response block, after
    // its heading and before any older content.
    let echo_pos = content
        .find("> **Queue prompt:**")
        .unwrap_or_else(|| panic!("response should echo the consumed prompt:\n{content}"));
    let heading_pos = content
        .find("### Re: queue prompt copy")
        .expect("response heading present");
    assert!(
        heading_pos < echo_pos,
        "echo must follow this cycle's response heading:\n{content}"
    );
    assert!(
        content.contains("> Make queue responses copy the originating prompt."),
        "echo should quote the first prompt line:\n{content}"
    );
    assert!(
        content.contains("> This line documents the request."),
        "echo should quote the full multi-line prompt:\n{content}"
    );
}

#[test]
fn finalize_skips_queue_consumption_when_user_prompt_diff_targets_other_work() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let baseline_content = queue_doc_content();
    fs::write(&doc, &baseline_content).unwrap();
    init_git_repo(tmp.path(), &doc);

    let current = baseline_content.replace(
        "<!-- agent:boundary:1234abcd -->",
        "❯ Continue with plan-auto-queue-continuation-after-finalize.md\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, current).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: continuation — gpt-5\nImplemented and verified.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "`--pending-edit \"fix1=...\"`",
        ));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("- do #fix1"),
        "queue head should remain open when the active prompt targets other work"
    );
    assert!(
        !content.contains("- ~~do #fix1~~"),
        "queue head must not be marked complete"
    );
    assert!(content.contains("queue_active: true"));
}

#[test]
fn finalize_keeps_free_text_queue_head_when_cycle_answers_foreign_exchange_prompt() {
    // #queue-head-struck-on-foreign-exchange-answer: a cycle that answers a NEW
    // unrelated `agent:exchange` prompt must NOT strike an unrelated FREE-TEXT
    // queue head. Previously any non-empty response struck the free-text head,
    // consuming work that was never done (live repro: a `lazily-rs plan-update`
    // head struck in HEAD with the file never edited).
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let queue_head = "Make queue responses copy the originating prompt.";
    let content = format!(
        "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n### Re: older\nOld response.\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto -->\n- {queue_head}\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n"
    );
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);

    // The user adds a NEW unrelated prompt to the exchange this cycle (a
    // question, so the imperative-directive guard does not require execution
    // evidence — this test isolates the queue-consumption decision).
    let current = content.replace(
        "<!-- agent:boundary:1234abcd -->",
        "❯ Which module owns queue consumption?\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, current).unwrap();
    checkpoint_baseline(tmp.path(), &content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: which module owns queue consumption — gpt-5\nwrite.rs owns it.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("kept free-text head"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("queue_active: true"),
        "free-text head answering foreign work must keep the queue active:\n{content}"
    );
    assert!(
        content.contains(queue_head) && !content.contains(&format!("~{queue_head}~")),
        "the free-text head must remain queued, not struck:\n{content}"
    );
}

#[test]
fn finalize_consumes_queue_prompt_after_dispatch_directive() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = queue_doc_content_with_dispatch();
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);

    let current = content.replace(
        "<!-- agent:boundary:1234abcd -->",
        "❯ do [#has9]\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, current).unwrap();
    checkpoint_baseline(tmp.path(), &content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: do #has9 — gpt-5\nChanged paths: src/write.rs.\nCommands: cargo test finalize_queue.\nVerification: passed.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] consumed"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("dispatch #spec-test-build-install-commit-push"),
        "batch dispatch directive should remain while later prompts are still queued"
    );
    assert!(
        content.contains("- ~~do [#has9]~~"),
        "completed queue item should be struck through"
    );
    assert!(
        content.contains("- do [#5pr6]"),
        "next queue item should remain open"
    );
    assert!(
        content.contains("queue_active: true"),
        "queue_active should stay true when a later prompt remains"
    );
}

#[test]
fn finalize_drains_queue_and_clears_active_on_last_prompt() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let single_prompt = "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ prior prompt\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto -->\n- describe the project\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
    fs::write(&doc, single_prompt).unwrap();
    init_git_repo(tmp.path(), &doc);

    let current = single_prompt.replace(
        "<!-- agent:boundary:1234abcd -->",
        "❯ describe the project\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, current).unwrap();
    checkpoint_baseline(tmp.path(), single_prompt);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
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
        "queue body should be cleared when the last prompt is consumed"
    );
    assert!(
        !content.contains("- ~~describe the project~~"),
        "drained queue should not retain completed items"
    );
    assert!(
        content.contains("queue: stop"),
        "queue_active should be false when drained"
    );
    assert!(
        !content.contains("auto"),
        "auto attribute should be stripped on drain"
    );
}

#[test]
fn finalize_drains_queue_and_removes_dispatch_directive_on_last_prompt() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\ndispatch #spec-test-build-install-commit-push\n- do [#has9]\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);

    let current = content.replace(
        "<!-- agent:boundary:1234abcd -->",
        "❯ do [#has9]\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, current).unwrap();
    checkpoint_baseline(tmp.path(), content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: do #has9 — gpt-5\nChanged paths: src/write.rs.\nCommands: cargo test finalize_queue.\nVerification: passed.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] drained"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("dispatch #spec-test-build-install-commit-push"),
        "drained queue should not retain the batch dispatch directive"
    );
    assert!(
        !content.contains("- do [#has9]"),
        "drained queue should remove the completed item"
    );
    assert!(
        !content.contains("- ~~do [#has9]~~"),
        "drained queue should not retain a struck-through last item"
    );
    assert!(
        content.contains("queue: stop"),
        "queue_active should be false when drained"
    );
}

#[test]
fn finalize_consumes_contiguous_queue_items_resolved_by_done_ids() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let baseline_content = "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ why did the queue stop?\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\npreset #spec-test-build-install-commit-push\n- do [#cspe]\n- do [#ctes]\n- do [#crem]\n- do [#cobs]\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n- [ ] [#cspe] Update specs.\n- [ ] [#ctes] Rewrite tests.\n- [ ] [#crem] Remove obsolete workaround.\n- [ ] [#cobs] Add observability criteria.\n<!-- /agent:backlog -->\n";
    fs::write(&doc, baseline_content).unwrap();
    init_git_repo(tmp.path(), &doc);

    let current = baseline_content.replace(
        "<!-- agent:boundary:1234abcd -->",
        "❯ Handle the whole queued batch in this response.\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, current).unwrap();
    checkpoint_baseline(tmp.path(), baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "cspe",
            "--done",
            "ctes",
            "--done",
            "crem",
            "--done",
            "cobs",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: queued batch — gpt-5\nChanged paths: specs.md, tests.rs, write.rs, ops.md.\nCommands: cargo test queue_batch.\nVerification: passed.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] consumed 4 item(s)"))
        .stderr(predicates::str::contains("[queue] drained"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("queue: stop"),
        "queue_active should clear after all done-backed queue items are consumed:\n{content}"
    );
    assert!(
        content.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"),
        "drained done-backed queue should be empty:\n{content}"
    );
    assert!(
        !content.contains("- do [#cspe]") && !content.contains("- ~~do [#cspe]~~"),
        "drained queue must not retain consumed prompts:\n{content}"
    );
}

#[test]
fn finalize_consumes_done_id_queue_items_interspersed_with_priority_prompt() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let priority_prompt = "agent-doc should be able to prioritize tasks before or interspersed within a group of `:round_pushpin:` annotated tasks.";
    let baseline_content = format!(
        "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ why did the queue stop?\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\npreset #spec-test-build-install-commit-push\n- {priority_prompt}\n- do [#cspe]\n- do [#ctes]\n- do [#crem]\n- do [#cobs]\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n- [ ] [#cspe] Update specs.\n- [ ] [#ctes] Rewrite tests.\n- [ ] [#crem] Remove obsolete workaround.\n- [ ] [#cobs] Add observability criteria.\n<!-- /agent:backlog -->\n"
    );
    fs::write(&doc, &baseline_content).unwrap();
    init_git_repo(tmp.path(), &doc);

    let reordered_queue = format!(
        "preset #spec-test-build-install-commit-push\n- do [#cspe]\n- do [#ctes]\n- do [#crem]\n- do [#cobs]\n- {priority_prompt}\n"
    );
    let current = baseline_content
        .replace(
            "<!-- agent:boundary:1234abcd -->",
            "❯ Handle the done-backed queued batch in this response.\n<!-- agent:boundary:1234abcd -->",
        )
        .replace(
            &format!(
                "preset #spec-test-build-install-commit-push\n- {priority_prompt}\n- do [#cspe]\n- do [#ctes]\n- do [#crem]\n- do [#cobs]\n"
            ),
            &reordered_queue,
        );
    fs::write(&doc, current).unwrap();
    checkpoint_baseline(tmp.path(), &baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "cspe",
            "--done",
            "ctes",
            "--done",
            "crem",
            "--done",
            "cobs",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: queued batch — gpt-5\nChanged paths: specs.md, tests.rs, write.rs, ops.md.\nCommands: cargo test queue_batch.\nVerification: passed.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] consumed 4 item(s)"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("queue: stop"),
        "queue_active should remain true while the interspersed priority prompt is still live:\n{content}"
    );
    assert!(
        content.contains("- ~~do [#cspe]~~")
            && content.contains("- ~~do [#ctes]~~")
            && content.contains("- ~~do [#crem]~~")
            && content.contains("- ~~do [#cobs]~~"),
        "all done-backed queue items should be struck:\n{content}"
    );
    assert!(
        content.contains(&format!("- {priority_prompt}")),
        "the unrelated priority prompt should stay unstruck:\n{content}"
    );
}

#[test]
fn finalize_does_not_consume_when_queue_inactive() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let inactive = "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do #fix1\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
    fs::write(&doc, inactive).unwrap();
    init_git_repo(tmp.path(), &doc);

    checkpoint_baseline(tmp.path(), inactive);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
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
fn finalize_queue_consume_updates_document_and_commit_atomically() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = queue_doc_content();
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);

    checkpoint_baseline(tmp.path(), &content);
    let current = content.replace(
        "<!-- agent:boundary:1234abcd -->",
        "❯ do #fix1\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, current).unwrap();
    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: do #fix1 — gpt-5\nChanged paths: src/write.rs.\nCommands: cargo test finalize_queue.\nVerification: passed.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] consumed"));

    let file_content = fs::read_to_string(&doc).unwrap();
    assert!(
        file_content.contains("- ~~do #fix1~~"),
        "first prompt marked complete in file"
    );

    let committed = head_blob(tmp.path());
    assert!(
        committed.contains("- ~~do #fix1~~"),
        "the same commit must contain the queue consumption"
    );
    assert_eq!(
        read_cycle_phase(tmp.path(), &doc).as_deref(),
        Some("committed")
    );
}

#[test]
fn finalize_fails_closed_when_active_queue_component_is_missing() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);

    checkpoint_baseline(tmp.path(), content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: describe the project — gpt-5\nResponse text.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "queue consume guard: queue_active is true but document has no agent:queue component",
        ));

    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("### Re: describe the project — gpt-5\nResponse text."),
        "HEAD blob should remain unchanged when required queue closeout cannot prove consumption"
    );
}

#[test]
fn finalize_freetext_polluted_queue_parses_but_active_empty_queue_still_fails_closed() {
    // The user reported that "failed to parse … agent:queue" is a bug: a queue
    // polluted with free-text must no longer brick the consume guard on a raw
    // PARSE error. Free-text is preserved as non-actionable Freeform. An active
    // queue with *no consumable prompt* still fails closed, but now with the
    // clearer "no prompt to consume" guard rather than an opaque parse error —
    // a real queue with actionable `do [#id]` entries (plus pollution) is
    // unaffected and consumes normally.
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n❯ describe the project\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\nnot a queue entry\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);

    checkpoint_baseline(tmp.path(), content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: describe the project — gpt-5\nResponse text.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains("no prompt to consume"));

    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("### Re: describe the project — gpt-5\nResponse text."),
        "HEAD blob should remain unchanged when an active queue has no consumable prompt"
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

    checkpoint_baseline(tmp.path(), &content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
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
    if snap_dir.exists() {
        for entry in fs::read_dir(&snap_dir).unwrap() {
            let entry = entry.unwrap();
            if entry.path().extension().is_some_and(|e| e == "md") {
                let snap = fs::read_to_string(entry.path()).unwrap();
                assert!(
                    !snap.contains("### Re: recommendations — gpt-5"),
                    "strict pre-write gates should leave snapshots untouched"
                );
            }
        }
    }

    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("### Re: recommendations — gpt-5"),
        "HEAD should remain unchanged when strict pre-commit gates reject finalize"
    );
}
