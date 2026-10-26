use agent_doc_hash::content_hash;
use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

/// `#integrationtesttargets`: this target drives the real `agent-doc` CLI, and
/// a controller launch by that CLI is production behavior, so every spawned
/// command opts in with this reason.
const CONTROLLER_LAUNCH_REASON: &str =
    "finalize/respond CLI commands close out through the project controller, which they launch as production does";

fn agent_doc() -> Command {
    let mut command = cargo_bin_cmd!("agent-doc");
    command.env(
        agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
        CONTROLLER_LAUNCH_REASON,
    );
    command
}

/// `#finalgrace`: one fixed claim-ledger clock for a test that leaves a queue
/// claim unacknowledged on purpose. An unstarted claim older than
/// `DISPATCH_EVIDENCE_GRACE_SECS` is a coordinator obligation that strict
/// closeout refuses, so measuring its age on the wall clock made the outcome
/// depend on how fast the test ran. The returned guard pins the clock for this
/// test thread's in-process claim calls; spawn CLI commands with
/// [`agent_doc_at_claim_clock`] so they read the same instant.
fn pin_claim_clock() -> (u64, agent_doc_queue_io::queue_claim::PinnedClaimClock) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    (
        now,
        agent_doc_queue_io::queue_claim::pin_claim_clock_for_current_thread(now),
    )
}

fn agent_doc_at_claim_clock(claim_clock_secs: u64) -> Command {
    let mut command = agent_doc();
    command.env(
        agent_doc_queue_io::queue_claim::TEST_CLAIM_CLOCK_ENV,
        claim_clock_secs.to_string(),
    );
    command
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

#[test]
fn status_set_updates_only_status_and_leaves_the_response_cycle_open() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test-session\nagent_doc_format: template\n",
        "agent: codex\nmodel: gpt-5\n---\n\n",
        "<!-- agent:exchange -->\n❯ investigate release\n",
        "<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n",
        "<!-- agent:status sync=\"Keep the published release current\" -->\n",
        "operator note\n<!-- /agent:status -->\n\n",
        "<!-- agent:queue -->\n- do [#release]\n<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n- [ ] [#release] publish\n<!-- /agent:backlog -->\n",
    );
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), content);
    let cycle = agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content))
        .expect("open response cycle");

    let component_body = |text: &str, open: &str, close: &str| {
        text.split_once(open)
            .unwrap()
            .1
            .split_once(close)
            .unwrap()
            .0
            .to_string()
    };
    let exchange_before = component_body(
        content,
        "<!-- agent:exchange -->",
        "<!-- /agent:exchange -->",
    );
    let queue_before = component_body(content, "<!-- agent:queue -->", "<!-- /agent:queue -->");

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "status",
            "set",
            doc.to_str().unwrap(),
            "--text",
            "Published: v1.2.3",
            "--force-disk",
        ])
        .assert()
        .success();

    let after = fs::read_to_string(&doc).unwrap();
    assert!(after.contains("operator note"), "{after}");
    assert!(
        after.contains("<!-- status:sync -->\nPublished: v1.2.3\n<!-- /status:sync -->"),
        "{after}"
    );
    assert_eq!(
        component_body(
            &after,
            "<!-- agent:exchange -->",
            "<!-- /agent:exchange -->"
        ),
        exchange_before
    );
    assert_eq!(
        component_body(&after, "<!-- agent:queue -->", "<!-- /agent:queue -->"),
        queue_before
    );
    assert!(!after.contains("### Re:"), "no response may be appended");

    let still_open = agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
        .unwrap()
        .expect("cycle remains present");
    assert_eq!(still_open.cycle_id, cycle.cycle_id);
    assert_eq!(still_open.phase.as_str(), "preflight_started");
    assert!(still_open.capture_id.is_none());
    assert!(still_open.response_sha256.is_none());

    // The standalone component write is agent-attributed, so the next
    // preflight baseline sees no synthetic operator edit from this checkpoint.
    assert_eq!(
        agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .as_deref(),
        Some(after.as_str())
    );
}

#[test]
fn status_repair_folds_nested_status_sync_and_leaves_the_cycle_open() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let status = concat!(
        "<!-- agent:status sync=\"Keep the published release current\" -->\n",
        "\n📤 Queue handoff: 5 reported items awaiting coordinator integration, verification, and closeout.\n",
        "Review pass done.\n\n",
        "<!-- status:sync -->\n",
        "Review pass done.\n\n",
        "<!-- status:sync -->\nPublished: v1.2.3\n<!-- /status:sync -->\n",
        "<!-- /status:sync -->\n",
        "<!-- /agent:status -->\n\n",
    );
    let content = format!(
        "---\nagent_doc_session: test-session\nagent_doc_format: template\n\
         agent: codex\nmodel: gpt-5\n---\n\n\
         <!-- agent:exchange -->\n❯ investigate release\n\
         <!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n\
         {status}<!-- agent:queue -->\n- do [#release]\n<!-- /agent:queue -->\n"
    );
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), &content);
    let cycle = agent_doc_cycle_state_io::start_preflight(&doc, Some(&content), Some(&content))
        .expect("open response cycle");

    agent_doc()
        .current_dir(tmp.path())
        .args(["status", "repair", doc.to_str().unwrap(), "--force-disk"])
        .assert()
        .success();

    let after = fs::read_to_string(&doc).unwrap();
    assert_eq!(after.matches("<!-- status:sync -->").count(), 1, "{after}");
    assert_eq!(after.matches("<!-- /status:sync -->").count(), 1, "{after}");
    assert_eq!(after.matches("Review pass done.").count(), 1, "{after}");
    assert!(
        after.contains("Review pass done.\n\n<!-- status:sync -->\nPublished: v1.2.3\n<!-- /status:sync -->\n<!-- /agent:status -->"),
        "{after}"
    );
    assert_eq!(
        after.replace(
            &after[after.find("<!-- agent:status").unwrap()..after.find("<!-- agent:queue").unwrap()],
            ""
        ),
        content.replace(status, ""),
        "bytes outside agent:status are unchanged"
    );
    agent_doc_document::status_projection::status_sync_directive(&after)
        .expect("repaired document satisfies the preflight contract");

    let still_open = agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
        .unwrap()
        .expect("cycle remains present");
    assert_eq!(still_open.cycle_id, cycle.cycle_id);
    assert_eq!(still_open.phase.as_str(), "preflight_started");

    // A second repair is a no-op.
    agent_doc()
        .current_dir(tmp.path())
        .args(["status", "repair", doc.to_str().unwrap(), "--force-disk"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&doc).unwrap(), after);
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
        // GH #119: the cycle is terminal (`abandoned`, commit refused by
        // design), so the closeout boundary must exit 0 — it used to exit 1
        // with `terminal proof: missing HEAD` while `session-check` reported
        // the same cycle `committed (terminal_proof_phase_repair)`.
        .success()
        .stderr(predicates::str::contains(
            "skipped ignored untracked path scratch/session.md",
        ))
        .stderr(predicates::str::contains("is ignored by .gitignore"))
        .stderr(predicates::str::contains("missing HEAD").not())
        // GH #96 second site: a queue-less document has no queue lifecycle to
        // project, so the commit must not warn that the projection failed.
        .stderr(predicates::str::contains("authority projection failed").not());
    assert!(
        fs::read_to_string(&doc)
            .unwrap()
            .contains("### Re: ignored doc"),
        "the response must still land on disk"
    );

    let show = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:scratch/session.md"])
        .output()
        .unwrap();
    assert!(
        !show.status.success(),
        "ignored untracked session doc must not be committed"
    );

    // `#ignoredpathwedge`: refusing the commit is correct; leaving the cycle
    // open is not. `write_applied` means "a terminal commit is still expected",
    // and for an ignored path none can ever follow — so the document read as
    // INTERRUPTED forever, and neither `repair` nor `reset` could clear it.
    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicates::str::contains("INTERRUPTED").not())
        .stdout(predicates::str::contains("write_applied").not())
        // GH #119: an abandoned cycle must never be promoted to `committed`;
        // nothing was committed.
        .stdout(predicates::str::contains("terminal_proof_phase_repair").not());
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

/// GH #293 / `#todoidcloseout`: a strict closeout must publish its todo patch
/// with an own id. Depending on another preflight leaves the committed UI
/// temporarily unaddressable and makes a later no-op closeout retain the gap.
#[test]
fn write_commit_backfills_patch_todo_id_before_commit() {
    let (tmp, doc) = setup_session_template_doc();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: todo id — gpt-5\nok\n<!-- /patch:exchange -->\n<!-- patch:todo -->\n- [ ] Say go to do the thing\n<!-- /patch:todo -->\n",
        )
        .assert()
        .success();

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(head_blob.status.success());
    let committed = String::from_utf8_lossy(&head_blob.stdout);
    assert!(
        committed.contains("- [ ] [#") && committed.contains("] Say go to do the thing"),
        "strict closeout must commit an addressable todo row:\n{committed}"
    );
    assert!(
        !committed.contains("- [ ] Say go to do the thing"),
        "the id-less patch row must not survive in HEAD:\n{committed}"
    );
}

/// The logical implication is enforced, not merely documented: if the typing
/// gate cannot coin a newly introduced row, strict closeout cannot publish it.
#[test]
fn write_commit_refuses_new_patch_todo_that_remains_idless() {
    let (tmp, doc) = setup_session_template_doc();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: unfinished todo — gpt-5\nok\n<!-- /patch:exchange -->\n<!-- patch:todo -->\n- [ ] Should we publish the\n<!-- /patch:todo -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "strict closeout refused before commit",
        ))
        .stderr(predicates::str::contains("Use --todo-add"));

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(head_blob.status.success());
    let committed = String::from_utf8_lossy(&head_blob.stdout);
    assert!(
        !committed.contains("Should we publish the"),
        "an id-less new todo must not cross the commit boundary:\n{committed}"
    );
}

/// GH #93: a backlog item quoting a component-marker *prefix* in backticks is
/// prose, so the final lint gate must not report the backlog as unclosed.
#[test]
fn write_commit_accepts_backticked_marker_prefix_in_backlog_item() {
    let (tmp, doc) = setup_session_template_doc();
    let content = session_template_doc_content().replace(
        "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        "<!-- agent:backlog -->\n- [ ] [#b] only `<!-- agent:` occurrence inside backticks\n<!-- /agent:backlog -->\n",
    );
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: prefix — gpt-5\nok\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success()
        .stderr(predicates::str::contains("unclosed-component").not());

    let head_blob = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&head_blob.stdout).contains("### Re: prefix — gpt-5"),
        "the response must commit beside a backticked marker prefix"
    );
}

/// GH #93: a blocking lint finding already in the document must refuse the
/// write before it mutates anything, not after the response is on disk.
#[test]
fn write_commit_lint_gate_refuses_before_mutating_document() {
    let (tmp, doc) = setup_session_template_doc();
    let content = format!(
        "{}\n<!-- agent:operator-notes -->\nscratch\n<!-- /agent:operator-notes -->\n",
        session_template_doc_content()
    );
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);
    let before = fs::read_to_string(&doc).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args(["write", "--commit", doc.to_str().unwrap()])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: gated — gpt-5\nbody\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains("INTERRUPTED before write"))
        .stderr(predicates::str::contains("agent-doc/unknown-component"));

    let after = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        before, after,
        "a lint-gate refusal must leave the document untouched"
    );
    let status = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["status", "--porcelain", "--", "session.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&status.stdout).trim().is_empty(),
        "a lint-gate refusal must leave no dirty session document"
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
    // `#loadbudgettests`: assert the consumed epoch from the state, and read it
    // with `load_op_capture` so a state read that errors fails the test instead
    // of passing it vacuously the way `!has_pending_editor_ops` did: the
    // finalize's controller has exited and left its socket behind, which every
    // read refused. With no listener, read the durable ledger cold.
    #[cfg(unix)]
    {
        let socket = tmp.path().join(".agent-doc/controller.sock");
        if socket.exists() && std::os::unix::net::UnixStream::connect(&socket).is_err() {
            fs::remove_file(&socket).unwrap();
        }
    }
    assert!(
        agent_doc_op_capture_io::load_op_capture(&doc)
            .unwrap()
            .is_none(),
        "the replayed editor-op epoch must be consumed after the write"
    );
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops.contains("run_stream_pending_editor_cut_replayed"),
        "{ops}"
    );
    // The consumed line is decided from the state, not from whether one clear
    // round trip beat the clock, so it holds under host load too.
    assert!(
        ops.contains("run_stream_pending_editor_cut_consumed"),
        "{ops}"
    );
    assert!(
        !ops.contains("run_stream_pending_editor_cut_clear_failed"),
        "{ops}"
    );
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
            && content.contains("## Done"),
        "--done should be reaped only after the direct write succeeds:\n{content}"
    );
    assert_ne!(
        initial_head,
        head_blob(tmp.path()),
        "editor-absent direct write should commit the response and tracked-work mutation"
    );
}

/// `#reapretainedcoverage`: exercise the complete retained-write branch.
///
/// The hook fires only after the stream path resolved authoritative document
/// content and durably captured the response. The command must return the real
/// retained marker while its same closeout archives `--done`; leaving `[x]`
/// would recreate the next-cycle interrupted/repair-clean contradiction.
#[test]
fn retained_stream_write_archives_done_in_the_same_closeout() {
    let (tmp, doc) = setup_session_stream_doc();
    insert_pending_item(&doc, "- [ ] [#rrta] Archive this retained closeout item\n");
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), &fs::read_to_string(&doc).unwrap());

    agent_doc()
        .current_dir(tmp.path())
        .env("AGENT_DOC_TEST_RETAIN_STREAM_WRITE_AFTER_RESOLVE", "1")
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--force-disk",
            "--done",
            "rrta",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: #rrta retained closeout — gpt-5\nImplemented and verified.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            agent_doc_document_realtime_io::RETAINED_FOR_RETRY_MARKER,
        ));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("- [ ] [#rrta]") && !content.contains("- [x] [#rrta]"),
        "the retained closeout must not leave an open or completed-unreaped item:\n{content}",
    );
    assert!(
        content.contains("<!-- agent:done -->")
            && content.contains("[#rrta] Archive this retained closeout item"),
        "the retained closeout must archive its completed item in the same mutation:\n{content}",
    );
    assert!(
        !content.contains("### Re: #rrta retained closeout"),
        "the seam must retain the response before visible application:\n{content}",
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
fn write_commit_malformed_response_rejects_before_done_dry_run_diagnostics() {
    let (tmp, doc) = setup_session_stream_doc();
    let original = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n### Re: already handled — gpt-5\nDone.\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n- [ ] [#done1] Close the loop\n<!-- /agent:backlog -->\n\n<!-- agent:queue auto -->\n- do [#done1]\n<!-- /agent:queue -->\n";
    fs::write(&doc, original).unwrap();
    init_git_repo(tmp.path(), &doc);

    let output = agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            "--commit",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "done1",
        ])
        .write_stdin(
            "### Re: done1 — gpt-5\n<!-- patch:exchange -->\nResolved.\n<!-- /patch:exchange -->\n",
        )
        .output()
        .unwrap();

    assert!(!output.status.success(), "malformed closeout must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsafe unmatched content around patch blocks"),
        "response-shape rejection must remain authoritative:\n{stderr}"
    );
    assert!(
        !stderr.contains("[pending] completed and reaped")
            && !stderr.contains("snapshot recovery warning during queue consume"),
        "tracked-work dry-run diagnostics must not precede response rejection:\n{stderr}"
    );
    assert_eq!(fs::read_to_string(&doc).unwrap(), original);
    assert_eq!(head_blob(tmp.path()), original);
}

/// `#snapmissingquiet` (GH #68 §4): a done-id queue consume planned against
/// virtual transaction content has no snapshot by design. That expected skip
/// goes to ops.log only; it must not print a console "snapshot recovery
/// warning" on every consume.
#[test]
fn write_commit_done_queue_consume_does_not_warn_about_expected_missing_snapshot() {
    let (tmp, doc) = setup_session_stream_doc();
    let original = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n### Re: already handled — gpt-5\nDone.\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n- [ ] [#done1] Close the loop\n<!-- /agent:backlog -->\n\n<!-- agent:queue auto -->\n- do [#done1]\n<!-- /agent:queue -->\n";
    fs::write(&doc, original).unwrap();
    init_git_repo(tmp.path(), &doc);

    let output = agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            "--commit",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "done1",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: done1 — gpt-5\n\nResolved.\n<!-- /patch:exchange -->\n",
        )
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "closeout should succeed:\n{stderr}"
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("- do [#done1]"),
        "queue head consumed:\n{content}"
    );
    assert!(
        !stderr.contains("snapshot is missing"),
        "expected missing-snapshot skip must stay off the console:\n{stderr}"
    );
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
fn finalize_late_claimed_response_ignores_unrelated_visible_queue_heads() {
    let (tmp, doc) = setup_session_stream_doc();
    // The claimed heads stay unacknowledged across both finalizes; pin the
    // claim clock so they remain inside the dispatch grace however slow the
    // run is (`#finalgrace`).
    let (claim_clock, _claim_clock) = pin_claim_clock();
    let issue_186 = "#gh-fix https://github.com/btakita/agent-doc/issues/186";
    let issue_187 = "#gh-fix https://github.com/btakita/agent-doc/issues/187";
    let tmux = "Make the tmux pane title contain the document name in all states.";
    let release = "release + publish";
    let original = fs::read_to_string(&doc).unwrap().replace(
        "<!-- agent:backlog -->",
        &format!(
            "<!-- agent:queue go -->\n- 🚧 {tmux}\n- 🚧 {issue_186}\n- 🚧 {release}\n- {issue_187}\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->"
        ),
    );
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), &original);

    // The setup response is unrelated to the queue. Temporarily claim every
    // selected head so the first cycle can commit without consuming the queue.
    for (item, owner) in [
        (tmux, "subagent:tmux"),
        (issue_186, "subagent:issue186"),
        (release, "coordinator:release"),
        (issue_187, "subagent:issue187"),
    ] {
        agent_doc_queue_io::queue_claim::claim(&doc, item, owner, 3600).unwrap();
    }
    agent_doc_at_claim_clock(claim_clock)
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: setup — gpt-5\n\nFirst response.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    agent_doc_queue_io::queue_claim::release(&doc, tmux).unwrap();
    agent_doc_queue_io::queue_claim::release(&doc, release).unwrap();
    let claims_before = agent_doc_queue_io::queue_claim::load_ledger(&doc).unwrap();

    // A worker result can arrive after the setup cycle committed. Its replay
    // reopen has an authoritative empty queue selection; stale visible markers
    // for unrelated work must not be reinterpreted as this response's scope.
    agent_doc_at_claim_clock(claim_clock)
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: GH #186 — gpt-5\n\nFixed and verified issue #186.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let head = head_blob(tmp.path());
    for queue_line in [
        format!("- 🚧 {tmux}"),
        format!("- 🚧 {issue_186}"),
        format!("- 🚧 {release}"),
        format!("- {issue_187}"),
    ] {
        assert!(
            head.contains(&queue_line),
            "late-response admission must leave unrelated queue text untouched:\n{head}"
        );
    }
    assert!(head.contains("### Re: GH #186 — gpt-5"));
    let cycle = agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
        .unwrap()
        .expect("late-response cycle state");
    assert_eq!(
        cycle.queue_selection_authority,
        agent_doc_cycle_state_io::QueueSelectionAuthority::OutOfBandCloseout,
        "the replay reopen must durably own its empty queue selection"
    );
    assert!(cycle.selected_free_text_queue_heads.is_empty());
    assert_eq!(
        agent_doc_queue_io::queue_claim::load_ledger(&doc).unwrap(),
        claims_before,
        "late-response closeout must preserve sibling claims"
    );
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
        // `#closeoutduplicatedcommit`: the refusal terminates — it names the
        // remedies executable in a committed cycle and never sends the agent
        // to preflight-and-resubmit a response HEAD already holds.
        .stderr(predicates::str::contains("already committed in HEAD"))
        .stderr(predicates::str::contains("agent-doc dedupe"))
        .stderr(predicates::str::contains(
            "Do NOT run preflight and resubmit",
        ));

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

/// A response-bearing tracked-work closeout can enter the same committed-cycle
/// compatibility lane as a pending-only write. The foreground lease attempt is
/// made before that lane reopens the cycle, so it must be reacquired against the
/// fresh cycle before the response capture can wake binary-owned recovery.
#[test]
fn response_and_done_reopen_acquires_fresh_cycle_owner_before_capture() {
    let (tmp, doc) = setup_session_stream_doc();
    let original = fs::read_to_string(&doc).unwrap().replace(
        "<!-- agent:backlog -->\n<!-- /agent:backlog -->",
        "<!-- agent:backlog -->\n- [ ] [#done1] Finish retained work\n<!-- /agent:backlog -->",
    );
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);
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

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
            "--done",
            "done1",
            "--no-followups",
        ])
        .write_stdin("<!-- patch:exchange -->\n### Re: second — gpt-5\n\nSecond response.\n<!-- /patch:exchange -->\n")
        .assert()
        .success();

    assert_eq!(
        read_cycle_phase(tmp.path(), &doc).as_deref(),
        Some("committed")
    );
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
        .unwrap()
        .expect("fresh closeout cycle state");
    assert!(
        state.requested_done_ids.contains(&"done1".to_string()),
        "the done witness must belong to the reopened cycle so retained recovery cannot drop the tracked-work half: {state:?}",
    );
    let head = head_blob(tmp.path());
    assert_eq!(
        head.matches("### Re: second — gpt-5").count(),
        1,
        "the response-bearing reopen must commit exactly one response:\n{head}",
    );
    assert!(
        !head.contains("- [ ] [#done1] Finish retained work"),
        "the same closeout must land its tracked-work mutation:\n{head}",
    );

    let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    let reopened = ops_log
        .rfind("baseline_replay_pending_only_reopened")
        .expect("the committed cycle should reopen");
    let after_reopen = &ops_log[reopened..];
    let acquired = after_reopen
        .find("closeout_owner_actor_claim")
        .expect("the fresh cycle should be claimed");
    let capture = after_reopen
        .find("closeout_mutation_plan_captured_before_authority_resolution")
        .expect("the response/mutation envelope should be captured");
    assert!(
        acquired < capture && after_reopen[acquired..capture].contains("outcome=Acquired"),
        "fresh-cycle ownership must be acquired before response capture:\n{after_reopen}",
    );
}

/// `#respondcloseoutbacklog`: a bare-tag `--backlog-add` delivered twice
/// through the `#committedwedge` reopened pending-only cycle is ONE mutation.
///
/// Live 2026-10-11 on `tasks/agent-doc/agent-doc-bugs.md`, the same
/// `--backlog-add "#loadbudgettests [agent-doc] ..."` plan reached the document
/// through two applications (the retained foreground write and the
/// captured-finalize resume, both against a cycle reopened by
/// `baseline_replay_pending_only_reopened`). The second saw `#loadbudgettests`
/// taken and filed `[#wallclockbudget] #loadbudgettests [agent-doc] ...` beside
/// the first, and the same-cycle queue mirror then enqueued both ids. Here the
/// second application is the identical write run again on the reopened
/// committed cycle: it must leave the backlog and the go queue untouched.
#[test]
fn reopened_pending_only_bare_tag_add_applied_twice_lands_exactly_once() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\nqueue: start\n---\n\n<!-- agent:exchange -->\n❯ do [#head]\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue priority go -->\n- do [#head]\n<!-- /agent:queue -->\n\n<!-- agent:backlog priority queue -->\n- [ ] [#head] current queue head\n<!-- /agent:backlog -->\n";
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
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: do [#head] — gpt-5\nDone.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();
    assert_eq!(
        read_cycle_phase(tmp.path(), &doc).as_deref(),
        Some("committed")
    );

    let add = "#loadbudgettests [agent-doc] Wall-clock budget tests fail under concurrent load";
    for application in 1..=2 {
        agent_doc()
            .current_dir(tmp.path())
            .args([
                "finalize",
                doc.to_str().unwrap(),
                "--force-disk",
                "--backlog-add",
                add,
                "--backlog-queue-placement",
                "append",
            ])
            .write_stdin(format!(
                "<!-- patch:exchange -->\n### Re: follow-ups ({application}) — gpt-5\nFiled the load-budget follow-up.\n<!-- /patch:exchange -->\n"
            ))
            .assert()
            .success();
        let head = head_blob(tmp.path());
        assert_eq!(
            head.matches("- [ ] [#loadbudgettests] [agent-doc] Wall-clock budget")
                .count(),
            1,
            "application {application}: the add must be filed exactly once under its given id:\n{head}"
        );
        assert!(
            !head.contains("] #loadbudgettests "),
            "application {application}: the add must never be re-filed under a coined id:\n{head}"
        );
        assert_eq!(
            head.matches("do [#loadbudgettests]").count(),
            1,
            "application {application}: the go queue must hold exactly one head for the add:\n{head}"
        );
    }

    let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("baseline_replay_pending_only_reopened"),
        "both applications must go through the reopened pending-only cycle:\n{ops_log}"
    );
    assert!(
        ops_log.contains("backlog_add_already_satisfied")
            && ops_log.contains("id=#loadbudgettests"),
        "the second application must be recorded as an already-satisfied add:\n{ops_log}"
    );
}

/// GH #378: `--backlog-add` with `--backlog-queue-placement` on a CRDT
/// template document whose runnable component is a heading-anchored go
/// `agent:run` with a heading that does not name the tag, beside a
/// heading-anchored `agent:loop` above it and with no `agent:queue` line at
/// all. The add lands in the backlog and mirrors into `agent:run` under both
/// placements; the loop never receives the id.
///
/// Before the fix the same-cycle mirror read its go-mode gate and its
/// pre/post id diff from the FIRST `queue` component, which is the loop: the
/// loop carries no `go`, so the backlog row landed and the mirror silently
/// did nothing (standalone openers fail the same way; the heading is not the
/// cause).
#[test]
fn backlog_add_mirrors_into_heading_anchored_run_beside_a_loop() {
    for placement in ["prepend", "append"] {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
        let doc = tmp.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test-session\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nagent: codex\nmodel: gpt-5\nqueue: start\n---\n\n",
            "<!-- agent:exchange -->\n❯ do [#head]\n<!-- agent:boundary:1234abcd -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Issues <!-- agent:loop id=issues interval=1h -->\n",
            "- poll issues\n",
            "<!-- /agent:loop -->\n\n",
            "## Work items <!-- agent:run priority subagents clear go -->\n",
            "- do [#head]\n",
            "<!-- /agent:run -->\n\n",
            "## Backlog <!-- agent:backlog priority queue -->\n",
            "- [ ] [#head] current queue head\n",
            "<!-- /agent:backlog -->\n",
        );
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
                "--backlog-add",
                "#followup record the follow-up",
                "--backlog-queue-placement",
                placement,
            ])
            .write_stdin(
                "<!-- patch:exchange -->\n### Re: do [#head] — gpt-5\nFiled the follow-up.\n<!-- /patch:exchange -->\n",
            )
            .assert()
            .success();

        let head = head_blob(tmp.path());
        assert!(
            head.contains("- [ ] [#followup] record the follow-up"),
            "{placement}: the backlog add must land:\n{head}"
        );
        let loop_start = head.find("## Issues <!-- agent:loop").unwrap();
        let loop_end = head.find("<!-- /agent:loop -->").unwrap();
        assert!(
            !head[loop_start..loop_end].contains("followup"),
            "{placement}: the loop must never receive a mirrored id:\n{head}"
        );
        let run_start = head
            .find("## Work items <!-- agent:run")
            .expect("the heading-anchored run opener survives");
        let run_end = head.find("<!-- /agent:run -->").unwrap();
        let run = &head[run_start..run_end];
        assert_eq!(
            run.matches("do [#followup]").count(),
            1,
            "{placement}: the mirrored id must land in `agent:run` exactly once:\n{head}"
        );
        let followup_at = run.find("do [#followup]").unwrap();
        let head_at = run.find("do [#head]").unwrap();
        match placement {
            "prepend" => assert!(followup_at < head_at, "prepend: {run}"),
            _ => assert!(followup_at > head_at, "append: {run}"),
        }
    }
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
    assert!(content.contains("## Done"));
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
fn finalize_reports_concurrent_prompt_added_after_baseline_as_pending_steering() {
    // `#steerinterruptexit`: a prompt the operator added while the turn ran is
    // pending steering, not a failed closeout. Finalize commits the response,
    // exits 0, and its terminal report names the steering verbatim; the late
    // prompt stays outside the committed snapshot for the next cycle.
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
        .success()
        .stderr(predicates::str::contains("[session-check] steering pending:"))
        .stderr(predicates::str::contains("[session-check] INTERRUPTED").not())
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
        .success()
        .stdout(predicates::str::contains(
            "[session-check] steering pending:",
        ))
        .stdout(predicates::str::contains("dispatch=address_now"))
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
        !content.contains("\nqueue: "),
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
fn finalize_consumes_resolved_preset_from_expansion_without_literal_quote() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "agent: codex\n",
        "model: gpt-5\n",
        "queue_active: true\n",
        "prompt_presets:\n",
        "  '#actionable-review': Add actionable review items into backlog + queue\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "### Re: older\nOld response.\n",
        "<!-- agent:boundary:1234abcd -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- #actionable-review\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n",
    );
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), content);

    let response = concat!(
        "<!-- patch:exchange -->\n",
        "### Re: actionable review — gpt-5\n\n",
        "Add actionable review items into backlog + queue. Added three items.\n",
        "<!-- /patch:exchange -->\n",
    );
    assert!(
        !response.contains("> **Queue prompt:**"),
        "the regression requires completion without a literal queue-prompt quote"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(response)
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
        !queue_section.contains("#actionable-review"),
        "the resolved preset head should drain after its expansion is answered:\n{content}"
    );
    assert!(
        !content.contains("queue_active: true"),
        "draining the only preset head should deactivate the queue:\n{content}"
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
        !content.contains("\nqueue: "),
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
fn finalize_keeps_deferred_leading_free_text_queue_head() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n",
        "<!-- agent:exchange -->\n### Re: older\nOld response.\n",
        "<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue preset=\"#subagents #auth\" priority go -->\n",
        "- #upgrade\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
    );
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--force-disk",
        ])
        .write_stdin(concat!(
            "<!-- patch:exchange -->\n",
            "### Re: #upgrade to 0.35.471 — gpt-5\n\n",
            "> **Queue prompt:** #upgrade\n\n",
            "**Deferred:** claimed as `subagent:upgrade471`. It starts after this response commits.\n",
            "<!-- /patch:exchange -->\n",
        ))
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("\n- #upgrade\n"),
        "the deferred leading head must remain live after closeout:\n{content}"
    );
    assert!(
        !content.contains("\n- ~~#upgrade~~\n"),
        "the deferred leading head must not be struck:\n{content}"
    );
    assert!(
        content.contains("queue_active: true"),
        "a queue retaining its only head must stay active:\n{content}"
    );
}

/// GH #227 (operator decision): `preset=""` on the queue marker means
/// explicitly no preset. A write closeout must pass the lint gate without
/// `--lint off` and leave the operator's marker byte-identical.
#[test]
fn finalize_preserves_explicit_empty_preset_marker_without_lint_override() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let marker = "<!-- agent:queue subagents preset=\"\" priority go -->\n";
    let content = format!(
        "---\nagent_doc_format: template\nagent: codex\nmodel: gpt-5\nqueue_active: true\n---\n\n\
         <!-- agent:exchange -->\n### Re: older\nOld response.\n\
         <!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n\
         {marker}\
         - #upgrade\n\
         <!-- /agent:queue -->\n\n\
         <!-- agent:backlog -->\n<!-- /agent:backlog -->\n"
    );
    fs::write(&doc, &content).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), &content);

    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin(concat!(
            "<!-- patch:exchange -->\n",
            "### Re: #upgrade — gpt-5\n\n",
            "> **Queue prompt:** #upgrade\n\n",
            "**Deferred:** claimed as `subagent:upgrade`. It starts after this response commits.\n",
            "<!-- /patch:exchange -->\n",
        ))
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains(marker),
        "the explicit empty preset must survive the write byte-identical:\n{content}"
    );
    assert!(content.contains("### Re: #upgrade — gpt-5"), "{content}");
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
        !content.contains("\nqueue: "),
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
        !content.contains("\nqueue: "),
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
        !content.contains("\nqueue: "),
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
        !content.contains("\nqueue: "),
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

/// `#donequeuestrike`: a `--done` whose reap is deferred still owns the queue
/// strike.
///
/// Observed live 2026-09-10: a closeout whose response write was retained took
/// the `reap_done_in_same_write == false` branch, which transitioned the backlog
/// item to `[x]` and never touched `agent:queue`. Nothing failed — session-check
/// reported ok/committed — while the operator-visible queue still served three
/// completed items to the next drain. `write --pending-only` without `--commit`
/// reaches the same branch deterministically.
#[test]
fn pending_only_done_without_commit_strikes_its_queue_head() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let baseline_content = "---\nagent_doc_format: template\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n\u{276f} start\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do [#dqsa]\n- do [#dqsb]\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n- [ ] [#dqsa] first tracked item\n- [ ] [#dqsb] second tracked item\n<!-- /agent:backlog -->\n";
    fs::write(&doc, baseline_content).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-only",
            "--done",
            "dqsa",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("- do [#dqsa]"),
        "the completed id must not remain a live queue prompt:\n{content}"
    );
    assert!(
        content.contains("- ~~do [#dqsa]~~"),
        "the completed id's queue head must be struck:\n{content}"
    );
    assert!(
        content.contains("- do [#dqsb]"),
        "an unrelated queue head must stay live:\n{content}"
    );
    assert!(
        content.contains("[#dqsa]") && !content.contains("- [ ] [#dqsa]"),
        "the backlog item must be transitioned out of the open state:\n{content}"
    );
}

/// A gate is a terminal queue resolution just like `--done`. Both transitions
/// must land in the tracked-work projection before an editor-retained response
/// can defer later closeout stages.
#[test]
fn pending_only_done_and_gate_drain_the_queue_atomically() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let baseline_content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "queue: start\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "\u{276f} start\n",
        "<!-- agent:boundary:1234abcd -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue priority start -->\n",
        "- do [#implementhttpshaiven]\n",
        "- do [#havn1361contracts]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#implementhttpshaiven] implement HTTP integration\n",
        "- [ ] [#havn1361contracts] publish contracts\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:review -->\n",
        "<!-- /agent:review -->\n",
    );
    fs::write(&doc, baseline_content).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), baseline_content);

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            doc.to_str().unwrap(),
            "--force-disk",
            "--pending-only",
            "--done",
            "implementhttpshaiven",
            "--backlog-edit",
            "havn1361contracts=publish contracts after operator approval",
            "--backlog-gate",
            "havn1361contracts",
        ])
        .assert()
        .success()
        .stderr(predicates::str::contains("[queue] drained"));

    let content = fs::read_to_string(&doc).unwrap();
    assert!(!content.contains("\nqueue: "), "{content}");
    assert!(
        content.contains("<!-- agent:queue priority -->\n<!-- /agent:queue -->"),
        "{content}"
    );
    assert!(
        !content.contains("- do [#implementhttpshaiven]"),
        "{content}"
    );
    assert!(!content.contains("- do [#havn1361contracts]"), "{content}");
    assert!(
        content.contains("[#havn1361contracts]")
            && content.contains("publish contracts after operator approval"),
        "{content}"
    );
    let backlog_body = content
        .split_once("<!-- agent:backlog -->")
        .and_then(|(_, tail)| tail.split_once("<!-- /agent:backlog -->"))
        .map(|(body, _)| body)
        .unwrap();
    let review_body = content
        .split_once("<!-- agent:review -->")
        .and_then(|(_, tail)| tail.split_once("<!-- /agent:review -->"))
        .map(|(body, _)| body)
        .unwrap();
    assert!(!backlog_body.contains("havn1361contracts"), "{content}");
    assert!(review_body.contains("havn1361contracts"), "{content}");
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

/// `#prmergeguardpr`: closeout is one transaction. A tracked-work mutation the
/// envelope will reject must fail the turn BEFORE the response cell is
/// published — otherwise the operator reads a durable response claiming an item
/// is done beside a backlog that still shows it open, and only a manual
/// owning-pane commit can finish the half-applied cycle.
#[test]
fn finalize_rejecting_a_tracked_work_mutation_never_publishes_the_response() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n- [ ] [#gscaccess] existing tracked work\n<!-- /agent:backlog -->\n",
    )
    .unwrap();
    init_git_repo(tmp.path(), &doc);

    let original = fs::read_to_string(&doc).unwrap();
    checkpoint_baseline(tmp.path(), &original);

    // `[#gscaccess]` collides with the active backlog id, so the add is
    // rejectable — exactly the shape that used to strand a durable response.
    agent_doc()
        .current_dir(tmp.path())
        .args([
            "finalize",
            doc.to_str().unwrap(),
            "--stream",
            "--origin",
            "skill",
            "--done",
            "gscaccess",
            "--backlog-add",
            "[#gscaccess] duplicate id",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: split — gpt-5\n\nHalf-applied body.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .failure();

    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        !content.contains("### Re: split — gpt-5"),
        "a rejected mutation envelope must not leave the response on disk:\n{content}"
    );
    assert!(
        content.contains("- [ ] [#gscaccess] existing tracked work"),
        "the existing backlog item must stay open and unmodified:\n{content}"
    );
    assert!(
        !content.contains("duplicate id"),
        "the rejected add must not land:\n{content}"
    );

    let head_text = head_blob(tmp.path());
    assert!(
        !head_text.contains("### Re: split — gpt-5"),
        "HEAD must not carry the response of a rejected cycle:\n{head_text}"
    );
}

/// The pre-write validation pass is a dry run: a valid envelope still applies
/// exactly once, and the response and its mutations land together.
#[test]
fn finalize_valid_tracked_work_mutations_still_apply_exactly_once() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    fs::write(
        &doc,
        "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n- [ ] [#gscaccess] existing tracked work\n<!-- /agent:backlog -->\n",
    )
    .unwrap();
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
            "--done",
            "gscaccess",
            "--backlog-add",
            "id=followup a genuinely new item",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: whole — gpt-5\n\nWhole body.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let head_text = head_blob(tmp.path());
    assert_eq!(
        head_text.matches("### Re: whole — gpt-5").count(),
        1,
        "the response must land exactly once:\n{head_text}"
    );
    assert_eq!(
        head_text.matches("a genuinely new item").count(),
        1,
        "the dry run must not double-apply the add:\n{head_text}"
    );
    assert!(
        !head_text.contains("- [ ] [#gscaccess] existing tracked work"),
        "the --done item must not remain open:\n{head_text}"
    );
}

/// GH #64: on a project scaffolded by `init`, the very first `write --commit`
/// must reach a commit. It used to write a full-UUID boundary marker that the
/// bundled default lint dialect rejects, so the lint gate interrupted closeout.
#[test]
fn init_then_first_write_commit_commits_with_a_lintable_boundary() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    for args in [
        vec!["init", "-q", "."],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test User"],
    ] {
        assert!(
            ProcessCommand::new("git")
                .current_dir(root)
                .args(&args)
                .status()
                .unwrap()
                .success()
        );
    }
    agent_doc()
        .current_dir(root)
        .args(["init", "scratch.md"])
        .assert()
        .success();
    let commit_all = |message: &str| {
        assert!(
            ProcessCommand::new("git")
                .current_dir(root)
                .args(["add", "-A"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            ProcessCommand::new("git")
                .current_dir(root)
                .args(["commit", "-qm", message, "--no-verify"])
                .status()
                .unwrap()
                .success()
        );
    };
    commit_all("init");
    let doc = root.join("scratch.md");
    let mut content = fs::read_to_string(&doc).unwrap();
    content.push_str("test prompt\n");
    fs::write(&doc, content).unwrap();
    commit_all("prompt");

    agent_doc()
        .current_dir(root)
        .args(["write", "--commit", "scratch.md", "--origin", "skill"])
        .write_stdin("<!-- patch:exchange -->\nPlain response.\n<!-- /patch:exchange -->\n")
        .assert()
        .success();

    let written = fs::read_to_string(&doc).unwrap();
    let boundary_ids: Vec<&str> = written
        .lines()
        .filter_map(|line| line.trim().strip_prefix("<!-- agent:boundary:"))
        .filter_map(|rest| rest.strip_suffix(" -->"))
        .collect();
    assert!(!boundary_ids.is_empty(), "{written}");
    for id in boundary_ids {
        let hex = id.split(':').next().unwrap();
        assert!(
            hex.len() == 8 && hex.chars().all(|c| c.is_ascii_hexdigit()),
            "boundary id must be the short <hex> form, got `{id}`:\n{written}"
        );
    }
    let log = ProcessCommand::new("git")
        .current_dir(root)
        .args(["log", "--oneline"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).lines().count(),
        3,
        "the first write --commit must add a closeout commit"
    );
}

/// `#unstrikelost`: the operator un-strikes a struck free-text queue head while a
/// cycle is writing its response. The response neither quotes nor answers the
/// head, so the closeout must keep the operator's re-arm live in both the
/// materialized document and the commit.
fn unstrike_then_response_closeout_case(disk_holds_operator_cut: bool) {
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
        "❯ Dispatch the open heads\n",
        "<!-- agent:boundary:1234abcd -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- do [#stillopen]\n",
        "- ~~release + publish~~\n",
        "<!-- /agent:queue -->\n",
    );
    fs::write(&doc, base).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), base);
    let struck_at = base.find("~~release + publish~~").unwrap();
    let close_at = struck_at + "~~release + publish".len();
    agent_doc_op_capture_io::record_editor_ops(
        &doc,
        &content_hash(base),
        vec![
            agent_doc_merge::crdt::EditorOp::Delete {
                offset: close_at,
                len: 2,
            },
            agent_doc_merge::crdt::EditorOp::Delete {
                offset: struck_at,
                len: 2,
            },
        ],
    )
    .unwrap();
    if disk_holds_operator_cut {
        fs::write(
            &doc,
            base.replace("~~release + publish~~", "release + publish"),
        )
        .unwrap();
    }

    agent_doc()
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap(), "--stream"])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: dispatch — gpt-5\n\n> **Queue prompt:** do [#stillopen]\n\nDispatched to a subagent.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();

    let content = fs::read_to_string(&doc).unwrap();
    let head = head_blob(tmp.path());
    for (label, materialized) in [("document", &content), ("HEAD", &head)] {
        assert!(
            materialized.contains("Dispatched to a subagent."),
            "{label}:\n{materialized}"
        );
        assert!(
            materialized.contains("- release + publish\n")
                && !materialized.contains("~~release + publish~~"),
            "{label} re-struck the operator's unstrike:\n{materialized}"
        );
    }
}

#[test]
fn unstrike_then_response_closeout_without_quote_keeps_head_live_editor_absent() {
    unstrike_then_response_closeout_case(false);
}

#[test]
fn unstrike_then_response_closeout_without_quote_keeps_head_live_operator_saved() {
    unstrike_then_response_closeout_case(true);
}

// `#respondfinalizehard` (agent-doc-bugs.md, cycle-1791675315432, 2026-10-10):
// `respond --done <id>` landed its response, then hard-failed
// `queue consume: no prompt to consume` because every queue head was already
// struck and `<id>` had already moved to `agent:done`. The recovery
// `write --commit --done <id> --pending-only` then failed again with
// `failed to compose completed tracked-work target`. Consuming an
// already-consumed head (or `--done` on an already-done id) is an idempotent
// already-satisfied no-op; only a never-existing id still fails closed.
fn all_struck_queue_already_done_doc_content() -> String {
    "---\nagent_doc_session: test-session\nagent_doc_format: template\nagent_doc_write: crdt\nqueue: go\nqueue_active: true\nagent: codex\nmodel: gpt-5\n---\n\n<!-- agent:exchange -->\n❯ #gt-fix https://github.com/btakita/agent-doc/issues/234\n<!-- agent:boundary:1234abcd -->\n<!-- /agent:exchange -->\n\n<!-- agent:backlog queue -->\n- [ ] [#other1] Unrelated open item\n<!-- /agent:backlog -->\n\n<!-- agent:queue -->\n- ~~#gt-fix https://github.com/btakita/agent-doc/issues/234~~\n- ~~do [#done1]~~\n<!-- /agent:queue -->\n\n<!-- agent:done -->\n- [x] [#done1] Close the loop\n<!-- /agent:done -->\n".to_string()
}

const ALREADY_STRUCK_RESPONSE: &str = "<!-- patch:exchange -->\n### Re: gt-fix 234 — gpt-5\n> **Queue prompt:** #gt-fix https://github.com/btakita/agent-doc/issues/234\n\nAlready handled.\n<!-- /patch:exchange -->\n";

#[test]
fn respond_done_on_already_done_id_with_all_struck_queue_commits() {
    let (tmp, doc) = setup_session_stream_doc();
    let original = all_struck_queue_already_done_doc_content();
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);

    let output = agent_doc()
        .current_dir(tmp.path())
        .args(["respond", doc.to_str().unwrap(), "--done", "done1"])
        .write_stdin(ALREADY_STRUCK_RESPONSE)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "respond --done on an already-done id with an all-struck queue must commit:\n{stderr}"
    );
    assert!(
        !stderr.contains("no prompt to consume"),
        "an already-consumed head must not be reported as a missing prompt:\n{stderr}"
    );

    let head = head_blob(tmp.path());
    assert!(
        head.contains("### Re: gt-fix 234 — gpt-5") && head.contains("Already handled."),
        "the response must be committed, not wedged at write_applied:\n{head}"
    );
    assert!(
        head.contains("- ~~#gt-fix https://github.com/btakita/agent-doc/issues/234~~")
            && head.contains("- ~~do [#done1]~~"),
        "already-struck heads stay struck exactly once:\n{head}"
    );
    assert_eq!(
        head.matches("[#done1] Close the loop").count(),
        1,
        "an already-done id must not be archived twice:\n{head}"
    );
    assert!(
        head.contains("- [ ] [#other1] Unrelated open item"),
        "an unrelated open item must be untouched:\n{head}"
    );
    let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("queue_consume_drained_noop"),
        "the drained queue must be recorded as already satisfied:\n{ops_log}"
    );
}

#[test]
fn write_commit_done_on_already_done_id_with_all_struck_queue_commits() {
    let (tmp, doc) = setup_session_stream_doc();
    fs::write(&doc, all_struck_queue_already_done_doc_content()).unwrap();
    init_git_repo(tmp.path(), &doc);

    let output = agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            "--commit",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "done1",
        ])
        .write_stdin(ALREADY_STRUCK_RESPONSE)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "write --commit --done on an already-done id with an all-struck queue must commit:\n{stderr}"
    );
    assert!(
        !stderr.contains("failed to compose completed tracked-work target"),
        "the tracked-work composer must treat an already-done id as satisfied:\n{stderr}"
    );
    let head = head_blob(tmp.path());
    assert!(
        head.contains("Already handled."),
        "the response must be committed:\n{head}"
    );
    assert_eq!(head.matches("[#done1] Close the loop").count(), 1, "{head}");
}

#[test]
fn write_commit_pending_only_done_on_already_done_id_with_all_struck_queue_commits() {
    let (tmp, doc) = setup_session_stream_doc();
    // The incident's recovery shape: the response already landed (the cycle
    // wedged at `write_applied`), so only the tracked-work closeout remains.
    let original = all_struck_queue_already_done_doc_content().replace(
        "<!-- agent:boundary:1234abcd -->",
        "### Re: gt-fix 234 — gpt-5\n> **Queue prompt:** #gt-fix https://github.com/btakita/agent-doc/issues/234\n\nAlready handled.\n<!-- agent:boundary:1234abcd -->",
    );
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);

    let output = agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            "--commit",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "done1",
            "--pending-only",
        ])
        .write_stdin("")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "pending-only --done on an already-done id must be an idempotent no-op:\n{stderr}"
    );
    assert!(
        !stderr.contains("no prompt to consume")
            && !stderr.contains("failed to compose completed tracked-work target"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        original,
        "an already-satisfied closeout must not change the document"
    );
}

#[test]
fn write_commit_done_on_never_existing_id_with_all_struck_queue_fails_closed() {
    let (tmp, doc) = setup_session_stream_doc();
    let original = all_struck_queue_already_done_doc_content();
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);

    let output = agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            "--commit",
            doc.to_str().unwrap(),
            "--force-disk",
            "--done",
            "doen1",
            "--pending-only",
        ])
        .write_stdin("")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "a never-existing (typo) id must still fail closed:\n{stderr}"
    );
    assert!(
        stderr.contains("doen1"),
        "the failure must name the unknown id:\n{stderr}"
    );
    assert!(
        !stderr.contains("no prompt to consume"),
        "an unknown id must not be misreported as a queue consume failure:\n{stderr}"
    );
    assert_eq!(fs::read_to_string(&doc).unwrap(), original);
}

/// `#keptopenabsorbed` / `#editnorequeue` (haiven-dev tasks/sdk.md,
/// 2026-10-11): `--backlog-edit` is the keep-open flag. On an id whose queue
/// head is already struck (consumed), it narrows the backlog item and must not
/// bring the head back — only an explicit `backlog reopen --queue` /
/// `backlog requeue` re-arms a struck head. A go-mode queue with a
/// `queue`-attr backlog is the shape that made operator-gated items the driver
/// again; the following preflight's mirror must not resurrect it either.
#[test]
fn backlog_edit_on_a_struck_head_id_does_not_requeue_it() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
    let doc = tmp.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: editnorequeue\n",
        "agent_doc_format: template\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "### Re: earlier\n\n",
        "Done.\n",
        "<!-- agent:boundary:1234abcd -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue subagents priority go -->\n",
        "- ~~do [#quirkbigint]~~\n",
        "- do [#other]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog priority queue -->\n",
        "- [ ] [#quirkbigint] [blocked: #other] decoder half\n",
        "- [ ] [#other] other work\n",
        "<!-- /agent:backlog -->\n",
    );
    fs::write(&doc, content).unwrap();
    init_git_repo(tmp.path(), &doc);

    let queue_of = |doc: &Path| {
        let text = fs::read_to_string(doc).unwrap();
        let (_, tail) = text.split_once("<!-- agent:queue").unwrap();
        tail.split_once("<!-- /agent:queue -->")
            .unwrap()
            .0
            .to_string()
    };
    let assert_not_requeued = |stage: &str| {
        let queue = queue_of(&doc);
        assert!(
            queue.contains("~~do [#quirkbigint]~~"),
            "{stage}: the struck head stays struck:\n{queue}"
        );
        assert_eq!(
            queue.matches("[#quirkbigint]").count(),
            1,
            "{stage}: no live head was re-added for the kept-open id:\n{queue}"
        );
    };

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    assert_not_requeued("preflight");

    agent_doc()
        .current_dir(tmp.path())
        .args([
            "write",
            "--commit",
            doc.to_str().unwrap(),
            "--backlog-edit",
            "quirkbigint=[blocked: #other] decoder half; kept open 2026-10-11",
            // The cycle's own directive is the live `do [#other]` head. Keep it
            // open explicitly: before GH #275 the strict closeout reported this
            // test's expect-done refusal as "superseded" and exited 0 while the
            // committed document failed every later session-check.
            "--backlog-edit",
            "other=other work; kept open 2026-10-11",
        ])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: kept\n\nKept open.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();
    assert_not_requeued("write --commit --backlog-edit");
    assert!(
        fs::read_to_string(&doc)
            .unwrap()
            .contains("decoder half; kept open 2026-10-11"),
        "the edit landed"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    assert_not_requeued("next preflight");
}

/// Any-heading prefix: the live 2026-10-26 `tasks/agent-doc/agent-doc-bugs.md`
/// shape. The run component's opener shared a line with `## Queue: Main`, a
/// heading whose text does not match `agent:run`; the editor's
/// template-structure guard refused every save of it 363 times
/// (`non_standalone_component_marker:agent:run:open:line836`). Any ATX heading
/// now prefixes an opener, so the whole cycle (preflight, the template guard
/// on write, the merge, the commit, and session-check) runs on that line
/// unchanged.
#[test]
fn respond_closes_out_a_document_whose_run_opener_shares_a_nonmatching_heading() {
    let (tmp, doc) = setup_session_stream_doc();
    let opener = "## Queue: Main <!-- agent:run id=\"main\" subagents preset=\"#spec-test-build-install-commit-push\" priority go -->";
    let with_queue = session_stream_doc_content().replace(
        "<!-- agent:backlog -->",
        &format!(
            "{opener}\n- operator head left for a later cycle\n<!-- /agent:run -->\n\n<!-- agent:backlog -->"
        ),
    );
    let committed = with_queue.replace("❯ Please reply\n", "");
    fs::write(&doc, &committed).unwrap();
    init_git_repo(tmp.path(), &doc);
    fs::write(snapshot_path(tmp.path(), &doc), &committed).unwrap();
    fs::write(&doc, &with_queue).unwrap();

    let read = agent_doc()
        .current_dir(tmp.path())
        .args(["read", doc.to_str().unwrap(), "--component", "queue"])
        .output()
        .unwrap();
    assert!(
        read.status.success(),
        "read --component queue: {}",
        String::from_utf8_lossy(&read.stderr)
    );
    let queue_body = String::from_utf8_lossy(&read.stdout);
    assert!(
        queue_body.contains("operator head left for a later cycle")
            && !queue_body.contains("Queue: Main"),
        "the heading text is outside the component body:\n{queue_body}"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();

    let output = agent_doc()
        .current_dir(tmp.path())
        .env("AGENT_DOC_SESSION_CHECK_SETTLE_SECS", "1")
        .args(["respond", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin("<!-- patch:exchange -->\n### Re: reply — gpt-5\n\nReplied.\n<!-- /patch:exchange -->\n")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "respond must close out a heading-prefixed opener:\n{stderr}"
    );
    assert!(
        !stderr.contains("non_standalone_component_marker"),
        "the template-structure guard must accept the opener:\n{stderr}"
    );

    let head = head_blob(tmp.path());
    assert!(head.contains("Replied."), "{head}");
    assert!(
        head.contains(&format!("{opener}\n")),
        "the heading-prefixed opener is committed unchanged:\n{head}"
    );
    assert_eq!(
        fs::read_to_string(&doc).unwrap().matches(opener).count(),
        1,
        "the opener is neither duplicated nor split"
    );

    agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .assert()
        .success();
}

/// GH #275: the strict closeout commits BEFORE its terminal session check. When
/// that check interrupts deterministically on the committed bytes (here a
/// drifted open backlog row parked in an HTML comment, the shadow-backlog guard
/// `--todo-add` rows used to trip), the cycle reads `committed`, and `respond`
/// used to report "an earlier transient closeout error was superseded and needs
/// no action" and exit 0 while every later `session-check` failed. A committed
/// cycle supersedes the error only if a fresh session check is clean.
#[test]
fn respond_does_not_supersede_a_session_check_refusal_that_survives_its_commit() {
    let (tmp, doc) = setup_session_stream_doc();
    let committed = session_stream_doc_content().replace("❯ Please reply\n", "");
    fs::write(&doc, &committed).unwrap();
    init_git_repo(tmp.path(), &doc);
    fs::write(snapshot_path(tmp.path(), &doc), &committed).unwrap();
    fs::write(&doc, session_stream_doc_content()).unwrap();

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    assert_eq!(
        read_cycle_phase(tmp.path(), &doc).as_deref(),
        Some("preflight_started")
    );

    // A drifted backlog row lands outside every component after preflight
    // admitted the turn, so only the closeout's terminal session check sees it.
    let mut drifted = fs::read_to_string(&doc).unwrap();
    drifted.push_str("\n<!-- parked copy\n- [ ] [#lost1] Drifted out of backlog\n-->\n");
    fs::write(&doc, &drifted).unwrap();

    let output = agent_doc()
        .current_dir(tmp.path())
        .env("AGENT_DOC_SESSION_CHECK_SETTLE_SECS", "1")
        .args(["respond", doc.to_str().unwrap(), "--force-disk"])
        .write_stdin("<!-- patch:exchange -->\n### Re: reply — gpt-5\n\nReplied.\n<!-- /patch:exchange -->\n")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let ops = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();

    assert!(
        head_blob(tmp.path()).contains("Replied."),
        "precondition: the closeout committed the response before its session check:\n{stderr}"
    );
    assert!(
        !output.status.success(),
        "a session-check refusal that survives the commit must fail respond:\n{stderr}"
    );
    assert!(
        !stderr.contains("needs no action")
            && !ops.contains("strict_closeout_error_settled_committed"),
        "the refusal must not be reported as superseded:\n{stderr}\n{ops}"
    );
    assert!(
        stderr.contains("outside live agent:backlog") && stderr.contains("#lost1"),
        "respond must name the live refusal:\n{stderr}"
    );

    let check = agent_doc()
        .current_dir(tmp.path())
        .args(["session-check", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !check.status.success(),
        "the refusal respond reported is the one session-check still reports"
    );
}

/// `#reportedrelease`: src/haiven-dev/tasks/docs.md, 2026-10-15. The
/// operator added a free-text queue question after the coordinator's cycle had
/// committed; the coordinator claimed it for a subagent and released the claim
/// when the worker reported. `queue release` must hand back the exact
/// persist-now closeout (the `> **Queue prompt:**` echo), and a late finalize
/// (auto-reopened from a HEAD that predates the line) carrying that echo must
/// strike the line and end its release receipt, so nothing re-serves or
/// re-dispatches the answered question.
#[test]
fn finalize_strikes_a_mid_turn_head_whose_reported_worker_was_released() {
    let (tmp, doc) = setup_session_stream_doc();
    // The test crosses two real CLI closeouts while nextest is running the
    // rest of the suite. Keep every in-process and subprocess claim decision
    // on one logical instant so scheduler load cannot age either attempt into
    // a different lifecycle branch mid-scenario (`#finalgrace`).
    let (claim_clock, _claim_clock) = pin_claim_clock();
    let setup = "Setup head claimed for a worker.";
    let question = "Does the page status enum have an archived value? What are the values?";
    let original = fs::read_to_string(&doc).unwrap().replace(
        "<!-- agent:backlog -->",
        &format!(
            "<!-- agent:queue go -->\n- {setup}\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->"
        ),
    );
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);
    checkpoint_baseline(tmp.path(), &original);
    // The unrelated setup head is claimed AND worker-bound: a claim left
    // dispatching (`🟡`) with no worker acknowledgement becomes a coordinator
    // obligation after `DISPATCH_EVIDENCE_GRACE_SECS` (10s), and finalize then
    // refuses closeout over it. Left unacknowledged, this test passed only when
    // it finished within 10s of wall clock, and failed under a loaded
    // `make check-fast`.
    let (_, setup_claim) =
        agent_doc_queue_io::queue_claim::claim(&doc, setup, "subagent:setup", 3600).unwrap();
    agent_doc_queue_io::queue_claim::start_attempt(
        &doc,
        setup,
        "subagent:setup",
        "worker:setup",
        3600,
        setup_claim.generation,
    )
    .unwrap();
    agent_doc_at_claim_clock(claim_clock)
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap(), "--stream", "--origin", "skill"])
        .write_stdin(
            "<!-- patch:exchange -->\n### Re: setup — gpt-5\n\nFirst response.\n<!-- /patch:exchange -->\n",
        )
        .assert()
        .success();
    assert!(!head_blob(tmp.path()).contains(question));

    // The operator adds the question after the commit; the coordinator claims
    // it for a worker, and releases the claim when the worker reports. A fresh
    // claim is dispatching (`🟡`); only a generation-matched refresh/start
    // receipt can truthfully advance it to running (`🚧`).
    let current = fs::read_to_string(&doc).unwrap().replace(
        &format!("- {setup}\n<!-- /agent:queue -->"),
        &format!("- {setup}\n- {question}\n<!-- /agent:queue -->"),
    );
    fs::write(&doc, &current).unwrap();
    agent_doc_at_claim_clock(claim_clock)
        .current_dir(tmp.path())
        .args([
            "queue",
            "claim",
            doc.to_str().unwrap(),
            "--item",
            question,
            "--owner",
            "subagent:page-status-enum",
        ])
        .assert()
        .success();
    let dispatching_projection = fs::read_to_string(&doc).unwrap();
    assert!(
        dispatching_projection.contains(&format!(
            "- 🟡 [#subagent:page-status-enum#1] {question}\n"
        )) && dispatching_projection.contains(&format!(
            "- 🚧 [#subagent:setup#1] {setup}\n"
        )),
        "a fresh claim projects dispatching beside a worker-bound row:\n{dispatching_projection}"
    );
    agent_doc_at_claim_clock(claim_clock)
        .current_dir(tmp.path())
        .args([
            "queue",
            "start",
            doc.to_str().unwrap(),
            "--item",
            question,
            "--owner",
            "subagent:page-status-enum",
            "--generation",
            "1",
            "--worker-id",
            "worker:page-status-enum",
        ])
        .assert()
        .success();
    let claimed_projection = fs::read_to_string(&doc).unwrap();
    assert!(
        claimed_projection.contains(&format!(
            "- 🚧 [#subagent:setup#1] {setup}\n"
        )) && claimed_projection.contains(&format!(
            "- 🚧 [#subagent:page-status-enum#1] {question}\n"
        )),
        "start advances only its own generation to worker-bound:\n{claimed_projection}"
    );
    let released = agent_doc_at_claim_clock(claim_clock)
        .current_dir(tmp.path())
        .args([
            "queue",
            "release",
            doc.to_str().unwrap(),
            "--item",
            question,
            "--generation",
            "1",
        ])
        .assert()
        .success();
    let released = String::from_utf8_lossy(&released.get_output().stderr).into_owned();
    assert!(
        released.contains("subagent:page-status-enum already reported")
            && released.contains(&format!("`> **Queue prompt:** {question}`"))
            && released.contains("Do NOT dispatch a new subagent"),
        "release must hand back the persist-now closeout:\n{released}"
    );
    let released_projection = fs::read_to_string(&doc).unwrap();
    assert!(
        released_projection.contains(&format!(
            "- 🚧 [#subagent:setup#1] {setup}\n"
        )) && released_projection.contains(&format!(
            "- ☑️ [#subagent:page-status-enum#1] {question}\n"
        ))
            && !released_projection.contains(&format!("- 🟡 {question}\n")),
        "reported release must retain a distinct awaiting-integration marker:\n{released_projection}"
    );
    assert_eq!(
        agent_doc_queue_io::claim_release_receipt::load(&doc)
            .unwrap()
            .receipts
            .len(),
        1,
        "a reported release records its receipt"
    );

    // Force the ordering that failed under the full release suite: closeout
    // observes the pre-question turn baseline after the worker claim has been
    // released. The exact-attempt receipt, not projection timing or the ☑️
    // marker, must authorize the exact echoed answer.
    checkpoint_baseline(tmp.path(), &original);
    let receipt = agent_doc_queue_io::claim_release_receipt::load(&doc)
        .unwrap()
        .receipts
        .remove(0);
    assert_eq!(receipt.generation, 1);
    assert_eq!(
        receipt.worker_id.as_deref(),
        Some("worker:page-status-enum")
    );
    assert!(receipt.attests_exact_attempt());

    agent_doc_at_claim_clock(claim_clock)
        .current_dir(tmp.path())
        .args(["finalize", doc.to_str().unwrap(), "--stream", "--origin", "skill"])
        .write_stdin(format!(
            "<!-- patch:exchange -->\n### Re: page status enum — gpt-5\n\n> **Queue prompt:** {question}\n\nYes: `current`, `draft`, `archived`, `historical`, `trashed`, `deleted`, `any`.\n<!-- /patch:exchange -->\n"
        ))
        .assert()
        .success();

    let head = head_blob(tmp.path());
    assert!(head.contains("`archived`, `historical`"), "{head}");
    assert!(
        head.contains(&format!("~~{question}")),
        "the echoed, worker-reported head must be struck:\n{head}"
    );
    assert!(
        head.contains(&format!(
            "- 🚧 [#subagent:setup#1] {setup}\n"
        )),
        "the unrelated claimed head stays queued:\n{head}"
    );
    assert!(
        agent_doc_queue_io::claim_release_receipt::load(&doc)
            .unwrap()
            .receipts
            .is_empty(),
        "the strike's closeout reconciliation ends the receipt"
    );
}
