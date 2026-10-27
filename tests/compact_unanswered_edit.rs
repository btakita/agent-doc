//! `#compactabsorbsedit`: a Compact Exchange commit must never absorb an
//! unanswered operator edit into the committed snapshot/HEAD.
//!
//! Observed 2026-10-27 on `tasks/agent-doc/agent-doc-bugs.md`: at 12:47:52Z a
//! `commit_document` refused the queue drift as an UNANSWERED edit
//! (`commit_blocked_unproved_head_current_component_drift
//! verdict=unanswered_edit_pending`). The operator then pasted two queue rows
//! (`editor_op_capture_proof operator_ops=2 non_operator_ops=0`, 12:47:57Z to
//! 12:48:01Z) and, at 12:48:48Z, pressed the IDE's Compact Exchange action
//! (`agent-doc compact <file> --component exchange --commit`). Its
//! `controller_compact_document commit=true` at 12:48:49Z built the committed
//! snapshot from the editor text, so the two unanswered rows landed in HEAD
//! (`493b81b9bd`) as if answered, and preflight's lifecycle convergence then
//! counted them as authored repeats (`#queuerowdup`).
//!
//! Compact owns only the archiveable region of its target cell. Every sibling
//! cell of the committed projection therefore comes from the pre-compact
//! baseline: an operator's sibling edit stays live drift, still visible to the
//! next preflight as an unanswered edit.

use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;

/// `#integrationtesttargets`: this target drives the real `agent-doc` CLI, and
/// a controller launch by that CLI is production behavior, so every spawned
/// command opts in with this reason.
const CONTROLLER_LAUNCH_REASON: &str = "compact --commit and preflight close out through the project controller, which they launch as production does";

const OPERATOR_ROW: &str = "- do [#operatorrow]";

const COMMITTED_DOC: &str = concat!(
    "---\nagent_doc_session: compactabsorbsedit\nagent: claude\nagent_doc_format: template\n---\n\n",
    "## Exchange\n\n",
    "<!-- agent:exchange patch=append -->\n",
    "### Re: one\n\nbody one\n\n",
    "### Re: two\n\nbody two\n\n",
    "### Re: three\n\nbody three\n\n",
    "<!-- agent:boundary:a1b2c3d4 -->\n",
    "<!-- /agent:exchange -->\n\n",
    "## Queue\n\n",
    "<!-- agent:queue -->\n",
    "<!-- /agent:queue -->\n",
);

fn agent_doc(root: &Path, args: &[&str]) -> std::process::Output {
    let mut command = cargo_bin_cmd!("agent-doc");
    command.env(
        agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
        CONTROLLER_LAUNCH_REASON,
    );
    // `#testisolationtests`: keep the project-root walk inside the fixture so
    // it never adopts `$HOME/.agent-doc`.
    command.env("AGENT_DOC_ROOT_CEILING_DIRECTORIES", root);
    if std::env::var_os(agent_doc_fs::TEST_RUNTIME_SCOPE_ENV).is_none() {
        command.env(agent_doc_fs::TEST_RUNTIME_SCOPE_ENV, std::env::temp_dir());
    }
    command.env_remove("TMUX").env_remove("TMUX_PANE");
    command.current_dir(root).args(args).output().unwrap()
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = ProcessCommand::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// A committed cycle: HEAD and the snapshot baseline both hold the document.
fn committed_session(root: &Path) -> std::path::PathBuf {
    fs::create_dir_all(root.join(".agent-doc/locks")).unwrap();
    fs::write(root.join(".gitignore"), ".agent-doc/\n").unwrap();
    let doc = root.join("session.md");
    fs::write(&doc, COMMITTED_DOC).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test User"]);
    git(root, &["add", ".gitignore", "session.md"]);
    git(root, &["commit", "-q", "--no-verify", "-m", "initial"]);
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        COMMITTED_DOC,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    doc
}

#[test]
fn compact_exchange_commit_leaves_an_unanswered_queue_row_uncommitted() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let doc = committed_session(root);
    let path = doc.to_str().unwrap();

    // The operator types a new queue row after the committed cycle.
    let edited = COMMITTED_DOC.replace(
        "<!-- agent:queue -->\n",
        &format!("<!-- agent:queue -->\n{OPERATOR_ROW}\n"),
    );
    assert_ne!(edited, COMMITTED_DOC);
    fs::write(&doc, &edited).unwrap();

    // The IDE's Compact Exchange action.
    let output = agent_doc(
        root,
        &["compact", path, "--component", "exchange", "--commit"],
    );
    assert!(
        output.status.success(),
        "compact --commit failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let head = git(root, &["show", "HEAD:session.md"]);
    assert!(
        !head.contains("### Re: one"),
        "the compaction itself must land in HEAD:\n{head}"
    );
    assert!(
        !head.contains(OPERATOR_ROW),
        "compact must not commit the operator's unanswered queue row as answered work:\n{head}"
    );
    let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
        .unwrap()
        .unwrap();
    assert!(
        !snapshot.contains(OPERATOR_ROW),
        "the snapshot baseline must not absorb the unanswered row:\n{snapshot}"
    );

    let live = fs::read_to_string(&doc).unwrap();
    assert!(
        live.contains(OPERATOR_ROW),
        "the operator's row must survive in the document:\n{live}"
    );
    assert!(
        !live.contains("### Re: one"),
        "the visible document is compacted:\n{live}"
    );

    // The next preflight still sees the row as the operator's unanswered edit.
    let output = agent_doc(root, &["preflight", path]);
    assert!(
        output.status.success(),
        "preflight failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let preflight: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
            panic!(
                "preflight stdout is not JSON ({err}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
    assert_eq!(
        preflight["no_changes"],
        serde_json::Value::Bool(false),
        "preflight must report the unanswered edit: {preflight:#}"
    );
    let diff = preflight["diff"].as_str().unwrap_or_default();
    assert!(
        diff.lines().any(|line| line == format!("+{OPERATOR_ROW}")),
        "preflight's snapshot diff must carry the operator row as an addition: {preflight:#}"
    );
    let annotated = preflight["annotated_diff"].as_str().unwrap_or_default();
    assert!(
        annotated
            .lines()
            .any(|line| line == format!("[user+] {OPERATOR_ROW}")),
        "preflight must attribute the row to the operator: {preflight:#}"
    );
}
